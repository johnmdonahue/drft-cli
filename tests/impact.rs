mod common;
use common::drft_bin;
use std::fs;
use tempfile::TempDir;

#[test]
fn impact_shows_transitive_dependents() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), "").unwrap();
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
    fs::write(dir.path().join("drft.toml"), "").unwrap();
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
fn impact_parser_filter() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "[parsers.markdown]\n[parsers.frontmatter]\n",
    )
    .unwrap();
    // index.md links to setup.md via markdown only
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup").unwrap();

    // Without filter: index.md depends on setup.md
    let all = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "impact", "setup.md"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&all.stdout);
    assert!(stdout.contains("index.md"));

    // With frontmatter filter: no dependents (link is markdown, not frontmatter)
    let filtered = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "impact",
            "setup.md",
            "--parser",
            "frontmatter",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&filtered.stdout);
    assert!(stdout.contains("no dependents"));
}

/// v0.7 regression: a file inside a subdirectory containing a stray
/// `drft.toml` must still be resolvable by `drft impact`. Under v0.6 the
/// subdirectory would have been treated as a child graph and the file
/// excluded from the parent's graph.
#[test]
fn impact_resolves_file_under_nested_drft_toml() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), "include = [\"**/*.md\"]\n").unwrap();
    fs::write(dir.path().join("index.md"), "[inner](nested/inner.md)").unwrap();

    let nested = dir.path().join("nested");
    fs::create_dir(&nested).unwrap();
    fs::write(nested.join("drft.toml"), "").unwrap();
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
    fs::write(dir.path().join("drft.toml"), "").unwrap();
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
