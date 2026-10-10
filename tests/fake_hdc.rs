// Standalone std-only test fixture, compiled by integration.rs into a temp dir.
use std::{
    env,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::PathBuf,
    process::{self, Command},
    thread,
    time::Duration,
};
fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let root = PathBuf::from(env::var("OMH_FAKE_DIR").unwrap());
    if args.first().is_some_and(|s| s == "--job") {
        println!("job started");
        thread::sleep(Duration::from_millis(args[1].parse().unwrap()));
        println!("BUSINESS_SUCCESS");
        return;
    }
    if args.first().is_some_and(|s| s == "--nested-job") {
        let status = Command::new(env::var("OMH_BIN").unwrap())
            .args(["exec", "--", "shell", "echo", "nested"])
            .status()
            .unwrap();
        assert!(status.success());
        println!("BUSINESS_SUCCESS");
        return;
    }
    if args.first().is_some_and(|s| s == "--tree-job") {
        let mut child = Command::new(env::current_exe().unwrap())
            .args(["--heartbeat"])
            .spawn()
            .unwrap();
        fs::write(root.join("grandchild.pid"), child.id().to_string()).unwrap();
        let _ = child.wait();
        return;
    }
    if args.first().is_some_and(|s| s == "--heartbeat") {
        let mut output = OpenOptions::new()
            .create(true)
            .append(true)
            .open(root.join("heartbeat"))
            .unwrap();
        loop {
            output.write_all(b".").unwrap();
            output.flush().unwrap();
            thread::sleep(Duration::from_millis(30));
        }
    }
    let mut events = OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("events"))
        .unwrap();
    writeln!(events, "{}", args.join("|")).unwrap();
    if args == ["list", "targets"] {
        print!(
            "{}",
            fs::read_to_string(root.join("devices")).unwrap_or_default()
        );
        return;
    }
    if args == ["kill", "-r"] {
        if root.join("restart-fails").exists() {
            eprintln!("[Fail] restart");
            process::exit(1);
        }
        println!("restart success");
        return;
    }
    assert_eq!(args[0], "-t");
    let cmd = &args[2..];
    if cmd == ["shell", "bm", "get", "-u"] {
        if root.join("invalid-udid").exists() {
            println!("[Fail] no device UDID");
            return;
        }
        if root.join("query-fails").exists() {
            process::exit(17);
        }
        println!("udid of current device is :\n{}", "A".repeat(64));
        return;
    }
    if cmd == ["shell", "getprop", "hw_sc.build.os.deviceType"] {
        println!("phone");
        return;
    }
    if cmd.len() == 4 && cmd[0..3] == ["shell", "param", "get"] {
        let value = match cmd[3].as_str() {
            "const.product.name" => "Test Phone",
            "const.product.model" => "TEST-01",
            "ohos.boot.chiptype" => "TestChip",
            "const.product.brand" => "TestBrand",
            "const.ohos.fullname" => "OpenHarmony-7.0",
            "const.ohos.apiversion" => "26",
            _ => "",
        };
        println!("{value}");
        return;
    }
    if cmd.starts_with(&["shell".into(), "aa".into(), "force-stop".into()]) {
        if root.join("app-absent").exists() {
            println!(
                "error: failed to force stop process.\nError Code:10104002  Error Message:Failed to retrieve specified package information.\nError cause: The application corresponding to the specified package name is not installed."
            );
            return;
        }
        if root.join("cleanup-fails").exists() {
            println!("[Fail] force stop");
            return;
        }
        println!("force stop process successfully.");
        return;
    }
    if cmd[0] == "hilog" {
        loop {
            println!("log data");
            thread::sleep(Duration::from_millis(30));
        }
    }
    if cmd == ["shell", "cat"] {
        let mut data = Vec::new();
        std::io::stdin().read_to_end(&mut data).unwrap();
        std::io::stdout().write_all(&data).unwrap();
        return;
    }
    if cmd == ["shell", "fail"] {
        eprintln!("business failure");
        process::exit(37);
    }
    if cmd.starts_with(&["shell".into(), "sleep".into()]) {
        thread::sleep(Duration::from_millis(cmd[2].parse().unwrap()));
        return;
    }
    if cmd[0] == "fport" || cmd[0] == "rport" {
        let rules = root.join(format!("{}-forwards", args[1]));
        if cmd[1] == "ls" {
            print!("{}", fs::read_to_string(rules).unwrap_or_default());
            return;
        }
        if cmd[1] == "rm" {
            assert_eq!(cmd.len(), 4);
            fs::write(rules, "").unwrap();
            println!("Remove forward ruler success");
            return;
        }
        fs::write(rules, format!("{} {}", cmd[1], cmd[2])).unwrap();
        println!("forward success");
        return;
    }
    println!("{}", cmd.join("|"));
}
