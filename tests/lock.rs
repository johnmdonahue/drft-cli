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

/// Several paths lock in one invocation, leaving everything else stale. The
/// scoped form exists so the assertion "this was reviewed" stays narrow enough
/// to be true, and one-path-per-invocation pushed callers toward the bulk form.
#[test]
fn scoped_lock_accepts_several_paths() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    for name in ["a", "b", "c"] {
        fs::write(dir.path().join(format!("{name}.md")), format!("# {name}")).unwrap();
    }
    lock(dir.path());

    for name in ["a", "b", "c"] {
        fs::write(
            dir.path().join(format!("{name}.md")),
            format!("# {name} edited"),
        )
        .unwrap();
    }

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock", "a.md", "b.md"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let stdout = check(dir.path());
    assert!(!stdout.contains("stale-node]: a.md"), "got: {stdout}");
    assert!(!stdout.contains("stale-node]: b.md"), "got: {stdout}");
    assert!(
        stdout.contains("stale-node]: c.md"),
        "c.md was not named and must stay stale, got: {stdout}"
    );
}

/// Every path resolves before any is written. A partial lock would claim some
/// files were reviewed and drop the rest without saying so.
#[test]
fn scoped_lock_writes_nothing_when_a_path_is_unresolvable() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(dir.path().join("a.md"), "# a").unwrap();
    lock(dir.path());
    fs::write(dir.path().join("a.md"), "# a edited").unwrap();

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "lock",
            "a.md",
            "typo.md",
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2), "unresolvable path exits 2");
    let stdout = check(dir.path());
    assert!(
        stdout.contains("stale-node]: a.md"),
        "a.md must not be locked when a later path fails, got: {stdout}"
    );
}

/// Locking a path whose file has been deleted drops its lock entry, clearing
/// `removed-node`. The deletion is the finding, so naming the vanished path is how
/// you review it — the case the graph alone cannot resolve, since the node is gone.
#[test]
fn scoped_lock_drops_a_removed_node() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(dir.path().join("index.md"), "[doomed](doomed.md)").unwrap();
    fs::write(dir.path().join("doomed.md"), "# Doomed").unwrap();
    lock(dir.path());

    fs::remove_file(dir.path().join("doomed.md")).unwrap();
    assert!(check(dir.path()).contains("removed-node"));

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock", "doomed.md"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "locking a removed path should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !check(dir.path()).contains("removed-node"),
        "removed-node should be cleared after locking the deleted path"
    );
}

/// A batch of a repaired live path and a deleted one locks in one atomic call: the
/// live path re-snapshots, the deleted one drops. This is the workflow the earlier
/// one-path-per-call form could not express without the forbidden bare `drft lock`.
#[test]
fn scoped_lock_batches_a_live_update_with_a_drop() {
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

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "lock",
            "index.md",
            "doomed.md",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());

    let stdout = check(dir.path());
    for finding in ["removed-node", "removed-edge", "stale"] {
        assert!(
            !stdout.contains(finding),
            "expected no {finding} after the batch lock, got: {stdout}"
        );
    }
}

/// Locking a directory is a no-op, not a panic. A directory node carries no hash
/// and no edges, so it is never a lock entry; naming one has nothing to snapshot.
#[test]
fn scoped_lock_of_a_directory_is_a_noop() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::create_dir(dir.path().join("sub")).unwrap();
    fs::write(dir.path().join("sub").join("a.md"), "# A").unwrap();
    fs::write(dir.path().join("index.md"), "[sub](sub)").unwrap();
    lock(dir.path());

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock", "sub"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "locking a directory should succeed, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// An argument with an extension resolves to that exact node, not a `.md`-appended
/// variant. With both `a.md` and `a.md.md` present, `lock a.md` must snapshot
/// `a.md` — the `.md` fallback is only for a bare doc name.
#[test]
fn scoped_lock_of_an_extensioned_path_does_not_prefer_a_dot_md_variant() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(dir.path().join("a.md"), "# A").unwrap();
    fs::write(dir.path().join("a.md.md"), "# A dot md").unwrap();
    lock(dir.path());

    fs::write(dir.path().join("a.md"), "# A edited").unwrap();
    fs::write(dir.path().join("a.md.md"), "# A dot md edited").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock", "a.md"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let stdout = check(dir.path());
    assert!(
        !stdout.contains("stale-node]: a.md "),
        "a.md itself should have been locked, got: {stdout}"
    );
    assert!(
        stdout.contains("stale-node]: a.md.md"),
        "a.md.md must stay stale — it was not the named path, got: {stdout}"
    );
}
