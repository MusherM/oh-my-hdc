use crate::{model::*, process, transport};
use anyhow::{Context, Result, bail, ensure};
use fs2::FileExt;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs::{self, File},
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::Child,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

struct Ticket {
    id: String,
    device: Option<String>,
    packages: Vec<String>,
    expires: u64,
    seen: u64,
    lease: Option<String>,
}
struct Backend {
    home: PathBuf,
    hdc: PathBuf,
    state: State,
    tickets: VecDeque<Ticket>,
    children: BTreeMap<String, Child>,
    cleaning: BTreeSet<String>,
    maintenance_running: bool,
    stopping: bool,
    orphan_operations: Vec<PathBuf>,
}
type Shared = Arc<Mutex<Backend>>;

fn idle_ms() -> u64 {
    if cfg!(debug_assertions) {
        std::env::var("OMH_TEST_IDLE_MS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(IDLE_MS)
    } else {
        IDLE_MS
    }
}

impl Backend {
    fn persist(&self) -> Result<()> {
        save(&self.home.join("state.json"), &self.state)
    }
    fn lease(&self, token: &str) -> Result<Lease> {
        let lease = self.state.leases.get(token).context("unknown lease")?;
        ensure!(
            lease.state == "active",
            "lease is {}; acquire a new device",
            lease.state
        );
        ensure!(
            self.state
                .devices
                .get(&lease.device)
                .is_some_and(|d| d.online && d.state == "leased"),
            "device unavailable"
        );
        ensure!(
            lease.job.is_some() || now().saturating_sub(lease.touched) < idle_ms(),
            "lease expired"
        );
        if let Some(job) = &lease.job {
            ensure!(
                !self.state.runs[job].dir.join("result.json").exists(),
                "job has finished; acquire a new lease"
            );
        }
        Ok(lease.clone())
    }
    fn schedule(&mut self) {
        let time = now();
        self.tickets.retain(|t| {
            t.lease.is_some() || (time < t.expires && time.saturating_sub(t.seen) < 15_000)
        });
        if self.state.maintenance.is_some() || self.stopping {
            return;
        }
        for ticket in &mut self.tickets {
            if ticket.lease.is_some() {
                continue;
            }
            let choice = self
                .state
                .devices
                .iter()
                .find(|(id, d)| {
                    d.online
                        && d.state == "free"
                        && ticket.device.as_ref().is_none_or(|wanted| wanted == *id)
                })
                .map(|(id, _)| id.clone());
            if let Some(device) = choice {
                let token = id();
                let d = self.state.devices.get_mut(&device).unwrap();
                d.state = "leased".into();
                d.packages = ticket.packages.clone();
                self.state.leases.insert(
                    token.clone(),
                    Lease {
                        token: token.clone(),
                        device,
                        packages: ticket.packages.clone(),
                        touched: time,
                        state: "active".into(),
                        job: None,
                        cleanup_error: None,
                    },
                );
                ticket.lease = Some(token);
            }
        }
    }
    fn mark_cleanup(&mut self, device: &str, reason: &str) {
        if let Some(d) = self.state.devices.get_mut(device) {
            d.state = "cleaning".into();
            d.reason = Some(reason.into());
        }
        for lease in self
            .state
            .leases
            .values_mut()
            .filter(|l| l.device == device && matches!(l.state.as_str(), "active" | "blocked"))
        {
            lease.state = "cleaning".into();
            lease.cleanup_error = None;
        }
        for run in self.state.runs.values().filter(|r| {
            self.state
                .leases
                .get(&r.lease)
                .is_some_and(|l| l.device == device)
        }) {
            if !process::stopped(&run.dir) {
                let _ = process::cancel(&run.dir);
            }
        }
    }
    #[allow(clippy::too_many_arguments)] // One launch boundary shared by exec, logs and jobs.
    fn start(
        &mut self,
        lease: Lease,
        kind: &str,
        program: PathBuf,
        args: Vec<String>,
        cwd: PathBuf,
        timeout: u64,
        output: Option<PathBuf>,
        extra_env: BTreeMap<String, String>,
    ) -> Result<Value> {
        let run_id = id();
        let dir = self.home.join("runs").join(&run_id);
        private_dir(&dir)?;
        let mut env = extra_env;
        env.insert("OMH_HOME".into(), self.home.to_string_lossy().into());
        env.insert("OMH_LEASE".into(), lease.token.clone());
        env.insert("OMH_DEVICE".into(), lease.device.clone());
        let binary = std::env::current_exe()?;
        env.insert("OMH_BIN".into(), binary.to_string_lossy().into());
        let mut path = vec![binary.parent().unwrap().to_path_buf()];
        path.extend(std::env::split_paths(
            &env.get("PATH")
                .map(std::ffi::OsString::from)
                .unwrap_or_else(|| std::env::var_os("PATH").unwrap_or_default()),
        ));
        env.insert(
            "PATH".into(),
            std::env::join_paths(path)?.to_string_lossy().into(),
        );
        let spec = Spec {
            program,
            args,
            cwd,
            env,
            stream_input: kind == "exec",
            stdout: output.unwrap_or_else(|| dir.join("stdout")),
            stderr: dir.join("stderr"),
            deadline: if kind == "logs" {
                u64::MAX
            } else {
                now().checked_add(timeout).context("timeout overflow")?
            },
        };
        let run = Run {
            id: run_id.clone(),
            lease: lease.token.clone(),
            kind: kind.into(),
            dir: dir.clone(),
            deadline: spec.deadline,
            handled: false,
        };
        self.state
            .devices
            .get_mut(&lease.device)
            .unwrap()
            .runs
            .push(run_id.clone());
        self.state.runs.insert(run_id.clone(), run);
        let active = self.state.leases.get_mut(&lease.token).unwrap();
        if kind == "job" {
            active.job = Some(run_id.clone());
        }
        if kind != "logs" {
            active.touched = now();
        }
        // Journal BEFORE launching. A crash in the spawn gap quarantines the device.
        self.persist()?;
        match process::launch(&dir, &spec) {
            Ok(child) => {
                self.children.insert(run_id.clone(), child);
            }
            Err(error) => {
                save(
                    &dir.join("result.json"),
                    &Outcome {
                        exit_code: 125,
                        reason: format!("launch: {error:#}"),
                        tree_stopped: true,
                    },
                )?;
            }
        }
        Ok(json!({"run": run_id, "stdout": spec.stdout, "stderr": spec.stderr}))
    }
}

pub fn validate(args: &[String]) -> Result<()> {
    ensure!(!args.is_empty(), "missing hdc command");
    ensure!(
        !args[0].starts_with('-'),
        "global hdc options are forbidden; omh selects the device"
    );
    match args[0].as_str() {
        "install" | "uninstall" => ensure!(args.len() > 1, "missing package arguments"),
        "shell" => {
            ensure!(
                args.len() > 1,
                "interactive hdc shell is unsupported; provide a command"
            );
            let text = args[1..].join(" ").to_ascii_lowercase();
            ensure!(
                !text.contains("reboot")
                    && !text.contains("shutdown")
                    && !text.contains("setenforce"),
                "connection/system maintenance is not supported through exec"
            );
            ensure!(
                !regex::Regex::new(r"(^|[\s;/])hilog(?:\s|$)")?.is_match(&text),
                "use omh logs for log streaming"
            );
        }
        "file" => ensure!(
            args.len() > 3 && matches!(args[1].as_str(), "send" | "recv"),
            "use file send/recv SOURCE DESTINATION"
        ),
        "fport" | "rport" => {
            ensure!(
                args.len() == 3 && args[1].contains(':') && args[2].contains(':'),
                "only creation of session-owned forwards is supported"
            );
            ensure!(
                args[1..]
                    .iter()
                    .all(|s| !s.chars().any(char::is_whitespace)),
                "forward endpoints must not contain whitespace"
            );
        }
        "bugreport" | "jpid" => {}
        "hilog" | "track-jpid" => bail!("streaming commands require omh logs or a managed job"),
        _ => bail!(
            "unsupported/global hdc command; use omh maintenance restart for service recovery"
        ),
    }
    Ok(())
}

fn handle(shared: &Shared, request: Request) -> Result<Value> {
    // The client can observe a result before the periodic tick. Reconcile it
    // before accepting the next operation so back-to-back exec calls are valid.
    if !matches!(
        &request,
        Request::Ping | Request::Status | Request::Input { .. }
    ) {
        tick(shared)?;
    }
    let mut b = shared.lock().unwrap();
    ensure!(
        !b.stopping || matches!(&request, Request::Ping | Request::Status | Request::Stop),
        "daemon is stopping"
    );
    let value = match request {
        Request::Ping => {
            json!({"version": env!("CARGO_PKG_VERSION"), "pid": std::process::id(), "hdc": b.hdc})
        }
        Request::Status => {
            json!({"devices": b.state.devices, "queue": b.tickets.iter().filter(|t| t.lease.is_none()).map(|t| json!({"ticket":t.id,"device":t.device})).collect::<Vec<_>>(), "maintenance": b.state.maintenance})
        }
        Request::Acquire {
            ticket,
            device,
            packages,
            wait_ms,
        } => {
            ensure!(uuid::Uuid::parse_str(&ticket).is_ok(), "invalid ticket");
            let pattern = regex::Regex::new(r"^[A-Za-z][A-Za-z0-9_]*(\.[A-Za-z0-9_]+)+$")?;
            ensure!(
                !packages.is_empty() && packages.iter().all(|p| pattern.is_match(p)),
                "declare valid application package names with --package"
            );
            if let Some(t) = b.tickets.iter_mut().find(|t| t.id == ticket) {
                t.seen = now();
            } else {
                b.tickets.push_back(Ticket {
                    id: ticket.clone(),
                    device,
                    packages,
                    expires: now() + wait_ms.max(1000),
                    seen: now(),
                    lease: None,
                });
            }
            b.schedule();
            let t = b
                .tickets
                .iter()
                .find(|t| t.id == ticket)
                .context("request expired")?;
            if let Some(token) = &t.lease {
                json!({"lease":token, "device":b.state.leases[token].device})
            } else {
                json!({"ticket":ticket,"state":"queued"})
            }
        }
        Request::CancelTicket { ticket } => {
            b.tickets.retain(|t| t.id != ticket);
            json!({"state":"removed"})
        }
        Request::Exec { lease, args, cwd } => {
            validate(&args)?;
            let lease = b.lease(&lease)?;
            ensure!(
                !b.state
                    .runs
                    .values()
                    .any(|r| r.lease == lease.token && r.kind == "exec" && !r.handled),
                "one foreground command per lease; wait for the previous command"
            );
            if matches!(args[0].as_str(), "fport" | "rport") {
                let rule = format!("{} {}", args[1], args[2]);
                ensure!(
                    !b.state.devices.values().any(|d| d.forwards.contains(&rule)),
                    "forward already belongs to a session"
                );
                let existing = list_forwards(&b.home, &b.hdc, &lease.device)?;
                ensure!(
                    !existing.contains(&rule),
                    "forward already exists outside this session; refusing to take ownership"
                );
                // Reserve cleanup even if hdc succeeds and the supervisor crashes before reporting.
                b.state
                    .devices
                    .get_mut(&lease.device)
                    .unwrap()
                    .forwards
                    .push(rule);
            }
            let mut command = vec!["-t".into(), lease.device.clone()];
            command.extend(args);
            let hdc = b.hdc.clone();
            b.start(
                lease,
                "exec",
                hdc,
                command,
                cwd,
                idle_ms(),
                None,
                BTreeMap::new(),
            )?
        }
        Request::Deveco {
            lease,
            runtime,
            args,
            cwd,
            timeout_ms,
        } => {
            let lease = b.lease(&lease)?;
            ensure!(
                b.state.maintenance.is_none(),
                "maintenance pending; no new signing jobs"
            );
            ensure!(
                lease.job.is_none(),
                "signing requires a lease without an existing job"
            );
            ensure!(
                !b.state
                    .runs
                    .values()
                    .any(|r| r.lease == lease.token && r.kind == "exec" && !r.handled),
                "wait for the foreground command before signing"
            );
            ensure!(
                args.starts_with(&["signature".into(), "generate".into()]) && timeout_ms > 0,
                "invalid signing request"
            );
            let command = runtime.args(args);
            b.start(
                lease,
                "job",
                runtime.node,
                command,
                cwd,
                timeout_ms,
                None,
                runtime.env,
            )?
        }
        Request::DevecoHdc { lease, args, cwd } => {
            let lease = b.lease(&lease)?;
            ensure!(
                lease.job.is_some(),
                "DevEco HDC requires a managed signing job"
            );
            let command = crate::deveco::device_command(&args, &lease.device)?;
            if let Some(args) = command {
                ensure!(
                    !b.state
                        .runs
                        .values()
                        .any(|r| r.lease == lease.token && r.kind == "exec" && !r.handled),
                    "one foreground command per lease"
                );
                let mut command = vec!["-t".into(), lease.device.clone()];
                command.extend(args);
                let hdc = b.hdc.clone();
                b.start(
                    lease,
                    "exec",
                    hdc,
                    command,
                    cwd,
                    idle_ms(),
                    None,
                    BTreeMap::new(),
                )?
            } else {
                json!({"targets": format!("{}\n", lease.device)})
            }
        }
        Request::Logs { lease, output } => {
            let lease = b.lease(&lease)?;
            ensure!(
                !b.state
                    .runs
                    .values()
                    .any(|r| r.lease == lease.token && r.kind == "logs" && !r.handled),
                "a log collector already exists for this lease"
            );
            let args = vec!["-t".into(), lease.device.clone(), "hilog".into()];
            let hdc = b.hdc.clone();
            let cwd = b.home.clone();
            b.start(
                lease,
                "logs",
                hdc,
                args,
                cwd,
                u64::MAX - now() - 1,
                Some(output),
                BTreeMap::new(),
            )?
        }
        Request::Job {
            lease,
            args,
            cwd,
            timeout_ms,
        } => {
            let lease = b.lease(&lease)?;
            ensure!(
                b.state.maintenance.is_none(),
                "maintenance pending; no new long tests"
            );
            ensure!(lease.job.is_none(), "lease already has a managed job");
            ensure!(
                !args.is_empty() && timeout_ms > 0,
                "job requires a command and positive timeout"
            );
            ensure!(
                !b.state
                    .runs
                    .values()
                    .any(|r| r.lease == lease.token && r.kind == "exec" && !r.handled),
                "wait for the foreground command before starting a job"
            );
            let program = if Path::new(&args[0]).is_absolute() {
                PathBuf::from(&args[0])
            } else if args[0].contains('/') || args[0].contains('\\') {
                cwd.join(&args[0])
            } else {
                transport::resolve(&args[0])?
            };
            b.start(
                lease,
                "job",
                program,
                args[1..].to_vec(),
                cwd,
                timeout_ms,
                None,
                BTreeMap::new(),
            )?
        }
        Request::RunStatus { run } => {
            let run = b.state.runs.get(&run).context("unknown run")?;
            let spec: Spec = read(&run.dir.join("spec.json"))?;
            let result = read::<Outcome>(&run.dir.join("result.json")).ok();
            let lease = b.state.leases.get(&run.lease).unwrap();
            json!({"run":run.id,"kind":run.kind,"result":result,"stdout":spec.stdout,"stderr":spec.stderr,"device_state":b.state.devices[&lease.device].state,"lease_state":lease.state,"cleanup_error":lease.cleanup_error})
        }
        Request::Input { run, bytes, eof } => {
            ensure!(bytes.len() <= 8192, "stdin chunk too large");
            let run = b.state.runs.get(&run).context("unknown run")?;
            ensure!(run.kind == "exec" && !run.handled, "stdin is closed");
            b.lease(&run.lease)?;
            ensure!(
                !run.dir.join("input.eof").exists(),
                "stdin is already closed"
            );
            File::options()
                .append(true)
                .open(run.dir.join("input"))?
                .write_all(&bytes)?;
            if eof {
                fs::write(run.dir.join("input.eof"), b"")?;
            }
            return Ok(json!({"ok":true}));
        }
        Request::Cancel { run } => {
            let run = b.state.runs.get(&run).context("unknown run")?.clone();
            process::cancel(&run.dir)?;
            let owner = &b.state.leases[&run.lease];
            if run.kind == "job" && owner.state == "active" && owner.job.as_deref() == Some(&run.id)
            {
                let device = owner.device.clone();
                b.mark_cleanup(&device, "job cancelled");
            }
            json!({"state":"cancelling"})
        }
        Request::Release { lease } => {
            let lease = b.state.leases.get(&lease).context("unknown lease")?.clone();
            if lease.state == "active" {
                b.mark_cleanup(&lease.device, "released");
            }
            let current = &b.state.leases[&lease.token];
            json!({"device":lease.device,"state":current.state,"cleanup_error":current.cleanup_error})
        }
        Request::Recover { device } => {
            let d = b.state.devices.get(&device).context("unknown device")?;
            ensure!(d.state == "blocked", "recover requires a blocked device");
            ensure!(d.online, "device is offline");
            b.mark_cleanup(&device, "manual recovery");
            json!({"device":device,"state":"cleaning"})
        }
        Request::Restart => {
            if b.state.maintenance.is_none()
                || b.state
                    .maintenance
                    .as_ref()
                    .is_some_and(|s| s.starts_with("failed:"))
            {
                b.state.maintenance = Some("draining".into());
            }
            json!({"maintenance":b.state.maintenance})
        }
        Request::Stop => {
            ensure!(
                b.state.devices.values().all(|d| d.state == "free"),
                "cannot stop daemon while a device is occupied or being inspected"
            );
            ensure!(
                b.state.maintenance.is_none(),
                "cannot stop daemon during maintenance"
            );
            ensure!(
                b.cleaning.is_empty() && b.children.is_empty(),
                "active operations remain"
            );
            b.stopping = true;
            json!({"stopped":true})
        }
    };
    b.persist()?;
    Ok(value)
}

fn discover(home: &Path, hdc: &Path) -> Result<BTreeSet<String>> {
    let (outcome, output, error) =
        process::bounded(home, hdc, vec!["list".into(), "targets".into()], 5000)?;
    ensure!(
        outcome.exit_code == 0 && outcome.tree_stopped && !failed(&output) && !failed(&error),
        "device discovery failed: {output} {error}"
    );
    Ok(output
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.starts_with('['))
        .filter_map(|s| s.split_whitespace().next().map(str::to_owned))
        .collect())
}

