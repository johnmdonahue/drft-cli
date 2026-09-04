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

#[test]
fn a_bare_path_does_not_drop_a_removed_markdown_entry() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(dir.path().join("guide.md"), "# Guide").unwrap();
    lock_all(dir.path());
    fs::remove_file(dir.path().join("guide.md")).unwrap();
    let before = fs::read_to_string(dir.path().join("drft.lock")).unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock", "guide"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("did you mean \"guide.md\"?"));
    assert_eq!(
        fs::read_to_string(dir.path().join("drft.lock")).unwrap(),
        before
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

/// Locking a directory fails. A directory is a graph node, but it carries no hash
/// or outbound edges and therefore has no lock entry. Exit 0 would assert that the
/// requested lock succeeded even though the command changed nothing.
#[test]
fn scoped_lock_of_a_directory_errors_and_writes_nothing() {
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
    assert_eq!(output.status.code(), Some(2));

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.is_empty(), "stdout={stdout:?}");
    assert!(
        stderr.contains("cannot lock directory node \"sub\""),
        "stderr={stderr:?}"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("drft.lock")).unwrap(),
        before,
        "a directory lock must not rewrite the baseline"
    );
}

/// Every exact-path spelling of a directory fails rather than being reinterpreted
/// as a recursive selector.
#[test]
fn every_directory_spelling_errors() {
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
        assert_eq!(output.status.code(), Some(2), "{spelling:?} should exit 2");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("cannot lock directory node"),
            "{spelling:?} did not explain the refusal: stderr={stderr:?}"
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
    assert_eq!(output.status.code(), Some(2));
    assert!(
        !dir.path().join("drft.lock").exists(),
        "a lock that wrote nothing must not create a lockfile"
    );
}

/// Every path is validated before any lock entry changes. A valid file must not
/// hide a directory operand that cannot be locked.
#[test]
fn a_directory_in_a_batch_prevents_every_write() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(dir.path().join("a.md"), "# A").unwrap();
    fs::create_dir(dir.path().join("sub")).unwrap();
    fs::write(dir.path().join("sub").join("b.md"), "# B").unwrap();
    lock_all(dir.path());
    fs::write(dir.path().join("a.md"), "# A changed").unwrap();
    let before = fs::read_to_string(dir.path().join("drft.lock")).unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock", "a.md", "sub/"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        fs::read_to_string(dir.path().join("drft.lock")).unwrap(),
        before,
        "a failed batch must leave the baseline byte-identical"
    );
    assert!(
        check(dir.path()).contains("stale-node]: a.md"),
        "the valid path in a failed batch must remain stale"
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
    // `--all` reports the count alone: it resolves nothing, so a per-node listing
    // would be a copy of `drft.lock` and, on a large graph, thousands of lines.
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout, "locked 3 nodes\n", "stdout={stdout:?}");

    // A scoped lock names what it locked, which is how a resolution the caller did
    // not expect becomes visible at the moment it happens.
    let scoped = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock", "index.md"])
        .output()
        .unwrap();
    let scoped_out = String::from_utf8_lossy(&scoped.stdout);
    assert_eq!(
        scoped_out, "locked 1 node\n  locked  index.md\n",
        "stdout={scoped_out:?}"
    );

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

/// A bare name resolves to the directory the caller spelled before any `.md`
/// variant invented from it, then fails because that exact node is not lockable.
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
    // The assertion has to name the finding, not the path. `index.md` links
    // `docs.md`, so the path appears in a `stale-edge` line whether or not
    // `docs.md` was wrongly locked — an assertion on the bare path passes with
    // the fix reverted, which makes it no test at all.
    assert!(check(dir.path()).contains("stale-node]: docs.md"));

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock", "docs"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("cannot lock directory node \"docs\""),
        "stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    let after = check(dir.path());
    assert!(
        after.contains("stale-node]: docs.md"),
        "locking the directory `docs` must not snapshot the file `docs.md`: {after}"
    );
}

/// An argument with an extension resolves to that exact node. With both `a.md`
/// and `a.md.md` present, `lock a.md` must snapshot only `a.md`.
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

