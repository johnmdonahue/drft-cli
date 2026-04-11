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

// ── Containment escape ─────────────────────────────────────────

/// An edge whose target resolves above the graph root is a boundary edge.
#[test]
fn boundary_edge_catches_parent_escape() {
    // outer/project is the graph; it links to outer/outside.md via ../
    let outer = TempDir::new().unwrap();
    let graph_root = outer.path().join("project");
    fs::create_dir(&graph_root).unwrap();
    fs::write(graph_root.join("drft.toml"), "").unwrap();
    fs::write(graph_root.join("index.md"), "[escape](../outside.md)").unwrap();
    fs::write(outer.path().join("outside.md"), "# Outside").unwrap();

    let output = drft_bin()
        .args(["-C", graph_root.to_str().unwrap(), "check"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("boundary-edge"),
        "expected boundary-edge for ../outside.md, got: {stdout}"
    );
    assert!(
        stdout.contains("../outside.md"),
        "expected target path in diagnostic, got: {stdout}"
    );
}
