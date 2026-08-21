mod common;
use common::drft_bin;
use std::fs;
use tempfile::TempDir;

/// Every JSON result document carries a `hints` key, empty included, so a
/// consumer can read `.hints[]` without first testing whether the key exists.
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

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("hint[unknown-rule]"), "stderr: {stderr}");
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
