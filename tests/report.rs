mod common;
use common::drft_bin;
use std::fs;
use tempfile::TempDir;

fn setup_graph() -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup").unwrap();
    dir
}

#[test]
fn report_all_json_contains_analyses_and_metrics() {
    let dir = setup_graph();
    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "report",
            "--format",
            "json",
        ])
        .output()
        .unwrap();

    assert!(output.status.success(), "drft report failed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let map = json.as_object().unwrap();

    // Should contain analyses
    assert!(map.contains_key("degree"), "missing degree analysis");
    assert!(map.contains_key("depth"), "missing depth analysis");
    assert!(map.contains_key("scc"), "missing scc analysis");

    // Should contain metrics
    assert!(
        map.contains_key("orphan_ratio"),
        "missing orphan_ratio metric"
    );
    assert!(
        map.contains_key("component_count"),
        "missing component_count metric"
    );
    assert!(
        map.contains_key("cycle_count"),
        "missing cycle_count metric"
    );

    // All keys are flat peers (no "analyses" or "metrics" grouping)
    assert!(!map.contains_key("analyses"));
    assert!(!map.contains_key("metrics"));
}

#[test]
fn report_filter_analysis_only() {
    let dir = setup_graph();
    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "report",
            "depth",
            "--format",
            "json",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let map = json.as_object().unwrap();

    assert_eq!(
        map.len(),
        1,
        "should contain only depth, got: {:?}",
        map.keys().collect::<Vec<_>>()
    );
    assert!(map.contains_key("depth"));
    assert!(map["depth"]["max_depth"].is_number());
}

#[test]
fn report_filter_metric_only() {
    let dir = setup_graph();
    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "report",
            "orphan_ratio",
            "--format",
            "json",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let map = json.as_object().unwrap();

    assert_eq!(
        map.len(),
        1,
        "should contain only orphan_ratio, got: {:?}",
        map.keys().collect::<Vec<_>>()
    );
    assert!(map.contains_key("orphan_ratio"));
    assert!(map["orphan_ratio"]["value"].is_number());
    assert!(map["orphan_ratio"]["kind"].is_string());
    assert!(map["orphan_ratio"]["dimension"].is_string());
}

#[test]
fn report_filter_mixed() {
    let dir = setup_graph();
    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "report",
            "depth",
            "orphan_ratio",
            "--format",
            "json",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let map = json.as_object().unwrap();

    assert_eq!(map.len(), 2);
    assert!(map.contains_key("depth"));
    assert!(map.contains_key("orphan_ratio"));
}

#[test]
fn report_invalid_name_errors() {
    let dir = setup_graph();
    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "report", "nonexistent"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unknown report name"),
        "expected helpful error, got: {stderr}"
    );
}