/// A path that used to be a file and is now a directory can still have its stale
/// entry dropped by naming it. That is the reviewed-deletion case a scoped lock
/// exists for, and the directory hint must not displace it.
#[test]
fn locking_a_path_that_became_a_directory_drops_its_entry() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "[graphs.md]\nparser = \"markdown\"\nfiles = [\"**/*\"]\n",
    )
    .unwrap();
    fs::write(dir.path().join("notes"), "# Notes\n[i](index.md)\n").unwrap();
    fs::write(dir.path().join("index.md"), "# Index").unwrap();
    lock_all(dir.path());

    fs::remove_file(dir.path().join("notes")).unwrap();
    fs::create_dir(dir.path().join("notes")).unwrap();
    fs::write(dir.path().join("notes").join("x.md"), "# Inner").unwrap();
    assert!(check(dir.path()).contains("removed-edge"));

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock", "notes"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("dropped 1 entry"), "stdout={stdout:?}");
    assert!(stdout.contains("dropped notes"), "stdout={stdout:?}");

    let after = check(dir.path());
    assert!(
        !after.contains("removed-edge"),
        "naming the converted path must clear its finding: {after}"
    );
}

/// A lockfile that parses to zero entries is not refused — there is nothing in it
/// to lose, so a scoped lock merges into it exactly as it would in a repo that had
/// never been locked. `no-baseline` is what reports the empty baseline, at
/// `check`, where it can be promoted to an error.
///
/// The sequence this protects: a scoped lock clearing the last reviewed deletion
/// empties the file, and the next scoped lock must still work.
#[test]
fn a_scoped_lock_merges_into_an_empty_baseline() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(dir.path().join("a.md"), "# A").unwrap();
    lock_all(dir.path());
    fs::write(dir.path().join("drft.lock"), "").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock", "a.md"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "an empty baseline has nothing to lose: stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let lockfile = fs::read_to_string(dir.path().join("drft.lock")).unwrap();
    assert!(
        lockfile.contains("path = \"a.md\""),
        "lockfile={lockfile:?}"
    );
}

/// `drft lock --all` rewrites the lockfile even when the tree no longer has
/// anything to record. It is the rebuild: afterwards the file reflects the tree.
/// Skipping the write left stale entries reported as `removed-node` and
/// unclearable by the one command whose job is to rewrite the file.
#[test]
fn lock_all_rebuilds_even_when_nothing_is_lockable() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "ignore = [\"drft.toml\"]\n\n[graphs.md]\nparser = \"markdown\"\nfiles = [\"**/*.md\"]\n",
    )
    .unwrap();
    fs::write(dir.path().join("a.md"), "# A").unwrap();
    lock_all(dir.path());
    fs::remove_file(dir.path().join("a.md")).unwrap();

    lock_all(dir.path());
    let after = check(dir.path());
    assert!(
        !after.contains("removed-node"),
        "a rebuild must clear entries for files that are gone: {after}"
    );
}

