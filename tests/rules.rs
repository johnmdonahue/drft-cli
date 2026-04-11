mod common;
use common::drft_bin;
use std::fs;
use tempfile::TempDir;

// ── Ignore rules ───────────────────────────────────────────────

#[test]
fn ignore_rules_suppresses_diagnostics() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "[rules.orphan-node]\nseverity = \"warn\"\nignore = [\"README.md\"]\n",
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
        "README.md should be suppressed by ignore-rules"
    );
    assert!(
        stdout.contains("other.md"),
        "other.md should still be flagged"
    );
}

// ── Lockfile version ───────────────────────────────────────────

#[test]
fn lockfile_contains_version() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), "").unwrap();
    fs::write(dir.path().join("index.md"), "# Hello").unwrap();

    drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock"])
        .output()
        .unwrap();

    let lockfile = fs::read_to_string(dir.path().join("drft.lock")).unwrap();
    assert!(lockfile.starts_with("lockfile_version = 2"));
}

// ── Fragmentation ─────────────────────────────────────────────

#[test]
fn fragmentation_rule_fires() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "[rules]\nfragmentation = \"warn\"\n",
    )
    .unwrap();
    fs::write(dir.path().join("a.md"), "[b](b.md)").unwrap();
    fs::write(dir.path().join("b.md"), "# B").unwrap();
    fs::write(dir.path().join("c.md"), "# C").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("fragmentation"),
        "expected fragmentation warning, got: {stdout}"
    );
    assert!(
        stdout.contains("disconnected component"),
        "expected disconnected component message, got: {stdout}"
    );
}
