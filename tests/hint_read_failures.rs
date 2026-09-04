mod common;

use common::drft_bin;
use serde_json::Value;
use std::fs;
use tempfile::TempDir;

const REPAIR: &str = "repair the unreadable files matched by this graph, then rerun";
const CONFIG: &str =
    "[graphs.fm]\nparser = 'frontmatter'\nfiles = ['bad.md']\nedge_keys = ['sources']\n";

fn json(dir: &TempDir, command: &[&str]) -> Value {
    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "--format", "json"])
        .args(command)
        .output()
        .unwrap();
    assert!(
        output.status.code().is_some_and(|code| code < 2),
        "{output:?}"
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn hint<'a>(result: &'a Value, graph: &str) -> &'a Value {
    result["hints"]
        .as_array()
        .unwrap()
        .iter()
        .find(|hint| {
            hint["name"] == "edge-keys-matched-nothing"
                && hint["locus"] == format!("graphs.{graph}")
        })
        .unwrap()
}

#[test]
fn read_failure_advice_survives_diagnostic_policy_and_read_commands() {
    for (rule, bytes) in [
        ("unreadable-frontmatter", &b"---\nsources: [\n---\n"[..]),
        ("unreadable-text", &b"\xff"[..]),
    ] {
        for policy in [
            "severity = 'off'",
            "severity = 'warn'",
            "severity = 'error'",
            "ignore = ['bad.md']",
        ] {
            let dir = TempDir::new().unwrap();
            fs::write(
                dir.path().join("drft.toml"),
                format!("{CONFIG}\n[rules.{rule}]\n{policy}\n"),
            )
            .unwrap();
            fs::write(dir.path().join("bad.md"), bytes).unwrap();
            fs::write(dir.path().join("seed.md"), "# seed").unwrap();
            for command in [
                &["check"][..],
                &["impact", "seed.md"][..],
                &["edges", "--all"][..],
            ] {
                let result = json(&dir, command);
                assert_eq!(
                    hint(&result, "fm")["next"],
                    REPAIR,
                    "{rule} {policy} {command:?}: {result}"
                );
            }
            let text = drft_bin()
                .args(["-C", dir.path().to_str().unwrap(), "edges", "--all"])
                .output()
                .unwrap();
            let stderr = String::from_utf8(text.stderr).unwrap();
            assert!(stderr.contains(REPAIR), "{stderr}");
            assert!(!stderr.contains("spelling"), "{stderr}");
        }
    }
}

#[test]
fn overlapping_graphs_partition_the_same_failed_file_independently() {
    for bytes in [&b"---\nsources: [\n---\n"[..], &b"\xff"[..]] {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("drft.toml"),
            format!(
                "{CONFIG}
[graphs.mixed]
parser = 'frontmatter'
files = ['*.md']
edge_keys = ['sources']
[graphs.readable]
parser = 'frontmatter'
files = ['plain.md']
edge_keys = ['sources']
[graphs.metadata]
parser = 'frontmatter'
files = ['bad.md']
"
            ),
        )
        .unwrap();
        fs::write(dir.path().join("bad.md"), bytes).unwrap();
        fs::write(dir.path().join("plain.md"), "---\nwrong: target.md\n---\n").unwrap();
        let result = json(&dir, &["check"]);
        assert_eq!(hint(&result, "fm")["next"], REPAIR);
        assert!(
            hint(&result, "mixed")["next"]
                .as_str()
                .unwrap()
                .starts_with(&format!("{REPAIR}; if no edges remain"))
        );
        assert!(
            hint(&result, "readable")["next"]
                .as_str()
                .unwrap()
                .contains("spelling")
        );
        assert!(
            !result["hints"]
                .as_array()
                .unwrap()
                .iter()
                .any(|h| h["locus"] == "graphs.metadata")
        );
    }
}

#[test]
fn walked_exclusions_do_not_become_failed_candidates() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), "ignore = ['bad.md']\n[graphs.fm]\nparser = 'frontmatter'\nfiles = ['*.md']\nedge_keys = ['sources']\n").unwrap();
    fs::write(dir.path().join("bad.md"), b"\xff").unwrap();
    fs::create_dir(dir.path().join("folder.md")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("absent", dir.path().join("link.md")).unwrap();
    let result = json(&dir, &["edges", "--all"]);
    assert!(
        hint(&result, "fm")["message"]
            .as_str()
            .unwrap()
            .contains("no file was read")
    );
    assert!(
        !hint(&result, "fm")["next"]
            .as_str()
            .unwrap()
            .contains("repair")
    );
}
