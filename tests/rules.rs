mod common;
use common::drft_bin;
use std::fs;
use tempfile::TempDir;

// ── Ignore rules ───────────────────────────────────────────────

#[test]
fn ignore_rules_suppresses_diagnostics() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "[rules.orphan-node]\nseverity = \"warn\"\nignore = [\"README.md\"]\n",
    )
    .unwrap();
    fs::write(dir.path().join("README.md"), "# Readme").unwrap();
    fs::write(dir.path().join("other.md"), "# Other").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("README.md"),
        "README.md should be suppressed by ignore-rules"
    );
    assert!(
        stdout.contains("other.md"),
        "other.md should still be flagged"
    );
}

// ── Lockfile version ───────────────────────────────────────────

#[test]
fn lockfile_contains_version() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), "").unwrap();
    fs::write(dir.path().join("index.md"), "# Hello").unwrap();

    drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock"])
        .output()
        .unwrap();

    let lockfile = fs::read_to_string(dir.path().join("drft.lock")).unwrap();
    assert!(lockfile.starts_with("lockfile_version = 2"));
}

// ── Redundant edge ────────────────────────────────────────────

/// redundant-edge warns by default.
#[test]
fn redundant_edge_warn_by_default() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), "").unwrap();
    fs::write(dir.path().join("a.md"), "[b](b.md) [c](c.md)").unwrap();
    fs::write(dir.path().join("b.md"), "[c](c.md)").unwrap();
    fs::write(dir.path().join("c.md"), "# C").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("redundant-edge"),
        "redundant-edge should warn by default, got: {stdout}"
    );
}

/// redundant-edge enabled via drft.toml produces diagnostics.
#[test]
fn redundant_edge_enabled_via_config() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "[rules]\nredundant-edge = \"warn\"\n",
    )
    .unwrap();
    fs::write(dir.path().join("a.md"), "[b](b.md) [c](c.md)").unwrap();
    fs::write(dir.path().join("b.md"), "[c](c.md)").unwrap();
    fs::write(dir.path().join("c.md"), "# C").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("redundant-edge"),
        "expected redundant-edge diagnostic, got: {stdout}"
    );
    assert!(
        stdout.contains("a.md"),
        "expected source a.md in output, got: {stdout}"
    );
    assert!(
        stdout.contains("c.md"),
        "expected target c.md in output, got: {stdout}"
    );
    assert!(
        stdout.contains("via"),
        "expected via in output, got: {stdout}"
    );
}

/// --rule flag overrides explicitly-off rule to warn.
#[test]
fn redundant_edge_via_rule_flag() {
    let dir = TempDir::new().unwrap();
    // Explicitly disable, then override with --rule
    fs::write(
        dir.path().join("drft.toml"),
        "[rules]\nredundant-edge = \"off\"\n",
    )
    .unwrap();
    fs::write(dir.path().join("a.md"), "[b](b.md) [c](c.md)").unwrap();
    fs::write(dir.path().join("b.md"), "[c](c.md)").unwrap();
    fs::write(dir.path().join("c.md"), "# C").unwrap();

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "check",
            "--rule",
            "redundant-edge",
        ])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("redundant-edge"),
        "expected redundant-edge diagnostic via --rule flag, got: {stdout}"
    );
}

/// redundant-edge JSON output has expected fields.
#[test]
fn redundant_edge_json() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "[rules]\nredundant-edge = \"warn\"\n",
    )
    .unwrap();
    fs::write(dir.path().join("a.md"), "[b](b.md) [c](c.md)").unwrap();
    fs::write(dir.path().join("b.md"), "[c](c.md)").unwrap();
    fs::write(dir.path().join("c.md"), "# C").unwrap();

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
    let diagnostics = v["diagnostics"].as_array().unwrap();
    let redundant: Vec<&serde_json::Value> = diagnostics
        .iter()
        .filter(|d| d["rule"] == "redundant-edge")
        .collect();
    assert_eq!(redundant.len(), 1);
    assert_eq!(redundant[0]["source"], "a.md");
    assert_eq!(redundant[0]["target"], "c.md");
    assert!(redundant[0]["via"].is_string());
    assert!(redundant[0]["fix"].is_string());
}

// ── Layer violation ───────────────────────────────────────────

#[test]
fn layer_violation_rule_fires() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "[rules]\nlayer-violation = \"warn\"\n",
    )
    .unwrap();
    // a → b → c, a → c (skip-layer: depth 0 → depth 2)
    fs::write(dir.path().join("a.md"), "[b](b.md) [c](c.md)").unwrap();
    fs::write(dir.path().join("b.md"), "[c](c.md)").unwrap();
    fs::write(dir.path().join("c.md"), "# C").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("layer-violation"),
        "expected layer-violation warning, got: {stdout}"
    );
    assert!(
        stdout.contains("skip-layer"),
        "expected skip-layer message, got: {stdout}"
    );
}

// ── Fragility ─────────────────────────────────────────────────

#[test]
fn fragility_rule_fires() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "[rules]\nfragility = \"warn\"\n",
    )
    .unwrap();
    fs::write(dir.path().join("a.md"), "[b](b.md)").unwrap();
    fs::write(dir.path().join("b.md"), "[c](c.md)").unwrap();
    fs::write(dir.path().join("c.md"), "# C").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("fragility"),
        "expected fragility warning, got: {stdout}"
    );
}

// ── Fragmentation ─────────────────────────────────────────────

#[test]
fn fragmentation_rule_fires() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "[rules]\nfragmentation = \"warn\"\n",
    )
    .unwrap();
    fs::write(dir.path().join("a.md"), "[b](b.md)").unwrap();
    fs::write(dir.path().join("b.md"), "# B").unwrap();
    fs::write(dir.path().join("c.md"), "# C").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("fragmentation"),
        "expected fragmentation warning, got: {stdout}"
    );
    assert!(
        stdout.contains("disconnected component"),
        "expected disconnected component message, got: {stdout}"
    );
}

// ── Containment escape ─────────────────────────────────────────

/// Issue #9: ../  links should trigger boundary-violation rule.
#[test]
fn boundary_violation_catches_escape() {
    let dir = TempDir::new().unwrap();
    let child = dir.path().join("docs");
    fs::create_dir(&child).unwrap();
    fs::write(child.join("drft.toml"), "").unwrap();
    fs::write(child.join("index.md"), "[escape](../README.md)").unwrap();
    fs::write(dir.path().join("README.md"), "# Root").unwrap();

    let output = drft_bin()
        .args(["-C", child.to_str().unwrap(), "check"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("boundary-violation"),
        "expected boundary-violation for ../README.md, got: {stdout}"
    );
}
