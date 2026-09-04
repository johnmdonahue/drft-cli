mod common;
use common::drft_bin;
use serde_json::Value;
use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn nested_fixture() -> (TempDir, std::path::PathBuf) {
    let repo = TempDir::new().unwrap();
    let status = Command::new("git")
        .args(["init", "-q"])
        .current_dir(repo.path())
        .status()
        .unwrap();
    assert!(status.success());
    fs::write(repo.path().join(".gitignore"), "/project/ignored.md\n").unwrap();

    let root = repo.path().join("project");
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(
        root.join("drft.toml"),
        "[graphs.markdown]\nparser = \"markdown\"\nfiles = [\"**/*.md\"]\n",
    )
    .unwrap();
    fs::write(root.join("keep.md"), "# Keep\n").unwrap();
    fs::write(root.join("ignored.md"), "# Ignored\n").unwrap();
    fs::write(root.join("docs/.gitignore"), "draft.md\n").unwrap();
    (repo, root)
}

#[test]
fn nodes_honor_repository_gitignore_above_graph_root() {
    let (_repo, root) = nested_fixture();
    let output = drft_bin()
        .args([
            "-C",
            root.to_str().unwrap(),
            "--format",
            "json",
            "nodes",
            "--all",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "drft nodes failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    let ids: Vec<_> = json["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"keep.md"), "got: {ids:?}");
    assert!(!ids.contains(&"ignored.md"), "got: {ids:?}");
}

#[test]
fn show_ignores_reports_sources_without_touching_the_lock() {
    let (_repo, root) = nested_fixture();
    let lock = root.join("drft.lock");
    fs::write(&lock, "sentinel\n").unwrap();

    let output = drft_bin()
        .args([
            "-C",
            root.to_str().unwrap(),
            "--format",
            "json",
            "config",
            "--show-ignores",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "drft config failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["gitignore"]["enabled"], true);
    assert_eq!(
        json["gitignore"]["files"],
        serde_json::json!(["../.gitignore", "docs/.gitignore"])
    );
    assert_eq!(json["git_exclude"]["enabled"], false);
    assert_eq!(json["git_global"]["enabled"], false);
    assert_eq!(json["dot_ignore"]["enabled"], false);
    assert_eq!(fs::read_to_string(lock).unwrap(), "sentinel\n");
}

#[test]
fn show_ignores_text_names_enabled_and_disabled_sources() {
    let (_repo, root) = nested_fixture();
    let output = drft_bin()
        .args(["-C", root.to_str().unwrap(), "config", "--show-ignores"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "repository .gitignore: enabled\n  files:\n    ../.gitignore\n    docs/.gitignore\n.ignore: disabled\n.git/info/exclude: disabled\nglobal excludes: disabled\n"
    );
    assert!(output.stderr.is_empty());
}