/// A closed reader is not the command's failure, in either format.
///
/// `println!` panics on a broken pipe, so `drft lock --all | head` would abort
/// with exit 101 where the previously silent command exited 0. Two things make
/// this test real rather than decorative: the output has to exceed the pipe
/// buffer (64KB) or no SIGPIPE is ever delivered, and the exit code checked has
/// to be drft's own — a shell pipeline reports the exit of `head`, which is 0 no
/// matter how the writer died.
#[test]
fn lock_output_survives_a_closed_pipe() {
    for format in [vec![], vec!["--format", "json"]] {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
        // Enough nodes that either rendering comfortably exceeds the pipe buffer.
        for i in 0..8000 {
            fs::write(dir.path().join(format!("n{i}.md")), "# Note").unwrap();
        }

        let mut args = vec!["-C", dir.path().to_str().unwrap()];
        args.extend(format.iter().copied());
        args.extend(["lock", "--all"]);

        let mut child = drft_bin()
            .args(&args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();

        // Close the read end while the writer is still producing.
        drop(child.stdout.take());
        let output = child.wait_with_output().unwrap();

        assert!(
            output.status.success(),
            "{format:?} aborted on a closed pipe: status={:?} stderr={:?}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

/// A path names only the cwd-relative node the caller supplied.
#[test]
fn a_lock_path_does_not_fall_through_to_the_graph_root() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::create_dir(dir.path().join("docs")).unwrap();
    fs::write(dir.path().join("README.md"), "# Root").unwrap();
    fs::write(
        dir.path().join("docs").join("a.md"),
        "# A\n[r](../README.md)",
    )
    .unwrap();
    lock_all(dir.path());

    let output = drft_bin()
        .args([
            "-C",
            dir.path().join("docs").to_str().unwrap(),
            "lock",
            "README.md",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let after = fs::read_to_string(dir.path().join("drft.lock")).unwrap();

    // The same spelling from the graph root remains exact.
    let quiet = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock", "README.md"])
        .output()
        .unwrap();
    assert!(quiet.status.success());

    // Omitting `.md` never selects the Markdown file; it only suggests it.
    let convenience = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock", "README"])
        .output()
        .unwrap();
    assert_eq!(convenience.status.code(), Some(2));
    let convenience_err = String::from_utf8_lossy(&convenience.stderr);
    assert!(
        convenience_err.contains("did you mean \"README.md\"?"),
        "the correction must not be selected: stderr={convenience_err:?}"
    );
    let final_lock = fs::read_to_string(dir.path().join("drft.lock")).unwrap();
    assert_eq!(
        after, final_lock,
        "failed locks must not rewrite the baseline"
    );
}

/// `drft lock --all` reports the entries its rebuild removes.
///
/// It never read the file it was replacing, so it answered `dropped: []` however
/// much it dropped. Widening an `ignore` pattern and rebuilding therefore took
/// entries out of the baseline in silence — the one remaining route to losing
/// coverage without being told.
#[test]
fn lock_all_reports_the_entries_it_drops() {
    let dir = TempDir::new().unwrap();
    let base =
        "ignore = [\"drft.toml\"]\n\n[graphs.md]\nparser = \"markdown\"\nfiles = [\"**/*.md\"]\n";
    fs::write(dir.path().join("drft.toml"), base).unwrap();
    for name in ["a.md", "b.md", "c.md"] {
        fs::write(dir.path().join(name), "# Note").unwrap();
    }
    lock_all(dir.path());

    fs::write(
        dir.path().join("drft.toml"),
        "ignore = [\"drft.toml\", \"b.md\", \"c.md\"]\n\n[graphs.md]\nparser = \"markdown\"\nfiles = [\"**/*.md\"]\n",
    )
    .unwrap();

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "--format",
            "json",
            "lock",
            "--all",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    let dropped: Vec<&str> = v["dropped"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d.as_str().unwrap())
        .collect();
    assert_eq!(
        dropped,
        vec!["b.md", "c.md"],
        "the rebuild removed two entries and must say so: {stdout}"
    );
}

/// Naming one path twice is one lock. A shell substitution concatenating two
/// diffs will do it, and a count that over-reports is the untrustworthy number
/// this report exists to replace.
#[test]
fn duplicate_paths_are_locked_once() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(dir.path().join("a.md"), "# A").unwrap();
    lock_all(dir.path());

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock", "a.md", "./a.md"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.starts_with("locked 1 node\n"),
        "one path named twice is one lock: stdout={stdout:?}"
    );
}

/// A rebuild over a lockfile drft cannot read says its drop list is incomplete.
///
/// `--all` replaces the file regardless, which is correct — it is the rebuild.
/// But it cannot know what the unreadable bytes held, and reporting `dropped: []`
/// there would read as "nothing was dropped" when the truth is "I could not tell".
#[test]
fn a_rebuild_over_an_unreadable_lockfile_says_its_drops_are_unlisted() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(dir.path().join("a.md"), "# A").unwrap();
    lock_all(dir.path());
    fs::write(dir.path().join("drft.lock"), "not valid toml {{{").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock", "--all"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("replaced-unreadable-lock"),
        "an unlisted drop set must say so: stderr={stderr:?}"
    );
    // The rebuild is what fixes the file, so it must not also advise running it.
    assert!(
        !stderr.contains("unparseable-lock"),
        "a successful rebuild must not warn about the file it just replaced: {stderr:?}"
    );
}

/// A locked path that carries no content to snapshot, and is not a directory,
/// says why rather than reporting a silent success.
///
/// An escaping symlink is a node in the graph but has no hash and no outbound
/// edge, so it is never a lock entry — the same `locked 0 nodes` a directory used
/// to give with no explanation.
#[test]
fn locking_a_path_with_nothing_to_snapshot_says_why() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(dir.path().join("index.md"), "# Index").unwrap();

    let outside = TempDir::new().unwrap();
    let target = outside.path().join("elsewhere.md");
    fs::write(&target, "# Elsewhere").unwrap();
    std::os::unix::fs::symlink(&target, dir.path().join("escaping.md")).unwrap();
    lock_all(dir.path());

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock", "escaping.md"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.contains("locked 0 nodes"), "stdout={stdout:?}");
    assert!(
        stderr.contains("nothing-to-lock"),
        "a zero-node lock must say why: stderr={stderr:?}"
    );
}
