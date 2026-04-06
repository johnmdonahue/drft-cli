mod common;
use common::drft_bin;
use std::fs;
use tempfile::TempDir;

#[test]
fn graph_json_follows_jgf() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), "").unwrap();
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
    assert_eq!(v["graph"]["nodes"]["index.md"]["metadata"]["type"], "file");
    assert!(!v["graph"]["edges"].as_array().unwrap().is_empty());
}

#[test]
fn graph_parser_filter_reduces_edges() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "[parsers.markdown]\n[parsers.frontmatter]\n",
    )
    .unwrap();
    // frontmatter links to docs/setup.md, markdown links to config.md — different targets
    let docs = dir.path().join("docs");
    fs::create_dir(&docs).unwrap();
    fs::write(
        dir.path().join("index.md"),
        "---\nsources:\n  - docs/setup.md\n---\n[config](config.md)",
    )
    .unwrap();
    fs::write(docs.join("setup.md"), "# Setup").unwrap();
    fs::write(dir.path().join("config.md"), "# Config").unwrap();

    // Without filter: both markdown and frontmatter edges
    let all = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "graph"])
        .output()
        .unwrap();
    let all_json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&all.stdout)).unwrap();
    let all_edges = all_json["graph"]["edges"].as_array().unwrap();
    assert_eq!(all_edges.len(), 2);

    // With filter: only frontmatter edges
    let filtered = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "graph",
            "--parser",
            "frontmatter",
        ])
        .output()
        .unwrap();
    let filtered_json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&filtered.stdout)).unwrap();
    let filtered_edges = filtered_json["graph"]["edges"].as_array().unwrap();
    assert_eq!(filtered_edges.len(), 1);
    assert_eq!(filtered_edges[0]["parser"], "frontmatter");
}

#[test]
fn graph_unknown_parser_errors() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), "").unwrap();
    fs::write(dir.path().join("index.md"), "# Index").unwrap();

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "graph",
            "--parser",
            "nonexistent",
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown parser \"nonexistent\""));
    assert!(stderr.contains("available:"));
}

#[test]
fn graph_recursive_produces_multiple_graphs() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), "").unwrap();
    fs::write(dir.path().join("index.md"), "[child](child/index.md)").unwrap();

    let child = dir.path().join("child");
    fs::create_dir(&child).unwrap();
    fs::write(child.join("index.md"), "# Child").unwrap();
    fs::write(child.join("drft.toml"), "").unwrap();

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
