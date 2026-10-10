use crate::{model::*, transport};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::PathBuf, process::Command};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Runtime {
    pub node: PathBuf,
    pub cli: PathBuf,
    pub adapter: PathBuf,
    pub env: BTreeMap<String, String>,
}

impl Runtime {
    pub fn discover(
        home: &std::path::Path,
        cli: Option<PathBuf>,
        node: Option<PathBuf>,
    ) -> Result<Self> {
        let node = transport::resolve(node.as_deref().and_then(|p| p.to_str()).unwrap_or("node"))
            .context("DevEco integration requires Node.js on PATH or --node")?;
        let cli = match cli {
            Some(path) => fs::canonicalize(path)?,
            None => {
                let mut found = None;
                for dir in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
                    for file in [
                        dir.join("devecocli"),
                        dir.join("node_modules/@deveco/deveco-cli/dist/cli.js"),
                    ] {
                        if file.is_file() {
                            let path = fs::canonicalize(file)?;
                            if path.extension().is_some_and(|e| e == "js") {
                                found = Some(path);
                                break;
                            }
                        }
                    }
                    if found.is_some() {
                        break;
                    }
                }
                found.context("DevEco CLI not found; install @deveco/deveco-cli or pass --cli /path/to/dist/cli.js")?
            }
        };
        let package = cli
            .parent()
            .and_then(|p| p.parent())
            .context("invalid DevEco CLI path")?
            .join("package.json");
        let meta: serde_json::Value = read(&package)?;
        ensure!(
            meta["name"] == "@deveco/deveco-cli" && meta["version"] == "1.2.1",
            "DevEco adapter currently supports @deveco/deveco-cli 1.2.1; verify a new version before enabling it"
        );
        let dir = home.join("deveco").join(id());
        private_dir(&dir)?;
        let adapter = dir.join("adapter.cjs");
        fs::write(&adapter, include_str!("deveco.cjs"))?;
        let mut env: BTreeMap<_, _> = std::env::vars()
            .filter(|(k, _)| {
                matches!(
                    k.as_str(),
                    "DEVECO_CLI_STUDIO_PATH"
                        | "DEVECO_CLI_CLT_PATH"
                        | "DEVECO_CLI_DATA_DIR"
                        | "DEVECO_CLI_AUTH_SOURCE"
                        | "DEVECO_CODE_AUTH_DIR"
                        | "JAVA_HOME"
                        | "PATH"
                )
            })
            .collect();
        env.insert("DEVECO_CLI_DISABLE_UPDATE".into(), "all".into());
        env.insert("DEVECO_CLI_DEBUG".into(), String::new());
        Ok(Self {
            node,
            cli,
            adapter,
            env,
        })
    }

    pub fn args(&self, args: Vec<String>) -> Vec<String> {
        let mut result = vec![
            "--require".into(),
            self.adapter.to_string_lossy().into(),
            self.cli.to_string_lossy().into(),
        ];
        result.extend(args);
        result
    }

    pub fn auth(&self, action: &str, browser: &str) -> Result<i32> {
        let browser = if browser == "auto" {
            if std::env::var_os("CODEX_THREAD_ID").is_some()
                || std::env::var_os("CODEX_SESSION_ID").is_some()
            {
                "codex"
            } else {
                "default"
            }
        } else {
            browser
        };
        let status = Command::new(&self.node)
            .args(self.args(vec!["auth".into(), action.into()]))
            .envs(&self.env)
            .env("OMH_DEVECO_BROWSER", browser)
            .env_remove("OMH_LEASE")
            .status()
            .context("start DevEco authentication")?;
        Ok(status.code().unwrap_or(125))
    }
}

// An intentionally narrow compatibility surface for signature generate 1.2.1.
pub fn device_command(args: &[String], device: &str) -> Result<Option<Vec<String>>> {
    if args == ["list", "targets"] {
        return Ok(None);
    }
    let args = if args.first().is_some_and(|a| a == "-c") {
        &args[1..]
    } else {
        args
    };
    ensure!(
        args.len() >= 3 && args[0] == "-t" && args[1] == device,
        "DevEco HDC target must match the active lease"
    );
    let command = &args[2..];
    ensure!(
        command == ["shell", "bm", "get", "-u"]
            || command == ["shell", "getprop", "hw_sc.build.os.deviceType"],
        "unsupported DevEco signing HDC command"
    );
    Ok(Some(command.to_vec()))
}
