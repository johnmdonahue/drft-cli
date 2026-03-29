use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn drft_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_drft"))
}

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

// ── drft init ──────────────────────────────────────────────────

#[test]
fn init_creates_config() {
    let dir = TempDir::new().unwrap();
    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "init"])
        .output()
        .unwrap();

    assert!(output.status.success(), "expected exit code 0");
    let config = fs::read_to_string(dir.path().join("drft.toml")).unwrap();
    assert!(config.contains("[rules]"));
    assert!(config.contains("broken-link"));
}

#[test]
fn init_fails_if_exists() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), "# existing").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "init"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2), "expected exit code 2");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("already exists"),
        "expected 'already exists' in stderr: {stderr}"
    );
}

// ── drft lock ──────────────────────────────────────────────────

/// Scenario 5: First lock — creates drft.lock, subsequent check is clean.
#[test]
fn scenario_5_first_lock() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup").unwrap();

    // Lock
    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock"])
        .output()
        .unwrap();
    assert!(output.status.success(), "lock should exit 0");

    // Verify lockfile exists and has correct format
    let lockfile = fs::read_to_string(dir.path().join("drft.lock")).unwrap();
    assert!(lockfile.contains("lockfile_version = 1"));
    assert!(lockfile.contains("index.md"));
    assert!(lockfile.contains("setup.md"));
    assert!(lockfile.contains("b3:"));
    assert!(lockfile.contains(r#"type = "document""#));

    // Check should be clean
    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout, "", "expected clean check after lock, got: {stdout}");
    assert!(output.status.success());
}

/// Scenario 6: Staleness — dependency changed after lock.
#[test]
fn scenario_6_staleness_after_edit() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup").unwrap();

    // Lock
    drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock"])
        .output()
        .unwrap();

    // Edit setup.md
    fs::write(dir.path().join("setup.md"), "# Setup (edited)").unwrap();

    // Check should report staleness
    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("warn[stale]: index.md (stale via setup.md)"),
        "expected stale warning, got: {stdout}"
    );
    assert!(output.status.success(), "expected exit 0 (warning only)");
}

/// Scenario 7a: File removed — both broken-link and stale fire.
#[test]
fn scenario_7a_file_removed() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup").unwrap();

    // Lock
    drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock"])
        .output()
        .unwrap();

    // Delete setup.md
    fs::remove_file(dir.path().join("setup.md")).unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("broken-link"),
        "expected broken-link warning, got: {stdout}"
    );
    assert!(
        stdout.contains("stale"),
        "expected stale warning, got: {stdout}"
    );
    assert!(output.status.success());
}

// ── drft lock --check ──────────────────────────────────────────

/// Scenario 24: lock --check when lockfile is current.
#[test]
fn scenario_24_lock_check_current() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup").unwrap();

    drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock"])
        .output()
        .unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock", "--check"])
        .output()
        .unwrap();

    assert!(output.status.success(), "expected exit 0");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "",
        "expected no stdout"
    );
}

/// Scenario 25: lock --check when lockfile is stale.
#[test]
fn scenario_25_lock_check_stale() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup").unwrap();

    drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock"])
        .output()
        .unwrap();

    // Edit a file
    fs::write(dir.path().join("setup.md"), "# Setup (edited)").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock", "--check"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1), "expected exit code 1");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("out of date"),
        "expected 'out of date' in stderr: {stderr}"
    );
}

/// Scenario 26: lock --check with no lockfile.
#[test]
fn scenario_26_lock_check_no_lockfile() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock", "--check"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1), "expected exit code 1");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not found"),
        "expected 'not found' in stderr: {stderr}"
    );
}

// ── Scopes & recursive ────────────────────────────────────────

/// Scenario 10: Child scope — unsealed. Parent links to child file, no violation.
#[test]
fn scenario_10_child_scope_unsealed() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("index.md"),
        "[overview](research/overview.md)",
    )
    .unwrap();

    let research = dir.path().join("research");
    fs::create_dir(&research).unwrap();
    fs::write(research.join("overview.md"), "# Overview").unwrap();
    fs::write(research.join("notes.md"), "# Notes").unwrap();

    // Lock child first, then parent
    drft_bin()
        .args(["-C", research.to_str().unwrap(), "lock"])
        .output()
        .unwrap();
    drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock"])
        .output()
        .unwrap();

    // Verify lockfile has frontier + virtual nodes
    let lockfile = fs::read_to_string(dir.path().join("drft.lock")).unwrap();
    assert!(lockfile.contains("type = \"frontier\""));
    assert!(lockfile.contains("type = \"virtual\""));

    // Check should be clean (unsealed = no encapsulation)
    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("encapsulation"),
        "unsealed scope should not trigger encapsulation"
    );
    assert!(output.status.success());
}

