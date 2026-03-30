mod common;
use common::drft_bin;
use std::fs;
use tempfile::TempDir;

#[test]
fn check_json_envelope_clean() {
    let dir = TempDir::new().unwrap();
    // A clean graph: no orphans, no cycles, no fragmentation
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup").unwrap();
    // Suppress warnings for this test — we're testing the JSON envelope shape
    fs::write(
        dir.path().join("drft.toml"),
        "[rules]\norphan = \"off\"\nfragility = \"off\"\n",
    )
    .unwrap();

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
    assert_eq!(v["status"], "clean");
    assert_eq!(v["total"], 0);
    assert_eq!(v["errors"], 0);
    assert_eq!(v["warnings"], 0);
}
