mod common;
use common::drft_bin;
use std::fs;
use tempfile::TempDir;

/// Declares the markdown graph — there are no default graphs, so tests that
/// exercise link edges must declare it.
const MD_CONFIG: &str = "[graphs.markdown]\nparser = \"markdown\"\nfiles = [\"**/*.md\"]\n";

/// A graph with all links resolved fires no unresolved-edge and no errors.
#[test]
fn clean_graph_has_no_unresolved_or_errors() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), MD_CONFIG).unwrap();
    fs::write(
        dir.path().join("index.md"),
        "[setup](setup.md) and [faq](faq.md)",
    )
    .unwrap();
    fs::write(dir.path().join("setup.md"), "[config](config.md)").unwrap();
    fs::write(dir.path().join("config.md"), "# Config").unwrap();
    fs::write(dir.path().join("faq.md"), "[index](index.md)").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("unresolved-edge"),
        "expected no broken links, got: {stdout}"
    );
    assert!(
        !stdout.contains("error["),
        "expected no errors, got: {stdout}"
    );
    assert!(output.status.success(), "expected exit code 0");
}

/// A broken link fires unresolved-edge as a warning, exit 0.
#[test]
fn broken_link_warns() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), MD_CONFIG).unwrap();
    fs::write(
        dir.path().join("index.md"),
        "[setup](setup.md) and [missing](gone.md)",
    )
    .unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("warn[unresolved-edge]: index.md"),
        "expected unresolved-edge warning on index.md, got: {stdout}"
    );
    assert!(
        stdout.contains("gone.md"),
        "expected gone.md in output, got: {stdout}"
    );
    assert!(
        output.status.success(),
        "expected exit code 0 (warning only)"
    );
}

/// The JSON envelope is `{diagnostics, summary}`; each diagnostic carries
/// `{name, severity, subject, _graphs, message}`.
#[test]
fn broken_link_json_shape() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), MD_CONFIG).unwrap();
    fs::write(dir.path().join("index.md"), "[missing](gone.md)").unwrap();

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "--format",
            "json",
            "check",
        ])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    assert_eq!(v["summary"]["errors"], 0);
    assert!(v["summary"]["warnings"].as_u64().unwrap() >= 1);

    let diagnostics = v["diagnostics"].as_array().unwrap();
    let broken = diagnostics
        .iter()
        .find(|d| d["name"] == "unresolved-edge")
        .expect("expected unresolved-edge diagnostic");
    assert_eq!(broken["severity"], "warn");
    assert_eq!(broken["subject"], "index.md");
    assert_eq!(
        broken["target"], "gone.md",
        "target should name the missing node"
    );
    assert_eq!(broken["_graphs"], serde_json::json!(["@markdown"]));
    assert!(output.status.success());
}

/// A rule promoted to `error` in config exits 1.
#[test]
fn broken_link_error_severity_exits_1() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        format!("{MD_CONFIG}[rules]\nunresolved-edge = \"error\"\n"),
    )
    .unwrap();
    fs::write(dir.path().join("index.md"), "[missing](gone.md)").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("error[unresolved-edge]"),
        "expected error-level unresolved-edge, got: {stdout}"
    );
    assert_eq!(output.status.code(), Some(1), "expected exit code 1");
}

/// detached-node warns by default for a file with no links in or out.
#[test]
fn detached_node_warns_by_default() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), MD_CONFIG).unwrap();
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup").unwrap();
    fs::write(dir.path().join("orphan.md"), "# Orphan").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("warn[detached-node]: orphan.md (no connections)"),
        "expected detached-node warning for orphan.md, got: {stdout}"
    );
    assert!(output.status.success(), "warnings should exit 0");
}

/// A per-rule `ignore` glob drops findings whose subject matches.
#[test]
fn detached_node_ignore_glob() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "[rules.detached-node]\nignore = [\"orphan.md\"]\n",
    )
    .unwrap();
    fs::write(dir.path().join("orphan.md"), "# Orphan").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("orphan.md"),
        "orphan.md should be ignored, got: {stdout}"
    );
    assert!(output.status.success());
}

/// Running without drft.toml fails with exit 2.
#[test]
fn no_config_exits_with_error() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("index.md"), "# Hello").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no drft.toml found"),
        "expected config error, got: {stderr}"
    );
    assert_eq!(output.status.code(), Some(2), "expected exit code 2");
}

/// With no lockfile, `check` says so once rather than reporting nothing.
///
/// This is the failure that motivated the rule: a lockfile went missing, `check`
/// was run as the verification step, exit 0 came back, and exit 0 was read as
/// proof the graph was fine. Every staleness rule had become a no-op.
#[test]
fn a_missing_lockfile_reports_no_baseline() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), MD_CONFIG).unwrap();
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("no-baseline"), "stdout={stdout:?}");
    assert!(stdout.contains("drft.lock"), "stdout={stdout:?}");
}