/// Scenario 12: Encapsulation violation — link to non-manifest file in sealed scope.
#[test]
fn scenario_12_encapsulation_violation() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("index.md"),
        "[overview](research/overview.md)\n[internal](research/internal.md)",
    )
    .unwrap();

    let research = dir.path().join("research");
    fs::create_dir(&research).unwrap();
    fs::write(research.join("overview.md"), "# Overview").unwrap();
    fs::write(research.join("internal.md"), "# Internal").unwrap();

    // Lock child with manifest (only overview exposed)
    drft_bin()
        .args([
            "-C",
            research.to_str().unwrap(),
            "lock",
            "--manifest",
            "overview.md",
        ])
        .output()
        .unwrap();
    drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock"])
        .output()
        .unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("encapsulation"),
        "expected encapsulation violation, got: {stdout}"
    );
    assert!(
        stdout.contains("research/internal.md"),
        "should mention the non-manifest file"
    );
    assert!(output.status.success(), "default severity is warn");
}

/// Recursive lock creates all lockfiles bottom-up.
#[test]
fn recursive_lock() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("index.md"), "[child](child/index.md)").unwrap();

    let child = dir.path().join("child");
    fs::create_dir(&child).unwrap();
    fs::write(child.join("drft.toml"), "").unwrap();
    fs::write(child.join("index.md"), "# Child").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock", "--recursive"])
        .output()
        .unwrap();
    assert!(output.status.success());

    // Both lockfiles should exist
    assert!(dir.path().join("drft.lock").exists());
    assert!(child.join("drft.lock").exists());

    // lock --check --recursive should pass
    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "lock",
            "--check",
            "--recursive",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
}

/// Recursive check runs child scopes with their own config.
#[test]
fn recursive_check_with_child_config() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("index.md"), "# Root").unwrap();

    let child = dir.path().join("child");
    fs::create_dir(&child).unwrap();
    fs::write(child.join("drft.toml"), "[rules]\norphan = \"warn\"\n").unwrap();
    fs::write(child.join("drft.lock"), "lockfile_version = 1\n").unwrap();
    fs::write(child.join("linked.md"), "# Linked").unwrap();
    fs::write(child.join("orphan.md"), "# Orphan").unwrap();

    // Non-recursive: no child diagnostics
    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("orphan"),
        "non-recursive should not show child diagnostics"
    );

    // Recursive: child's orphan config kicks in
    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check", "--recursive"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("[child]"),
        "recursive should show scope header, got: {stdout}"
    );
    assert!(stdout.contains("orphan"), "child's orphan rule should fire");
}

/// Scope boundary staleness — child gains drft.lock after parent locked.
#[test]
fn scope_boundary_staleness() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("index.md"), "[docs](docs/readme.md)").unwrap();

    let docs = dir.path().join("docs");
    fs::create_dir(&docs).unwrap();
    fs::write(docs.join("readme.md"), "# Docs").unwrap();

    // Lock parent (docs/ has no drft.lock yet)
    drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock"])
        .output()
        .unwrap();

    // Now create a child scope in docs/
    drft_bin()
        .args(["-C", docs.to_str().unwrap(), "lock"])
        .output()
        .unwrap();

    // Parent should detect the new scope boundary
    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("scope boundary changed"),
        "expected scope boundary staleness, got: {stdout}"
    );
}

// ── Impact ─────────────────────────────────────────────────────

