use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const IDLE_MS: u64 = 600_000;
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
pub fn save<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let tmp = path.with_extension(format!("{}.tmp", id()));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        use std::io::Write;
        file.write_all(&serde_json::to_vec_pretty(value)?)?;
        file.sync_all()?;
        fs::rename(&tmp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(tmp);
    }
    result
}
pub fn read<T: for<'a> Deserialize<'a>>(path: &Path) -> Result<T> {
    serde_json::from_slice(&fs::read(path).with_context(|| format!("read {}", path.display()))?)
        .map_err(Into::into)
}
pub fn private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct DeviceInfo {
    pub name: Option<String>,
    pub model: Option<String>,
    pub chip: Option<String>,
    pub brand: Option<String>,
    pub os_version: Option<String>,
    pub api_version: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Device {
    pub online: bool,
    pub state: String,
    pub reason: Option<String>,
    pub packages: Vec<String>,
    pub forwards: Vec<String>,
    pub runs: Vec<String>,
    #[serde(default)]
    pub info: Option<DeviceInfo>,
}
impl Device {
    pub fn new() -> Self {
        Self {
            online: true,
            state: "free".into(),
            reason: None,
            packages: vec![],
            forwards: vec![],
            runs: vec![],
            info: None,
        }
    }
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Lease {
    pub token: String,
    pub device: String,
    pub packages: Vec<String>,
    pub touched: u64,
    pub state: String,
    pub job: Option<String>,
    #[serde(default)]
    pub cleanup_error: Option<String>,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Run {
    pub id: String,
    pub lease: String,
    pub kind: String,
    pub dir: PathBuf,
    pub deadline: u64,
    pub handled: bool,
}
#[derive(Clone, Serialize, Deserialize, Debug, Default)]
pub struct State {
    pub devices: BTreeMap<String, Device>,
    pub leases: BTreeMap<String, Lease>,
    pub runs: BTreeMap<String, Run>,
    pub maintenance: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Spec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
    pub stream_input: bool,
    pub stdout: PathBuf,
    pub stderr: PathBuf,
    pub deadline: u64,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Outcome {
    pub exit_code: i32,
    pub reason: String,
    pub tree_stopped: bool,
}
#[derive(Serialize, Deserialize)]
pub struct Endpoint {
    pub address: String,
    pub secret: String,
}
#[derive(Serialize, Deserialize)]
pub struct Envelope {
    pub secret: String,
    pub request: Request,
}
#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    Deveco {
        lease: String,
        runtime: crate::deveco::Runtime,
        args: Vec<String>,
        cwd: PathBuf,
        timeout_ms: u64,
    },
    DevecoHdc {
        lease: String,
        args: Vec<String>,
        cwd: PathBuf,
    },
    Ping,
    Status,
    Acquire {
        ticket: String,
        device: Option<String>,
        packages: Vec<String>,
        wait_ms: u64,
    },
    CancelTicket {
        ticket: String,
    },
    Exec {
        lease: String,
        args: Vec<String>,
        cwd: PathBuf,
    },
    Logs {
        lease: String,
        output: PathBuf,
    },
    Job {
        lease: String,
        args: Vec<String>,
        cwd: PathBuf,
        timeout_ms: u64,
    },
    RunStatus {
        run: String,
    },
    Input {
        run: String,
        bytes: Vec<u8>,
        eof: bool,
    },
    Cancel {
        run: String,
    },
    Release {
        lease: String,
    },
    Recover {
        device: String,
    },
    Restart,
    Stop,
}
