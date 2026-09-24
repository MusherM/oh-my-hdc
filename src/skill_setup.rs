use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

pub const ASSETS: &[(&str, &str)] = include!(concat!(env!("OUT_DIR"), "/skills.rs"));
const STATE: &str = ".codex/omh-skills.json";
const BEGIN: &str = "<!-- omh:skills:start -->";
const END: &str = "<!-- omh:skills:end -->";
const ROUTING: &str = "<!-- omh:skills:start -->
## omh 项目技能路由

- 本项目鸿蒙应用开发、构建、部署及 UI 验证使用 [omh-harmonyos-app-dev](.agents/skills/omh-harmonyos-app-dev/SKILL.md)。
- 本项目真实功耗、耗电及内存测量使用 [omh-harmonyos-app-optimization](.agents/skills/omh-harmonyos-app-optimization/SKILL.md)。
- 所有设备操作遵循 [omh](.agents/skills/omh/SKILL.md) 的租约、托管任务和清理契约；不得直接执行其他 skill 或脚本中的原生 HDC 操作。接入异常先修复，不能绕过调度。
- 按任务读取上述项目版本，不依赖同名 skill 的覆盖优先级；普通 harmonyos-app-dev / harmonyos-app-optimization 不作为本项目的设备执行入口。纯本机构建不占用设备。
<!-- omh:skills:end -->";

#[derive(Default, Deserialize, Serialize)]
struct State {
    // Exact installed contents allow upgrades without overwriting user edits.
    files: BTreeMap<String, String>,
    routing: String,
}

pub struct Plan {
    writes: Vec<(PathBuf, String)>,
    link_agents: bool,
    project: PathBuf,
}

// Never follow project symlinks into a user's global skill/config directory.
pub fn check_path(project: &Path, relative: &str) -> Result<()> {
    let mut path = project.to_path_buf();
    let parts: Vec<_> = Path::new(relative).components().collect();
    for (index, part) in parts.iter().enumerate() {
        ensure!(
            matches!(part, std::path::Component::Normal(_)),
            "invalid setup path"
        );
        path.push(part.as_os_str());
        match fs::symlink_metadata(&path) {
            Ok(meta) => {
                ensure!(
                    !meta.file_type().is_symlink(),
                    "setup refuses symlink: {}",
                    path.display()
                );
                ensure!(
                    if index + 1 == parts.len() {
                        meta.is_file()
                    } else {
                        meta.is_dir()
                    },
                    "unexpected setup path type: {}",
                    path.display()
                );
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

pub fn prepare(project: &Path) -> Result<Plan> {
    check_path(project, STATE)?;
    check_path(project, "AGENTS.md")?;
    check_path(project, "CLAUDE.md")?;
    let previous: State = match fs::read_to_string(project.join(STATE)) {
        Ok(text) => serde_json::from_str(&text)
            .context("invalid omh skill install record; preserve it and resolve before setup")?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => State::default(),
        Err(e) => return Err(e.into()),
    };
    let mut writes = Vec::new();
    let mut next = State {
        routing: ROUTING.into(),
        ..State::default()
    };
    for &(relative, content) in ASSETS {
        let target = format!(".agents/skills/{relative}");
        check_path(project, &target)?;
        let path = project.join(&target);
        if path.exists() {
            let current = fs::read_to_string(&path)?;
            ensure!(
                current == content || previous.files.get(relative) == Some(&current),
                "existing skill differs; preserve it and update explicitly: {}",
                path.display()
            );
            if current != content {
                writes.push((path, content.into()));
            }
        } else {
            writes.push((path, content.into()));
        }
        next.files.insert(relative.into(), content.into());
    }
    let link_agents = !project.join("AGENTS.md").exists();
    let instructions = project.join(if link_agents {
        "CLAUDE.md"
    } else {
        "AGENTS.md"
    });
    let original = if instructions.exists() {
        fs::read_to_string(&instructions)?
    } else {
        String::new()
    };
    let updated = merge_routing(&original, &previous.routing)?;
    if updated != original {
        writes.push((instructions, updated));
    }
    let record = serde_json::to_string_pretty(&next)? + "\n";
    if fs::read_to_string(project.join(STATE)).ok().as_deref() != Some(&record) {
        writes.push((project.join(STATE), record));
    }
    Ok(Plan {
        writes,
        link_agents,
        project: project.to_owned(),
    })
}

fn merge_routing(original: &str, previous: &str) -> Result<String> {
    let starts: Vec<_> = original.match_indices(BEGIN).collect();
    let ends: Vec<_> = original.match_indices(END).collect();
    match (starts.as_slice(), ends.as_slice()) {
        ([], []) => Ok(format!(
            "{original}{}{ROUTING}\n",
            if original.is_empty() {
                ""
            } else if original.ends_with('\n') {
                "\n"
            } else {
                "\n\n"
            }
        )),
        ([(start, _)], [(end, _)]) if start < end => {
            let end = end + END.len();
            let current = &original[*start..end];
            ensure!(
                current == ROUTING || current == previous,
                "omh routing block was modified; preserve it and update explicitly"
            );
            Ok(format!(
                "{}{ROUTING}{}",
                &original[..*start],
                &original[end..]
            ))
        }
        _ => bail!("malformed or duplicate omh routing markers; resolve before setup"),
    }
}

impl Plan {
    pub fn apply(self) -> Result<()> {
        for (path, content) in self.writes {
            fs::create_dir_all(path.parent().unwrap())?;
            // In-place writes preserve existing AGENTS/CLAUDE hard links.
            fs::write(path, content)?;
        }
        if self.link_agents {
            fs::hard_link(
                self.project.join("CLAUDE.md"),
                self.project.join("AGENTS.md"),
            )?;
        }
        Ok(())
    }
}

pub fn complete(project: &Path) -> bool {
    ASSETS.iter().all(|(path, content)| {
        fs::read_to_string(project.join(".agents/skills").join(path))
            .ok()
            .as_deref()
            == Some(content)
    }) && fs::read_to_string(project.join("AGENTS.md")).is_ok_and(|s| s.contains(ROUTING))
}
