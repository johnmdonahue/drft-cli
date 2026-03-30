mod common;
use common::drft_bin;
use std::fs;
use tempfile::TempDir;

/// Scenario 1: Zero setup — just markdown files, all links valid.
/// drft check should produce no output and exit 0.
#[test]
fn scenario_1_zero_setup_clean() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("index.md"),
        "[setup](setup.md) and [faq](faq.md)",
    )
    .unwrap();
    fs::write(dir.path().join("setup.md"), "[config](config.md)").unwrap();
    fs::write(dir.path().join("config.md"), "# Config").unwrap();
    fs::write(dir.path().join("faq.md"), "# FAQ").unwrap();
    fs::write(dir.path().join("orphan.md"), "# Orphan").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "",
        "expected no output for clean check"
    );
    assert!(output.status.success(), "expected exit code 0");
}

/// Scenario 2: Broken link — one link target does not exist.
/// drft check should warn and exit 0.
#[test]
fn scenario_2_broken_link() {
    let dir = TempDir::new().unwrap();
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
    assert_eq!(
        stdout.trim(),
        "warn[broken-link]: index.md \u{2192} gone.md (file not found)",
    );
    assert!(
        output.status.success(),
        "expected exit code 0 (warning only)"
    );
}

/// Scenario 2 with JSON format.
#[test]
fn scenario_2_broken_link_json() {
    let dir = TempDir::new().unwrap();
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
    assert_eq!(v["status"], "warn");
    assert_eq!(v["total"], 1);
    assert_eq!(v["errors"], 0);
    assert_eq!(v["warnings"], 1);
    let diag = &v["diagnostics"][0];
    assert_eq!(diag["rule"], "broken-link");
    assert_eq!(diag["severity"], "warn");
    assert_eq!(diag["source"], "index.md");
    assert_eq!(diag["target"], "gone.md");
    assert_eq!(diag["message"], "file not found");
    assert!(output.status.success());
}

/// Scenario 3: Broken link escalated to error via config.
/// Should exit 1.
#[test]
fn scenario_3_broken_link_error_severity() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "[rules]\nbroken-link = \"error\"\n",
    )
    .unwrap();
    fs::write(dir.path().join("index.md"), "[missing](gone.md)").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.trim(),
        "error[broken-link]: index.md \u{2192} gone.md (file not found)",
    );
    assert_eq!(output.status.code(), Some(1), "expected exit code 1");
}

/// Scenario 4: Cycle detection.
/// a.md → b.md → c.md → a.md should warn.
#[test]
fn scenario_4_cycle_detection() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.md"), "[b](b.md)").unwrap();
    fs::write(dir.path().join("b.md"), "[c](c.md)").unwrap();
    fs::write(dir.path().join("c.md"), "[a](a.md)").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("warn[cycle]"),
        "expected cycle warning, got: {stdout}"
    );
    assert!(
        stdout.contains("cycle detected"),
        "expected 'cycle detected' message"
    );
    // All three nodes should appear in the cycle path
    assert!(stdout.contains("a.md"), "cycle should include a.md");
    assert!(stdout.contains("b.md"), "cycle should include b.md");
    assert!(stdout.contains("c.md"), "cycle should include c.md");
    assert!(
        output.status.success(),
        "expected exit code 0 (warning only)"
    );
}

/// Scenario 20: Directory links.
/// A link to a directory should warn.
#[test]
fn scenario_20_directory_link() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("index.md"), "[guides](guides/)").unwrap();
    let guides = dir.path().join("guides");
    fs::create_dir(&guides).unwrap();
    fs::write(guides.join("README.md"), "# Guides").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("warn[directory-link]"),
        "expected directory-link warning, got: {stdout}"
    );
    assert!(
        stdout.contains("guides"),
        "should mention the directory target"
    );
    assert!(output.status.success(), "expected exit code 0");
}

/// Scenario 7b/23: Orphan rule — off by default.
/// orphan.md has no inbound links but orphan rule is off, so no output.
#[test]
fn scenario_23_orphan_off_by_default() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("index.md"), "# Hello").unwrap();
    fs::write(dir.path().join("orphan.md"), "# Orphan").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("orphan"),
        "orphan rule should be off by default"
    );
    assert!(output.status.success());
}

/// Scenario 7b: Orphan rule — enabled via config.
#[test]
fn scenario_7b_orphan_enabled() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), "[rules]\norphan = \"warn\"\n").unwrap();
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup").unwrap();
    fs::write(dir.path().join("orphan.md"), "# Orphan").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("warn[orphan]: orphan.md (no inbound links)"),
        "expected orphan warning for orphan.md, got: {stdout}"
    );
    assert!(output.status.success());
}

/// Scenario 29: --rule filtering.
/// Only the specified rule runs.
#[test]
fn scenario_29_rule_filtering() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), "[rules]\norphan = \"warn\"\n").unwrap();
    fs::write(dir.path().join("index.md"), "[missing](gone.md)").unwrap();
    fs::write(dir.path().join("orphan.md"), "# Orphan").unwrap();

    // Only run orphan rule — broken-link should not appear
    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "check",
            "--rule",
            "orphan",
        ])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("orphan"),
        "orphan rule should run, got: {stdout}"
    );
    assert!(
        !stdout.contains("broken-link"),
        "broken-link should not run when --rule orphan is specified"
    );
    assert!(output.status.success());
}

/// --rule with off rule overrides to warn.
#[test]
fn rule_flag_overrides_off_to_warn() {
    let dir = TempDir::new().unwrap();
    // orphan is off by default
    fs::write(dir.path().join("index.md"), "# Hello").unwrap();
    fs::write(dir.path().join("orphan.md"), "# Orphan").unwrap();

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "check",
            "--rule",
            "orphan",
        ])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("warn[orphan]"),
        "orphan should run at warn when specified via --rule, got: {stdout}"
    );
    assert!(output.status.success());
}
