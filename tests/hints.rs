mod common;
use common::drft_bin;
use std::fs;
use tempfile::TempDir;

/// Every JSON *result document* carries a `hints` key, empty included, so a
/// consumer can read `.hints[]` without first testing whether the key exists.
/// `graph` is excluded on purpose and covered separately below: its JSON root is
/// a JGF document, where a sibling key would cost the translatability the format
/// was chosen for. `lock` prints a result document and so belongs here.
#[test]
fn json_documents_always_carry_a_hints_key() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(dir.path().join("index.md"), "# Index").unwrap();

    for command in [
        vec!["nodes"],
        vec!["edges"],
        vec!["check"],
        vec!["impact", "index.md"],
        vec!["lock", "index.md"],
    ] {
        let mut args = vec!["-C", dir.path().to_str().unwrap(), "--format", "json"];
        args.extend(command.iter().copied());
        let output = drft_bin().args(&args).output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
        assert!(
            v["hints"].is_array(),
            "{command:?} has no hints array: {stdout}"
        );
    }
}

/// `drft graph --format json` is a JGF document, whose root is exactly `graph`.
/// Hints would be a sibling key the format does not define, so they take stderr
/// instead — in both formats.
#[test]
fn graph_json_keeps_a_bare_jgf_root_and_hints_on_stderr() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        format!(
            "{}[rules]\nnot-a-rule = \"error\"\n",
            common::DEFAULT_CONFIG
        ),
    )
    .unwrap();
    fs::write(dir.path().join("index.md"), "# Index").unwrap();

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "--format",
            "json",
            "graph",
        ])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    assert_eq!(v.as_object().unwrap().keys().collect::<Vec<_>>(), ["graph"]);

    // JSON in, JSON out: the hints take a stderr envelope rather than the text
    // rendering, so a consumer parsing stderr still gets structure.
    let stderr = String::from_utf8_lossy(&output.stderr);
    let e: serde_json::Value =
        serde_json::from_str(stderr.trim()).expect("stderr is a JSON envelope");
    assert_eq!(e["hints"][0]["name"], "unknown-rule");
}

/// An unknown rule name configures nothing, which is silent by construction —
/// so it becomes a hint carrying the config key as its locus.
#[test]
fn unknown_rule_hints_with_the_config_key_as_locus() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        format!(
            "{}[rules]\nstale-nodes = \"error\"\n",
            common::DEFAULT_CONFIG
        ),
    )
    .unwrap();
    fs::write(dir.path().join("index.md"), "# Index").unwrap();

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
    let hint = &v["hints"][0];
    assert_eq!(hint["name"], "unknown-rule");
    assert_eq!(hint["locus"], "rules.stale-nodes");
    assert!(
        hint["next"].as_str().unwrap().contains("stale-node"),
        "next should list the built-ins: {hint}"
    );
}

/// A glob that matches nothing is a legitimate empty answer, so it stays exit 0
/// — and reads exactly like a clean projection of nothing, which is what the
/// hint exists to distinguish.
#[test]
fn zero_match_selector_hints_without_failing() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(dir.path().join("index.md"), "# Index").unwrap();

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "--format",
            "json",
            "nodes",
            "**/*.rs",
        ])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    assert_eq!(v["total"], 0);
    assert_eq!(v["hints"][0]["name"], "zero-match-selector");
    assert_eq!(v["hints"][0]["locus"], "**/*.rs");
    assert!(
        output.status.success(),
        "a hint must not change the exit code"
    );
}

/// The guard the hint must not replace: an empty argument list to `lock` is a
/// collapsed shell substitution, and still refuses.
#[test]
fn hints_do_not_downgrade_the_empty_lock_guard() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(dir.path().join("index.md"), "# Index").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
}

/// A projection big enough to crowd its reader says so, reporting both the node
/// count and the rendered size. Text and JSON measure their own rendering, so
/// each gets the hint on its own output.
#[test]
fn large_projection_hints_on_rendered_size() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    // Enough nodes to push the rendered projection past the threshold. Each file
    // carries frontmatter so its node metadata, not just its key, has weight.
    for i in 0..400 {
        fs::write(
            dir.path().join(format!("note-{i:03}.md")),
            format!("---\npurpose: node {i} in a projection large enough to warn about\n---\n\n# Note {i}\n"),
        )
        .unwrap();
    }

    let json = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "--format",
            "json",
            "nodes",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&json.stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    let hint = v["hints"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["name"] == "large-projection")
        .expect("expected a large-projection hint");
    assert!(
        hint["message"].as_str().unwrap().contains("KB of output"),
        "message should carry both numbers: {hint}"
    );
    assert!(json.status.success());

    let text = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "nodes"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&text.stderr);
    assert!(
        stderr.contains("hint[large-projection]"),
        "text hints ride stderr: {stderr}"
    );
    // The projection itself stays on stdout, uncontaminated by the advice.
    assert!(!String::from_utf8_lossy(&text.stdout).contains("hint["));
}

/// An unparseable lockfile is recoverable, so it hints and points at the one
/// `--all` lock that is correct rather than failing the command.
#[test]
fn unparseable_lock_hints_and_check_still_runs() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(dir.path().join("index.md"), "# Index").unwrap();
    fs::write(dir.path().join("drft.lock"), "not valid toml {{{").unwrap();

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
    let hint = v["hints"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["name"] == "unparseable-lock")
        .expect("expected an unparseable-lock hint");
    assert_eq!(hint["locus"], "drft.lock");
    assert!(hint["next"].as_str().unwrap().contains("--all"));
}

