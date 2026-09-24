use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
};

// A deliberately conservative guardrail, not a shell interpreter or sandbox.
// Covers direct commands, paths, common wrappers and inline shell commands.
pub fn blocked(command: &str) -> bool {
    let mut tokens = Vec::new();
    let mut token = String::new();
    let mut quote = None;
    for c in command.chars() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            } else {
                token.push(c);
            }
        } else if c == '\'' || c == '"' {
            quote = Some(c);
        } else if c.is_whitespace() || ";|&()\n".contains(c) {
            if !token.is_empty() {
                tokens.push(std::mem::take(&mut token));
            }
            if ";|&()\n".contains(c) {
                tokens.push(";".into());
            }
        } else {
            token.push(c);
        }
    }
    if !token.is_empty() {
        tokens.push(token);
    }
    let mut head = true;
    let mut shell = false;
    for (i, token) in tokens.iter().enumerate() {
        if token == ";" {
            head = true;
            shell = false;
            continue;
        }
        let base = token
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(token)
            .to_ascii_lowercase();
        if head {
            if token.contains('=')
                || matches!(
                    base.as_str(),
                    "env" | "command" | "exec" | "sudo" | "nohup" | "call" | "&"
                )
                || token.starts_with('-')
            {
                continue;
            }
            if matches!(base.as_str(), "hdc" | "hdc.exe") {
                return true;
            }
            shell = matches!(
                base.as_str(),
                "sh" | "bash"
                    | "zsh"
                    | "cmd"
                    | "cmd.exe"
                    | "pwsh"
                    | "powershell"
                    | "powershell.exe"
            );
            head = false;
        } else if shell
            && matches!(
                token.to_ascii_lowercase().as_str(),
                "-c" | "-lc" | "/c" | "-command"
            )
        {
            return blocked(&tokens[i + 1..].join(" "));
        }
        // Catch command substitutions even when embedded in quoted arguments.
        if let Some((_, tail)) = token.split_once("$(")
            && blocked(tail.trim_end_matches(')'))
        {
            return true;
        }
        if let Some((_, tail)) = token.split_once('`')
            && blocked(tail.trim_end_matches('`'))
        {
            return true;
        }
    }
    false
}

pub fn hook() -> Result<()> {
    let mut input = String::new();
    std::io::stdin()
        .take(1_048_576)
        .read_to_string(&mut input)?;
    let value: Value = serde_json::from_str(&input)?;
    let command = value
        .pointer("/tool_input/command")
        .or_else(|| value.pointer("/tool_input/cmd"))
        .and_then(Value::as_str)
        .unwrap_or("");
    if blocked(command) {
        println!(
            "{}",
            json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"Use omh acquire / omh exec / omh logs. Direct hdc bypasses shared-device scheduling."}})
        );
    } else {
        println!("{{}}");
    }
    Ok(())
}

