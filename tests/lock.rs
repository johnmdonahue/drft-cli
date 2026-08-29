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

fn lock_all(dir: &std::path::Path) {
    let output = drft_bin()
        .args(["-C", dir.to_str().unwrap(), "lock", "--all"])
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

    lock_all(dir.path());

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

    lock_all(dir.path());
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

    lock_all(dir.path());
    fs::write(dir.path().join("setup.md"), "# Setup (edited)").unwrap();
    assert!(check(dir.path()).contains("stale"));

    lock_all(dir.path());
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

    lock_all(dir.path());
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
    lock_all(dir.path());

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
    lock_all(dir.path());
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
    lock_all(dir.path());

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
    lock_all(dir.path());

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

/// Locking a directory writes nothing, and says so. A directory node carries no
/// hash and no edges, so it is never a lock entry and there is nothing to
/// snapshot — but exiting 0 in silence made that indistinguishable from a lock
/// that covered the subtree.
#[test]
fn scoped_lock_of_a_directory_reports_zero_and_hints() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::create_dir(dir.path().join("sub")).unwrap();
    fs::write(dir.path().join("sub").join("a.md"), "# A").unwrap();
    fs::write(dir.path().join("index.md"), "[sub](sub)").unwrap();
    lock_all(dir.path());
    let before = fs::read_to_string(dir.path().join("drft.lock")).unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock", "sub"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.contains("locked 0 nodes"), "stdout={stdout:?}");
    assert!(stderr.contains("directory-lock"), "stderr={stderr:?}");
    assert_eq!(
        fs::read_to_string(dir.path().join("drft.lock")).unwrap(),
        before,
        "a directory lock must not rewrite the baseline"
    );
}

/// Every spelling of a directory reports zero. `docs`, `docs/` and a path from a
/// subdirectory all resolve to the same node, and each used to exit 0 in silence.
#[test]
fn every_directory_spelling_reports_zero() {
    for spelling in ["sub", "sub/", "./sub"] {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub").join("a.md"), "# A").unwrap();
        fs::write(dir.path().join("index.md"), "[sub](sub)").unwrap();
        lock_all(dir.path());

        let output = drft_bin()
            .args(["-C", dir.path().to_str().unwrap(), "lock", spelling])
            .output()
            .unwrap();
        assert!(output.status.success(), "{spelling:?} should exit 0");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("locked 0 nodes"),
            "{spelling:?} was silent: stdout={stdout:?}"
        );
    }
}

/// A directory lock in a repo that has never been locked writes no lockfile.
///
/// It used to write one containing `node = []` — a valid, parseable, zero-entry
/// baseline produced by a command that reported success. Every staleness rule
/// then compared against nothing while the file's presence made the baseline look
/// established.
#[test]
fn a_directory_lock_does_not_manufacture_an_empty_baseline() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::create_dir(dir.path().join("sub")).unwrap();
    fs::write(dir.path().join("sub").join("a.md"), "# A").unwrap();
    fs::write(dir.path().join("index.md"), "[sub](sub)").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock", "sub"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(
        !dir.path().join("drft.lock").exists(),
        "a lock that wrote nothing must not create a lockfile"
    );
}

/// `lock` reports what it wrote, in both formats. Without it, a lock covering
/// five files and one covering none are indistinguishable without reading
/// `drft.lock` by hand.
#[test]
fn lock_reports_what_it_wrote() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock", "--all"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("locked 3 nodes"), "stdout={stdout:?}");
    for name in ["index.md", "setup.md", "drft.toml"] {
        assert!(
            stdout.contains(name),
            "{name} missing from stdout={stdout:?}"
        );
    }

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "--format",
            "json",
            "lock",
            "index.md",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    assert_eq!(v["locked"].as_array().unwrap().len(), 1);
    assert_eq!(v["locked"][0], "index.md");
    assert_eq!(v["dropped"].as_array().unwrap().len(), 0);
}

