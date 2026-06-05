mod common;
use common::drft_bin;
use std::fs;
use tempfile::TempDir;

fn check(dir: &std::path::Path) -> String {
    let output = drft_bin()
        .args(["-C", dir.to_str().unwrap(), "check"])
        .output()
        .unwrap();
    assert!(output.status.success(), "check should exit 0 on warnings");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn lock(dir: &std::path::Path) {
    let output = drft_bin()
        .args(["-C", dir.to_str().unwrap(), "lock"])
        .output()
        .unwrap();
    assert!(output.status.success(), "lock should exit 0");
}

/// First lock writes drft.lock in the path-keyed format (node hashes + nested
/// edge target hashes), with no version field. A subsequent check is clean.
#[test]
fn first_lock_then_clean_check() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup").unwrap();

    lock(dir.path());

    let lockfile = fs::read_to_string(dir.path().join("drft.lock")).unwrap();
    assert!(lockfile.contains("[[node]]"));
    assert!(lockfile.contains("path = \"index.md\""));
    assert!(lockfile.contains("[[node.edge]]"));
    assert!(lockfile.contains("target = \"setup.md\""));
    assert!(lockfile.contains("b3:"));
    assert!(
        !lockfile.contains("version"),
        "lockfile should carry no version field"
    );
    // drft.lock is drft's own artifact, never a graph node.
    assert!(
        !lockfile.contains("path = \"drft.lock\""),
        "the lockfile should not list itself as a node"
    );

    let stdout = check(dir.path());
    assert!(
        !stdout.contains("stale"),
        "expected no staleness after lock, got: {stdout}"
    );
}

/// Editing a dependency after lock reports stale-node on the edited file and
/// stale-edge on its dependent.
#[test]
fn edit_dependency_reports_stale_node_and_stale_edge() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup").unwrap();

    lock(dir.path());
    fs::write(dir.path().join("setup.md"), "# Setup (edited)").unwrap();

    let stdout = check(dir.path());
    assert!(
        stdout.contains("warn[stale-node]: setup.md"),
        "expected stale-node on the edited file, got: {stdout}"
    );
    assert!(
        stdout.contains("warn[stale-edge]: index.md"),
        "expected stale-edge on the dependent, got: {stdout}"
    );
}

/// Re-locking after an edit clears the staleness.
#[test]
fn relock_clears_staleness() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup").unwrap();

    lock(dir.path());
    fs::write(dir.path().join("setup.md"), "# Setup (edited)").unwrap();
    assert!(check(dir.path()).contains("stale"));

    lock(dir.path());
    assert!(
        !check(dir.path()).contains("stale"),
        "re-locking should clear staleness"
    );
}

/// Deleting a linked file after lock reports unresolved-edge on the linker and
/// removed-node for the deleted file.
#[test]
fn deleted_file_reports_unresolved_and_removed_node() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup").unwrap();

    lock(dir.path());
    fs::remove_file(dir.path().join("setup.md")).unwrap();

    let stdout = check(dir.path());
    assert!(
        stdout.contains("unresolved-edge"),
        "expected unresolved-edge, got: {stdout}"
    );
    assert!(
        stdout.contains("removed-node"),
        "expected removed-node, got: {stdout}"
    );
}
