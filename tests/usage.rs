//! Subprocess coverage for the experimental, local-only usage collector.
//!
//! These deliberately inspect the published wire records rather than reaching
//! into command code: collection must never alter a command's observable result.

mod common;

use common::drft_bin;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::process::{Output, Stdio};
use tempfile::TempDir;

#[cfg(unix)]
use base64::{Engine as _, engine::general_purpose::STANDARD};
#[cfg(unix)]
use std::ffi::OsString;
#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;
#[cfg(unix)]
use std::os::unix::process::CommandExt;

const GRAPH: &str = "\
[graphs.markdown]
parser = \"markdown\"
files = [\"**/*.md\"]

[rules]
detached-node = \"off\"
no-baseline = \"off\"
";

fn config(enabled: bool) -> String {
    format!("{GRAPH}\n[experimental.usage]\nenabled = {enabled}\n")
}

fn visible_config(enabled: bool) -> String {
    format!(
        "[graphs.markdown]\nparser = \"markdown\"\nfiles = [\"**/*.md\"]\n\n[rules]\ndetached-node = \"off\"\nno-baseline = \"off\"\n\n[experimental.usage]\nenabled = {enabled}\n"
    )
}

struct Fixture {
    temp: TempDir,
    entry: PathBuf,
    graph: PathBuf,
    home: PathBuf,
    xdg: PathBuf,
}

impl Fixture {
    fn new(enabled: bool) -> Self {
        let temp = TempDir::new().unwrap();
        let entry = temp.path().join("entry");
        let graph = temp.path().join("graph");
        let home = temp.path().join("home");
        let xdg = temp.path().join("xdg-cache");
        fs::create_dir_all(&entry).unwrap();
        fs::create_dir_all(&graph).unwrap();
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&xdg).unwrap();
        fs::write(common::config_path(&graph), config(enabled)).unwrap();
        fs::write(graph.join("seed.md"), "# Seed\n").unwrap();
        fs::write(graph.join("dependent.md"), "[seed](seed.md)\n").unwrap();
        Self {
            temp,
            entry,
            graph,
            home,
            xdg,
        }
    }

    fn write_config(&self, enabled: bool) {
        fs::write(common::config_path(&self.graph), config(enabled)).unwrap();
    }

    fn lock_bytes(&self) -> Option<Vec<u8>> {
        let path = common::lock_path(&self.graph);
        path.exists().then(|| fs::read(path).unwrap())
    }

    fn restore_lock(&self, bytes: Option<Vec<u8>>) {
        let path = common::lock_path(&self.graph);
        match bytes {
            Some(bytes) => fs::write(path, bytes).unwrap(),
            None if path.exists() => fs::remove_file(path).unwrap(),
            None => {}
        }
    }

    fn command(&self, args: &[&str]) -> std::process::Command {
        let mut command = drft_bin();
        command
            .current_dir(&self.entry)
            .env("HOME", &self.home)
            .env("XDG_CACHE_HOME", &self.xdg)
            .args(["-C", self.graph.to_str().unwrap()])
            .args(args);
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().unwrap()
    }

    fn argv(&self, args: &[&str]) -> Vec<String> {
        let mut expected = vec![env!("CARGO_BIN_EXE_drft").to_owned()];
        expected.extend(["-C".to_owned(), self.graph.to_str().unwrap().to_owned()]);
        expected.extend(args.iter().map(|arg| (*arg).to_owned()));
        expected
    }
}

#[cfg(target_os = "macos")]
fn selected_cache(fixture: &Fixture) -> PathBuf {
    fixture.home.join("Library/Caches/drft/usage")
}

#[cfg(target_os = "linux")]
fn selected_cache(fixture: &Fixture) -> PathBuf {
    fixture.xdg.join("drft/usage")
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|arg| arg["value"].as_str().unwrap().to_owned())
        .collect()
}

