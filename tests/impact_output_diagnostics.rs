mod common;

use common::drft_bin;
use serde_json::{Value, json};
use std::{fs, process::Output};
use tempfile::TempDir;

const INVALID: &str = "---\nnote: `unquoted span`\nsources:\n  - ./target.md\n---\nbody\n";

fn fixture(config: &str) -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), config).unwrap();
    fs::write(dir.path().join("target.md"), "# Target\n").unwrap();
    dir
}

fn run(dir: &TempDir, args: &[&str]) -> Output {
    drft_bin()
        .arg("-C")
        .arg(dir.path())
        .args(args)
        .output()
        .unwrap()
}

fn document(output: &Output) -> Value {
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

fn impact(dir: &TempDir, direction: &str) -> Value {
    document(&run(
        dir,
        &[
            "impact",
            "target.md",
            "--direction",
            direction,
            "--format",
            "json",
        ],
    ))
}

#[test]
fn unreadable_declaration_explains_empty_impact_with_and_without_history() {
    for baseline in [false, true] {
        let dir = fixture(common::DEFAULT_CONFIG);
        if baseline {
            fs::write(
                dir.path().join("doc.md"),
                "---\nsources: [./target.md]\n---\n",
            )
            .unwrap();
            assert!(run(&dir, &["lock", "doc.md", "target.md"]).status.success());
        }
        fs::write(dir.path().join("doc.md"), INVALID).unwrap();
        let before = fs::read(dir.path().join("drft.lock")).ok();
        let result = impact(&dir, "inbound");
        assert_eq!(result["impacted"], json!([]));
        assert_eq!(result["total"], 0);
        let diagnostics = result["diagnostics"].as_array().unwrap();
        assert!(
            diagnostics
                .iter()
                .any(|f| f["name"] == "unreadable-frontmatter" && f["subject"] == "doc.md")
        );
        assert_eq!(
            diagnostics.iter().any(|f| f["name"] == "removed-edge"),
            baseline
        );
        assert!(diagnostics.iter().all(|f| matches!(
            f["name"].as_str().unwrap(),
            "unreadable-frontmatter" | "removed-edge"
        )));
        let output = run(&dir, &["impact", "target.md"]);
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(
            text.starts_with("no dependents found in the current graph\n"),
            "{text}"
        );
        assert!(
            text.contains(
                "graph read has diagnostics; dependency coverage may be incomplete\nwarn["
            )
        );
        assert!(text.contains("unreadable-frontmatter]: doc.md:1"));
        assert!(!String::from_utf8(output.stderr).unwrap().contains("warn["));
        assert_eq!(fs::read(dir.path().join("drft.lock")).ok(), before);
    }
}

#[test]
fn construction_scope_covers_disconnected_and_metadata_only_graphs_in_every_direction() {
    let dir = fixture(
        "[graphs.metadata]\nparser = \"frontmatter\"\nfiles = [\"doc.md\"]\n[graphs.other]\nparser = \"markdown\"\nfiles = [\"bad.md\"]\n",
    );
    fs::write(dir.path().join("doc.md"), INVALID).unwrap();
    fs::write(dir.path().join("bad.md"), [0xff]).unwrap();
    for (direction, empty) in [
        ("inbound", "no dependents"),
        ("outbound", "no dependencies"),
        ("both", "no connected nodes"),
    ] {
        for depth in ["1", "all"] {
            let result = document(&run(
                &dir,
                &[
                    "impact",
                    "target.md",
                    "--direction",
                    direction,
                    "--depth",
                    depth,
                    "--format",
                    "json",
                ],
            ));
            assert_eq!(result["diagnostics"].as_array().unwrap().len(), 2);
            assert_eq!(result["diagnostics"][0]["_graphs"], json!(["@metadata"]));
            assert_eq!(result["diagnostics"][1]["_graphs"], json!(["@other"]));
            assert_eq!(result["hints"], json!([]));
            let output = run(
                &dir,
                &[
                    "impact",
                    "target.md",
                    "--direction",
                    direction,
                    "--depth",
                    depth,
                ],
            );
            let text = String::from_utf8(output.stdout).unwrap();
            assert!(text.starts_with(&format!("{empty} found in the current graph\n")));
            assert!(
                text.contains(
                    "graph read has diagnostics; dependency coverage may be incomplete\n"
                )
            );
        }
    }
}

#[test]
fn isolated_seed_is_quiet_with_missing_or_empty_baseline() {
    let dir = fixture(common::MARKDOWN_ONLY_CONFIG);
    for baseline in [None, Some("")] {
        if let Some(content) = baseline {
            fs::write(dir.path().join("drft.lock"), content).unwrap();
        }
        for (direction, empty) in [
            ("inbound", "no dependents"),
            ("outbound", "no dependencies"),
            ("both", "no connected nodes"),
        ] {
            let result = impact(&dir, direction);
            assert_eq!(result["diagnostics"], json!([]));
            assert_eq!(result["hints"], json!([]));
            let output = run(&dir, &["impact", "target.md", "--direction", direction]);
            assert_eq!(
                String::from_utf8(output.stdout).unwrap(),
                format!("{empty} found\n")
            );
            assert!(output.stderr.is_empty());
        }
    }
}

#[test]
fn configured_diagnostics_do_not_gate_impact_or_change_raw_hint_advice() {
    for setting in ["warn", "error", "off", "ignore"] {
        let rule = if setting == "ignore" {
            "[rules.unreadable-frontmatter]\nignore = [\"doc.md\"]\n".to_owned()
        } else {
            format!("[rules]\nunreadable-frontmatter = \"{setting}\"\n")
        };
        let config = format!(
            "[graphs.fm]\nparser = \"frontmatter\"\nfiles = [\"doc.md\"]\nedge_keys = [\"sources\"]\n{rule}"
        );
        let dir = fixture(&config);
        fs::write(dir.path().join("doc.md"), INVALID).unwrap();
        let result = impact(&dir, "inbound");
        let hidden = matches!(setting, "off" | "ignore");
        assert_eq!(result["diagnostics"].as_array().unwrap().is_empty(), hidden);
        if !hidden {
            assert_eq!(result["diagnostics"][0]["severity"], setting);
        }
        assert_eq!(
            result["hints"][0]["next"],
            "repair the unreadable files matched by this graph, then rerun"
        );
        let text = run(&dir, &["impact", "target.md"]);
        assert_eq!(text.status.code(), Some(0));
        let text = String::from_utf8(text.stdout).unwrap();
        assert_eq!(text.contains("graph read has diagnostics"), !hidden);
        assert_eq!(text.contains("in the current graph"), !hidden);
        let check = run(&dir, &["check", "--format", "json"]);
        assert_eq!(
            check.status.code(),
            Some(if setting == "error" { 1 } else { 0 })
        );
        let check: Value = serde_json::from_slice(&check.stdout).unwrap();
        assert_eq!(
            check["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|f| f["name"] == "unreadable-frontmatter")
                .cloned()
                .collect::<Vec<_>>(),
            result["diagnostics"].as_array().unwrap().clone()
        );
    }
}

#[test]
fn traversal_diagnostic_policy_and_error_exit_match_the_read_contract() {
    for setting in ["warn", "error", "off"] {
        let dir = fixture(&format!(
            "{}\n[rules]\nunresolved-edge = \"{setting}\"\n",
            common::MARKDOWN_ONLY_CONFIG
        ));
        fs::write(dir.path().join("target.md"), "[missing](missing.md)").unwrap();
        let result = impact(&dir, "outbound");
        assert_eq!(
            result["diagnostics"].as_array().unwrap().is_empty(),
            setting == "off"
        );
        if setting != "off" {
            assert_eq!(result["diagnostics"][0]["severity"], setting);
        }
        let text = run(&dir, &["impact", "target.md", "--direction", "outbound"]);
        assert!(
            !String::from_utf8(text.stdout)
                .unwrap()
                .contains("graph read has diagnostics")
        );
        assert_eq!(
            run(&dir, &["check"]).status.code(),
            Some(if setting == "error" { 1 } else { 0 })
        );
    }
}

#[test]
fn diagnostics_and_explanation_are_in_exact_atomic_output_budgets() {
    let dir = fixture(common::DEFAULT_CONFIG);
    fs::write(dir.path().join("doc.md"), INVALID).unwrap();
    for format in ["text", "json"] {
        let output = run(&dir, &["impact", "target.md", "--format", format]);
        assert!(output.status.success());
        let exact = output.stdout.len().to_string();
        let under = (output.stdout.len() - 1).to_string();
        let accepted = run(
            &dir,
            &[
                "impact",
                "target.md",
                "--format",
                format,
                "--max-bytes",
                &exact,
            ],
        );
        assert_eq!(accepted.stdout, output.stdout);
        assert!(accepted.status.success());
        let refused = run(
            &dir,
            &[
                "impact",
                "target.md",
                "--format",
                format,
                "--max-bytes",
                &under,
            ],
        );
        assert_eq!(refused.status.code(), Some(2));
        assert!(refused.stdout.is_empty());
        let error = if format == "json" {
            serde_json::from_slice::<Value>(&refused.stderr).unwrap()["error"]
                .as_str()
                .unwrap()
                .to_owned()
        } else {
            String::from_utf8(refused.stderr).unwrap()
        };
        assert!(
            error.contains("increase --max-bytes or repair the read failures"),
            "{error}"
        );
        assert!(
            error.contains("construction diagnostics cover all configured graphs"),
            "{error}"
        );
    }
}

#[test]
fn large_global_diagnostics_keep_their_scope_in_size_advice() {
    let dir = fixture("[graphs.fm]\nparser = \"frontmatter\"\nfiles = [\"broken/*.md\"]\n");
    fs::create_dir(dir.path().join("broken")).unwrap();
    for i in 0..480 {
        fs::write(dir.path().join(format!("broken/{i:04}.md")), INVALID).unwrap();
    }
    for format in ["text", "json"] {
        let output = run(&dir, &["impact", "target.md", "--format", format]);
        assert!(output.status.success());
        let (message, next) = if format == "json" {
            let result = document(&output);
            assert_eq!(result["total"], 0);
            let hint = result["hints"]
                .as_array()
                .unwrap()
                .iter()
                .find(|h| h["name"] == "large-projection")
                .unwrap();
            (
                hint["message"].as_str().unwrap().to_owned(),
                hint["next"].as_str().unwrap().to_owned(),
            )
        } else {
            let stderr = String::from_utf8(output.stderr.clone()).unwrap();
            assert!(stderr.contains("hint[large-projection]"));
            (stderr.clone(), stderr)
        };
        assert!(message.contains("0 nodes plus diagnostics"), "{message}");
        assert!(next.contains("increase --max-bytes or repair the read failures"));
        assert!(next.contains("construction diagnostics cover all configured graphs"));
        let exact = output.stdout.len().to_string();
        let accepted = run(
            &dir,
            &[
                "impact",
                "target.md",
                "--format",
                format,
                "--max-bytes",
                &exact,
            ],
        );
        assert_eq!(accepted.stdout, output.stdout);
        let under = (output.stdout.len() - 1).to_string();
        let refused = run(
            &dir,
            &[
                "impact",
                "target.md",
                "--format",
                format,
                "--max-bytes",
                &under,
            ],
        );
        assert_eq!(refused.status.code(), Some(2));
        assert!(refused.stdout.is_empty());
    }
}

#[test]
fn corrupt_lock_is_a_hint_and_lock_io_failure_is_an_error() {
    let dir = fixture(common::MARKDOWN_ONLY_CONFIG);
    let corrupt = "not valid toml {{{";
    fs::write(dir.path().join("drft.lock"), corrupt).unwrap();
    let result = impact(&dir, "inbound");
    assert_eq!(result["diagnostics"], json!([]));
    assert_eq!(result["hints"][0]["name"], "unparseable-lock");
    assert_eq!(
        fs::read_to_string(dir.path().join("drft.lock")).unwrap(),
        corrupt
    );
    fs::remove_file(dir.path().join("drft.lock")).unwrap();
    fs::create_dir(dir.path().join("drft.lock")).unwrap();
    let output = run(&dir, &["impact", "target.md"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
}

#[test]
fn historical_pairs_report_losses_without_extending_traversal() {
    let dir = fixture(common::MARKDOWN_ONLY_CONFIG);
    fs::write(dir.path().join("doc.md"), "[target](target.md)").unwrap();
    fs::write(dir.path().join("older.md"), "[doc](doc.md)").unwrap();
    fs::write(dir.path().join("peer.md"), "[target](target.md)").unwrap();
    assert!(
        run(
            &dir,
            &["lock", "doc.md", "older.md", "peer.md", "target.md"]
        )
        .status
        .success()
    );
    fs::remove_file(dir.path().join("doc.md")).unwrap();
    fs::write(dir.path().join("older.md"), "# Older").unwrap();
    fs::write(dir.path().join("peer.md"), "# Peer").unwrap();
    let before = fs::read(dir.path().join("drft.lock")).unwrap();
    let result = document(&run(
        &dir,
        &["impact", "target.md", "--depth", "all", "--format", "json"],
    ));
    assert_eq!(result["impacted"], json!([]));
    let ds = result["diagnostics"].as_array().unwrap();
    assert_eq!(ds.len(), 2, "{result}");
    assert!(
        ds.iter()
            .any(|f| f["name"] == "removed-node" && f["subject"] == "doc.md")
    );
    assert!(ds.iter().any(|f| f["name"] == "removed-edge"
        && f["subject"] == "peer.md"
        && f["target"] == "target.md"));
    let absent = run(&dir, &["impact", "doc.md"]);
    assert_eq!(absent.status.code(), Some(2));
    assert!(absent.stdout.is_empty());
    assert_eq!(fs::read(dir.path().join("drft.lock")).unwrap(), before);
}

#[cfg(unix)]
#[test]
fn finding_text_escapes_paths_while_json_preserves_bytes() {
    let dir = fixture(common::DEFAULT_CONFIG);
    let name = "broken\nrecord.md";
    fs::write(dir.path().join(name), INVALID).unwrap();
    let result = impact(&dir, "inbound");
    assert_eq!(result["diagnostics"][0]["subject"], name);
    let output = run(&dir, &["impact", "target.md"]);
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("broken\\nrecord.md:1"), "{text}");
    assert_eq!(text.lines().count(), 3);
}

#[test]
fn guide_advertises_diagnostics_lock_reads_and_non_gating_errors() {
    let dir = fixture(common::MARKDOWN_ONLY_CONFIG);
    let guide = document(&run(&dir, &["guide", "--format", "json"]));
    let command = guide["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "impact")
        .unwrap();
    assert!(
        command["reads"]
            .as_array()
            .unwrap()
            .contains(&json!("drft.lock"))
    );
    assert!(command.to_string().contains("diagnostics"));
    let boundary = command["boundary"].to_string();
    assert!(boundary.contains("completed read exits 0 even with error diagnostics"));
    assert!(boundary.contains("metadata-only graphs"));
    assert!(boundary.contains("never extend traversal"));
}
