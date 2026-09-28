use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Child, Command, Output, Stdio},
    sync::OnceLock,
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;
fn fake() -> &'static PathBuf {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        let root = tempfile::tempdir().unwrap().keep();
        let binary = root.join(if cfg!(windows) {
            "fake-hdc.exe"
        } else {
            "fake-hdc"
        });
        let status = Command::new("rustc")
            .args(["--edition", "2024"])
            .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fake_hdc.rs"))
            .arg("-o")
            .arg(&binary)
            .status()
            .unwrap();
        assert!(status.success());
        binary
    })
}
struct Env {
    root: TempDir,
    daemon: Child,
}
impl Env {
    fn new(idle: u64, devices: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("devices"), devices).unwrap();
        let log = fs::File::create(root.path().join("test-daemon.log")).unwrap();
        let daemon = Command::new(env!("CARGO_BIN_EXE_omh"))
            .args(["--home"])
            .arg(root.path().join("home"))
            .arg("--hdc")
            .arg(fake())
            .arg("__daemon")
            .env("OMH_FAKE_DIR", root.path())
            .env("OMH_TEST_IDLE_MS", idle.to_string())
            .stdin(Stdio::null())
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap();
        let env = Self { root, daemon };
        wait(|| env.root.path().join("home/endpoint.json").exists());
        wait(|| {
            env.value(&["devices"])["devices"]
                .as_object()
                .is_some_and(|d| d.len() == devices.lines().count())
        });
        env
    }
    fn command(&self, args: &[&str]) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_omh"));
        c.arg("--home")
            .arg(self.root.path().join("home"))
            .arg("--json")
            .args(args)
            .env("OMH_FAKE_DIR", self.root.path());
        c
    }
    fn output(&self, args: &[&str]) -> Output {
        let output = self.command(args).output().unwrap();
        if !output.status.success() {
            let mut log = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.root.path().join("client-errors.log"))
                .unwrap();
            writeln!(
                log,
                "command: {args:?}\n{}",
                String::from_utf8_lossy(&output.stderr)
            )
            .unwrap();
        }
        output
    }
    fn value(&self, args: &[&str]) -> Value {
        let o = self.output(args);
        assert!(
            o.status.success(),
            "{:?}: {}",
            args,
            String::from_utf8_lossy(&o.stderr)
        );
        serde_json::from_slice(&o.stdout).unwrap()
    }
    fn acquire(&self, device: &str) -> String {
        self.value(&[
            "acquire",
            "--package",
            "com.test.app",
            "--device",
            device,
            "--wait",
            "3s",
        ])["lease"]
            .as_str()
            .unwrap()
            .into()
    }
    fn state(&self, device: &str) -> String {
        self.value(&["status"])["devices"][device]["state"]
            .as_str()
            .unwrap()
            .into()
    }
    fn events(&self) -> String {
        fs::read_to_string(self.root.path().join("events")).unwrap_or_default()
    }
}
impl Drop for Env {
    fn drop(&mut self) {
        if thread::panicking() {
            eprintln!("test evidence retained: {}", self.root.path().display());
            self.root.disable_cleanup(true);
        }
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
        thread::sleep(Duration::from_millis(200));
    }
}
fn wait(mut test: impl FnMut() -> bool) {
    let start = Instant::now();
    while !test() {
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "timed out waiting for invariant"
        );
        thread::sleep(Duration::from_millis(60));
    }
}

#[test]
fn devices_include_read_only_hardware_and_os_info() {
    let e = Env::new(10000, "A\n");
    wait(|| e.value(&["devices"])["devices"]["A"]["info"]["name"] == "Test Phone");
    let info = &e.value(&["devices"])["devices"]["A"]["info"];
    assert_eq!(info["model"], "TEST-01");
    assert_eq!(info["chip"], "TestChip");
    assert_eq!(info["brand"], "TestBrand");
    assert_eq!(info["os_version"], "OpenHarmony-7.0");
    assert_eq!(info["api_version"], "26");
    assert_eq!(e.state("A"), "free");
}

