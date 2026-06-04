mod common;
use common::drft_bin;
use std::fs;
use tempfile::TempDir;

/// A graph with all links resolved fires no unresolved-edge and no errors.
#[test]
fn clean_graph_has_no_unresolved_or_errors() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), "").unwrap();
    fs::write(
        dir.path().join("index.md"),
        "[setup](setup.md) and [faq](faq.md)",
    )
    .unwrap();
    fs::write(dir.path().join("setup.md"), "[config](config.md)").unwrap();
    fs::write(dir.path().join("config.md"), "# Config").unwrap();
    fs::write(dir.path().join("faq.md"), "[index](index.md)").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("unresolved-edge"),
        "expected no broken links, got: {stdout}"
    );
    assert!(
        !stdout.contains("error["),
        "expected no errors, got: {stdout}"
    );
    assert!(output.status.success(), "expected exit code 0");
}

/// A broken link fires unresolved-edge as a warning, exit 0.
#[test]
fn broken_link_warns() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), "").unwrap();
    fs::write(
        dir.path().join("index.md"),
        "[setup](setup.md) and [missing](gone.md)",
    )
    .unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("warn[unresolved-edge]: index.md"),
        "expected unresolved-edge warning on index.md, got: {stdout}"
    );
    assert!(
        stdout.contains("gone.md"),
        "expected gone.md in output, got: {stdout}"
    );
    assert!(
        output.status.success(),
        "expected exit code 0 (warning only)"
    );
}

/// The JSON envelope is `{diagnostics, summary}`; each diagnostic carries
/// `{name, severity, subject, _graphs, message}`.
#[test]
fn broken_link_json_shape() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), "").unwrap();
    fs::write(dir.path().join("index.md"), "[missing](gone.md)").unwrap();

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "--format",
            "json",
            "check",
        ])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    assert_eq!(v["summary"]["errors"], 0);
    assert!(v["summary"]["warnings"].as_u64().unwrap() >= 1);

    let diagnostics = v["diagnostics"].as_array().unwrap();
    let broken = diagnostics
        .iter()
        .find(|d| d["name"] == "unresolved-edge")
        .expect("expected unresolved-edge diagnostic");
    assert_eq!(broken["severity"], "warn");
    assert_eq!(broken["subject"], "index.md");
    assert_eq!(broken["_graphs"], serde_json::json!(["@markdown"]));
    assert!(
        broken["message"].as_str().unwrap().contains("gone.md"),
        "message should name the missing target"
    );
    assert!(output.status.success());
}

/// A rule promoted to `error` in config exits 1.
#[test]
fn broken_link_error_severity_exits_1() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "[rules]\nunresolved-edge = \"error\"\n",
    )
    .unwrap();
    fs::write(dir.path().join("index.md"), "[missing](gone.md)").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("error[unresolved-edge]"),
        "expected error-level unresolved-edge, got: {stdout}"
    );
    assert_eq!(output.status.code(), Some(1), "expected exit code 1");
}

/// detached-node warns by default for a file with no links in or out.
#[test]
fn detached_node_warns_by_default() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), "").unwrap();
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup").unwrap();
    fs::write(dir.path().join("orphan.md"), "# Orphan").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("warn[detached-node]: orphan.md (no connections)"),
        "expected detached-node warning for orphan.md, got: {stdout}"
    );
    assert!(output.status.success(), "warnings should exit 0");
}

/// A per-rule `ignore` glob drops findings whose subject matches.
#[test]
fn detached_node_ignore_glob() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "[rules.detached-node]\nignore = [\"orphan.md\"]\n",
    )
    .unwrap();
    fs::write(dir.path().join("orphan.md"), "# Orphan").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("orphan.md"),
        "orphan.md should be ignored, got: {stdout}"
    );
    assert!(output.status.success());
}

/// Running without drft.toml fails with exit 2.
#[test]
fn no_config_exits_with_error() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("index.md"), "# Hello").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no drft.toml found"),
        "expected config error, got: {stderr}"
    );
    assert_eq!(output.status.code(), Some(2), "expected exit code 2");
}
