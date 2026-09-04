mod common;
use common::drft_bin;
use std::fs;
use std::path::Path;
use std::process::Stdio;
use tempfile::TempDir;

fn assert_closed_pipe(dir: &Path, args: &[&str]) {
    let mut child = drft_bin()
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    drop(child.stdout.take());
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{args:?} failed after its stdout reader closed: status={:?} stderr={:?}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "{args:?} wrote to stderr after its stdout reader closed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Closing a result reader stops every read command quietly, in either format.
///
/// Long names and two edges per file push every result beyond the pipe buffer, so
/// dropping the read end exercises a real write failure rather than a race with a
/// small result that the operating system accepted in full.
#[test]
fn read_outputs_survive_a_closed_pipe() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        format!(
            "{}\n[rules]\nunresolved-edge = \"error\"\ndetached-node = \"off\"\n",
            common::MARKDOWN_ONLY_CONFIG
        ),
    )
    .unwrap();
    fs::write(dir.path().join("seed.md"), "# Seed").unwrap();
    fs::write(dir.path().join("lonely.md"), "# Lonely").unwrap();
    for i in 0..1200 {
        let name = format!("node-{i:04}-{}.md", "x".repeat(72));
        fs::write(
            dir.path().join(name),
            format!("[seed](seed.md)\n[missing](missing-{i:04}.md)"),
        )
        .unwrap();
    }

    for args in [
        vec!["config", "--show-ignores"],
        vec!["--format", "json", "config", "--show-ignores"],
        vec!["graph", "--raw"],
        vec!["graph"],
        vec!["--format", "json", "graph"],
        vec!["nodes", "--all"],
        vec!["--format", "json", "nodes", "--all"],
        vec!["edges", "--all"],
        vec!["--format", "json", "edges", "--all"],
        vec!["impact", "seed.md"],
        vec!["--format", "json", "impact", "seed.md"],
        vec!["impact", "lonely.md"],
        vec!["check"],
        vec!["--format", "json", "check"],
    ] {
        assert_closed_pipe(dir.path(), &args);
    }
}

/// A clean check emits the `{diagnostics, summary}` envelope with empty
/// diagnostics and zero counts.
///
/// The baseline is established first, deliberately. Without one there is nothing
/// to compare against, so every staleness rule is a no-op and the envelope comes
/// back empty for a reason that has nothing to do with the tree being clean —
/// which is what `no-baseline` now reports.
#[test]
fn check_json_envelope_clean() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "[index](index.md)").unwrap();
    // Silence detached-node so we exercise the clean envelope shape; index and
    // setup link each other via markdown, so their edges resolve cleanly.
    fs::write(
        dir.path().join("drft.toml"),
        format!(
            "{}[rules]\ndetached-node = \"off\"\n",
            common::DEFAULT_CONFIG
        ),
    )
    .unwrap();

    let baseline = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock", "--all"])
        .output()
        .unwrap();
    assert!(baseline.status.success());

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
    assert_eq!(
        v["diagnostics"].as_array().unwrap().len(),
        0,
        "diagnostics: {}",
        v["diagnostics"]
    );
    assert_eq!(v["summary"]["errors"], 0);
    assert_eq!(v["summary"]["warnings"], 0);
    assert!(output.status.success());
}
