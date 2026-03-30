mod common;
use common::drft_bin;
use std::fs;
use tempfile::TempDir;

#[test]
fn init_creates_config() {
    let dir = TempDir::new().unwrap();
    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "init"])
        .output()
        .unwrap();

    assert!(output.status.success(), "expected exit code 0");
    let config = fs::read_to_string(dir.path().join("drft.toml")).unwrap();
    assert!(config.contains("[rules]"));
    assert!(config.contains("broken-link"));
}

#[test]
fn init_fails_if_exists() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), "# existing").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "init"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2), "expected exit code 2");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("already exists"),
        "expected 'already exists' in stderr: {stderr}"
    );
}