#[test]
fn exclusive_sessions_fifo_and_stream_semantics() {
    let e = Env::new(10000, "A\nB\n");
    let a = e.acquire("A");
    let b = e.acquire("B");
    let mut pending = e
        .command(&[
            "acquire",
            "--package",
            "com.test.other",
            "--device",
            "A",
            "--wait",
            "5s",
        ])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    thread::sleep(Duration::from_millis(200));
    assert!(pending.try_wait().unwrap().is_none());
    assert_eq!(e.value(&["status"])["queue"].as_array().unwrap().len(), 1);
    let o = e.output(&["exec", "--lease", &b, "--", "shell", "echo", "hello world"]);
    assert!(o.status.success());
    assert_eq!(
        String::from_utf8(o.stdout).unwrap().trim(),
        "shell|echo|hello world"
    );
    let mut cat = e
        .command(&["exec", "--lease", &b, "--", "shell", "cat"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    cat.stdin
        .take()
        .unwrap()
        .write_all(b"binary\0input\n")
        .unwrap();
    let out = cat.wait_with_output().unwrap();
    assert!(out.status.success());
    assert_eq!(out.stdout, b"binary\0input\n");
    assert_eq!(
        e.output(&["exec", "--lease", &b, "--", "shell", "fail"])
            .status
            .code(),
        Some(37)
    );
    e.value(&["release", "--lease", &a]);
    let acquired = pending.wait_with_output().unwrap();
    assert!(acquired.status.success());
    let next: Value = serde_json::from_slice(&acquired.stdout).unwrap();
    assert_ne!(next["lease"], a);
    assert!(
        !e.output(&["exec", "--lease", &a, "--", "shell", "echo", "stale"])
            .status
            .success()
    );
    e.value(&["release", "--lease", &a]); // idempotent, cannot release the new owner's lease
    assert_eq!(e.state("A"), "leased");
    e.value(&["release", "--lease", next["lease"].as_str().unwrap()]);
    e.value(&["release", "--lease", &b]);
    assert!(e.events().contains("force-stop|com.test.app"));
}
#[test]
fn logs_do_not_renew_and_long_job_survives_idle() {
    let e = Env::new(900, "A\n");
    let a = e.acquire("A");
    let log = e.root.path().join("log.txt");
    let collector = e.value(&["logs", "--lease", &a, "--output", log.to_str().unwrap()]);
    wait(|| e.state("A") == "free");
    let n = fs::metadata(&log).unwrap().len();
    assert!(n > 0);
    thread::sleep(Duration::from_millis(150));
    assert_eq!(n, fs::metadata(&log).unwrap().len());
    assert!(
        e.value(&["job", "status", collector["run"].as_str().unwrap()])["result"]["tree_stopped"]
            .as_bool()
            .unwrap()
    );
    let a = e.acquire("A");
    let job = e.value(&[
        "job",
        "start",
        "--lease",
        &a,
        "--timeout",
        "1h",
        "--",
        fake().to_str().unwrap(),
        "--job",
        "2400",
    ]);
    thread::sleep(Duration::from_millis(1250));
    assert_eq!(e.state("A"), "leased");
    wait(|| e.state("A") == "free");
    let result = e.value(&["job", "status", job["run"].as_str().unwrap()]);
    assert_eq!(result["result"]["exit_code"], 0);
    assert!(
        fs::read_to_string(result["stdout"].as_str().unwrap())
            .unwrap()
            .contains("BUSINESS_SUCCESS")
    );
}
#[test]
fn managed_job_can_call_omh_and_timeout_releases() {
    let e = Env::new(10000, "A\n");
    let a = e.acquire("A");
    let job = e.value(&[
        "job",
        "start",
        "--lease",
        &a,
        "--timeout",
        "5s",
        "--",
        fake().to_str().unwrap(),
        "--nested-job",
    ]);
    wait(|| e.state("A") == "free");
    assert_eq!(
        e.value(&["job", "status", job["run"].as_str().unwrap()])["result"]["exit_code"],
        0
    );
    let a = e.acquire("A");
    e.value(&["job", "cancel", job["run"].as_str().unwrap()]);
    assert_eq!(e.state("A"), "leased");
    let job = e.value(&[
        "job",
        "start",
        "--lease",
        &a,
        "--timeout",
        "300ms",
        "--",
        fake().to_str().unwrap(),
        "--job",
        "5000",
    ]);
    wait(|| e.state("A") == "free");
    let result = e.value(&["job", "status", job["run"].as_str().unwrap()]);
    assert_eq!(result["result"]["reason"], "timeout");
    assert_eq!(result["result"]["exit_code"], 124);
}

#[test]
fn concurrent_device_cleanup_does_not_confuse_live_operations_with_orphans() {
    let e = Env::new(10000, "A\nB\n");
    let a = e.acquire("A");
    let b = e.acquire("B");
    let first = e
        .command(&["release", "--lease", &a])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let second = e
        .command(&["release", "--lease", &b])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    for out in [
        first.wait_with_output().unwrap(),
        second.wait_with_output().unwrap(),
    ] {
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    assert_eq!(e.state("A"), "free");
    assert_eq!(e.state("B"), "free");
}

#[test]
fn forward_ownership_and_exact_cleanup() {
    let e = Env::new(10000, "A\n");
    let a = e.acquire("A");
    fs::write(e.root.path().join("A-forwards"), "tcp:1111 tcp:2222").unwrap();
    assert!(
        !e.output(&["exec", "--lease", &a, "--", "fport", "tcp:1111", "tcp:2222"])
            .status
            .success()
    );
    fs::write(e.root.path().join("A-forwards"), "").unwrap();
    assert!(
        e.output(&["exec", "--lease", &a, "--", "fport", "tcp:1234", "tcp:4321"])
            .status
            .success()
    );
    e.value(&["release", "--lease", &a]);
    assert!(e.events().contains("fport|rm|tcp:1234|tcp:4321"));
    assert_eq!(
        fs::read_to_string(e.root.path().join("A-forwards")).unwrap(),
        ""
    );
}

#[test]
fn cancelling_job_stops_descendants_before_reassignment() {
    let e = Env::new(10000, "A\n");
    let a = e.acquire("A");
    let job = e.value(&[
        "job",
        "start",
        "--lease",
        &a,
        "--timeout",
        "1h",
        "--",
        fake().to_str().unwrap(),
        "--tree-job",
    ]);
    let heartbeat = e.root.path().join("heartbeat");
    wait(|| fs::metadata(&heartbeat).is_ok_and(|m| m.len() > 0));
    e.value(&["job", "cancel", job["run"].as_str().unwrap()]);
    wait(|| e.state("A") == "free");
    let count = fs::metadata(&heartbeat).unwrap().len();
    let new = e.acquire("A");
    thread::sleep(Duration::from_millis(150));
    assert_eq!(fs::metadata(&heartbeat).unwrap().len(), count);
    e.value(&["release", "--lease", &new]);
}

#[test]
fn simultaneous_processes_cannot_double_allocate() {
    let e = Env::new(10000, "A\n");
    // Discovery publishes the device before its hardware inspection finishes.
    // Start the contention window only once the device can actually be leased.
    wait(|| e.state("A") == "free");
    let mut clients = Vec::new();
    for _ in 0..8 {
        clients.push(
            e.command(&["acquire", "--package", "com.test.app", "--wait", "400ms"])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        );
    }
    let outputs: Vec<Output> = clients
        .into_iter()
        .map(|child| child.wait_with_output().unwrap())
        .collect();
    let winners: Vec<Value> = outputs
        .iter()
        .filter(|out| out.status.success())
        .map(|out| serde_json::from_slice(&out.stdout).unwrap())
        .collect();
    assert_eq!(winners.len(), 1, "client results: {outputs:?}");
    e.value(&["release", "--lease", winners[0]["lease"].as_str().unwrap()]);
}

#[test]
fn completed_command_does_not_wait_for_periodic_reconciliation() {
    let e = Env::new(10000, "A\n");
    let lease = e.acquire("A");
    for _ in 0..16 {
        let out = e.output(&["exec", "--lease", &lease, "--", "shell", "fail"]);
        assert_eq!(
            out.status.code(),
            Some(37),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    e.value(&["release", "--lease", &lease]);
}

#[test]
fn daemon_crash_stops_logs_and_quarantines_until_recovery() {
    let mut e = Env::new(10000, "A\n");
    let a = e.acquire("A");
    let log = e.root.path().join("crash-log");
    let run = e.value(&["logs", "--lease", &a, "--output", log.to_str().unwrap()]);
    wait(|| fs::metadata(&log).unwrap().len() > 0);
    e.daemon.kill().unwrap();
    e.daemon.wait().unwrap();
    let result = e
        .root
        .path()
        .join("home/runs")
        .join(run["run"].as_str().unwrap())
        .join("result.json");
    wait(|| result.exists());
    let outcome: Value = serde_json::from_slice(&fs::read(&result).unwrap()).unwrap();
    assert_eq!(outcome["tree_stopped"], true);
    let log_file = fs::File::create(e.root.path().join("restart-daemon.log")).unwrap();
    let old_endpoint = fs::read_to_string(e.root.path().join("home/endpoint.json")).unwrap();
    e.daemon = e
        .command(&["--hdc", fake().to_str().unwrap(), "__daemon"])
        .stdout(log_file.try_clone().unwrap())
        .stderr(log_file)
        .spawn()
        .unwrap();
    // Wait on endpoint PID, avoiding accidental auto-start while the child boots.
    wait(|| fs::read_to_string(e.root.path().join("home/endpoint.json")).unwrap() != old_endpoint);
    wait(|| e.value(&["status"])["devices"]["A"]["online"] == true);
    assert!(
        !e.output(&["exec", "--lease", &a, "--", "shell", "echo", "stale"])
            .status
            .success()
    );
    e.value(&["recover", "--device", "A"]);
    wait(|| e.state("A") == "free");
}
#[test]
fn cleanup_failure_quarantines_and_recovery_retries() {
    let e = Env::new(10000, "A\n");
    let a = e.acquire("A");
    fs::write(e.root.path().join("cleanup-fails"), "").unwrap();
    assert!(!e.output(&["release", "--lease", &a]).status.success());
    assert_eq!(e.state("A"), "blocked");
    assert!(
        !e.output(&["acquire", "--package", "com.test.app", "--wait", "200ms"])
            .status
            .success()
    );
    fs::remove_file(e.root.path().join("cleanup-fails")).unwrap();
    e.value(&["recover", "--device", "A"]);
    wait(|| e.state("A") == "free");
}

#[test]
fn known_uninstalled_app_is_already_clean() {
    let e = Env::new(10000, "A\n");
    let a = e.acquire("A");
    fs::write(e.root.path().join("app-absent"), "").unwrap();
    e.value(&["release", "--lease", &a]);
    assert_eq!(e.state("A"), "free");
}
#[test]
fn maintenance_drains_coalesces_and_never_restarts_active_test() {
    let e = Env::new(10000, "A\nB\n");
    let a = e.acquire("A");
    let b = e.acquire("B");
    e.value(&["maintenance", "restart"]);
    e.value(&["maintenance", "restart"]);
    e.value(&["release", "--lease", &a]);
    assert!(!e.events().contains("kill|-r"));
    assert!(
        !e.output(&[
            "job",
            "start",
            "--lease",
            &b,
            "--timeout",
            "1s",
            "--",
            fake().to_str().unwrap(),
            "--job",
            "1"
        ])
        .status
        .success()
    );
    e.value(&["release", "--lease", &b]);
    wait(|| e.value(&["status"])["maintenance"].is_null());
    assert_eq!(e.events().matches("kill|-r").count(), 1);
    fs::write(e.root.path().join("restart-fails"), "").unwrap();
    e.value(&["maintenance", "restart"]);
    wait(|| {
        e.value(&["status"])["maintenance"]
            .as_str()
            .is_some_and(|s| s.starts_with("failed:"))
    });
    thread::sleep(Duration::from_millis(300));
    assert_eq!(e.events().matches("kill|-r").count(), 2);
}
#[test]
fn disconnect_invalidates_without_switching_devices() {
    let e = Env::new(10000, "A\nB\n");
    let a = e.acquire("A");
    fs::write(e.root.path().join("devices"), "B\n").unwrap();
    wait(|| e.state("A") == "blocked");
    assert!(
        !e.output(&["exec", "--lease", &a, "--", "shell", "echo", "bad"])
            .status
            .success()
    );
    fs::write(e.root.path().join("devices"), "A\nB\n").unwrap();
    wait(|| e.value(&["status"])["devices"]["A"]["online"] == true);
    assert_eq!(e.state("A"), "blocked");
    e.value(&["recover", "--device", "A"]);
    wait(|| e.state("A") == "free");
}
#[test]
fn codex_setup_is_idempotent_and_does_not_claim_runtime_protection() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join(".codex")).unwrap();
    fs::write(dir.path().join(".codex/config.toml"), "model = 'keep-me'\n").unwrap();
    for _ in 0..2 {
        let out = Command::new(env!("CARGO_BIN_EXE_omh"))
            .args(["setup", "codex", "--project"])
            .arg(dir.path())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let v: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(v["protected"], false);
    }
    let config = fs::read_to_string(dir.path().join(".codex/config.toml")).unwrap();
    assert!(config.contains("keep-me"));
    assert_eq!(config.matches("[[hooks.PreToolUse]]").count(), 1);
    let mut hook = Command::new(env!("CARGO_BIN_EXE_omh"))
        .arg("hook")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    hook.stdin
        .take()
        .unwrap()
        .write_all(
            json!({"tool_input":{"command":"hdc shell ls"}})
                .to_string()
                .as_bytes(),
        )
        .unwrap();
    let out = hook.wait_with_output().unwrap();
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["hookSpecificOutput"]["permissionDecision"], "deny");
}
