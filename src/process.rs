//! A small supervisor owns each process tree. The daemon holds its stdin open;
//! EOF cancels the tree even when the daemon was killed without running Drop.
use crate::model::*;
use anyhow::{Context, Result};
use command_group::CommandGroup;
use fs2::FileExt;
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};

pub fn launch(dir: &Path, spec: &Spec) -> Result<Child> {
    private_dir(dir)?;
    save(&dir.join("spec.json"), spec)?;
    File::create(dir.join("input"))?;
    // Fail before spawning if output paths are invalid. Never overwrite user evidence.
    for path in [&spec.stdout, &spec.stderr] {
        File::options()
            .write(true)
            .create_new(true)
            .open(path)
            .with_context(|| format!("create output {}", path.display()))?;
    }
    let child = Command::new(std::env::current_exe()?)
        .arg("__run")
        .arg(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("start process supervisor");
    if child.is_err() {
        // No supervisor was started; avoid leaving a false unresolved operation.
        let _ = fs::remove_dir_all(dir);
    }
    child
}

pub fn supervise(dir: &Path) -> Result<()> {
    let lock = File::options()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join("runner.lock"))?;
    lock.lock_exclusive()?;
    let result = supervise_inner(dir);
    if let Err(error) = result {
        // Unknown cleanup is deliberately not a successful result.
        save(
            &dir.join("result.json"),
            &Outcome {
                exit_code: 125,
                reason: format!("supervisor: {error:#}"),
                tree_stopped: false,
            },
        )?;
    }
    Ok(())
}

