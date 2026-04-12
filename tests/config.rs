mod common;
use common::drft_bin;
use std::fs;
use tempfile::TempDir;

#[test]
fn config_show_defaults() {
    let dir = TempDir::new().unwrap();
    // Empty drft.toml — should show defaults
    fs::write(dir.path().join("drft.toml"), "").unwrap();
    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "config", "show"])
        .output()
        .unwrap();

    assert!(output.status.success(), "expected exit code 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Defaults include *.md
    assert!(
        stdout.contains("*.md"),
        "expected default include '*.md' in output: {stdout}"
    );
    // Default parser is markdown
    assert!(
        stdout.contains("[parsers.markdown]"),
        "expected markdown parser section: {stdout}"
    );
    // Default rules should be present
    assert!(
        stdout.contains("unresolved-edge"),
        "expected unresolved-edge rule: {stdout}"
    );
}

#[test]
fn config_show_with_config() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        r#"
include = ["*.md", "*.yaml"]
exclude = ["drafts/*"]

[parsers.markdown]

[rules]
stale = "error"
orphan-node = "off"
"#,
    )
    .unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "config", "show"])
        .output()
        .unwrap();

    assert!(output.status.success(), "expected exit code 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("*.yaml"),
        "expected *.yaml in include: {stdout}"
    );
    assert!(
        stdout.contains("drafts/*"),
        "expected drafts/* in exclude: {stdout}"
    );
}

#[test]
fn config_show_json_format() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        r#"
include = ["*.md"]

[rules]
stale = "error"
"#,
    )
    .unwrap();

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "config",
            "show",
            "--format",
            "json",
        ])
        .output()
        .unwrap();

    assert!(output.status.success(), "expected exit code 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("expected valid JSON: {e}\n{stdout}"));
    assert_eq!(parsed["include"][0], "*.md");
    assert_eq!(parsed["rules"]["stale"]["severity"], "error");
}
