mod common;
use common::drft_bin;
use std::fs;
use tempfile::TempDir;

#[test]
fn impact_shows_transitive_dependents() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "[config](config.md)").unwrap();
    fs::write(dir.path().join("config.md"), "# Config").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "impact", "config.md"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("setup.md"), "setup.md depends on config.md");
    assert!(
        stdout.contains("index.md"),
        "index.md transitively depends on config.md"
    );
    assert!(output.status.success());
}

#[test]
fn impact_json_format() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup").unwrap();

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "--format",
            "json",
            "impact",
            "setup.md",
        ])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    assert_eq!(v["total"], 1);
    assert_eq!(v["impacted"][0]["node"], "index.md");
    assert!(v["impacted"][0]["fix"].as_str().is_some());
    assert!(output.status.success());
}

#[test]
fn impact_md_extension_fallback() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup").unwrap();

    // "setup" without .md should resolve to "setup.md"
    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "impact", "setup"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("index.md"));
    assert!(output.status.success());
}
