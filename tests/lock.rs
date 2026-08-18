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

/// Run `drft lock <paths>` and return (exit code, stderr).
fn lock_paths(dir: &std::path::Path, paths: &[&str]) -> (i32, String) {
    let output = drft_bin()
        .args(["-C", dir.to_str().unwrap(), "lock"])
        .args(paths)
        .output()
        .unwrap();
    (
        output.status.code().unwrap(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
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

/// Scope-locking a path that is in the lockfile but gone from disk drops its
/// entry, clearing `removed-node` — the reviewed-deletion case. This is the
/// finding whose only other remedy was the whole-graph `drft lock`.
#[test]
fn scope_lock_drops_removed_node() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(dir.path().join("index.md"), "[doomed](doomed.md)").unwrap();
    fs::write(dir.path().join("doomed.md"), "# Doomed").unwrap();

    lock(dir.path());
    fs::remove_file(dir.path().join("doomed.md")).unwrap();
    assert!(check(dir.path()).contains("removed-node"));

    let (code, stderr) = lock_paths(dir.path(), &["doomed.md"]);
    assert_eq!(
        code, 0,
        "locking a removed path should succeed, stderr: {stderr}"
    );
    assert!(
        stderr.contains("unlocked removed node: doomed.md"),
        "expected an unlock notice, got: {stderr}"
    );
    assert!(
        !check(dir.path()).contains("removed-node"),
        "removed-node should be cleared after scope-locking the deleted path"
    );
}

/// The issue's headline case: a batch of a live (repaired) path and a deleted one
/// in a single call. The live path re-locks and the deleted one drops, atomically
/// — no all-or-nothing abort. After it, the deletion leaves no findings.
#[test]
fn batch_lock_updates_live_and_drops_removed() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(
        dir.path().join("index.md"),
        "[guide](guide.md) [doomed](doomed.md)",
    )
    .unwrap();
    fs::write(dir.path().join("guide.md"), "# Guide").unwrap();
    fs::write(dir.path().join("doomed.md"), "# Doomed").unwrap();

    lock(dir.path());
    // Delete one target and repair the citing document in the same breath.
    fs::remove_file(dir.path().join("doomed.md")).unwrap();
    fs::write(dir.path().join("index.md"), "[guide](guide.md)").unwrap();

    let (code, _) = lock_paths(dir.path(), &["index.md", "doomed.md"]);
    assert_eq!(code, 0, "a batch of live + removed paths should succeed");

    let stdout = check(dir.path());
    for finding in ["removed-node", "removed-edge", "stale"] {
        assert!(
            !stdout.contains(finding),
            "expected no {finding} after the batch lock, got: {stdout}"
        );
    }
}

/// An unresolved path does not abort the call: paths that resolve are written,
/// the miss is reported, and the exit code is non-zero. The batch is tolerant,
/// not all-or-nothing.
#[test]
fn lock_reports_misses_but_writes_resolved() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup").unwrap();
    lock(dir.path());

    // Edit a file, then lock it alongside a nonexistent path.
    fs::write(dir.path().join("setup.md"), "# Setup (edited)").unwrap();
    let (code, stderr) = lock_paths(dir.path(), &["setup.md", "nope.md"]);

    assert_eq!(code, 2, "a miss should make the call exit non-zero");
    assert!(
        stderr.contains("node not found: \"nope.md\""),
        "expected the miss to be reported, got: {stderr}"
    );
    // setup.md was written despite the miss, so its own stale-node clears. (The
    // dependent index.md keeps a stale-edge until it too is locked — ordinary
    // scoped-lock behavior, not a property of the miss.)
    assert!(
        !check(dir.path()).contains("stale-node]: setup.md"),
        "the resolved path should still have been locked despite the miss"
    );
}