fn device_info(home: &Path, hdc: &Path, device: &str) -> DeviceInfo {
    fn read_param(home: &Path, hdc: &Path, device: &str, key: &str) -> Option<String> {
        let (result, output, error) = process::bounded(
            home,
            hdc,
            vec![
                "-t".into(),
                device.into(),
                "shell".into(),
                "param".into(),
                "get".into(),
                key.into(),
            ],
            5000,
        )
        .ok()?;
        if result.exit_code != 0 || !result.tree_stopped || failed(&output) || failed(&error) {
            return None;
        }
        let value = output.trim();
        (!value.is_empty()).then(|| value.to_owned())
    }

    DeviceInfo {
        name: read_param(home, hdc, device, "const.product.name"),
        model: read_param(home, hdc, device, "const.product.model"),
        chip: read_param(home, hdc, device, "ohos.boot.chiptype"),
        brand: read_param(home, hdc, device, "const.product.brand"),
        os_version: read_param(home, hdc, device, "const.ohos.fullname"),
        api_version: read_param(home, hdc, device, "const.ohos.apiversion"),
    }
}
fn failed(text: &str) -> bool {
    let t = text.to_lowercase();
    t.contains("[fail]") || t.contains("[error]") || t.contains("error:")
}

fn list_forwards(home: &Path, hdc: &Path, device: &str) -> Result<String> {
    let (r, out, err) = process::bounded(
        home,
        hdc,
        vec!["-t".into(), device.into(), "fport".into(), "ls".into()],
        5000,
    )?;
    ensure!(
        r.exit_code == 0 && r.tree_stopped && !failed(&out) && !failed(&err),
        "cannot inspect forwards: {out} {err}"
    );
    Ok(out)
}

