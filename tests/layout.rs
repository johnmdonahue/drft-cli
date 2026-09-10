mod common;

use common::drft_bin;
use std::fs;
use tempfile::TempDir;

const LEGACY_CONFIG: &str = "\
[experimental.usage]
enabled = true

[graphs.markdown]
parser = \"markdown\"
files = [\"**/*.md\"]
";

fn run(root: &std::path::Path, home: &std::path::Path, args: &[&str]) -> std::process::Output {
    drft_bin()
        .env("HOME", home)
        .env("XDG_CACHE_HOME", home.join("cache"))
        .args(["-C", root.to_str().unwrap()])
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn every_repository_command_rejects_the_legacy_layout_without_mutation() {
    let dir = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), LEGACY_CONFIG).unwrap();
    fs::write(dir.path().join("drft.lock"), "legacy baseline sentinel\n").unwrap();
    fs::write(dir.path().join("doc.md"), "# Doc\n").unwrap();

    let commands: &[&[&str]] = &[
        &["config", "--show-ignores"],
        &["graph"],
        &["nodes", "--all"],
        &["edges", "--all"],
        &["impact", "doc.md"],
        &["check"],
        &["lock", "doc.md"],
        &["lock", "--all"],
    ];

    for command in commands {
        for json in [false, true] {
            let mut args = Vec::new();
            if json {
                args.extend(["--format", "json"]);
            }
            args.extend_from_slice(command);
            let output = run(dir.path(), home.path(), &args);
            assert_eq!(output.status.code(), Some(2), "args={args:?}");
            assert!(output.stdout.is_empty(), "args={args:?}");
            let stderr = String::from_utf8(output.stderr).unwrap();
            if json {
                let error: serde_json::Value = serde_json::from_str(stderr.trim()).unwrap();
                assert_eq!(error["exit_code"], 2);
                assert_eq!(error["hints"], serde_json::json!([]));
                assert!(
                    error["error"]
                        .as_str()
                        .unwrap()
                        .contains("manual migration")
                );
            } else {
                assert!(stderr.starts_with("error: legacy drft project files"));
            }
        }
    }

    assert_eq!(
        fs::read(dir.path().join("drft.toml")).unwrap(),
        LEGACY_CONFIG.as_bytes()
    );
    assert_eq!(
        fs::read(dir.path().join("drft.lock")).unwrap(),
        b"legacy baseline sentinel\n"
    );
    assert!(!dir.path().join(".drft").exists());
    assert!(!home.path().join("cache/drft/usage").exists());
}

#[test]
fn a_legacy_lock_never_degrades_to_a_missing_baseline_or_gets_rebuilt() {
    let dir = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    fs::write(
        common::config_path(dir.path()),
        common::MARKDOWN_ONLY_CONFIG,
    )
    .unwrap();
    fs::write(dir.path().join("drft.lock"), "legacy baseline sentinel\n").unwrap();
    fs::write(dir.path().join("doc.md"), "# Doc\n").unwrap();

    for command in [
        &["check"][..],
        &["impact", "doc.md"],
        &["lock", "doc.md"],
        &["lock", "--all"],
    ] {
        let output = run(dir.path(), home.path(), command);
        assert_eq!(output.status.code(), Some(2), "command={command:?}");
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.contains("move `drft.lock` to `.drft/lock.toml`"));
        assert!(!stderr.contains("no-baseline"));
    }

    assert_eq!(
        fs::read(dir.path().join("drft.lock")).unwrap(),
        b"legacy baseline sentinel\n"
    );
    assert!(!dir.path().join(".drft/lock.toml").exists());
}

#[test]
fn coexistence_reports_both_conflicts_and_selects_neither() {
    let dir = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    fs::write(
        common::config_path(dir.path()),
        common::MARKDOWN_ONLY_CONFIG,
    )
    .unwrap();
    fs::write(common::lock_path(dir.path()), "current baseline\n").unwrap();
    fs::write(dir.path().join("drft.toml"), "legacy config\n").unwrap();
    fs::write(dir.path().join("drft.lock"), "legacy baseline\n").unwrap();

    let output = run(dir.path(), home.path(), &["check"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("both `drft.toml` and `.drft/config.toml` exist"));
    assert!(stderr.contains("both `drft.lock` and `.drft/lock.toml` exist"));
    assert_eq!(
        fs::read(common::lock_path(dir.path())).unwrap(),
        b"current baseline\n"
    );
    assert_eq!(
        fs::read(dir.path().join("drft.lock")).unwrap(),
        b"legacy baseline\n"
    );
}

#[test]
fn a_nearer_legacy_config_stops_before_a_farther_current_config() {
    let outer = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    fs::write(
        common::config_path(outer.path()),
        common::MARKDOWN_ONLY_CONFIG,
    )
    .unwrap();
    let child = outer.path().join("child");
    let work = child.join("work");
    fs::create_dir_all(&work).unwrap();
    fs::write(child.join("drft.toml"), LEGACY_CONFIG).unwrap();

    let output = run(&work, home.path(), &["check"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("move `drft.toml` to `.drft/config.toml`")
    );
}

#[test]
fn init_refuses_an_orphan_legacy_lock_and_preserves_it() {
    let dir = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.lock"), "legacy baseline sentinel\n").unwrap();

    let output = run(dir.path(), home.path(), &["init"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("move `drft.lock` to `.drft/lock.toml`")
    );
    assert_eq!(
        fs::read(dir.path().join("drft.lock")).unwrap(),
        b"legacy baseline sentinel\n"
    );
    assert!(!dir.path().join(".drft/config.toml").exists());
}

#[test]
fn init_preserves_a_current_lock_and_writes_only_the_config() {
    let dir = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    fs::write(common::lock_path(dir.path()), "current baseline sentinel\n").unwrap();

    let output = run(dir.path(), home.path(), &["init"]);
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    assert!(dir.path().join(".drft/config.toml").is_file());
    assert_eq!(
        fs::read(common::lock_path(dir.path())).unwrap(),
        b"current baseline sentinel\n"
    );
}

#[test]
fn init_keeps_its_exact_directory_scope() {
    let outer = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    fs::write(outer.path().join("drft.toml"), LEGACY_CONFIG).unwrap();
    let child = outer.path().join("child");
    fs::create_dir(&child).unwrap();

    let output = run(&child, home.path(), &["init"]);
    assert!(output.status.success());
    assert!(child.join(".drft/config.toml").is_file());
    assert_eq!(
        fs::read(outer.path().join("drft.toml")).unwrap(),
        LEGACY_CONFIG.as_bytes()
    );
}

#[test]
fn discovery_keeps_the_project_directory_as_graph_root_and_lock_location() {
    let dir = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    fs::write(
        common::config_path(dir.path()),
        common::MARKDOWN_ONLY_CONFIG,
    )
    .unwrap();
    fs::write(dir.path().join("doc.md"), "# Doc\n").unwrap();
    let child = dir.path().join("child");
    fs::create_dir(&child).unwrap();

    let output = run(&child, home.path(), &["lock", "../doc.md"]);
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(dir.path().join(".drft/lock.toml").is_file());
    assert!(!child.join(".drft/lock.toml").exists());
    assert!(
        fs::read_to_string(common::lock_path(dir.path()))
            .unwrap()
            .contains("path = \"doc.md\"")
    );
}
