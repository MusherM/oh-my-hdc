use std::{env, fs, path::Path};

fn collect(root: &Path, dir: &Path, entries: &mut Vec<(String, String)>) {
    for entry in fs::read_dir(dir).expect("read skill directory") {
        let entry = entry.expect("read skill entry");
        let path = entry.path();
        let kind = entry.file_type().expect("skill file type");
        assert!(
            !kind.is_symlink(),
            "bundled skills must not contain symlinks"
        );
        if kind.is_dir() {
            collect(root, &path, entries);
        } else {
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_str()
                .unwrap()
                .replace('\\', "/");
            assert!(
                matches!(
                    path.extension().and_then(|s| s.to_str()),
                    Some("md" | "py" | "yaml")
                ),
                "unexpected skill asset: {relative}"
            );
            let content = fs::read_to_string(&path).expect("UTF-8 skill asset");
            entries.push((relative, content));
        }
    }
}

fn main() {
    println!("cargo:rerun-if-changed=skills");
    let root = Path::new("skills");
    let mut entries = Vec::new();
    collect(root, root, &mut entries);
    entries.sort();
    let mut source = String::from("&[\n");
    for (path, content) in entries {
        source.push_str(&format!("({path:?}, {content:?}),\n"));
    }
    source.push_str("]\n");
    fs::write(
        Path::new(&env::var("OUT_DIR").unwrap()).join("skills.rs"),
        source,
    )
    .unwrap();
}
