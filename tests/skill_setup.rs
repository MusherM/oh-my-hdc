use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    process::{Command, Output},
};

fn setup(project: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_omh"))
        .args(["setup", "codex", "--project"])
        .arg(project)
        .current_dir(project)
        .output()
        .unwrap()
}

fn install(project: &Path) {
    let out = setup(project);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["skills"].as_array().unwrap().len(), 3);
    assert_eq!(value["protected"], false);
}

fn snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn visit(root: &Path, dir: &Path, result: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(root, &path, result);
            } else {
                result.insert(
                    path.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut result = BTreeMap::new();
    visit(root, root, &mut result);
    result
}

#[test]
fn installs_complete_embedded_bundle_and_routes_without_user_skills() {
    let dir = tempfile::tempdir().unwrap();
    install(dir.path());
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("skills");
    assert_eq!(
        snapshot(&source),
        snapshot(&dir.path().join(".agents/skills"))
    );
    let agents = fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
    assert!(agents.contains("omh-harmonyos-app-dev/SKILL.md"));
    assert!(agents.contains("omh-harmonyos-app-optimization/SKILL.md"));
    assert_eq!(
        agents,
        fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap()
    );
    let before = snapshot(dir.path());
    install(dir.path());
    assert_eq!(before, snapshot(dir.path()));
}

#[test]
fn adopts_legacy_skill_and_preserves_independent_instructions_and_config() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".agents/skills/omh")).unwrap();
    fs::write(
        dir.path().join(".agents/skills/omh/SKILL.md"),
        include_str!("../skills/omh/SKILL.md"),
    )
    .unwrap();
    fs::create_dir(dir.path().join(".codex")).unwrap();
    let config = "model = 'keep-me'\n[[hooks.PreToolUse]]\nmatcher = 'keep'\n[[hooks.PreToolUse.hooks]]\ntype = 'command'\ncommand = 'echo keep'\n";
    fs::write(dir.path().join(".codex/config.toml"), config).unwrap();
    fs::write(
        dir.path().join("AGENTS.md"),
        "project rules without newline",
    )
    .unwrap();
    fs::write(dir.path().join("CLAUDE.md"), "independent rules\n").unwrap();
    install(dir.path());
    assert!(
        fs::read_to_string(dir.path().join("AGENTS.md"))
            .unwrap()
            .starts_with("project rules without newline\n\n")
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap(),
        "independent rules\n"
    );
    let updated = fs::read_to_string(dir.path().join(".codex/config.toml")).unwrap();
    assert!(updated.contains("echo keep"));
    assert!(updated.contains("keep-me"));
    assert!(fs::read_dir(dir.path().join(".codex")).unwrap().any(|e| {
        let path = e.unwrap().path();
        path.extension().is_some_and(|e| e == "bak") && fs::read_to_string(path).unwrap() == config
    }));
    install(dir.path());
}

#[test]
fn conflicts_are_reported_before_any_project_changes() {
    for conflict in ["skill", "reference", "routing", "toml", "record", "markers"] {
        let dir = tempfile::tempdir().unwrap();
        install(dir.path());
        match conflict {
            "skill" => fs::write(
                dir.path()
                    .join(".agents/skills/omh-harmonyos-app-dev/SKILL.md"),
                "user skill",
            )
            .unwrap(),
            "reference" => fs::write(
                dir.path()
                    .join(".agents/skills/omh-harmonyos-app-optimization/references/power.md"),
                "user reference",
            )
            .unwrap(),
            "routing" => {
                let path = dir.path().join("AGENTS.md");
                let text = fs::read_to_string(&path)
                    .unwrap()
                    .replace("纯本机构建不占用设备", "user change");
                fs::write(path, text).unwrap();
            }
            "toml" => fs::write(dir.path().join(".codex/config.toml"), "[invalid").unwrap(),
            "record" => fs::write(dir.path().join(".codex/omh-skills.json"), "invalid").unwrap(),
            "markers" => {
                fs::write(dir.path().join("AGENTS.md"), "<!-- omh:skills:start -->").unwrap()
            }
            _ => unreachable!(),
        }
        // A missing resource would normally be repaired; conflicts must prevent it.
        fs::remove_file(
            dir.path()
                .join(".agents/skills/omh-harmonyos-app-dev/references/sources.md"),
        )
        .unwrap();
        let before = snapshot(dir.path());
        assert!(!setup(dir.path()).status.success(), "{conflict}");
        assert_eq!(before, snapshot(dir.path()), "{conflict}");
    }
}

#[test]
fn untracked_existing_skill_is_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir
        .path()
        .join(".agents/skills/omh-harmonyos-app-optimization/references");
    fs::create_dir_all(&path).unwrap();
    fs::write(path.join("power.md"), "existing user content").unwrap();
    let before = snapshot(dir.path());
    assert!(!setup(dir.path()).status.success());
    assert_eq!(before, snapshot(dir.path()));
}