/// A bare name resolves to the path the caller spelled before any `.md` variant
/// invented from it.
///
/// With both `docs/` and `docs.md` present, `drft lock docs` used to snapshot
/// `docs.md` — clearing its `stale-node` finding and writing a durable "this was
/// reviewed" claim against a file the caller never named, silently.
#[test]
fn a_bare_name_prefers_the_exact_path_over_a_dot_md_sibling() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::create_dir(dir.path().join("docs")).unwrap();
    fs::write(dir.path().join("docs.md"), "# Sibling").unwrap();
    fs::write(dir.path().join("docs").join("a.md"), "# A").unwrap();
    fs::write(
        dir.path().join("index.md"),
        "[a](docs/a.md) and [d](docs.md)",
    )
    .unwrap();
    lock_all(dir.path());

    fs::write(dir.path().join("docs.md"), "# Sibling CHANGED").unwrap();
    assert!(check(dir.path()).contains("docs.md"));

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock", "docs"])
        .output()
        .unwrap();
    assert!(output.status.success());

    assert!(
        check(dir.path()).contains("docs.md"),
        "locking the directory `docs` must not snapshot the file `docs.md`"
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
    lock_all(dir.path());

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

/// Zero paths is a usage error, not the whole graph. `drft lock $(cmd)` where
/// `cmd` printed nothing hands drft exactly this argv, so inferring "every node"
/// from it would turn a scoped invocation into a whole-graph review assertion
/// with nothing in the output saying so.
#[test]
fn lock_with_no_paths_errors_and_writes_nothing() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(dir.path().join("a.md"), "# a").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2), "usage error exits 2");
    assert!(
        !dir.path().join("drft.lock").exists(),
        "a refused lock must not write a lockfile"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("drft lock <path>...") && stderr.contains("--all"),
        "the error should name both remedies, got: {stderr}"
    );
}

/// The refusal leaves an existing lockfile untouched, so a mis-expanded command
/// cannot re-snapshot the baseline on its way to failing.
#[test]
fn lock_with_no_paths_leaves_an_existing_lockfile_alone() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(dir.path().join("a.md"), "# a").unwrap();
    lock_all(dir.path());
    let before = fs::read_to_string(dir.path().join("drft.lock")).unwrap();

    fs::write(dir.path().join("a.md"), "# a edited").unwrap();
    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));

    assert_eq!(
        fs::read_to_string(dir.path().join("drft.lock")).unwrap(),
        before,
        "the lockfile should be byte-identical after a refused lock"
    );
    let stdout = check(dir.path());
    assert!(
        stdout.contains("stale-node]: a.md"),
        "the staleness the refused lock would have cleared must still be reported, got: {stdout}"
    );
}

/// `--all` names the whole-graph lock the bare form used to be: every node,
/// including ones never passed on the command line.
#[test]
fn lock_all_locks_every_node() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    for name in ["a", "b", "c"] {
        fs::write(dir.path().join(format!("{name}.md")), format!("# {name}")).unwrap();
    }
    lock_all(dir.path());

    for name in ["a", "b", "c"] {
        fs::write(
            dir.path().join(format!("{name}.md")),
            format!("# {name} edited"),
        )
        .unwrap();
    }

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock", "--all"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let stdout = check(dir.path());
    assert!(
        !stdout.contains("stale"),
        "--all should clear every node, got: {stdout}"
    );
}

/// `--all` and paths state two different scopes. Honoring either one silently
/// would be the same mis-scoped write the flag exists to prevent, so it errors.
#[test]
fn lock_all_with_paths_is_a_usage_error() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(dir.path().join("a.md"), "# a").unwrap();
    fs::write(dir.path().join("b.md"), "# b").unwrap();
    lock_all(dir.path());
    fs::write(dir.path().join("a.md"), "# a edited").unwrap();
    fs::write(dir.path().join("b.md"), "# b edited").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock", "--all", "a.md"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let stdout = check(dir.path());
    assert!(
        stdout.contains("stale-node]: a.md") && stdout.contains("stale-node]: b.md"),
        "neither scope should have been written, got: {stdout}"
    );
}

/// The refusal reaches a JSON consumer as an envelope, not as bare stderr prose.
/// The `--format` scan that produces it runs before clap parses, so it is easy to
/// break without noticing from the text path alone.
#[test]
fn lock_with_no_paths_errors_as_a_json_envelope() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(dir.path().join("a.md"), "# a").unwrap();

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "lock",
            "--format",
            "json",
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    let parsed: serde_json::Value =
        serde_json::from_str(stderr.trim()).unwrap_or_else(|e| panic!("not JSON ({e}): {stderr}"));
    assert_eq!(parsed["exit_code"], 2);
    assert!(
        parsed["error"]
            .as_str()
            .is_some_and(|e| e.contains("--all")),
        "the envelope should name the remedy, got: {parsed}"
    );
}

/// `--all` has no short form. Spelling out the call that asserts whole-graph
/// review is the point, and a long flag is greppable — so a hook or CI check can
/// forbid `--all` while leaving scoped locks alone.
#[test]
fn lock_all_has_no_short_form() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(dir.path().join("a.md"), "# a").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock", "-a"])
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "-a must not be an alias for --all"
    );
    assert!(
        !dir.path().join("drft.lock").exists(),
        "-a must not have locked anything"
    );
}
