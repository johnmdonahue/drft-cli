mod common;
use common::drft_bin;
use std::fs;
use tempfile::TempDir;

/// Scenario 5: First lock — creates drft.lock, subsequent check is clean.
#[test]
fn scenario_5_first_lock() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup").unwrap();

    // Lock
    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock"])
        .output()
        .unwrap();
    assert!(output.status.success(), "lock should exit 0");

    // Verify lockfile exists and has correct format
    let lockfile = fs::read_to_string(dir.path().join("drft.lock")).unwrap();
    assert!(lockfile.contains("lockfile_version = 2"));
    assert!(lockfile.contains("index.md"));
    assert!(lockfile.contains("setup.md"));
    assert!(lockfile.contains("b3:"));
    assert!(lockfile.contains(r#"type = "source""#));

    // Check should show no staleness after lock
    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("stale"),
        "expected no staleness after lock, got: {stdout}"
    );
    assert!(output.status.success());
}

/// Scenario 6: Staleness — dependency changed after lock.
#[test]
fn scenario_6_staleness_after_edit() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup").unwrap();

    // Lock
    drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock"])
        .output()
        .unwrap();

    // Edit setup.md
    fs::write(dir.path().join("setup.md"), "# Setup (edited)").unwrap();

    // Check should report staleness
    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("warn[stale]"),
        "expected stale warning, got: {stdout}"
    );
    assert!(
        stdout.contains("index.md"),
        "expected index.md in stale output, got: {stdout}"
    );
}

/// Scenario 7a: File removed — both broken-link and stale fire.
#[test]
fn scenario_7a_file_removed() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup").unwrap();

    // Lock
    drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock"])
        .output()
        .unwrap();

    // Delete setup.md
    fs::remove_file(dir.path().join("setup.md")).unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("broken-link"),
        "expected broken-link warning, got: {stdout}"
    );
    assert!(
        stdout.contains("stale"),
        "expected stale warning, got: {stdout}"
    );
}

// ── drft lock --check ──────────────────────────────────────────

/// Scenario 24: lock --check when lockfile is current.
#[test]
fn scenario_24_lock_check_current() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup").unwrap();

    drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock"])
        .output()
        .unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock", "--check"])
        .output()
        .unwrap();

    assert!(output.status.success(), "expected exit 0");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "",
        "expected no stdout"
    );
}

/// Scenario 25: lock --check when lockfile is stale.
#[test]
fn scenario_25_lock_check_stale() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup").unwrap();

    drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock"])
        .output()
        .unwrap();

    // Edit a file
    fs::write(dir.path().join("setup.md"), "# Setup (edited)").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock", "--check"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1), "expected exit code 1");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("out of date"),
        "expected 'out of date' in stderr: {stderr}"
    );
}

/// Scenario 26: lock --check with no lockfile.
#[test]
fn scenario_26_lock_check_no_lockfile() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock", "--check"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1), "expected exit code 1");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not found"),
        "expected 'not found' in stderr: {stderr}"
    );
}