fn cleanup(
    home: &Path,
    hdc: &Path,
    device: &str,
    d: &Device,
    runs: &BTreeMap<String, Run>,
    orphan_operations: &[PathBuf],
) -> Result<()> {
    for run_id in &d.runs {
        let run = runs.get(run_id).context("missing run journal")?;
        process::cancel(&run.dir)?;
        let end = now() + 5000;
        while !process::stopped(&run.dir) && now() < end {
            thread::sleep(Duration::from_millis(50));
        }
        ensure!(
            process::stopped(&run.dir),
            "cannot confirm process tree stopped for run {run_id}; inspect {}",
            run.dir.display()
        );
    }
    ensure!(d.online, "device offline; reconnect and run omh recover");
    // An interrupted maintenance/cleanup command could still affect a new lease.
    for path in orphan_operations {
        ensure!(
            !path.exists() || process::stopped(path),
            "unresolved operation {}; inspect supervisor before recovery",
            path.display()
        );
    }
    for forward in &d.forwards {
        if !list_forwards(home, hdc, device)?.contains(forward) {
            continue;
        }
        let mut args = vec!["-t".into(), device.into(), "fport".into(), "rm".into()];
        args.extend(forward.split_whitespace().map(str::to_owned));
        let (r, out, err) = process::bounded(home, hdc, args, 10_000)?;
        ensure!(
            r.exit_code == 0
                && r.tree_stopped
                && !failed(&out)
                && !failed(&err)
                && out.to_lowercase().contains("success"),
            "forward cleanup failed: {out} {err}"
        );
        ensure!(
            !list_forwards(home, hdc, device)?.contains(forward),
            "forward still exists after removal: {forward}"
        );
    }
    for package in &d.packages {
        let (r, out, err) = process::bounded(
            home,
            hdc,
            vec![
                "-t".into(),
                device.into(),
                "shell".into(),
                "aa".into(),
                "force-stop".into(),
                package.clone(),
            ],
            10_000,
        )?;
        let absent = out.contains("Error Code:10104002")
            && out.contains(
                "The application corresponding to the specified package name is not installed.",
            );
        let success = r.exit_code == 0
            && !failed(&out)
            && !failed(&err)
            && out
                .to_lowercase()
                .contains("force stop process successfully");
        ensure!(
            r.tree_stopped && (success || absent),
            "application cleanup not confirmed for {package}: {out} {err}"
        );
    }
    Ok(())
}

