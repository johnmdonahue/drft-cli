mod common;
use common::drft_bin;
use std::fs;
use tempfile::TempDir;

/// A clean check emits the `{diagnostics, summary}` envelope with empty
/// diagnostics and zero counts.
#[test]
fn check_json_envelope_clean() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "[index](index.md)").unwrap();
    // Silence detached-node so we exercise the clean envelope shape; index and
    // setup link each other via markdown, so their edges resolve cleanly.
    fs::write(
        dir.path().join("drft.toml"),
        format!(
            "{}[rules]\ndetached-node = \"off\"\n",
            common::DEFAULT_CONFIG
        ),
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
    assert_eq!(v["diagnostics"].as_array().unwrap().len(), 0);
    assert_eq!(v["summary"]["errors"], 0);
    assert_eq!(v["summary"]["warnings"], 0);
    assert!(output.status.success());
}