#[test]
fn impact_shows_transitive_dependents() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "[config](config.md)").unwrap();
    fs::write(dir.path().join("config.md"), "# Config").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "impact", "config.md"])
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
fn impact_json_format() {
    let dir = TempDir::new().unwrap();
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

#[test]
fn impact_md_extension_fallback() {
    let dir = TempDir::new().unwrap();
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

// ── Custom rules ───────────────────────────────────────────────

#[test]
fn custom_rule_integration() {
    let dir = TempDir::new().unwrap();

    let scripts = dir.path().join("scripts");
    fs::create_dir(&scripts).unwrap();
    fs::write(
        scripts.join("count-nodes.sh"),
        "#!/bin/sh\necho '{\"message\": \"custom check ran\", \"node\": \"test\"}'\n",
    )
    .unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            scripts.join("count-nodes.sh"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }

    fs::write(
        dir.path().join("drft.toml"),
        "[custom-rules.count-nodes]\ncommand = \"./scripts/count-nodes.sh\"\nseverity = \"warn\"\n",
    )
    .unwrap();
    fs::write(dir.path().join("index.md"), "# Hello").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("count-nodes"),
        "custom rule should appear in output, got: {stdout}"
    );
    assert!(stdout.contains("custom check ran"));
    assert!(output.status.success());
}

// ── Frontmatter & wikilinks ────────────────────────────────────

#[test]
fn frontmatter_sources_create_edges() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("analysis.md"),
        "---\nsources:\n  - ./data/notes.md\n---\n\n# Analysis\n",
    )
    .unwrap();
    let data = dir.path().join("data");
    fs::create_dir(&data).unwrap();
    fs::write(data.join("notes.md"), "# Notes").unwrap();

    // Lock and verify the edge exists
    drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock"])
        .output()
        .unwrap();

    let lockfile = fs::read_to_string(dir.path().join("drft.lock")).unwrap();
    assert!(lockfile.contains("analysis.md"));
    assert!(lockfile.contains("data/notes.md"));
    assert!(
        lockfile.contains(r#"type = "frontmatter""#),
        "edge should be frontmatter type"
    );

    // Edit the source, check for staleness
    fs::write(data.join("notes.md"), "# Notes (edited)").unwrap();
    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("stale"),
        "frontmatter dep should trigger staleness, got: {stdout}"
    );
}

#[test]
fn wikilinks_create_edges() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("index.md"), "See [[setup]] for details.").unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup").unwrap();

    // Lock and verify wikilink edge
    drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock"])
        .output()
        .unwrap();

    let lockfile = fs::read_to_string(dir.path().join("drft.lock")).unwrap();
    assert!(lockfile.contains("setup.md"));
    assert!(
        lockfile.contains(r#"type = "wikilink""#),
        "edge should be wikilink type"
    );

    // Broken wikilink should be caught
    fs::write(dir.path().join("index.md"), "See [[missing]] here.").unwrap();
    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("broken-link"),
        "broken wikilink should fire broken-link, got: {stdout}"
    );
}

// ── Graph export ───────────────────────────────────────────────

#[test]
fn graph_json_follows_jgf() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "graph"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");

    // JGF single graph format
    assert!(v["graph"]["directed"].as_bool().unwrap());
    assert!(v["graph"]["nodes"]["index.md"].is_object());
    assert!(v["graph"]["nodes"]["setup.md"].is_object());
    assert_eq!(
        v["graph"]["nodes"]["index.md"]["metadata"]["type"],
        "document"
    );
    assert!(!v["graph"]["edges"].as_array().unwrap().is_empty());
}

#[test]
fn graph_recursive_produces_multiple_graphs() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("index.md"), "[child](child/index.md)").unwrap();

    let child = dir.path().join("child");
    fs::create_dir(&child).unwrap();
    fs::write(child.join("index.md"), "# Child").unwrap();
    fs::write(child.join("drft.lock"), "lockfile_version = 1\n").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "graph", "--recursive"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");

    // JGF multi-graph format
    let graphs = v["graphs"].as_array().expect("should have graphs array");
    assert!(graphs.len() >= 2, "should have root + child graph");
}

// ── JSON envelope ──────────────────────────────────────────────

#[test]
fn check_json_envelope_clean() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup").unwrap();

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
    assert_eq!(v["status"], "clean");
    assert_eq!(v["total"], 0);
    assert_eq!(v["errors"], 0);
    assert_eq!(v["warnings"], 0);
}

// ── Ignore rules ───────────────────────────────────────────────

#[test]
fn ignore_rules_suppresses_diagnostics() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "[rules]\norphan = \"warn\"\n\n[ignore-rules]\norphan = [\"README.md\"]\n",
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
    fs::write(dir.path().join("index.md"), "# Hello").unwrap();

    drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock"])
        .output()
        .unwrap();

    let lockfile = fs::read_to_string(dir.path().join("drft.lock")).unwrap();
    assert!(lockfile.starts_with("lockfile_version = 1"));
}

