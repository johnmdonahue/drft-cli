mod common;
use common::drft_bin;
use std::fs;
use tempfile::TempDir;

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
        "source"
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
    fs::write(child.join("drft.lock"), "lockfile_version = 2\n").unwrap();

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
