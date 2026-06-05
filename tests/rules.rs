mod common;
use common::drft_bin;
use std::fs;
use tempfile::TempDir;

/// A per-rule `ignore` glob suppresses diagnostics for matching subjects while
/// leaving others flagged.
#[test]
fn ignore_glob_suppresses_diagnostics() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "[rules.detached-node]\nignore = [\"README.md\"]\n",
    )
    .unwrap();
    fs::write(dir.path().join("README.md"), "# Readme").unwrap();
    fs::write(dir.path().join("other.md"), "# Other").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("README.md"),
        "README.md should be suppressed by the ignore glob, got: {stdout}"
    );
    assert!(
        stdout.contains("other.md"),
        "other.md should still be flagged, got: {stdout}"
    );
}