pub fn setup(project: &Path) -> Result<Value> {
    let project = fs::canonicalize(project)?;
    let skills = crate::skill_setup::prepare(&project)?;
    crate::skill_setup::check_path(&project, ".codex/config.toml")?;
    let config_dir = project.join(".codex");
    let path = config_dir.join("config.toml");
    let original = if path.exists() {
        fs::read_to_string(&path)?
    } else {
        String::new()
    };
    let mut doc = original.parse::<toml_edit::DocumentMut>()?;
    let binary = std::env::current_exe()?.canonicalize()?;
    // JSON double quoting works for the native Windows command line; Unix uses
    // single-quote escaping so paths containing $, backticks or spaces stay literal.
    #[cfg(unix)]
    let command = format!("'{}' hook", binary.to_string_lossy().replace('\'', "'\\''"));
    #[cfg(windows)]
    let command = format!("\"{}\" hook", binary.display());
    if doc.get("hooks").is_none() {
        doc["hooks"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    if doc["hooks"].get("PreToolUse").is_none() {
        doc["hooks"]["PreToolUse"] =
            toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new());
    }
    let groups = doc["hooks"]["PreToolUse"]
        .as_array_of_tables_mut()
        .context("existing hooks.PreToolUse is not an array of tables; merge manually")?;
    const MARKER: &str = "omh: checking shared-device access";
    for group in groups.iter_mut() {
        if let Some(handlers) = group
            .get_mut("hooks")
            .and_then(|x| x.as_array_of_tables_mut())
        {
            for handler in handlers.iter_mut() {
                if handler.get("statusMessage").and_then(|x| x.as_str()) == Some(MARKER) {
                    handler["command"] = toml_edit::value(command.clone());
                }
            }
        }
    }
    if !groups.iter().any(|g| {
        g.get("hooks")
            .and_then(|x| x.as_array_of_tables())
            .is_some_and(|handlers| {
                handlers
                    .iter()
                    .any(|h| h.get("command").and_then(|x| x.as_str()) == Some(&command))
            })
    }) {
        let mut group = toml_edit::Table::new();
        group["matcher"] = toml_edit::value("^Bash$");
        let mut handler = toml_edit::Table::new();
        handler["type"] = toml_edit::value("command");
        handler["command"] = toml_edit::value(command);
        handler["timeout"] = toml_edit::value(5);
        handler["statusMessage"] = toml_edit::value(MARKER);
        let mut handlers = toml_edit::ArrayOfTables::new();
        handlers.push(handler);
        group["hooks"] = toml_edit::Item::ArrayOfTables(handlers);
        groups.push(group);
    }
    doc["features"]["hooks"] = toml_edit::value(true);
    // Validate all skill, routing and TOML conflicts before changing any files.
    skills.apply()?;
    fs::create_dir_all(&config_dir)?;
    if !original.is_empty() && original != doc.to_string() {
        let backup = config_dir.join(format!("config.toml.{}.bak", crate::model::id()));
        fs::write(backup, &original)?;
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)?;
    file.write_all(doc.to_string().as_bytes())?;
    Ok(
        json!({"installed":true,"skills":["omh","omh-harmonyos-app-dev","omh-harmonyos-app-optimization"],"protected":false,"project":project,"next":"Restart Codex in this trusted project, review/trust the hook with /hooks, then ask it to run hdc --version. The tool call must be DENIED before hdc executes. Run omh --version to verify the allowed path. Installation alone does not prove protection."}),
    )
}

pub fn doctor(project: &Path) -> Value {
    let config = fs::read_to_string(project.join(".codex/config.toml")).unwrap_or_default();
    json!({"skill_installed":project.join(".agents/skills/omh/SKILL.md").exists(),"skill_bundle_current":crate::skill_setup::complete(project),"hook_config_present":config.contains("PreToolUse") && config.contains(" hook"),"protected":false,"runtime_verification":"REQUIRED: from the actual Codex task, hdc --version must be denied and omh --version allowed. A standalone CLI cannot attest that the caller actually runs trusted hooks."})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn doctor_checks_the_full_bundle_without_claiming_runtime_protection() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(doctor(dir.path())["skill_bundle_current"], false);
        setup(dir.path()).unwrap();
        assert_eq!(doctor(dir.path())["skill_bundle_current"], true);
        assert_eq!(doctor(dir.path())["protected"], false);
        fs::remove_file(
            dir.path().join(
                ".agents/skills/omh-harmonyos-app-optimization/references/omh-measurement.md",
            ),
        )
        .unwrap();
        assert_eq!(doctor(dir.path())["skill_bundle_current"], false);
        setup(dir.path()).unwrap();
        fs::write(dir.path().join("AGENTS.md"), "routing removed").unwrap();
        assert_eq!(doctor(dir.path())["skill_bundle_current"], false);
    }

    #[test]
    fn direct_and_wrapped_commands() {
        for command in [
            "hdc shell ls",
            "hdc.exe list targets",
            "/sdk/bin/hdc shell ls",
            r#"& "C:\Program Files\sdk\hdc.exe" shell ls"#,
            "echo ok | hdc shell cat",
            "cd /tmp && env X=1 hdc list targets",
            "bash -lc 'hdc shell ls'",
            "echo $(hdc list targets)",
        ] {
            assert!(blocked(command), "{command}");
        }
        for command in [
            "omh exec -- shell ls",
            "echo hdc",
            "cat README.md",
            "omh --version",
        ] {
            assert!(!blocked(command), "{command}");
        }
    }
}
