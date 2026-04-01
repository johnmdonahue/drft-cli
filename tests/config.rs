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
        stdout.contains("dangling-edge"),
        "expected dangling-edge rule: {stdout}"
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
    // Should not have a path field when not recursive
    assert!(
        parsed.get("path").is_none(),
        "non-recursive should not have path"
    );
}

#[test]
fn config_show_recursive() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "include = [\"*.md\"]\n[rules]\nstale = \"error\"\n",
    )
    .unwrap();

    // Create a child graph
    let child = dir.path().join("sub");
    fs::create_dir(&child).unwrap();
    fs::write(
        child.join("drft.toml"),
        "[rules]\ndangling-edge = \"error\"\n",
    )
    .unwrap();

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "config",
            "show",
            "--recursive",
        ])
        .output()
        .unwrap();

    assert!(output.status.success(), "expected exit code 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should contain labels for both graphs
    assert!(
        stdout.contains("# ."),
        "expected root label '# .': {stdout}"
    );
    assert!(
        stdout.contains("# sub"),
        "expected child label '# sub': {stdout}"
    );
}

#[test]
fn config_show_recursive_json() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), "include = [\"*.md\"]\n").unwrap();

    let child = dir.path().join("child");
    fs::create_dir(&child).unwrap();
    fs::write(child.join("drft.toml"), "[rules]\nstale = \"error\"\n").unwrap();

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "config",
            "show",
            "--recursive",
            "--format",
            "json",
        ])
        .output()
        .unwrap();

    assert!(output.status.success(), "expected exit code 0");
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Recursive JSON emits a single array of config objects
    let parsed: Vec<serde_json::Value> =
        serde_json::from_str(&stdout).expect("expected valid JSON array");
    assert_eq!(parsed.len(), 2, "expected 2 graphs in array");
    assert_eq!(parsed[0]["path"], ".", "expected root path");
    assert_eq!(parsed[1]["path"], "child", "expected child path");
}