fn tick(shared: &Shared) -> Result<()> {
    let mut b = shared.lock().unwrap();
    let done: Vec<_> = b
        .state
        .runs
        .values()
        .filter(|r| !r.handled)
        .filter_map(|r| {
            read::<Outcome>(&r.dir.join("result.json"))
                .ok()
                .map(|o| (r.clone(), o))
        })
        .collect();
    for (run, outcome) in done {
        b.state.runs.get_mut(&run.id).unwrap().handled = true;
        let lease = b.state.leases[&run.lease].clone();
        if lease.state != "active" {
            continue;
        }
        if run.kind == "job"
            || !outcome.tree_stopped
            || outcome.reason != "exited" && run.kind == "exec"
        {
            b.mark_cleanup(&lease.device, &format!("{}: {}", run.kind, outcome.reason));
        } else if run.kind == "exec" {
            b.state.leases.get_mut(&run.lease).unwrap().touched = now();
        }
    }
    // A result file can precede supervisor exit. Keep the handle until reaped.
    b.children
        .retain(|_, child| matches!(child.try_wait(), Ok(None)));
    let expired: Vec<_> = b
        .state
        .leases
        .values()
        .filter(|l| {
            l.state == "active" && l.job.is_none() && now().saturating_sub(l.touched) >= idle_ms()
        })
        .map(|l| l.device.clone())
        .collect();
    for device in expired {
        b.mark_cleanup(&device, "idle timeout");
    }
    let todo: Vec<_> = b
        .state
        .devices
        .iter()
        .filter(|(id, d)| d.state == "cleaning" && !b.cleaning.contains(*id))
        .map(|(id, d)| (id.clone(), d.clone()))
        .collect();
    for (device, d) in todo {
        b.cleaning.insert(device.clone());
        let (home, hdc, runs, orphans) = (
            b.home.clone(),
            b.hdc.clone(),
            b.state.runs.clone(),
            b.orphan_operations.clone(),
        );
        let shared = shared.clone();
        thread::spawn(move || {
            let result = cleanup(&home, &hdc, &device, &d, &runs, &orphans);
            let mut b = shared.lock().unwrap();
            b.cleaning.remove(&device);
            let d = b.state.devices.get_mut(&device).unwrap();
            match result {
                Ok(()) if d.online => {
                    d.state = "free".into();
                    d.reason = None;
                    d.packages.clear();
                    d.forwards.clear();
                    d.runs.clear();
                }
                other => {
                    d.state = "blocked".into();
                    d.reason = Some(
                        other
                            .err()
                            .map(|e| format!("{e:#}"))
                            .unwrap_or_else(|| "device disconnected during cleanup".into()),
                    );
                }
            }
            let error = d.reason.clone();
            let success = d.state == "free";
            for lease in b
                .state
                .leases
                .values_mut()
                .filter(|l| l.device == device && l.state == "cleaning")
            {
                lease.state = if success { "released" } else { "blocked" }.into();
                lease.cleanup_error = if success { None } else { error.clone() };
            }
            if let Err(error) = b.persist() {
                eprintln!("persist cleanup: {error:#}");
                std::process::exit(125);
            }
        });
    }
    if b.state.maintenance.as_deref() == Some("draining")
        && !b.maintenance_running
        && !b
            .state
            .devices
            .values()
            .any(|d| matches!(d.state.as_str(), "leased" | "cleaning" | "inspecting"))
    {
        if b.state.devices.values().any(|d| d.state == "blocked") {
            b.state.maintenance = Some("failed: blocked devices must be recovered first".into());
        } else {
            b.maintenance_running = true;
            b.state.maintenance = Some("restarting".into());
            let (home, hdc, shared, orphans) = (
                b.home.clone(),
                b.hdc.clone(),
                shared.clone(),
                b.orphan_operations.clone(),
            );
            thread::spawn(move || {
                let result = (|| -> Result<BTreeSet<String>> {
                    ensure!(
                        orphans.iter().all(|p| !p.exists() || process::stopped(p)),
                        "unresolved operation from previous daemon; inspect before restarting"
                    );
                    let (r, out, err) =
                        process::bounded(&home, &hdc, vec!["kill".into(), "-r".into()], 15_000)?;
                    ensure!(
                        r.exit_code == 0 && r.tree_stopped && !failed(&out) && !failed(&err),
                        "restart failed: {out} {err}"
                    );
                    discover(&home, &hdc)
                })();
                let mut b = shared.lock().unwrap();
                b.maintenance_running = false;
                match result {
                    Ok(devices) => {
                        apply_devices(&mut b, devices);
                        b.state.maintenance = None;
                    }
                    Err(e) => b.state.maintenance = Some(format!("failed: {e:#}")),
                }
                if let Err(e) = b.persist() {
                    eprintln!("persist maintenance: {e:#}");
                    std::process::exit(125);
                }
            });
        }
    }
    b.schedule();
    b.persist()
}