/// `lock` prints no result document, so its hints have nowhere to embed. They
/// must still reach the reader — in JSON, as an envelope on stderr, the shape
/// the error path already uses.
///
/// The case that matters: an unparseable lockfile used to read as absent, so a
/// scoped lock replaced the whole file with only the paths named — every other
/// entry gone, and the nodes behind them left as unlocked leaves whose loss no
/// rule reports. A hint cannot mitigate that, because the destruction happens
/// anyway. A scoped lock cannot preserve a baseline it cannot read, so it refuses,
/// and the assertion that matters is that the file is still there afterward.
#[test]
fn a_scoped_lock_refuses_to_replace_an_unparseable_baseline() {
    for format in [vec![], vec!["--format", "json"]] {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
        for name in ["a.md", "b.md", "c.md"] {
            fs::write(dir.path().join(name), "# Note").unwrap();
        }

        let baseline = drft_bin()
            .args(["-C", dir.path().to_str().unwrap(), "lock", "--all"])
            .output()
            .unwrap();
        assert!(baseline.status.success());

        let corrupt = "not valid toml {{{";
        fs::write(dir.path().join("drft.lock"), corrupt).unwrap();

        let mut args = vec!["-C", dir.path().to_str().unwrap()];
        args.extend(format.iter().copied());
        args.extend(["lock", "a.md"]);
        let output = drft_bin().args(&args).output().unwrap();

        assert!(
            !output.status.success(),
            "{format:?} should refuse rather than truncate the baseline"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("drft.lock")).unwrap(),
            corrupt,
            "{format:?} rewrote a lockfile it could not read"
        );

        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("could not be parsed"),
            "{format:?} refused without saying why: stderr={stderr:?}"
        );
    }
}

/// Hints raised before a failure ride the error envelope in JSON, rather than
/// vanishing because no result document was ever printed.
#[test]
fn error_envelope_carries_hints() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        format!(
            "{}[rules]\nnot-a-rule = \"error\"\n",
            common::DEFAULT_CONFIG
        ),
    )
    .unwrap();
    fs::write(dir.path().join("index.md"), "# Index").unwrap();

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "--format",
            "json",
            "nodes",
            "no-such-file.md",
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    let v: serde_json::Value = serde_json::from_str(stderr.trim()).expect("JSON error envelope");
    assert_eq!(v["exit_code"], 2);
    assert_eq!(v["hints"][0]["name"], "unknown-rule");
}

/// The threshold has to hold in both directions: a small projection must stay
/// quiet, or the hint means nothing when it does fire.
#[test]
fn small_projection_raises_no_hint() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(dir.path().join("index.md"), "# Index").unwrap();

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "--format",
            "json",
            "nodes",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    assert_eq!(v["hints"].as_array().unwrap().len(), 0, "got: {v}");
}

/// A repeated selector is one mistake. Hints dedupe with the keys they resolve.
#[test]
fn a_repeated_selector_hints_once() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(dir.path().join("index.md"), "# Index").unwrap();

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "--format",
            "json",
            "nodes",
            "**/*.rs",
            "**/*.rs",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    assert_eq!(v["hints"].as_array().unwrap().len(), 1, "got: {v}");
}

/// `next` names a move the reader can actually make, so the whole-graph verbs —
/// which take neither a selector nor `--namespace` / `--field` — must not be
/// told to use one.
#[test]
fn large_projection_next_fits_the_command() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    for i in 0..400 {
        fs::write(
            dir.path().join(format!("note-{i:03}.md")),
            format!("---\npurpose: node {i} in a projection large enough to warn about\n---\n\n# Note {i}\n"),
        )
        .unwrap();
    }

    let graph = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "graph"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&graph.stderr);
    assert!(
        stderr.contains("hint[large-projection]"),
        "stderr: {stderr}"
    );
    assert!(
        !stderr.contains("--namespace"),
        "graph takes no --namespace, so next must not name it: {stderr}"
    );

    let nodes = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "nodes"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&nodes.stderr);
    assert!(stderr.contains("--namespace"), "stderr: {stderr}");
}

/// The rename: `unresolved-edge` names its likely cause under `cause`, both in
/// the JSON document and in text output.
#[test]
fn unresolved_edge_names_its_cause_not_a_hint() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::create_dir(dir.path().join("docs")).unwrap();
    fs::write(dir.path().join("lib.rs"), "// root file").unwrap();
    // A root-relative path from inside docs/: resolves from the graph root, not
    // from the declaring file, which is the case that earns a cause.
    fs::write(
        dir.path().join("docs/guide.md"),
        "---\nsources:\n  - lib.rs\n---\n\n# Guide\n",
    )
    .unwrap();

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
    let finding = v["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "unresolved-edge")
        .expect("expected an unresolved-edge finding");
    assert!(finding.get("hint").is_none(), "renamed away: {finding}");
    assert!(
        finding["cause"]
            .as_str()
            .unwrap()
            .contains("resolves from the graph root"),
        "got: {finding}"
    );

    let text = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "--color",
            "never",
            "check",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&text.stdout);
    assert!(stdout.contains("  cause: "), "got: {stdout}");
}
