mod common;
use common::drft_bin;
use std::fs;
use tempfile::TempDir;

/// Scenario 10: Child graph — open (no interface). Parent links to child file, no violation.
#[test]
fn scenario_10_child_graph_open() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), "").unwrap();
    fs::write(
        dir.path().join("index.md"),
        "[overview](research/overview.md)",
    )
    .unwrap();

    let research = dir.path().join("research");
    fs::create_dir(&research).unwrap();
    fs::write(research.join("drft.toml"), "").unwrap();
    fs::write(research.join("overview.md"), "# Overview").unwrap();
    fs::write(research.join("notes.md"), "# Notes").unwrap();

    // Lock child first, then parent
    drft_bin()
        .args(["-C", research.to_str().unwrap(), "lock"])
        .output()
        .unwrap();
    drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock"])
        .output()
        .unwrap();

    // Verify lockfile has graph + child-graph file nodes
    let lockfile = fs::read_to_string(dir.path().join("drft.lock")).unwrap();
    assert!(lockfile.contains(r#"type = "directory""#));
    assert!(lockfile.contains(r#"type = "file""#));

    // Check should be clean (unsealed = no encapsulation)
    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("encapsulation-violation"),
        "open graph (no interface) should not trigger encapsulation"
    );
    assert!(output.status.success());
}

/// Scenario 12: Encapsulation violation — link to non-interface file in child graph.
#[test]
fn scenario_12_encapsulation_violation() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), "[parsers]\nmarkdown = true\n").unwrap();
    fs::write(
        dir.path().join("index.md"),
        "[overview](research/overview.md)\n[internal](research/internal.md)",
    )
    .unwrap();

    let research = dir.path().join("research");
    fs::create_dir(&research).unwrap();
    fs::write(research.join("overview.md"), "# Overview").unwrap();
    fs::write(research.join("internal.md"), "# Internal").unwrap();

    // Child config with interface (only overview exposed)
    fs::write(
        research.join("drft.toml"),
        "[parsers]\nmarkdown = true\n\n[interface]\nfiles = [\"overview.md\"]\n",
    )
    .unwrap();

    // Lock child, then parent
    drft_bin()
        .args(["-C", research.to_str().unwrap(), "lock"])
        .output()
        .unwrap();
    drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock"])
        .output()
        .unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("encapsulation-violation"),
        "expected encapsulation-violation, got: {stdout}"
    );
    assert!(
        stdout.contains("research/internal.md"),
        "should mention the non-interface file"
    );
    assert!(output.status.success(), "default severity is warn");
}

/// Recursive lock creates all lockfiles bottom-up.
#[test]
fn recursive_lock() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), "").unwrap();
    fs::write(dir.path().join("index.md"), "[child](child/index.md)").unwrap();

    let child = dir.path().join("child");
    fs::create_dir(&child).unwrap();
    fs::write(child.join("drft.toml"), "").unwrap();
    fs::write(child.join("index.md"), "# Child").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock", "--recursive"])
        .output()
        .unwrap();
    assert!(output.status.success());

    // Both lockfiles should exist
    assert!(dir.path().join("drft.lock").exists());
    assert!(child.join("drft.lock").exists());

    // lock --check --recursive should pass
    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "lock",
            "--check",
            "--recursive",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
}

/// Recursive check runs child graphs with their own config.
#[test]
fn recursive_check_with_child_config() {
    let dir = TempDir::new().unwrap();
    // Parent disables orphan so only child's config triggers it
    fs::write(
        dir.path().join("drft.toml"),
        "[rules]\norphan-node = \"off\"\n",
    )
    .unwrap();
    fs::write(dir.path().join("index.md"), "# Root").unwrap();

    let child = dir.path().join("child");
    fs::create_dir(&child).unwrap();
    fs::write(child.join("drft.toml"), "[rules]\norphan-node = \"warn\"\n").unwrap();
    fs::write(child.join("drft.lock"), "lockfile_version = 2\n").unwrap();
    fs::write(child.join("linked.md"), "# Linked").unwrap();
    fs::write(child.join("orphan.md"), "# Orphan").unwrap();

    // Non-recursive: no child diagnostics
    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("orphan"),
        "non-recursive should not show child diagnostics, got: {stdout}"
    );

    // Recursive: child's orphan config kicks in
    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check", "--recursive"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("[child]"),
        "recursive should show graph header, got: {stdout}"
    );
    assert!(stdout.contains("orphan"), "child's orphan rule should fire");
}

/// Scope boundary staleness — child gains drft.lock after parent locked.
#[test]
fn graph_boundary_staleness() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), "").unwrap();
    fs::write(dir.path().join("index.md"), "[docs](docs/readme.md)").unwrap();

    let docs = dir.path().join("docs");
    fs::create_dir(&docs).unwrap();
    fs::write(docs.join("readme.md"), "# Docs").unwrap();

    // Lock parent (docs/ has no drft.toml yet — not a child graph)
    drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock"])
        .output()
        .unwrap();

    // Now create a child graph in docs/ by adding drft.toml + locking
    fs::write(docs.join("drft.toml"), "").unwrap();
    drft_bin()
        .args(["-C", docs.to_str().unwrap(), "lock"])
        .output()
        .unwrap();

    // Parent should detect the new graph boundary
    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("graph boundary changed"),
        "expected graph boundary staleness, got: {stdout}"
    );
}
