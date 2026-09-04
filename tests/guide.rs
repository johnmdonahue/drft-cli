mod common;

use common::drft_bin;
use serde_json::Value;
use std::fs;
use tempfile::TempDir;

fn guide() -> Value {
    let output = drft_bin()
        .args(["guide", "--format", "json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn guide_needs_no_repository_config_or_accessible_directory() {
    let dir = TempDir::new().unwrap();
    for invalid_config in [false, true] {
        if invalid_config {
            fs::write(dir.path().join("drft.toml"), "[broken").unwrap();
            fs::write(dir.path().join("drft.lock"), "do not touch").unwrap();
        }
        for format in ["text", "json"] {
            let output = drft_bin()
                .current_dir(dir.path())
                .args([
                    "-C",
                    "does-not-exist",
                    "guide",
                    "--format",
                    format,
                    "--color",
                    "always",
                ])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(output.stderr.is_empty());
            assert!(!output.stdout.contains(&0x1b));
            if format == "json" {
                let value: Value = serde_json::from_slice(&output.stdout).unwrap();
                assert_eq!(value["schema_version"], "drft-guide/1");
                assert_eq!(value["drft_version"], env!("CARGO_PKG_VERSION"));
            }
        }
        if invalid_config {
            assert_eq!(
                fs::read_to_string(dir.path().join("drft.toml")).unwrap(),
                "[broken"
            );
            assert_eq!(
                fs::read_to_string(dir.path().join("drft.lock")).unwrap(),
                "do not touch"
            );
        } else {
            assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
        }
    }
}

#[test]
fn guide_is_discoverable_and_has_no_graph_controls() {
    let help = drft_bin().arg("--help").output().unwrap();
    assert!(String::from_utf8_lossy(&help.stdout).contains("guide"));
    for args in [
        vec!["guide", "--max-bytes", "1"],
        vec!["guide", "unexpected"],
    ] {
        let output = drft_bin().args(args).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn guide_result_fields_match_the_actual_serializers() {
    let document = guide();
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::MARKDOWN_ONLY_CONFIG).unwrap();
    fs::write(dir.path().join("a.md"), "# A\n").unwrap();
    fs::write(dir.path().join("b.md"), "# B\n[A](a.md)\n").unwrap();
    for (name, args) in [
        ("lock", vec!["lock", "a.md"]),
        ("nodes", vec!["nodes", "--all"]),
        ("edges", vec!["edges", "--all"]),
        ("impact", vec!["impact", "a.md"]),
        ("check", vec!["check"]),
    ] {
        let output = drft_bin()
            .current_dir(dir.path())
            .args(args)
            .args(["--format", "json"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        let record = document["commands"]
            .as_array()
            .unwrap()
            .iter()
            .find(|record| record["name"] == name)
            .unwrap();
        let mut advertised: Vec<_> = record["success_document"]["fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect();
        advertised.sort_unstable();
        let mut actual: Vec<_> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        actual.sort_unstable();
        assert_eq!(advertised, actual, "{name}");
    }
}

#[test]
fn check_status_and_graph_channel_exceptions_match_the_guide() {
    let document = guide();
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.md"), "# A\n").unwrap();
    for (severity, expected) in [("warn", 0), ("error", 1)] {
        fs::write(
            dir.path().join("drft.toml"),
            format!("[rules]\nno-baseline = \"{severity}\"\n"),
        )
        .unwrap();
        let output = drft_bin()
            .current_dir(dir.path())
            .args(["check", "--format", "json"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(expected));
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(
            result["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["name"] == "no-baseline")
        );
        assert!(
            document["exit_codes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["code"] == expected)
        );
    }
    fs::write(
        dir.path().join("drft.toml"),
        "[rules]\nunknown-test-rule = \"warn\"\n",
    )
    .unwrap();
    let graph = drft_bin()
        .current_dir(dir.path())
        .args(["graph", "--format", "json"])
        .output()
        .unwrap();
    assert!(graph.status.success());
    let stdout: Value = serde_json::from_slice(&graph.stdout).unwrap();
    assert_eq!(
        stdout.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["graph"]
    );
    let stderr: Value = serde_json::from_slice(&graph.stderr).unwrap();
    assert!(!stderr["hints"].as_array().unwrap().is_empty());
    let raw = drft_bin()
        .current_dir(dir.path())
        .args(["graph", "--raw", "--format", "text"])
        .output()
        .unwrap();
    assert!(raw.status.success());
    let _: Value = serde_json::from_slice(&raw.stdout).unwrap();
    assert!(String::from_utf8_lossy(&raw.stderr).contains("hint[unknown-rule]"));
    let raw_policy = document["output"]["exceptions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["mode"] == "graph --raw")
        .unwrap();
    assert_eq!(raw_policy["hint_channel"], "stderr");
    assert_eq!(raw_policy["hint_format"], "selected-format");
    assert_eq!(document["output"]["json_colorized"], false);
    assert_eq!(document["output"]["truncates"], false);
}
