mod common;

use common::drft_bin;
use std::fs;
use tempfile::TempDir;

fn fixture() -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(dir.path().join("seed.md"), "# Sééd\n").unwrap();
    fs::write(dir.path().join("dependent.md"), "[seed](seed.md)\n").unwrap();
    dir
}

fn assert_exact_boundary(dir: &TempDir, args: &[&str], json_error: bool) {
    let baseline = drft_bin()
        .arg("-C")
        .arg(dir.path())
        .args(args)
        .output()
        .unwrap();
    assert!(
        baseline.status.success(),
        "baseline {args:?} failed: {}",
        String::from_utf8_lossy(&baseline.stderr)
    );
    let bytes = baseline.stdout.len();
    assert!(bytes > 0);

    let exact = drft_bin()
        .arg("-C")
        .arg(dir.path())
        .args(args)
        .args(["--max-bytes", &bytes.to_string()])
        .output()
        .unwrap();
    assert!(exact.status.success(), "exact budget failed for {args:?}");
    assert_eq!(exact.stdout, baseline.stdout);

    let over = drft_bin()
        .arg("-C")
        .arg(dir.path())
        .args(args)
        .args(["--max-bytes", &(bytes - 1).to_string()])
        .output()
        .unwrap();
    assert_eq!(over.status.code(), Some(2), "{args:?}");
    assert!(over.stdout.is_empty(), "{args:?} emitted partial stdout");
    let stderr = String::from_utf8_lossy(&over.stderr);
    assert!(stderr.contains(&format!("{bytes} bytes")), "{stderr}");
    assert!(
        stderr.contains(&format!("--max-bytes {}", bytes - 1)),
        "{stderr}"
    );
    if json_error {
        let envelope: serde_json::Value = serde_json::from_slice(&over.stderr)
            .unwrap_or_else(|e| panic!("{args:?} did not emit a JSON error: {e}: {stderr}"));
        assert_eq!(envelope["exit_code"], 2);
    }
}

#[test]
fn every_read_surface_refuses_before_stdout_at_the_exact_utf8_boundary() {
    let dir = fixture();
    for (args, json_error) in [
        (vec!["nodes", "--all"], false),
        (vec!["--format", "json", "nodes", "--all"], true),
        (vec!["edges", "--all"], false),
        (vec!["--format", "json", "edges", "--all"], true),
        (vec!["graph"], false),
        (vec!["--format", "json", "graph"], true),
        (vec!["graph", "--raw"], true),
        (vec!["impact", "seed.md"], false),
        (vec!["--format", "json", "impact", "seed.md"], true),
    ] {
        assert_exact_boundary(&dir, &args, json_error);
    }
}

#[test]
fn nodes_and_edges_require_an_explicit_base_set() {
    let dir = fixture();
    for command in ["nodes", "edges"] {
        let missing = drft_bin()
            .arg("-C")
            .arg(dir.path())
            .arg(command)
            .output()
            .unwrap();
        assert_eq!(missing.status.code(), Some(2));
        assert!(missing.stdout.is_empty());

        let conflict = drft_bin()
            .arg("-C")
            .arg(dir.path())
            .args([command, "--all", "seed.md"])
            .output()
            .unwrap();
        assert_eq!(conflict.status.code(), Some(2));
    }

    for (command, namespace, field) in [
        ("nodes", "markdown", "anchors"),
        ("edges", "markdown", "line"),
    ] {
        for filter in ["--namespace", "--field"] {
            let value = if filter == "--namespace" {
                namespace
            } else {
                field
            };
            let filtered_all = drft_bin()
                .arg("-C")
                .arg(dir.path())
                .args([command, "--all", filter, value])
                .output()
                .unwrap();
            assert!(
                filtered_all.status.success(),
                "{command} {filter} failed: {}",
                String::from_utf8_lossy(&filtered_all.stderr)
            );
        }
    }
}

#[test]
fn budget_errors_preserve_hints_in_the_formats_error_channel() {
    let dir = fixture();
    fs::write(
        dir.path().join("drft.toml"),
        format!(
            "{}\n[rules]\nnot-a-rule = \"warn\"\n",
            common::DEFAULT_CONFIG
        ),
    )
    .unwrap();

    let text = drft_bin()
        .arg("-C")
        .arg(dir.path())
        .args(["--color", "never", "nodes", "--all", "--max-bytes", "0"])
        .output()
        .unwrap();
    assert_eq!(text.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&text.stderr);
    assert!(stderr.starts_with("hint[unknown-rule]"), "{stderr}");
    assert_eq!(stderr.matches("hint[unknown-rule]").count(), 1);
    assert!(stderr.contains("\nerror: rendered output is"), "{stderr}");

    for args in [
        vec!["--format", "json", "nodes", "--all"],
        vec!["graph", "--raw"],
    ] {
        let output = drft_bin()
            .arg("-C")
            .arg(dir.path())
            .args(args)
            .args(["--max-bytes", "0"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        let envelope: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(envelope["exit_code"], 2);
        assert_eq!(envelope["hints"][0]["name"], "unknown-rule");
    }
}