// ── Redundant edge ────────────────────────────────────────────

/// redundant-edge is off by default — no output even with redundant edges.
#[test]
fn redundant_edge_off_by_default() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.md"), "[b](b.md) [c](c.md)").unwrap();
    fs::write(dir.path().join("b.md"), "[c](c.md)").unwrap();
    fs::write(dir.path().join("c.md"), "# C").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("redundant-edge"),
        "redundant-edge should be off by default"
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

/// --rule redundant-edge overrides off to warn.
#[test]
fn redundant_edge_via_rule_flag() {
    let dir = TempDir::new().unwrap();
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

// ── Report command ────────────────────────────────────────────

#[test]
fn report_text_output() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.md"), "[b](b.md) [c](c.md)").unwrap();
    fs::write(dir.path().join("b.md"), "[c](c.md)").unwrap();
    fs::write(dir.path().join("c.md"), "# C").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "report"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("=== degree ==="),
        "expected degree header, got: {stdout}"
    );
    assert!(
        stdout.contains("=== transitive-reduction ==="),
        "expected transitive-reduction header, got: {stdout}"
    );
    assert!(
        stdout.contains("a.md"),
        "expected source in report, got: {stdout}"
    );
}

#[test]
fn report_json_output() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.md"), "[b](b.md) [c](c.md)").unwrap();
    fs::write(dir.path().join("b.md"), "[c](c.md)").unwrap();
    fs::write(dir.path().join("c.md"), "# C").unwrap();

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "--format",
            "json",
            "report",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");

    // Degree analysis is present
    assert!(
        v["analyses"]["degree"]["nodes"].is_array(),
        "expected degree analysis in JSON output"
    );

    // Transitive reduction analysis is present
    let tr = &v["analyses"]["transitive-reduction"];
    let edges = tr["redundant_edges"].as_array().unwrap();
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0]["source"], "a.md");
    assert_eq!(edges[0]["target"], "c.md");
}

#[test]
fn report_no_redundancy() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.md"), "[b](b.md)").unwrap();
    fs::write(dir.path().join("b.md"), "# B").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "report"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("no redundant edges"),
        "expected clean report, got: {stdout}"
    );
}

#[test]
fn report_unknown_analysis_exits_2() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.md"), "# A").unwrap();

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "report",
            "--analysis",
            "nonexistent",
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn report_degree_text_output() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.md"), "[b](b.md) [c](c.md)").unwrap();
    fs::write(dir.path().join("b.md"), "[c](c.md)").unwrap();
    fs::write(dir.path().join("c.md"), "# C").unwrap();

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "report",
            "--analysis",
            "degree",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("=== degree ==="),
        "expected degree header, got: {stdout}"
    );
    assert!(
        stdout.contains("a.md"),
        "expected a.md in degree output, got: {stdout}"
    );
    // a.md has out:2 (links to b and c)
    assert!(
        stdout.contains("out:2"),
        "expected out:2 for a.md, got: {stdout}"
    );
}

#[test]
fn report_degree_json_output() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.md"), "[b](b.md) [c](c.md)").unwrap();
    fs::write(dir.path().join("b.md"), "[c](c.md)").unwrap();
    fs::write(dir.path().join("c.md"), "# C").unwrap();

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "--format",
            "json",
            "report",
            "--analysis",
            "degree",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    let deg = &v["analyses"]["degree"];
    let nodes = deg["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 3);

    let a = nodes.iter().find(|n| n["node"] == "a.md").unwrap();
    assert_eq!(a["in_degree"], 0);
    assert_eq!(a["out_degree"], 2);

    let c = nodes.iter().find(|n| n["node"] == "c.md").unwrap();
    assert_eq!(c["in_degree"], 2);
    assert_eq!(c["out_degree"], 0);
}

#[test]
fn report_graph_stats_text() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.md"), "[b](b.md)").unwrap();
    fs::write(dir.path().join("b.md"), "[a](a.md)").unwrap();

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "report",
            "--analysis",
            "graph-stats",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("=== graph-stats ==="), "got: {stdout}");
    assert!(stdout.contains("nodes: 2"), "got: {stdout}");
    assert!(stdout.contains("edges: 2"), "got: {stdout}");
    assert!(stdout.contains("density:"), "got: {stdout}");
    assert!(stdout.contains("diameter: 1"), "got: {stdout}");
}