fn apply_devices(b: &mut Backend, online: BTreeSet<String>) {
    let disconnected: Vec<_> = b
        .state
        .devices
        .iter()
        .filter(|(id, d)| d.online && !online.contains(*id) && d.state == "leased")
        .map(|(id, _)| id.clone())
        .collect();
    for (id, d) in &mut b.state.devices {
        d.online = online.contains(id);
    }
    for id in online {
        b.state.devices.entry(id).or_insert_with(Device::new).online = true;
    }
    for id in disconnected {
        b.mark_cleanup(&id, "device disconnected");
    }
}

pub fn serve(home: PathBuf, hdc: Option<PathBuf>) -> Result<()> {
    private_dir(&home)?;
    private_dir(&home.join("operations"))?;
    let lock = File::options()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(home.join("daemon.lock"))?;
    if lock.try_lock_exclusive().is_err() {
        return Ok(());
    }
    let hdc = match hdc {
        Some(p) => fs::canonicalize(p)?,
        None if home.join("hdc.json").exists() => {
            fs::canonicalize(read::<PathBuf>(&home.join("hdc.json"))?)?
        }
        None => transport::resolve("hdc")?,
    };
    save(&home.join("hdc.json"), &hdc)?;
    let mut state: State = if home.join("state.json").exists() {
        read(&home.join("state.json"))?
    } else {
        State::default()
    };
    for d in state.devices.values_mut() {
        if d.state != "free" {
            d.state = "blocked".into();
            d.reason = Some("daemon restarted; run omh recover after supervisor cleanup".into());
        }
        d.online = false;
    }
    for lease in state.leases.values_mut().filter(|l| l.state != "released") {
        lease.state = "blocked".into();
        lease.cleanup_error = Some("daemon restarted; recover the device".into());
    }
    if state.maintenance.is_some() {
        state.maintenance = Some(
            "failed: daemon restarted during maintenance; inspect and retry explicitly".into(),
        );
    }
    let mut orphan_operations = Vec::new();
    for entry in fs::read_dir(home.join("operations"))? {
        let path = entry?.path();
        if path.is_dir() && path.join("spec.json").exists() {
            let spec: Spec = read(&path.join("spec.json"))?;
            if spec.args.first().is_none_or(|s| s != "list") && !process::stopped(&path) {
                orphan_operations.push(path);
            }
        }
    }
    if !orphan_operations.is_empty() {
        state.maintenance = Some("failed: unresolved operation from previous daemon".into());
    }
    let backend = Backend {
        home: home.clone(),
        hdc: hdc.clone(),
        state,
        tickets: VecDeque::new(),
        children: BTreeMap::new(),
        cleaning: BTreeSet::new(),
        maintenance_running: false,
        stopping: false,
        orphan_operations,
    };
    backend.persist()?;
    let shared = Arc::new(Mutex::new(backend));
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let secret = id();
    save(
        &home.join("endpoint.json"),
        &Endpoint {
            address: listener.local_addr()?.to_string(),
            secret: secret.clone(),
        },
    )?;
    let tick_state = shared.clone();
    thread::spawn(move || {
        loop {
            if let Err(error) = tick(&tick_state) {
                eprintln!("scheduler stopped: {error:#}");
                std::process::exit(125);
            }
            thread::sleep(Duration::from_millis(100));
        }
    });
    let discovery_state = shared.clone();
    thread::spawn(move || {
        loop {
            let busy = {
                let b = discovery_state.lock().unwrap();
                b.maintenance_running || b.stopping
            };
            if !busy {
                match discover(&home, &hdc) {
                    Ok(devices) => {
                        let pending = {
                            let mut b = discovery_state.lock().unwrap();
                            apply_devices(&mut b, devices);
                            let pending: Vec<_> = b
                                .state
                                .devices
                                .iter_mut()
                                .filter(|(_, d)| d.online && d.state == "free" && d.info.is_none())
                                .map(|(id, d)| {
                                    d.state = "inspecting".into();
                                    id.clone()
                                })
                                .collect();
                            if let Err(error) = b.persist() {
                                eprintln!("persist discovery: {error:#}");
                                std::process::exit(125);
                            }
                            pending
                        };
                        for device in pending {
                            let info = device_info(&home, &hdc, &device);
                            let mut b = discovery_state.lock().unwrap();
                            if let Some(d) = b.state.devices.get_mut(&device)
                                && d.state == "inspecting"
                            {
                                d.info = Some(info);
                                d.state = "free".into();
                            }
                            b.schedule();
                            if let Err(error) = b.persist() {
                                eprintln!("persist device info: {error:#}");
                                std::process::exit(125);
                            }
                        }
                    }
                    Err(error) => {
                        eprintln!("discovery: {error:#}");
                        apply_devices(&mut discovery_state.lock().unwrap(), BTreeSet::new());
                    }
                }
            }
            thread::sleep(Duration::from_secs(2));
        }
    });
    for stream in listener.incoming() {
        let shared = shared.clone();
        let secret = secret.clone();
        if let Ok(mut stream) = stream {
            thread::spawn(move || {
                let result = (|| -> Result<Value> {
                    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
                    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
                    let mut line = String::new();
                    BufReader::new((&mut stream).take(131_072)).read_line(&mut line)?;
                    let envelope: Envelope = serde_json::from_str(&line)?;
                    ensure!(envelope.secret == secret, "unauthorized local client");
                    let stop = matches!(&envelope.request, Request::Stop);
                    let result = handle(&shared, envelope.request);
                    let value = result?;
                    if stop {
                        serde_json::to_writer(&mut stream, &value)?;
                        stream.write_all(b"\n")?;
                        stream.flush()?;
                        std::process::exit(0);
                    }
                    Ok(value)
                })();
                let response = result.unwrap_or_else(|e| json!({"error":format!("{e:#}")}));
                let _ = serde_json::to_writer(&mut stream, &response);
                let _ = stream.write_all(b"\n");
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn command_boundaries() {
        for args in [
            vec!["kill", "-r"],
            vec!["-t", "other", "shell", "ls"],
            vec!["shell", "reboot"],
            vec!["shell", "hilog"],
            vec!["fport", "rm", "tcp:123"],
        ] {
            assert!(validate(&args.into_iter().map(String::from).collect::<Vec<_>>()).is_err());
        }
        assert!(validate(&["shell".into(), "echo".into(), "-t".into()]).is_ok());
    }
    #[test]
    fn eligible_fifo_does_not_block_other_devices() {
        let mut state = State::default();
        state.devices.insert("A".into(), Device::new());
        state.devices.insert("B".into(), Device::new());
        state.devices.get_mut("A").unwrap().state = "leased".into();
        let mut b = Backend {
            home: PathBuf::new(),
            hdc: PathBuf::new(),
            state,
            tickets: VecDeque::new(),
            children: BTreeMap::new(),
            cleaning: BTreeSet::new(),
            maintenance_running: false,
            stopping: false,
            orphan_operations: vec![],
        };
        for device in [Some("A".into()), None, None] {
            b.tickets.push_back(Ticket {
                id: id(),
                device,
                packages: vec!["com.test".into()],
                expires: now() + 10000,
                seen: now(),
                lease: None,
            });
        }
        b.schedule();
        assert!(b.tickets[0].lease.is_none());
        assert!(b.tickets[1].lease.is_some());
        assert!(b.tickets[2].lease.is_none());
        assert_eq!(b.state.leases.len(), 1);
    }
}