#[test]
fn upgrades_owned_files_and_repairs_missing_resources_without_touching_other_rules() {
    let dir = tempfile::tempdir().unwrap();
    install(dir.path());
    let record = dir.path().join(".codex/omh-skills.json");
    let mut previous: Value = serde_json::from_str(&fs::read_to_string(&record).unwrap()).unwrap();
    let relative = "omh-harmonyos-app-dev/references/build-deploy.md";
    previous["files"][relative] = Value::String("previous release contents".into());
    fs::write(
        dir.path().join(".agents/skills").join(relative),
        "previous release contents",
    )
    .unwrap();
    let old_route = "<!-- omh:skills:start -->\nold release routing\n<!-- omh:skills:end -->";
    previous["routing"] = Value::String(old_route.into());
    fs::write(
        dir.path().join("AGENTS.md"),
        format!("user prefix\n{old_route}\nuser suffix"),
    )
    .unwrap();
    fs::write(record, serde_json::to_string(&previous).unwrap()).unwrap();
    fs::remove_file(
        dir.path()
            .join(".agents/skills/omh-harmonyos-app-dev/scripts/ui_tree_inspect.py"),
    )
    .unwrap();
    install(dir.path());
    assert_eq!(
        snapshot(&Path::new(env!("CARGO_MANIFEST_DIR")).join("skills")),
        snapshot(&dir.path().join(".agents/skills"))
    );
    let agents = fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
    assert!(agents.starts_with("user prefix\n"));
    assert!(agents.ends_with("\nuser suffix"));
}

#[test]
fn existing_claude_rules_are_used_when_agents_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("CLAUDE.md"), "keep existing rules\n").unwrap();
    install(dir.path());
    let text = fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
    assert!(text.starts_with("keep existing rules\n"));
    assert_eq!(
        text,
        fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap()
    );
}

#[cfg(unix)]
#[test]
fn existing_instruction_hard_links_survive_and_symlink_targets_are_untouched() {
    use std::os::unix::fs::{MetadataExt, symlink};
    let dir = tempfile::tempdir().unwrap();
    let agents = dir.path().join("AGENTS.md");
    let claude = dir.path().join("CLAUDE.md");
    fs::write(&claude, "keep rules\n").unwrap();
    fs::hard_link(&claude, &agents).unwrap();
    let inode = fs::metadata(&agents).unwrap().ino();
    install(dir.path());
    assert_eq!(inode, fs::metadata(&agents).unwrap().ino());
    assert_eq!(inode, fs::metadata(&claude).unwrap().ino());
    for relative in [".agents", ".codex", "AGENTS.md"] {
        let project = tempfile::tempdir().unwrap();
        let global = tempfile::tempdir().unwrap();
        fs::write(global.path().join("sentinel"), "global rules").unwrap();
        let target = if relative == "AGENTS.md" {
            global.path().join("sentinel")
        } else {
            global.path().to_owned()
        };
        symlink(target, project.path().join(relative)).unwrap();
        assert!(!setup(project.path()).status.success());
        assert_eq!(
            snapshot(global.path()),
            BTreeMap::from([("sentinel".into(), b"global rules".to_vec())])
        );
        assert_eq!(fs::read_dir(project.path()).unwrap().count(), 1);
    }
}

#[test]
fn bundled_markdown_links_resolve_and_device_examples_use_omh() {
    let dir = tempfile::tempdir().unwrap();
    install(dir.path());
    let skills = dir.path().join(".agents/skills");
    let links = regex::Regex::new(r"\]\(([^)]+)\)").unwrap();
    for (relative, content) in snapshot(&skills) {
        if !relative.ends_with(".md") {
            continue;
        }
        let text = String::from_utf8(content).unwrap().replace("\r\n", "\n");
        if relative.ends_with("/SKILL.md") {
            let name = relative.split('/').next().unwrap();
            assert!(
                text.starts_with(&format!("---\nname: {name}\n")),
                "{relative}: incorrect skill name"
            );
            assert!(
                text.contains("\ndescription:"),
                "{relative}: missing description"
            );
        }
        for link in links.captures_iter(&text) {
            let target = &link[1];
            if target.contains("://") || target.starts_with('#') {
                continue;
            }
            assert!(
                skills
                    .join(&relative)
                    .parent()
                    .unwrap()
                    .join(target)
                    .exists(),
                "{relative}: {target}"
            );
        }
        if !relative.starts_with("omh/") {
            assert!(
                !text.contains("\"$HDC\""),
                "{relative}: raw executable variable"
            );
            assert!(!text.contains("hdc -t"), "{relative}: raw target override");
            assert!(
                !text.contains("hdc list targets"),
                "{relative}: raw discovery"
            );
            assert!(
                !text.contains("-- shell hilog"),
                "{relative}: bypasses logs API"
            );
        }
    }
}