/// A lockfile with no entries is the same fact as no lockfile: nothing to compare
/// against. The file existing is what made this one look established.
#[test]
fn an_empty_lockfile_reports_no_baseline() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), MD_CONFIG).unwrap();
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup").unwrap();
    fs::write(dir.path().join("drft.lock"), "node = []\n").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("no-baseline"), "stdout={stdout:?}");
}

/// `no-baseline` is a rule, not a hint, so a repo that wants the missing baseline
/// to fail its run can promote it. Hints never change an exit code, which is why
/// a hint-only answer would have left an automated caller exactly as blind.
#[test]
fn no_baseline_is_promotable_to_error() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        format!("{MD_CONFIG}\n[rules]\nno-baseline = \"error\"\n"),
    )
    .unwrap();
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(1),
        "a promoted no-baseline must fail the run"
    );
}

/// A node with no lock entry is reported. Before this it was compared against
/// nothing and reported nothing, so its coverage loss was invisible.
#[test]
fn a_node_absent_from_the_lockfile_is_reported() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), MD_CONFIG).unwrap();
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup").unwrap();
    fs::write(dir.path().join("orphan.md"), "# Orphan").unwrap();

    let lock = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock", "--all"])
        .output()
        .unwrap();
    assert!(lock.status.success());

    // Drop orphan.md's own entry, leaving every other entry intact.
    let lockfile = fs::read_to_string(dir.path().join("drft.lock")).unwrap();
    let mut parts = lockfile.split("[[node]]");
    let head = parts.next().unwrap().to_string();
    let kept: Vec<&str> = parts
        .filter(|block| !block.trim_start().starts_with("path = \"orphan.md\""))
        .collect();
    fs::write(
        dir.path().join("drft.lock"),
        format!("{head}[[node]]{}", kept.join("[[node]]")),
    )
    .unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("unlocked-node") && stdout.contains("orphan.md"),
        "stdout={stdout:?}"
    );
    assert!(
        !stdout.contains("unlocked-node]: index.md"),
        "only the dropped node is unlocked: stdout={stdout:?}"
    );
}

/// A correctly locked graph reports no `unlocked-node` at all.
///
/// A directory carries no hash and no outbound edge, so it is absent from a
/// correct lockfile by design. A rule that compared node counts, or derived its
/// own idea of what is lockable, would report every directory as a defect the day
/// it landed.
#[test]
fn a_fully_locked_graph_has_no_unlocked_node() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), MD_CONFIG).unwrap();
    fs::create_dir(dir.path().join("docs")).unwrap();
    fs::create_dir(dir.path().join("docs").join("deep")).unwrap();
    fs::write(dir.path().join("docs").join("a.md"), "# A").unwrap();
    fs::write(dir.path().join("docs").join("deep").join("b.md"), "# B").unwrap();
    fs::write(dir.path().join("index.md"), "[a](docs/a.md)").unwrap();

    let lock = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock", "--all"])
        .output()
        .unwrap();
    assert!(lock.status.success());

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("unlocked-node"),
        "a fully locked graph must be quiet: stdout={stdout:?}"
    );
    assert!(!stdout.contains("no-baseline"), "stdout={stdout:?}");
}

/// An unlocked node subsumes its outbound `new-edge` findings. The node having no
/// baseline is the one fact that explains every one of them, so it is stated once
/// rather than repeated per edge.
#[test]
fn an_unlocked_node_subsumes_its_new_edges() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), MD_CONFIG).unwrap();
    fs::write(dir.path().join("a.md"), "# A").unwrap();
    fs::write(dir.path().join("b.md"), "# B").unwrap();

    let lock = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock", "--all"])
        .output()
        .unwrap();
    assert!(lock.status.success());

    // A brand-new file linking both locked files: one unlocked node, two edges.
    fs::write(dir.path().join("new.md"), "[a](a.md) and [b](b.md)").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("unlocked-node"), "stdout={stdout:?}");
    assert!(
        !stdout.contains("new-edge"),
        "the unlocked node subsumes them: stdout={stdout:?}"
    );
}

