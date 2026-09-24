use crate::model::*;
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    net::TcpStream,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::Duration,
};

pub fn rpc(home: &Path, request: Request) -> Result<Value> {
    let endpoint: Endpoint = read(&home.join("endpoint.json"))?;
    let mut stream =
        TcpStream::connect_timeout(&endpoint.address.parse()?, Duration::from_secs(2))?;
    stream.set_read_timeout(Some(Duration::from_secs(15)))?;
    stream.set_write_timeout(Some(Duration::from_secs(15)))?;
    serde_json::to_writer(
        &mut stream,
        &Envelope {
            secret: endpoint.secret,
            request,
        },
    )?;
    stream.write_all(b"\n")?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    let value: Value = serde_json::from_str(&line).context("invalid daemon response")?;
    if let Some(error) = value.get("error") {
        bail!("{}", error.as_str().unwrap_or("daemon error"));
    }
    Ok(value)
}

pub fn ensure(home: &Path, hdc: Option<&Path>) -> Result<()> {
    private_dir(home)?;
    if let Ok(value) = rpc(home, Request::Ping) {
        if let Some(hdc) = hdc {
            let wanted = fs::canonicalize(hdc)?;
            if value["hdc"].as_str() != wanted.to_str() {
                bail!("daemon uses a different hdc; drain and stop it before changing SDKs");
            }
        }
        return Ok(());
    }
    let log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(home.join("daemon.log"))?;
    let mut command = Command::new(std::env::current_exe()?);
    command.arg("--home").arg(home);
    if let Some(hdc) = hdc {
        command.arg("--hdc").arg(hdc);
    }
    command
        .arg("__daemon")
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let mut child = command.spawn().context("start omh daemon")?;
    for _ in 0..100 {
        if rpc(home, Request::Ping).is_ok() {
            return Ok(());
        }
        if let Some(status) = child.try_wait()?
            && !status.success()
        {
            bail!(
                "daemon startup failed; inspect {}",
                home.join("daemon.log").display()
            );
        }
        thread::sleep(Duration::from_millis(100));
    }
    bail!(
        "daemon did not become ready; inspect {}",
        home.join("daemon.log").display()
    )
}

pub fn resolve(program: &str) -> Result<std::path::PathBuf> {
    let path = Path::new(program);
    if path.components().count() > 1 || path.is_absolute() {
        return fs::canonicalize(path).context("executable not found");
    }
    for dir in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        let candidate = dir.join(program);
        if candidate.is_file() {
            return Ok(fs::canonicalize(candidate)?);
        }
        #[cfg(windows)]
        {
            let candidate = dir.join(format!("{program}.exe"));
            if candidate.is_file() {
                return Ok(fs::canonicalize(candidate)?);
            }
        }
    }
    bail!("executable {program:?} not found; install hdc or pass --hdc /path/to/hdc")
}
