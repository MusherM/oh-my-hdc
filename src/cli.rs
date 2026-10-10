use crate::{guard, model::*, process, server, transport};
use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use serde_json::{Value, json};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::PathBuf,
    thread,
    time::Duration,
};

#[derive(Parser)]
#[command(
    version,
    about = "Exclusive HarmonyOS device sessions for coding agents"
)]
struct Cli {
    #[arg(long, env = "OMH_HOME", global = true)]
    home: Option<PathBuf>,
    #[arg(long, env = "OMH_HDC", global = true)]
    hdc: Option<PathBuf>,
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Cmd,
}
#[derive(Subcommand)]
enum Cmd {
    /// DevEco authentication and lease-scoped automatic signing (CLI 1.2.1).
    Deveco {
        #[arg(long)]
        cli: Option<PathBuf>,
        #[arg(long)]
        node: Option<PathBuf>,
        #[command(subcommand)]
        command: Deveco,
    },
    #[command(name = "__deveco-hdc", hide = true)]
    DevecoHdc {
        #[arg(long, env = "OMH_LEASE")]
        lease: String,
        #[arg(last = true, required = true)]
        args: Vec<String>,
    },
    Devices,
    Status,
    Acquire {
        #[arg(long)]
        device: Option<String>,
        #[arg(long = "package", required = true)]
        packages: Vec<String>,
        #[arg(long,default_value="10m",value_parser=duration)]
        wait: u64,
    },
    Exec {
        #[arg(long, env = "OMH_LEASE")]
        lease: String,
        #[arg(last = true, required = true)]
        args: Vec<String>,
    },
    Logs {
        #[arg(long, env = "OMH_LEASE")]
        lease: String,
        #[arg(long)]
        output: PathBuf,
    },
    Job {
        #[command(subcommand)]
        command: Job,
    },
    Release {
        #[arg(long, env = "OMH_LEASE")]
        lease: String,
    },
    Recover {
        #[arg(long)]
        device: String,
    },
    Maintenance {
        #[command(subcommand)]
        command: Maintenance,
    },
    Doctor {
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    Setup {
        #[command(subcommand)]
        command: Setup,
    },
    #[command(hide = true)]
    Hook,
    #[command(name = "__daemon", hide = true)]
    Daemon,
    #[command(name = "__stop", hide = true)]
    Stop,
    #[command(name = "__run", hide = true)]
    Run {
        directory: PathBuf,
    },
}
#[derive(Subcommand)]
enum Deveco {
    Auth {
        #[arg(value_parser = ["login", "status"])]
        action: String,
        #[arg(long, default_value = "auto", value_parser = ["auto", "codex", "default"])]
        browser: String,
    },
    Signature {
        #[command(subcommand)]
        command: Signature,
    },
}
#[derive(Subcommand)]
enum Signature {
    Generate {
        #[arg(long, env = "OMH_LEASE")]
        lease: String,
        #[arg(long, default_value = "default")]
        product: String,
        #[arg(long)]
        team_id: Option<String>,
        #[arg(long)]
        force: bool,
        #[arg(long, default_value = "10m", value_parser = duration)]
        timeout: u64,
    },
}
#[derive(Subcommand)]
enum Job {
    Start {
        #[arg(long, env = "OMH_LEASE")]
        lease: String,
        #[arg(long,value_parser=duration)]
        timeout: u64,
        #[arg(last = true, required = true)]
        args: Vec<String>,
    },
    Status {
        run: String,
    },
    Cancel {
        run: String,
    },
}
#[derive(Subcommand)]
enum Maintenance {
    Restart,
}
#[derive(Subcommand)]
enum Setup {
    Codex {
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
}

fn duration(text: &str) -> std::result::Result<u64, String> {
    let (digits, factor) = if let Some(v) = text.strip_suffix("ms") {
        (v, 1)
    } else if let Some(v) = text.strip_suffix('s') {
        (v, 1000)
    } else if let Some(v) = text.strip_suffix('m') {
        (v, 60_000)
    } else if let Some(v) = text.strip_suffix('h') {
        (v, 3_600_000)
    } else {
        (text, 1000)
    };
    digits
        .parse::<u64>()
        .ok()
        .and_then(|v| v.checked_mul(factor))
        .filter(|v| *v > 0 && *v <= 604_800_000)
        .ok_or_else(|| "use a positive duration up to 7 days, e.g. 30s, 10m, 90m".into())
}
fn absolute(path: PathBuf) -> Result<PathBuf> {
    Ok(if path.is_absolute() {
        path
    } else {
        std::env::current_dir()?.join(path)
    })
}
fn print(value: &Value, json: bool) -> Result<()> {
    // Human output remains structured to keep lease tokens and paths unambiguous.
    let text = if json {
        serde_json::to_string(value)?
    } else {
        serde_json::to_string_pretty(value)?
    };
    println!("{text}");
    Ok(())
}
fn tail(path: &str, offset: &mut u64, writer: &mut impl Write) -> Result<()> {
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(*offset))?;
    let mut bytes = [0; 8192];
    loop {
        let n = file.read(&mut bytes)?;
        if n == 0 {
            break;
        }
        writer.write_all(&bytes[..n])?;
        *offset += n as u64;
    }
    writer.flush()?;
    Ok(())
}
fn execute(home: PathBuf, value: Value, wait_release: bool) -> Result<()> {
    let run = value["run"].as_str().context("missing run id")?.to_owned();
    if wait_release {
        eprintln!("omh signing job {run}; inspect/cancel with omh job status/cancel {run}");
    }
    let input_home = home.clone();
    let input_run = run.clone();
    thread::spawn(move || {
        let mut buffer = [0; 8192];
        let mut stdin = std::io::stdin().lock();
        while let Ok(n) = stdin.read(&mut buffer) {
            if transport::rpc(
                &input_home,
                Request::Input {
                    run: input_run.clone(),
                    bytes: buffer[..n].to_vec(),
                    eof: n == 0,
                },
            )
            .is_err()
                || n == 0
            {
                break;
            }
        }
    });
    let (mut out_offset, mut err_offset) = (0, 0);
    loop {
        tail(
            value["stdout"].as_str().unwrap(),
            &mut out_offset,
            &mut std::io::stdout(),
        )?;
        tail(
            value["stderr"].as_str().unwrap(),
            &mut err_offset,
            &mut std::io::stderr(),
        )?;
        let status = transport::rpc(&home, Request::RunStatus { run: run.clone() })?;
        if !status["result"].is_null() {
            tail(
                value["stdout"].as_str().unwrap(),
                &mut out_offset,
                &mut std::io::stdout(),
            )?;
            tail(
                value["stderr"].as_str().unwrap(),
                &mut err_offset,
                &mut std::io::stderr(),
            )?;
            let result: Outcome = serde_json::from_value(status["result"].clone())?;
            if wait_release {
                if status["lease_state"] == "blocked" {
                    bail!(
                        "signing process exited {}; cleanup failed: {}",
                        result.exit_code,
                        status["cleanup_error"]
                    );
                }
                if status["lease_state"] != "released" {
                    thread::sleep(Duration::from_millis(100));
                    continue;
                }
            }
            if result.reason != "exited" || !result.tree_stopped {
                eprintln!(
                    "omh run {run}: {} (tree_stopped={})",
                    result.reason, result.tree_stopped
                );
            }
            std::process::exit(if result.tree_stopped {
                result.exit_code
            } else {
                125
            });
        }
        thread::sleep(Duration::from_millis(100));
    }
}

pub fn main() -> Result<()> {
    let cli = Cli::parse();
    match &cli.command {
        Cmd::Hook => return guard::hook(),
        Cmd::Run { directory } => return process::supervise(directory),
        Cmd::Setup {
            command: Setup::Codex { project },
        } => return print(&guard::setup(project)?, cli.json),
        _ => {}
    }
    let default = directories::ProjectDirs::from("", "", "omh")
        .context("user data directory unavailable")?
        .data_local_dir()
        .to_path_buf();
    let home = absolute(cli.home.unwrap_or(default))?;
    if let Cmd::Deveco {
        cli,
        node,
        command: Deveco::Auth { action, browser },
    } = &cli.command
    {
        let runtime = crate::deveco::Runtime::discover(&home, cli.clone(), node.clone())?;
        std::process::exit(runtime.auth(action, browser)?);
    }
    if matches!(cli.command, Cmd::Daemon) {
        return server::serve(home, cli.hdc);
    }
    transport::ensure(&home, cli.hdc.as_deref())?;
    let value = match cli.command {
        Cmd::Deveco {
            cli,
            node,
            command:
                Deveco::Signature {
                    command:
                        Signature::Generate {
                            lease,
                            product,
                            team_id,
                            force,
                            timeout,
                        },
                },
        } => {
            let runtime = crate::deveco::Runtime::discover(&home, cli, node)?;
            let mut args = vec![
                "signature".into(),
                "generate".into(),
                "--product".into(),
                product,
            ];
            if let Some(team) = team_id {
                args.extend(["--team-id".into(), team]);
            }
            if force {
                args.push("--force".into());
            }
            return execute(
                home.clone(),
                transport::rpc(
                    &home,
                    Request::Deveco {
                        lease,
                        runtime,
                        args,
                        cwd: std::env::current_dir()?,
                        timeout_ms: timeout,
                    },
                )?,
                true,
            );
        }
        Cmd::DevecoHdc { lease, args } => {
            let value = transport::rpc(
                &home,
                Request::DevecoHdc {
                    lease,
                    args,
                    cwd: std::env::current_dir()?,
                },
            )?;
            if let Some(targets) = value["targets"].as_str() {
                print!("{targets}");
                return Ok(());
            }
            return execute(home, value, false);
        }
        Cmd::Devices | Cmd::Status => transport::rpc(&home, Request::Status)?,
        Cmd::Stop => transport::rpc(&home, Request::Stop)?,
        Cmd::Acquire {
            device,
            packages,
            wait,
        } => {
            let ticket = id();
            let end = now() + wait;
            loop {
                let response = transport::rpc(
                    &home,
                    Request::Acquire {
                        ticket: ticket.clone(),
                        device: device.clone(),
                        packages: packages.clone(),
                        wait_ms: wait,
                    },
                )?;
                if response.get("lease").is_some() {
                    transport::rpc(&home, Request::CancelTicket { ticket })?;
                    break response;
                }
                if now() >= end {
                    transport::rpc(&home, Request::CancelTicket { ticket })?;
                    bail!("device acquisition timed out; no lease granted");
                }
                thread::sleep(Duration::from_millis(200));
            }
        }
        Cmd::Exec { lease, args } => {
            return execute(
                home.clone(),
                transport::rpc(
                    &home,
                    Request::Exec {
                        lease,
                        args,
                        cwd: std::env::current_dir()?,
                    },
                )?,
                false,
            );
        }
        Cmd::Logs { lease, output } => transport::rpc(
            &home,
            Request::Logs {
                lease,
                output: absolute(output)?,
            },
        )?,
        Cmd::Job { command } => match command {
            Job::Start {
                lease,
                args,
                timeout,
            } => transport::rpc(
                &home,
                Request::Job {
                    lease,
                    args,
                    cwd: std::env::current_dir()?,
                    timeout_ms: timeout,
                },
            )?,
            Job::Status { run } => transport::rpc(&home, Request::RunStatus { run })?,
            Job::Cancel { run } => transport::rpc(&home, Request::Cancel { run })?,
        },
        Cmd::Release { lease } => {
            let response = transport::rpc(
                &home,
                Request::Release {
                    lease: lease.clone(),
                },
            )?;
            let device = response["device"].as_str().unwrap();
            loop {
                let state = transport::rpc(
                    &home,
                    Request::Release {
                        lease: lease.clone(),
                    },
                )?;
                if state["state"] == "blocked" {
                    bail!("cleanup failed: {}", state["cleanup_error"]);
                }
                if state["state"] == "released" {
                    break json!({"device":device,"released":true});
                }
                thread::sleep(Duration::from_millis(100));
            }
        }
        Cmd::Recover { device } => transport::rpc(&home, Request::Recover { device })?,
        Cmd::Maintenance {
            command: Maintenance::Restart,
        } => transport::rpc(&home, Request::Restart)?,
        Cmd::Doctor { project } => {
            json!({"daemon":transport::rpc(&home,Request::Ping)?,"codex":guard::doctor(&project),"home":home})
        }
        _ => unreachable!(),
    };
    print(&value, cli.json)
}