fn supervise_inner(dir: &Path) -> Result<()> {
    let spec: Spec = read(&dir.join("spec.json"))?;
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut byte = [0];
        let _ = std::io::stdin().read(&mut byte);
        let _ = tx.send(());
    });
    let mut command = Command::new(&spec.program);
    command
        .args(&spec.args)
        .current_dir(&spec.cwd)
        .envs(&spec.env)
        .stdin(if spec.stream_input {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(
            File::options()
                .write(true)
                .truncate(true)
                .open(&spec.stdout)?,
        )
        .stderr(
            File::options()
                .write(true)
                .truncate(true)
                .open(&spec.stderr)?,
        );
    let mut builder = command.group();
    #[cfg(windows)]
    builder.kill_on_drop(true);
    let mut child = match builder.spawn() {
        Ok(child) => child,
        Err(error) => {
            return save(
                &dir.join("result.json"),
                &Outcome {
                    exit_code: 125,
                    reason: format!("spawn: {error}"),
                    tree_stopped: true,
                },
            );
        }
    };
    // Once spawning was attempted, absence of a result must never imply cleanup.
    if let Err(error) = save(&dir.join("pid.json"), &child.id()) {
        let _ = child.kill();
        let _ = child.inner().wait();
        return Err(error);
    }
    if let Some(mut stdin) = child.inner().stdin.take() {
        let input_dir = dir.to_path_buf();
        thread::spawn(move || {
            let Ok(mut file) = File::open(input_dir.join("input")) else {
                return;
            };
            let mut buffer = [0; 8192];
            loop {
                match file.read(&mut buffer) {
                    Ok(0) if input_dir.join("input.eof").exists() => break,
                    Ok(0) => thread::sleep(Duration::from_millis(25)),
                    Ok(n) => {
                        if stdin.write_all(&buffer[..n]).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
                if input_dir.join("result.json").exists() {
                    break;
                }
            }
        });
    }
    let (code, reason) = loop {
        if rx.try_recv().is_ok() {
            break (130, "daemon disconnected".to_owned());
        }
        if dir.join("cancel").exists() {
            break (130, "cancelled".to_owned());
        }
        if now() >= spec.deadline {
            break (124, "timeout".to_owned());
        }
        match child.inner().try_wait() {
            Ok(Some(status)) => break (status.code().unwrap_or(128), "exited".to_owned()),
            Ok(None) => thread::sleep(Duration::from_millis(25)),
            Err(error) => break (125, format!("wait: {error}")),
        }
    };
    // Kill descendants even when the root script exited successfully.
    let killed = match child.kill() {
        Ok(()) => true,
        #[cfg(unix)]
        Err(error) if error.raw_os_error() == Some(3) => true, // ESRCH
        Err(_) => false,
    };
    let reaped = child.inner().wait().is_ok();
    #[cfg(unix)]
    let stopped = killed && reaped && group_gone(child.id());
    #[cfg(not(unix))]
    let stopped = killed && reaped;
    save(
        &dir.join("result.json"),
        &Outcome {
            exit_code: code,
            reason,
            tree_stopped: stopped,
        },
    )
}

#[cfg(unix)]
fn group_gone(pid: u32) -> bool {
    // Do not hand off until the process group is actually gone. Zombies may
    // cause a conservative quarantine, which is safer than overlapping tests.
    for _ in 0..100 {
        let result = unsafe { libc::kill(-(pid as i32), 0) };
        if result == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    false
}

pub fn stopped(dir: &Path) -> bool {
    if read::<Outcome>(&dir.join("result.json")).is_ok_and(|r| r.tree_stopped) {
        return true;
    }
    #[cfg(unix)]
    {
        // A killed supervisor cannot write a result. Never signal a persisted
        // PID (it may have been reused); only verify that its group is absent.
        if let Ok(lock) = File::options()
            .read(true)
            .write(true)
            .open(dir.join("runner.lock"))
            && lock.try_lock_exclusive().is_ok()
            && let Ok(pid) = read::<u32>(&dir.join("pid.json"))
            && pid > 1
            && group_gone(pid)
        {
            return save(
                &dir.join("result.json"),
                &Outcome {
                    exit_code: 125,
                    reason: "supervisor lost; process group absence verified".into(),
                    tree_stopped: true,
                },
            )
            .is_ok();
        }
    }
    false
}

pub fn cancel(dir: &Path) -> Result<()> {
    fs::write(dir.join("cancel"), b"cancel")?;
    Ok(())
}

pub fn bounded(
    home: &Path,
    program: &Path,
    args: Vec<String>,
    timeout_ms: u64,
) -> Result<(Outcome, String, String)> {
    let dir = home.join("operations").join(id());
    private_dir(&dir)?;
    let spec = Spec {
        program: program.into(),
        args,
        cwd: home.into(),
        env: Default::default(),
        stream_input: false,
        stdout: dir.join("stdout"),
        stderr: dir.join("stderr"),
        deadline: now() + timeout_ms,
    };
    let mut supervisor = launch(&dir, &spec)?;
    let end = now() + timeout_ms + 5_000;
    loop {
        if let Ok(result) = read::<Outcome>(&dir.join("result.json")) {
            let _ = supervisor.wait();
            let out = fs::read_to_string(&spec.stdout).unwrap_or_default();
            let err = fs::read_to_string(&spec.stderr).unwrap_or_default();
            if result.tree_stopped {
                let _ = fs::remove_dir_all(&dir);
            }
            return Ok((result, out, err));
        }
        if now() > end {
            drop(supervisor.stdin.take());
            anyhow::bail!("operation supervisor did not finish: {}", dir.display());
        }
        thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn lost_supervisor_does_not_imply_dead_process_group() {
        let dir = tempfile::tempdir().unwrap();
        File::create(dir.path().join("runner.lock")).unwrap();
        let mut child = Command::new("sleep").arg("30").group_spawn().unwrap();
        save(&dir.path().join("pid.json"), &child.id()).unwrap();
        let considered_stopped = stopped(dir.path());
        child.kill().unwrap();
        child.inner().wait().unwrap();
        assert!(!considered_stopped);
        assert!(stopped(dir.path()));
        assert_eq!(
            read::<Outcome>(&dir.path().join("result.json"))
                .unwrap()
                .exit_code,
            125
        );
    }
}