/// A graph with nothing lockable in it stays quiet. A tree of directories alone
/// is consistent with having no lockfile, so `no-baseline` would be reporting an
/// absence that covers nothing.
#[test]
fn a_graph_with_nothing_lockable_reports_no_baseline_nothing() {
    let dir = TempDir::new().unwrap();
    // Top-level `ignore`, not a graph key — `[graphs.*]` denies unknown fields, so
    // an `ignore` inside one is a config error that exits 2 with empty stdout, and
    // an assertion on absent output would pass without testing anything.
    fs::write(
        dir.path().join("drft.toml"),
        "ignore = [\"**/*.md\", \"drft.toml\"]\n\n[graphs.md]\nparser = \"markdown\"\nfiles = [\"**/*.md\"]\n",
    )
    .unwrap();
    fs::create_dir(dir.path().join("empty")).unwrap();
    fs::write(dir.path().join("empty").join("a.md"), "# A").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "config must be valid or this asserts nothing: stderr={:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("no-baseline"),
        "nothing could have been locked: stdout={stdout:?}"
    );
}

/// An unlocked node that is the target of a locked edge does not claim its drift
/// is unchecked. The source's recorded target hash still catches the edit, and
/// `stale-edge` reports it in the same run — a message saying the file is
/// unchecked would contradict the finding printed beside it.
#[test]
fn an_unlocked_target_of_a_locked_edge_does_not_claim_it_is_unchecked() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), MD_CONFIG).unwrap();
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup").unwrap();

    let lock = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock", "--all"])
        .output()
        .unwrap();
    assert!(lock.status.success());

    // Drop setup.md's own entry, leaving index.md's locked edge to it intact.
    let lockfile = fs::read_to_string(dir.path().join("drft.lock")).unwrap();
    let mut parts = lockfile.split("[[node]]");
    let head = parts.next().unwrap().to_string();
    let kept: Vec<&str> = parts
        .filter(|block| !block.trim_start().starts_with("path = \"setup.md\""))
        .collect();
    fs::write(
        dir.path().join("drft.lock"),
        format!("{head}[[node]]{}", kept.join("[[node]]")),
    )
    .unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup CHANGED").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("stale-edge"), "stdout={stdout:?}");
    assert!(stdout.contains("unlocked-node"), "stdout={stdout:?}");
    // Assert against the message the rule actually emits. An assertion on the
    // absence of a phrase that appears nowhere else can never fail.
    assert!(
        stdout.contains("no baseline of its own"),
        "stdout={stdout:?}"
    );
}

/// Silencing `unlocked-node` restores the `new-edge` findings it stands in for.
///
/// The subsumption is applied after severity and ignore globs, so a node
/// configured to be quieter does not go dark: it reports what it reported before
/// the rule existed, with the line numbers `new-edge` carries. Subsuming before
/// the filter dropped both findings and lost coverage that predates this rule.
#[test]
fn silencing_unlocked_node_restores_the_new_edges_it_subsumes() {
    let dir = TempDir::new().unwrap();
    let config = MD_CONFIG.to_string();
    fs::write(dir.path().join("drft.toml"), &config).unwrap();
    fs::write(dir.path().join("a.md"), "# A").unwrap();
    fs::write(dir.path().join("b.md"), "# B").unwrap();

    let lock = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock", "--all"])
        .output()
        .unwrap();
    assert!(lock.status.success());
    fs::write(dir.path().join("new.md"), "[a](a.md) and [b](b.md)").unwrap();

    let run = |dir: &std::path::Path| {
        let out = drft_bin()
            .args(["-C", dir.to_str().unwrap(), "check"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    };

    let subsumed = run(dir.path());
    assert!(subsumed.contains("unlocked-node"), "{subsumed}");
    assert!(!subsumed.contains("new-edge"), "{subsumed}");

    fs::write(
        dir.path().join("drft.toml"),
        format!("{config}\n[rules]\nunlocked-node = \"off\"\n"),
    )
    .unwrap();

    let silenced = run(dir.path());
    assert!(!silenced.contains("unlocked-node"), "{silenced}");
    assert_eq!(
        silenced.matches("new-edge").count(),
        2,
        "both edges must be reported once the subsuming rule is off: {silenced}"
    );
}

/// Subsuming must not weaken the run. A `warn` `unlocked-node` standing in for an
/// `error` `new-edge` would turn exit 1 into exit 0 — a repo gating CI on
/// `new-edge` would stop failing with nothing saying so.
#[test]
fn subsumption_does_not_downgrade_a_more_severe_finding() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        format!("{MD_CONFIG}\n[rules]\nnew-edge = \"error\"\n"),
    )
    .unwrap();
    fs::write(dir.path().join("a.md"), "# A").unwrap();
    fs::write(dir.path().join("b.md"), "# B").unwrap();

    let lock = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock", "--all"])
        .output()
        .unwrap();
    assert!(lock.status.success());
    fs::write(dir.path().join("new.md"), "[a](a.md)\n[b](b.md)\n").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        output.status.code(),
        Some(1),
        "an error-severity new-edge must still fail the run: {stdout}"
    );
    assert!(stdout.contains("error[new-edge]"), "{stdout}");
}