fn exact_available(value: &Value) -> u64 {
    assert_eq!(value["status"], "exact", "{value}");
    value["bytes"].as_u64().unwrap()
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn partition_dir(fixture: &Fixture) -> PathBuf {
    let root = fixture.graph.canonicalize().unwrap();
    let id = drft::usage::identity::partition_id(root.as_os_str()).unwrap();
    selected_cache(fixture).join(id)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn events(fixture: &Fixture) -> Vec<(String, Value)> {
    let partition = partition_dir(fixture);
    if !partition.exists() {
        return Vec::new();
    }
    let mut records: Vec<_> = fs::read_dir(partition)
        .unwrap()
        .map(Result::unwrap)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            (name.ends_with(".start.json") || name.ends_with(".finish.json")).then(|| {
                (
                    name,
                    serde_json::from_slice(&fs::read(entry.path()).unwrap()).unwrap(),
                )
            })
        })
        .collect();
    records.sort_by(|left, right| left.0.cmp(&right.0));
    records
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn pair_for(fixture: &Fixture, matches_start: impl Fn(&Value) -> bool) -> (Value, Value) {
    let records = events(fixture);
    let mut pairs = std::collections::BTreeMap::<String, (Option<Value>, Option<Value>)>::new();
    for (_, value) in records {
        let id = value["id"].as_str().unwrap().to_owned();
        let pair = pairs.entry(id).or_default();
        match value["event"].as_str() {
            Some("start") => pair.0 = Some(value),
            Some("finish") => pair.1 = Some(value),
            other => panic!("unexpected event {other:?}"),
        }
    }
    let matches: Vec<_> = pairs
        .into_iter()
        .filter(|(_, (start, _))| start.as_ref().is_some_and(&matches_start))
        .collect();
    assert_eq!(matches.len(), 1, "expected exactly one matching usage pair");
    let (_, (start, finish)) = matches.into_iter().next().unwrap();
    let start = start.expect("start");
    let finish = finish.expect("finish");
    let id = start["id"].as_str().unwrap();
    assert!(matches!(
        drft::usage::record::classify(
            &serde_json::to_vec(&start).unwrap(),
            id,
            drft::usage::record::EventKind::Start,
        ),
        drft::usage::record::RecordState::Supported { .. }
    ));
    assert!(matches!(
        drft::usage::record::classify(
            &serde_json::to_vec(&finish).unwrap(),
            id,
            drft::usage::record::EventKind::Finish,
        ),
        drft::usage::record::RecordState::Supported { .. }
    ));
    (start, finish)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn pair_for_args(fixture: &Fixture, args: &[&str]) -> (Value, Value) {
    let expected = fixture.argv(args);
    pair_for(fixture, |start| strings(&start["argv"]) == expected)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn pair_for_config(fixture: &Fixture, bytes: &[u8]) -> (Value, Value) {
    let expected = format!("b3:{}", blake3::hash(bytes).to_hex());
    pair_for(fixture, |start| start["config_fingerprint"] == expected)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn assert_success_pair(
    fixture: &Fixture,
    args: &[&str],
    output: &Output,
    command: &str,
    mode: &str,
    coverage: &str,
) {
    let (start, finish) = pair_for_args(fixture, args);
    let source = fs::read(common::config_path(&fixture.graph)).unwrap();
    assert_eq!(start["schema"], "drft-usage");
    assert_eq!(start["revision"], 1);
    assert_eq!(start["id"], finish["id"]);
    assert_eq!(start["command"], command);
    assert_eq!(strings(&start["argv"]), fixture.argv(args));
    assert_eq!(
        start["config_fingerprint"],
        format!("b3:{}", blake3::hash(&source).to_hex())
    );
    assert_eq!(
        start["original_cwd"]["value"],
        fixture.entry.canonicalize().unwrap().to_str().unwrap()
    );
    assert_eq!(
        start["effective_directory"]["value"],
        fixture.graph.canonicalize().unwrap().to_str().unwrap()
    );
    assert_eq!(
        start["canonical_graph_root"]["value"],
        fixture.graph.canonicalize().unwrap().to_str().unwrap()
    );
    assert_eq!(finish["intended_exit"], "clean");
    assert_eq!(finish["completion"], "returned");
    assert_eq!(finish["output_mode"], mode);
    assert_eq!(
        finish["structured"]["findings"]["coverage"]["value"],
        coverage
    );
    assert_eq!(
        exact_available(&finish["stdout"]["observed_input_bytes"]),
        output.stdout.len() as u64
    );
    assert_eq!(finish["stdout"]["retained_bytes"], output.stdout.len());
    assert_eq!(finish["stdout"]["write_outcome"]["status"], "all_succeeded");
    assert_eq!(
        exact_available(&finish["stdout"]["writer_accepted_bytes"]["bytes"]),
        output.stdout.len() as u64
    );
    assert_eq!(finish["stderr"]["write_outcome"]["status"], "not_attempted");
    let unavailable = serde_json::json!({"status": "unavailable", "value": "not_observed"});
    let available = |value| serde_json::json!({"status": "available", "value": value});
    assert_eq!(
        finish["graph_sizes"],
        if mode == "raw_graph_set" {
            unavailable.clone()
        } else {
            available(serde_json::json!({"graphs": 2, "nodes": 2, "edges": 1}))
        }
    );
    let (nodes, edges, findings) = match command {
        "check" => (None, None, Some(0)),
        "graph" if mode != "raw_graph_set" => (Some(2), Some(1), None),
        "graph" | "nodes" | "lock" => (Some(2), None, None),
        "edges" => (None, Some(1), None),
        "impact" => (Some(1), None, Some(0)),
        _ => unreachable!(),
    };
    for (field, value) in [("nodes", nodes), ("edges", edges), ("findings", findings)] {
        assert_eq!(
            finish["result_sizes"]["value"][field],
            value
                .map(|n| available(serde_json::json!(n)))
                .unwrap_or_else(|| unavailable.clone()),
            "{command}: {field}"
        );
    }
}

/// Every covered command publishes a complete immutable pair after a successful
/// config load.  The command matrix fixes output mode and finding coverage at
/// the lifecycle boundary; the command tests themselves own document shape.
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn enabled_collection_pairs_every_covered_command_and_mode() {
    let fixture = Fixture::new(true);
    for (args, command, mode, coverage) in [
        (
            vec!["check"],
            "check",
            "text",
            "full_policy_filtered_evaluation",
        ),
        (
            vec!["--format", "json", "check"],
            "check",
            "json",
            "full_policy_filtered_evaluation",
        ),
        (vec!["graph"], "graph", "text", "construction_diagnostics"),
        (
            vec!["--format", "json", "graph"],
            "graph",
            "bare_jgf",
            "construction_diagnostics",
        ),
        (
            vec!["graph", "--raw"],
            "graph",
            "raw_graph_set",
            "construction_diagnostics",
        ),
        (
            vec!["nodes", "--all"],
            "nodes",
            "text",
            "construction_diagnostics",
        ),
        (
            vec!["--format", "json", "nodes", "--all"],
            "nodes",
            "json",
            "construction_diagnostics",
        ),
        (
            vec!["edges", "--all"],
            "edges",
            "text",
            "construction_diagnostics",
        ),
        (
            vec!["--format", "json", "edges", "--all"],
            "edges",
            "json",
            "construction_diagnostics",
        ),
        (
            vec!["impact", "seed.md"],
            "impact",
            "text",
            "selected_impact_diagnostics",
        ),
        (
            vec!["--format", "json", "impact", "seed.md"],
            "impact",
            "json",
            "selected_impact_diagnostics",
        ),
        (
            vec!["lock", "--all"],
            "lock",
            "text",
            "construction_diagnostics",
        ),
        (
            vec!["--format", "json", "lock", "--all"],
            "lock",
            "json",
            "construction_diagnostics",
        ),
    ] {
        // `lock` is intentionally part of the matrix.  Its own output is the
        // observation; the ignored lockfile keeps later graph identities stable.
        let output = fixture.run(&args);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_success_pair(&fixture, &args, &output, command, mode, coverage);
    }
}

/// Enabling collection is observational only.  Repeat the full supported
/// surface with and without the switch, restoring the mutable lockfile around
/// each pair so the comparison is against the same graph and baseline.
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn enabled_collection_preserves_every_command_result() {
    let fixture = Fixture::new(true);
    for args in [
        vec!["check"],
        vec!["--format", "json", "check"],
        vec!["graph"],
        vec!["--format", "json", "graph"],
        vec!["graph", "--raw"],
        vec!["nodes", "--all"],
        vec!["--format", "json", "nodes", "--all"],
        vec!["edges", "--all"],
        vec!["--format", "json", "edges", "--all"],
        vec!["impact", "seed.md"],
        vec!["--format", "json", "impact", "seed.md"],
        vec!["lock", "--all"],
        vec!["--format", "json", "lock", "--all"],
    ] {
        let original_lock = fixture.lock_bytes();
        fixture.write_config(false);
        let disabled = fixture.run(&args);
        fixture.restore_lock(original_lock.clone());

        fixture.write_config(true);
        let enabled = fixture.run(&args);
        fixture.restore_lock(original_lock);

        assert_eq!(enabled.status, disabled.status, "{args:?}");
        assert_eq!(enabled.stdout, disabled.stdout, "{args:?}");
        assert_eq!(enabled.stderr, disabled.stderr, "{args:?}");
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn excluded_commands_and_preconfig_failures_never_create_usage_cache() {
    for args in [
        vec!["init"],
        vec!["config", "--show-ignores"],
        vec!["guide"],
        vec!["--help"],
        vec!["--version"],
        vec!["nodes"],
    ] {
        let fixture = Fixture::new(true);
        let _ = fixture.run(&args);
        assert!(
            !selected_cache(&fixture).exists(),
            "excluded invocation created cache: {args:?}"
        );
    }

    let invalid_config = Fixture::new(true);
    fs::write(common::config_path(&invalid_config.graph), "[broken").unwrap();
    assert_eq!(invalid_config.run(&["graph"]).status.code(), Some(2));
    assert!(!selected_cache(&invalid_config).exists());

    let strict_config = Fixture::new(true);
    fs::write(
        common::config_path(&strict_config.graph),
        format!("{GRAPH}\n[experimental.usage]\nenabled = true\nnot-an-option = true\n"),
    )
    .unwrap();
    assert_eq!(strict_config.run(&["graph"]).status.code(), Some(2));
    assert!(!selected_cache(&strict_config).exists());

    let invalid_directory = Fixture::new(true);
    let missing = invalid_directory.temp.path().join("missing");
    let output = drft_bin()
        .current_dir(&invalid_directory.entry)
        .env("HOME", &invalid_directory.home)
        .env("XDG_CACHE_HOME", &invalid_directory.xdg)
        .args(["-C", missing.to_str().unwrap(), "graph"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(!selected_cache(&invalid_directory).exists());
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn enabled_records_preserve_error_statuses_and_result_modes() {
    let fixture = Fixture::new(true);

    fs::write(
        common::config_path(&fixture.graph),
        format!("{GRAPH}unresolved-edge = \"error\"\n\n[experimental.usage]\nenabled = true\n"),
    )
    .unwrap();
    fs::write(fixture.graph.join("broken.md"), "[gone](gone.md)\n").unwrap();
    let violation = fixture.run(&["check"]);
    assert_eq!(violation.status.code(), Some(1));
    let (_, finish) = pair_for_args(&fixture, &["check"]);
    assert_eq!(finish["intended_exit"], "violations");
    assert_eq!(finish["completion"], "returned");
    assert_eq!(finish["output_mode"], "text");

    let missing = fixture.run(&["impact", "not-in-the-graph.md"]);
    assert_eq!(missing.status.code(), Some(2));
    assert!(missing.stdout.is_empty());
    let (start, finish) = pair_for_args(&fixture, &["impact", "not-in-the-graph.md"]);
    assert_eq!(
        start["command"], "impact",
        "configuration loaded before this error"
    );
    assert_eq!(finish["intended_exit"], "usage_error");
    assert_eq!(finish["completion"], "command_error");
    assert_eq!(finish["output_mode"], "no_document");
    assert!(
        finish["stderr"]["observed_input_bytes"]["bytes"]
            .as_u64()
            .unwrap()
            > 0
    );

    let refused = fixture.run(&["--format", "json", "nodes", "--all", "--max-bytes", "0"]);
    assert_eq!(refused.status.code(), Some(2));
    assert!(refused.stdout.is_empty());
    let (_, finish) = pair_for_args(
        &fixture,
        &["--format", "json", "nodes", "--all", "--max-bytes", "0"],
    );
    assert_eq!(finish["intended_exit"], "usage_error");
    assert_eq!(finish["completion"], "output_budget_refused");
    assert_eq!(finish["output_mode"], "no_document");
    assert_eq!(finish["budget_refusal"]["budget_bytes"], 0);
    assert_eq!(finish["stdout"]["write_outcome"]["status"], "not_attempted");
    assert!(
        finish["stderr"]["observed_input_bytes"]["bytes"]
            .as_u64()
            .unwrap()
            > 0
    );
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn stderr_hints_are_captured_without_changing_the_result_mode() {
    let fixture = Fixture::new(true);
    fs::write(
        common::config_path(&fixture.graph),
        format!("{GRAPH}not-a-rule = \"warn\"\n\n[experimental.usage]\nenabled = true\n"),
    )
    .unwrap();
    let output = fixture.run(&["graph", "--raw"]);
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown-rule"));
    let (_, finish) = pair_for_args(&fixture, &["graph", "--raw"]);
    assert_eq!(finish["output_mode"], "raw_graph_set");
    assert_eq!(finish["structured"]["hints"]["total"]["value"], 1);
    assert_eq!(
        exact_available(&finish["stderr"]["observed_input_bytes"]),
        output.stderr.len() as u64
    );
    assert_eq!(finish["stderr"]["write_outcome"]["status"], "all_succeeded");
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn hint_observation_distinguishes_document_delivery_and_budget_error_routes() {
    let fixture = Fixture::new(true);
    fs::write(
        common::config_path(&fixture.graph),
        format!("{GRAPH}not-a-rule = \"warn\"\n\n[experimental.usage]\nenabled = true\n"),
    )
    .unwrap();

    let success = fixture.run(&["--format", "json", "nodes", "--all"]);
    assert!(success.status.success());
    let (_, finish) = pair_for_args(&fixture, &["--format", "json", "nodes", "--all"]);
    let observation = &finish["hint_observation"];
    assert_eq!(observation["embedding"], "result_document");
    assert_eq!(observation["route"], "stdout_document");
    assert_eq!(observation["suppression"], "none");
    assert_eq!(observation["write_attempt"], "attempted");

    let refused = fixture.run(&["--format", "json", "nodes", "--all", "--max-bytes", "0"]);
    assert_eq!(refused.status.code(), Some(2));
    assert!(refused.stdout.is_empty());
    let (_, finish) = pair_for_args(
        &fixture,
        &["--format", "json", "nodes", "--all", "--max-bytes", "0"],
    );
    let observation = &finish["hint_observation"];
    assert_eq!(observation["embedding"], "result_document");
    assert_eq!(observation["route"], "stderr_json");
    assert_eq!(observation["suppression"], "budget_refusal");
    assert_eq!(observation["write_attempt"], "attempted");
    assert_eq!(finish["stdout"]["write_outcome"]["status"], "not_attempted");

    let errored = fixture.run(&["--format", "json", "nodes", "missing.md"]);
    assert_eq!(errored.status.code(), Some(2));
    assert!(errored.stdout.is_empty());
    let (_, finish) = pair_for_args(&fixture, &["--format", "json", "nodes", "missing.md"]);
    let observation = &finish["hint_observation"];
    assert_eq!(observation["embedding"], "not_embedded");
    assert_eq!(observation["route"], "stderr_json");
    assert_eq!(observation["suppression"], "none");
    assert_eq!(observation["write_attempt"], "attempted");
    assert_eq!(finish["stdout"]["write_outcome"]["status"], "not_attempted");

    let raw = fixture.run(&["--format", "text", "graph", "--raw", "--max-bytes", "0"]);
    assert_eq!(raw.status.code(), Some(2));
    assert!(raw.stdout.is_empty());
    let (_, finish) = pair_for_args(
        &fixture,
        &["--format", "text", "graph", "--raw", "--max-bytes", "0"],
    );
    let observation = &finish["hint_observation"];
    assert_eq!(observation["embedding"], "not_embedded");
    assert_ne!(observation["suppression"], "earlier_write_failure");
    assert_eq!(observation["write_attempt"], "attempted");
    assert_eq!(finish["stdout"]["write_outcome"]["status"], "not_attempted");
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn collection_toggle_is_observable_but_disabled_or_absent_never_creates_cache() {
    for enabled in [false, true] {
        let fixture = Fixture::new(enabled);
        let output = fixture.run(&["graph"]);
        assert!(output.status.success());
        if enabled {
            assert_eq!(events(&fixture).len(), 2);
        } else {
            assert!(!selected_cache(&fixture).exists());
        }
    }

    let absent = Fixture::new(false);
    fs::write(common::config_path(&absent.graph), GRAPH).unwrap();
    assert!(absent.run(&["graph"]).status.success());
    assert!(!selected_cache(&absent).exists());

    let toggled = Fixture::new(true);
    let first_config = fs::read(common::config_path(&toggled.graph)).unwrap();
    let first = toggled.run(&["graph"]);
    assert!(first.status.success());
    let (first_start, _) = pair_for_config(&toggled, &first_config);
    toggled.write_config(false);
    let disabled = toggled.run(&["graph"]);
    assert_eq!(
        disabled.stdout, first.stdout,
        "usage configuration must not change graph output"
    );
    let after_disable = events(&toggled).len();
    assert_eq!(after_disable, 2);
    fs::write(
        common::config_path(&toggled.graph),
        format!(
            "{}# enabled again after an observable toggle\n",
            config(true)
        ),
    )
    .unwrap();
    let reenabled = toggled.run(&["graph"]);
    assert_eq!(reenabled.stdout, first.stdout);
    assert!(reenabled.status.success());
    let second_config = fs::read(common::config_path(&toggled.graph)).unwrap();
    let (second_start, _) = pair_for_config(&toggled, &second_config);
    assert_ne!(
        first_start["config_fingerprint"],
        second_start["config_fingerprint"]
    );
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn usage_config_is_never_graph_visible() {
    let fixture = Fixture::new(true);
    fs::write(common::config_path(&fixture.graph), visible_config(true)).unwrap();
    let enabled = fixture.run(&["--format", "json", "graph"]);
    assert!(enabled.status.success());
    let enabled: Value = serde_json::from_slice(&enabled.stdout).unwrap();

    fs::write(common::config_path(&fixture.graph), visible_config(false)).unwrap();
    let disabled = fixture.run(&["--format", "json", "graph"]);
    assert!(disabled.status.success());
    let disabled: Value = serde_json::from_slice(&disabled.stdout).unwrap();

    assert_eq!(enabled["graph"], disabled["graph"]);
    assert!(
        enabled["graph"]["nodes"]
            .as_object()
            .unwrap()
            .keys()
            .all(|path| path != ".drft" && !path.starts_with(".drft/"))
    );
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn unix_non_utf8_program_name_is_preserved_in_the_start_argv() {
    let fixture = Fixture::new(true);
    let mut command = fixture.command(&["graph"]);
    command.arg0(OsString::from_vec(b"drft-\xff".to_vec()));
    let output = command.output().unwrap();
    assert!(output.status.success());
    let (start, _) = pair_for(&fixture, |start| start["command"] == "graph");
    assert_eq!(start["argv"][0]["os_encoding"], "unix-bytes");
    assert_eq!(start["argv"][0]["encoding"], "base64");
    assert_eq!(
        STANDARD
            .decode(start["argv"][0]["value"].as_str().unwrap())
            .unwrap(),
        b"drft-\xff"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn linux_cache_selection_rejects_present_empty_or_relative_xdg_without_home_fallback() {
    for xdg in [PathBuf::new(), PathBuf::from("relative-cache")] {
        let fixture = Fixture::new(true);
        let mut command = fixture.command(&["graph"]);
        command.env("XDG_CACHE_HOME", xdg);
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!fixture.home.join(".cache/drft/usage").exists());
        assert!(!fixture.xdg.join("drft/usage").exists());
    }

    let fixture = Fixture::new(true);
    let mut command = fixture.command(&["graph"]);
    command.env_remove("XDG_CACHE_HOME");
    assert!(command.output().unwrap().status.success());
    assert!(fixture.home.join(".cache/drft/usage").exists());
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn collector_failures_and_lock_contention_leave_command_results_unchanged() {
    let fixture = Fixture::new(false);
    let baseline = fixture.run(&["graph"]);
    assert!(baseline.status.success());
    fixture.write_config(true);

    let cache = selected_cache(&fixture);
    let root = fixture.graph.canonicalize().unwrap();
    let mut partition = drft::usage::store::Partition::open_or_create(&cache, &root).unwrap();
    let guard = partition.try_lock().unwrap();
    let contended = fixture.run(&["graph"]);
    assert_eq!(contended.status, baseline.status);
    assert_eq!(contended.stdout, baseline.stdout);
    assert_eq!(
        events(&fixture).len(),
        0,
        "contention must not publish a partial record"
    );
    drop(guard);

    fs::write(partition_dir(&fixture).join("unknown-entry"), "keep me").unwrap();
    let failed = fixture.run(&["graph"]);
    assert_eq!(failed.status, baseline.status);
    assert_eq!(failed.stdout, baseline.stdout);
    assert_eq!(
        events(&fixture).len(),
        0,
        "storage failure must not change command behavior"
    );
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn large_findings_are_bounded_and_report_omission() {
    let fixture = Fixture::new(true);
    fs::write(
        common::config_path(&fixture.graph),
        format!("{GRAPH}unresolved-edge = \"error\"\n\n[experimental.usage]\nenabled = true\n"),
    )
    .unwrap();
    for i in 0..1800 {
        fs::write(
            fixture.graph.join(format!("broken-{i:04}.md")),
            format!("[missing](missing-{i:04}.md)\n"),
        )
        .unwrap();
    }
    let output = fixture.run(&["--format", "json", "check"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.len() > 64 * 1024);
    let (_, finish) = pair_for_args(&fixture, &["--format", "json", "check"]);
    let prefix = &finish["structured"]["findings"]["prefix"];
    let total = prefix["total"]["value"].as_u64().unwrap();
    let included = prefix["included"].as_u64().unwrap();
    let omitted = prefix["omitted"]["value"].as_u64().unwrap();
    assert!(
        included < total,
        "the fixture must exercise finding omission"
    );
    assert_eq!(included + omitted, total);
    assert!(finish["stdout"]["retained_bytes"].as_u64().unwrap() <= 64 * 1024);
    assert_eq!(finish["stdout"]["truncated"], true);
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn closed_stdout_is_a_clean_command_with_a_failed_write_finish() {
    let fixture = Fixture::new(true);
    for i in 0..1200 {
        fs::write(
            fixture
                .graph
                .join(format!("large-{i:04}-{}.md", "x".repeat(72))),
            "[seed](seed.md)\n",
        )
        .unwrap();
    }
    let mut child = fixture.command(&["nodes", "--all"]);
    child.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = child.spawn().unwrap();
    drop(child.stdout.take());
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let (_, finish) = pair_for_args(&fixture, &["nodes", "--all"]);
    assert_eq!(finish["intended_exit"], "clean");
    assert_eq!(finish["completion"], "stdout_write_failed");
    assert_eq!(finish["output_mode"], "text");
    assert_eq!(
        finish["stdout"]["write_outcome"]["status"],
        "unknown_acceptance"
    );
    assert_eq!(finish["stdout"]["write_outcome"]["failed"], true);
}

/// A closed stderr peer makes the existing error printer fail. The finish must
/// not be invented when finalization cannot report the command error; the
/// published start remains useful evidence.
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn failed_stderr_leaves_only_the_start_record() {
    use std::os::fd::OwnedFd;
    use std::os::unix::net::UnixStream;

    let fixture = Fixture::new(true);
    let (reader, writer) = UnixStream::pair().unwrap();
    drop(reader);
    let mut command = fixture.command(&["nodes", "--all", "--max-bytes", "0"]);
    command.stderr(Stdio::from(OwnedFd::from(writer)));
    let output = command.output().unwrap();
    assert!(!output.status.success());
    let records = events(&fixture);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].1["event"], "start");
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
#[test]
fn unsupported_platform_skips_collection_before_cache_path_lookup() {
    let fixture = Fixture::new(true);
    let output = fixture.run(&["graph"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!fixture.home.join("Library/Caches/drft/usage").exists());
    assert!(!fixture.xdg.join("drft/usage").exists());
}

/// Hold the graph-visible config bytes fixed while cache selection controls
/// whether the production lifecycle can activate.
#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn fixed_config_collection_preserves_output_status_and_lock_mutations() {
    for args in [
        vec!["check"],
        vec!["--format", "json", "check"],
        vec!["graph"],
        vec!["--format", "json", "graph"],
        vec!["graph", "--raw"],
        vec!["--format", "json", "graph", "--raw"],
        vec!["nodes", "--all"],
        vec!["--format", "json", "nodes", "--all"],
        vec!["edges", "--all"],
        vec!["--format", "json", "edges", "--all"],
        vec!["impact", "seed.md"],
        vec!["--format", "json", "impact", "seed.md"],
        vec!["lock", "--all"],
        vec!["--format", "json", "lock", "--all"],
        vec!["nodes", "missing.md"],
        vec!["--format", "json", "nodes", "missing.md"],
        vec!["nodes", "--all", "--max-bytes", "0"],
        vec!["--format", "json", "nodes", "--all", "--max-bytes", "0"],
    ] {
        let fixture = Fixture::new(true);
        let config_bytes = visible_config(true).replace(
            "[experimental.usage]",
            "[rules.misspelled]\nseverity = \"warn\"\n\n[experimental.usage]",
        );
        fs::write(common::config_path(&fixture.graph), &config_bytes).unwrap();
        let original_lock = fixture.lock_bytes();
        let inactive = fixture
            .command(&args)
            .env("HOME", "relative-home")
            .env("XDG_CACHE_HOME", "relative-cache")
            .output()
            .unwrap();
        assert!(!selected_cache(&fixture).exists());
        assert!(!fixture.entry.join("relative-home").exists());
        assert!(!fixture.entry.join("relative-cache").exists());
        let inactive_lock = fixture.lock_bytes();
        fixture.restore_lock(original_lock);
        let active = fixture.run(&args);
        assert_eq!(active.status, inactive.status, "{args:?}");
        assert_eq!(active.stdout, inactive.stdout, "{args:?}");
        assert_eq!(active.stderr, inactive.stderr, "{args:?}");
        assert_eq!(fixture.lock_bytes(), inactive_lock, "{args:?}");
        assert_eq!(
            fs::read(common::config_path(&fixture.graph)).unwrap(),
            config_bytes.as_bytes()
        );
        let (start, _) = pair_for_args(&fixture, &args);
        assert_eq!(
            start["config_fingerprint"],
            format!("b3:{}", blake3::hash(config_bytes.as_bytes()).to_hex())
        );
    }
}