#[test]
fn report_graph_stats_json() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.md"), "[b](b.md)").unwrap();
    fs::write(dir.path().join("b.md"), "[a](a.md)").unwrap();

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "--format",
            "json",
            "report",
            "--analysis",
            "graph-stats",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    let gs = &v["analyses"]["graph-stats"];
    assert_eq!(gs["node_count"], 2);
    assert_eq!(gs["edge_count"], 2);
    assert_eq!(gs["diameter"], 1);
    assert!(gs["density"].as_f64().unwrap() > 0.0);
}

#[test]
fn report_connected_components_text() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.md"), "[b](b.md)").unwrap();
    fs::write(dir.path().join("b.md"), "# B").unwrap();
    fs::write(dir.path().join("c.md"), "# C").unwrap(); // isolated

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "report",
            "--analysis",
            "connected-components",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("=== connected-components ==="),
        "expected header, got: {stdout}"
    );
    assert!(
        stdout.contains("2 components"),
        "expected 2 components, got: {stdout}"
    );
}

#[test]
fn report_connected_components_json() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.md"), "[b](b.md)").unwrap();
    fs::write(dir.path().join("b.md"), "# B").unwrap();
    fs::write(dir.path().join("c.md"), "# C").unwrap();

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "--format",
            "json",
            "report",
            "--analysis",
            "connected-components",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    let cc = &v["analyses"]["connected-components"];
    assert_eq!(cc["component_count"], 2);
    assert_eq!(cc["components"].as_array().unwrap().len(), 2);
}

#[test]
fn report_scc_text() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.md"), "[b](b.md)").unwrap();
    fs::write(dir.path().join("b.md"), "[c](c.md)").unwrap();
    fs::write(dir.path().join("c.md"), "[a](a.md)").unwrap();

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "report",
            "--analysis",
            "scc",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("=== scc ==="), "got: {stdout}");
    assert!(stdout.contains("1 non-trivial SCC"), "got: {stdout}");
    assert!(stdout.contains("3 nodes"), "got: {stdout}");
}

#[test]
fn report_scc_json() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.md"), "[b](b.md)").unwrap();
    fs::write(dir.path().join("b.md"), "[a](a.md)").unwrap();
    fs::write(dir.path().join("c.md"), "# C").unwrap();

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "--format",
            "json",
            "report",
            "--analysis",
            "scc",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    let scc = &v["analyses"]["scc"];
    assert_eq!(scc["nontrivial_count"], 1);
    assert_eq!(scc["sccs"][0]["members"].as_array().unwrap().len(), 2);
}

#[test]
fn report_depth_text() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.md"), "[b](b.md)").unwrap();
    fs::write(dir.path().join("b.md"), "[c](c.md)").unwrap();
    fs::write(dir.path().join("c.md"), "# C").unwrap();

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "report",
            "--analysis",
            "depth",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("=== depth ==="), "got: {stdout}");
    assert!(stdout.contains("depth 0:"), "got: {stdout}");
    assert!(stdout.contains("depth 2:"), "got: {stdout}");
}

#[test]
fn report_depth_json() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.md"), "[b](b.md)").unwrap();
    fs::write(dir.path().join("b.md"), "# B").unwrap();

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "--format",
            "json",
            "report",
            "--analysis",
            "depth",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    let dep = &v["analyses"]["depth"];
    assert_eq!(dep["max_depth"], 1);
    let nodes = dep["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 2);
}

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

#[test]
fn report_scc_acyclic() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.md"), "[b](b.md)").unwrap();
    fs::write(dir.path().join("b.md"), "# B").unwrap();

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "report",
            "--analysis",
            "scc",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("no non-trivial SCCs"),
        "expected acyclic message, got: {stdout}"
    );
}

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

/// Issue #9: ../  links should trigger containment rule.
#[test]
fn containment_catches_escape() {
    let dir = TempDir::new().unwrap();
    let child = dir.path().join("docs");
    fs::create_dir(&child).unwrap();
    fs::write(child.join("drft.lock"), "lockfile_version = 1\n").unwrap();
    fs::write(child.join("index.md"), "[escape](../README.md)").unwrap();
    fs::write(dir.path().join("README.md"), "# Root").unwrap();

    let output = drft_bin()
        .args(["-C", child.to_str().unwrap(), "check"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("containment"),
        "expected containment violation for ../README.md, got: {stdout}"
    );
}
