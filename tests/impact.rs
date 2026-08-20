mod common;
use common::drft_bin;
use std::fs;
use tempfile::TempDir;

#[test]
fn impact_shows_transitive_dependents() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "[config](config.md)").unwrap();
    fs::write(dir.path().join("config.md"), "# Config").unwrap();

    // Transitive reach is `--depth all` now that the default is one hop.
    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "impact",
            "config.md",
            "--depth",
            "all",
        ])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("setup.md"), "setup.md depends on config.md");
    assert!(
        stdout.contains("index.md"),
        "index.md transitively depends on config.md"
    );
    assert!(output.status.success());
}

#[test]
fn impact_defaults_to_one_hop() {
    // The question asked at an edit is "what names this file directly" — each hit
    // a promise someone wrote down. `index.md` sits a hop further back and is
    // reported only by its radius, not enumerated.
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "[config](config.md)").unwrap();
    fs::write(dir.path().join("config.md"), "# Config").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "impact", "config.md"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("setup.md"),
        "direct dependent, got: {stdout}"
    );
    assert!(
        !stdout.contains("index.md"),
        "depth-2 node should not be enumerated by default, got: {stdout}"
    );
    // The wider set is still signalled rather than hidden.
    assert!(stdout.contains("radius"), "got: {stdout}");
    assert!(output.status.success());
}

#[test]
fn impact_depth_zero_is_a_usage_error() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(dir.path().join("index.md"), "# Index").unwrap();

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "impact",
            "index.md",
            "--depth",
            "0",
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2), "usage errors exit 2");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--depth all"), "got: {stderr}");
}

#[test]
fn impact_json_format() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup").unwrap();

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "--format",
            "json",
            "impact",
            "setup.md",
        ])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    assert_eq!(v["total"], 1);
    assert_eq!(v["impacted"][0]["node"], "index.md");
    assert!(v["impacted"][0]["fix"].as_str().is_some());
    assert!(output.status.success());
}

/// `drft impact` requires a subject; with no path it is a usage error (exit 2).
#[test]
fn impact_requires_a_subject() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(dir.path().join("index.md"), "# Hello").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "impact"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
}

/// A scoped `drft lock <path>` refreshes only that node, leaving the rest of the
/// lockfile intact.
#[test]
fn scoped_lock_refreshes_only_one_node() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup").unwrap();

    drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock", "--all"])
        .output()
        .unwrap();

    // Edit both files, then scope-lock only setup.md.
    fs::write(dir.path().join("setup.md"), "# Setup (edited)").unwrap();
    fs::write(dir.path().join("index.md"), "[setup](setup.md) edited").unwrap();
    drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock", "setup.md"])
        .output()
        .unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    // setup.md was re-locked, so it is no longer a stale node.
    assert!(
        !stdout.contains("stale-node]: setup.md"),
        "setup.md should be re-locked, got: {stdout}"
    );
    // index.md was edited but not re-locked, so it is still stale.
    assert!(
        stdout.contains("stale-node]: index.md"),
        "index.md should remain stale, got: {stdout}"
    );
}

/// v0.7 regression: a file inside a subdirectory containing a stray
/// `drft.toml` must still be resolvable by `drft impact`. Under v0.6 the
/// subdirectory would have been treated as a child graph and the file
/// excluded from the parent's graph.
#[test]
fn impact_resolves_file_under_nested_drft_toml() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(dir.path().join("index.md"), "[inner](nested/inner.md)").unwrap();

    let nested = dir.path().join("nested");
    fs::create_dir(&nested).unwrap();
    fs::write(nested.join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(nested.join("inner.md"), "# Inner").unwrap();

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "impact",
            "nested/inner.md",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "expected impact to resolve the nested file, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("index.md"),
        "expected index.md to show as a dependent of nested/inner.md: {stdout}"
    );
}

#[test]
fn impact_md_extension_fallback() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup").unwrap();

    // "setup" without .md should resolve to "setup.md"
    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "impact", "setup"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("index.md"));
    assert!(output.status.success());
}

/// Regression (#66): a path argument resolves relative to the current directory,
/// not as a literal graph key — so running from a subdirectory with a
/// project-relative path finds the node, matching `git log <path>` behavior.
#[test]
fn impact_resolves_path_relative_to_cwd() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(
        dir.path().join("index.md"),
        "[model](projects/api/domain-model.md)",
    )
    .unwrap();
    let sub = dir.path().join("projects/api");
    fs::create_dir_all(&sub).unwrap();
    fs::write(sub.join("domain-model.md"), "# Domain model").unwrap();

    // Run from inside the project subdir with a project-relative path (no -C).
    let output = drft_bin()
        .current_dir(&sub)
        .args(["impact", "domain-model.md"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "expected cwd-relative path to resolve, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("index.md"),
        "expected index.md as a dependent, got: {stdout}"
    );
}

/// A genuinely absent path errors (exit 2) and suggests a node whose key ends
/// with the given argument.
#[test]
fn impact_missing_path_suggests_suffix_match() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(
        dir.path().join("index.md"),
        "[model](projects/api/domain-model.md)",
    )
    .unwrap();
    let sub = dir.path().join("projects/api");
    fs::create_dir_all(&sub).unwrap();
    fs::write(sub.join("domain-model.md"), "# Domain model").unwrap();

    // Bare filename from the root: can't resolve, but should suggest the match.
    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "impact",
            "domain-model.md",
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("did you mean") && stderr.contains("projects/api/domain-model.md"),
        "expected a single suffix suggestion, got: {stderr}"
    );
}

/// An ambiguous bare filename (same name under multiple dirs) lists the matches
/// rather than confidently suggesting an arbitrary one.
#[test]
fn impact_ambiguous_suffix_lists_matches() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(
        dir.path().join("index.md"),
        "[a](a/README.md) [b](b/README.md)",
    )
    .unwrap();
    for d in ["a", "b"] {
        fs::create_dir_all(dir.path().join(d)).unwrap();
        fs::write(dir.path().join(d).join("README.md"), "# Readme").unwrap();
    }

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "impact", "README.md"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("multiple matches")
            && stderr.contains("a/README.md")
            && stderr.contains("b/README.md"),
        "expected both matches listed, not a single guess, got: {stderr}"
    );
}
