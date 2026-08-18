mod common;
use common::drft_bin;
use serde_json::Value;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

/// A graph with a plain root doc and a `docs/` subtree whose files carry
/// frontmatter, one with a `purpose` key and one without.
fn fixture() -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(dir.path().join("index.md"), "# Index, no frontmatter").unwrap();

    let docs = dir.path().join("docs");
    fs::create_dir_all(docs.join("sub")).unwrap();
    fs::write(
        docs.join("a.md"),
        "---\npurpose: explains a\nstatus: draft\n---\n\n# A",
    )
    .unwrap();
    fs::write(docs.join("b.md"), "---\nstatus: final\n---\n\n# B").unwrap();
    fs::write(
        docs.join("sub").join("c.md"),
        "---\npurpose: explains c\n---\n\n# C",
    )
    .unwrap();
    dir
}

fn nodes_json(dir: &Path, extra: &[&str]) -> Value {
    let mut args = vec!["-C", dir.to_str().unwrap(), "--format", "json", "nodes"];
    args.extend_from_slice(extra);
    let output = drft_bin().args(&args).output().unwrap();
    assert!(
        output.status.success(),
        "drft nodes failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("valid JSON")
}

/// The node ids in a projection, in order.
fn ids(v: &Value) -> Vec<String> {
    v["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["id"].as_str().unwrap().to_string())
        .collect()
}

/// A node's projected metadata by id.
fn meta<'a>(v: &'a Value, id: &str) -> &'a Value {
    v["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == id)
        .unwrap_or_else(|| panic!("node {id} not in projection"))
        .get("metadata")
        .unwrap()
}

/// An exact file path projects that one node with all its namespace blocks.
#[test]
fn exact_path_projects_one_node() {
    let dir = fixture();
    let v = nodes_json(dir.path(), &["docs/a.md"]);
    assert_eq!(v["total"], 1);
    assert_eq!(ids(&v), vec!["docs/a.md"]);
    // Both the fs facts and the frontmatter block come through; _graphs does not.
    let m = meta(&v, "docs/a.md");
    assert_eq!(m["@fs"]["type"], "file");
    assert_eq!(m["@frontmatter"]["purpose"], "explains a");
    assert!(m.get("_graphs").is_none());
}

/// A bare directory is sugar for its recursive subtree: every node under `docs/`,
/// including nested ones, and nothing above it.
#[test]
fn bare_directory_selects_subtree() {
    let dir = fixture();
    let v = nodes_json(dir.path(), &["docs/"]);
    let keys = ids(&v);
    assert!(keys.iter().any(|k| k == "docs/a.md"));
    assert!(keys.iter().any(|k| k == "docs/b.md"));
    assert!(
        keys.iter().any(|k| k == "docs/sub/c.md"),
        "subtree recurses, got: {keys:?}"
    );
    assert!(
        !keys.iter().any(|k| k == "index.md"),
        "nothing above docs/, got: {keys:?}"
    );
}

/// A glob pattern matches node keys with the same vocabulary as `drft.toml` —
/// `*` stays within a path component, so `docs/*.md` excludes the nested file.
#[test]
fn glob_pattern_matches_node_keys() {
    let dir = fixture();
    let v = nodes_json(dir.path(), &["docs/*.md"]);
    let keys = ids(&v);
    assert!(keys.iter().any(|k| k == "docs/a.md"));
    assert!(keys.iter().any(|k| k == "docs/b.md"));
    assert!(
        !keys.iter().any(|k| k == "docs/sub/c.md"),
        "`*` does not cross a path separator, got: {keys:?}"
    );
}

/// With no selector, every node in the graph is returned.
#[test]
fn no_selector_returns_all_nodes() {
    let dir = fixture();
    let v = nodes_json(dir.path(), &[]);
    let keys = ids(&v);
    for path in [
        "index.md",
        "docs/a.md",
        "docs/b.md",
        "docs/sub/c.md",
        "drft.toml",
    ] {
        assert!(keys.iter().any(|k| k == path), "missing node {path}");
    }
}

/// `--namespace` filters the node set as well as the metadata: a node with no
/// block for the requested namespace drops out rather than appearing empty.
#[test]
fn namespace_filters_node_set_and_metadata() {
    let dir = fixture();
    let v = nodes_json(dir.path(), &["--namespace", "frontmatter"]);
    let keys = ids(&v);
    // index.md and drft.toml have no frontmatter block, so they drop out.
    assert!(
        !keys.iter().any(|k| k == "index.md"),
        "no frontmatter → dropped"
    );
    assert!(
        !keys.iter().any(|k| k == "drft.toml"),
        "no frontmatter → dropped"
    );
    // The docs remain, restricted to their @frontmatter block (no @fs).
    let a = meta(&v, "docs/a.md");
    assert!(a.get("@frontmatter").is_some());
    assert!(
        a.get("@fs").is_none(),
        "metadata restricted to the namespace"
    );
}

/// The `@`-prefixed namespace form is accepted alongside the bare name.
#[test]
fn namespace_accepts_prefixed_form() {
    let dir = fixture();
    let v = nodes_json(dir.path(), &["docs/a.md", "--namespace", "@fs"]);
    let a = meta(&v, "docs/a.md");
    assert!(a.get("@fs").is_some());
    assert!(a.get("@frontmatter").is_none());
}

/// An unknown namespace is a typo, not an empty answer: it errors (exit 2) and
/// lists the declared graphs.
#[test]
fn unknown_namespace_errors_and_lists_declared() {
    let dir = fixture();
    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "nodes",
            "--namespace",
            "bogus",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown namespace"), "got: {stderr}");
    assert!(
        stderr.contains("fs") && stderr.contains("frontmatter") && stderr.contains("markdown"),
        "declared graphs should be listed, got: {stderr}"
    );
}

/// `--field` narrows returned metadata to named keys and drops nodes that do not
/// declare it — the projection answers which files carry the field.
#[test]
fn field_narrows_and_lists_only_declaring_nodes() {
    let dir = fixture();
    let v = nodes_json(
        dir.path(),
        &["docs/", "--namespace", "frontmatter", "--field", "purpose"],
    );
    let keys = ids(&v);
    // a.md and sub/c.md declare purpose; b.md does not and drops out.
    assert!(keys.iter().any(|k| k == "docs/a.md"));
    assert!(keys.iter().any(|k| k == "docs/sub/c.md"));
    assert!(
        !keys.iter().any(|k| k == "docs/b.md"),
        "a node without the field drops out, got: {keys:?}"
    );
    assert_eq!(
        meta(&v, "docs/a.md")["@frontmatter"]["purpose"],
        "explains a"
    );
}

/// An unmatched field is a legitimate empty result, not an error: exit 0 with no
/// nodes.
#[test]
fn unmatched_field_is_empty_not_an_error() {
    let dir = fixture();
    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "--format",
            "json",
            "nodes",
            "docs/a.md",
            "--field",
            "nonexistent",
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "unmatched field is not an error");
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["total"], 0);
    assert!(v["nodes"].as_array().unwrap().is_empty());
}

/// A glob that matches nothing is a valid query with an empty result (exit 0),
/// distinct from a mistyped exact path.
#[test]
fn empty_glob_match_is_exit_zero() {
    let dir = fixture();
    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "--format",
            "json",
            "nodes",
            "nonesuch/**",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["total"], 0);
}

/// A mistyped exact path or directory errors rather than reading as an empty
/// answer.
#[test]
fn missing_exact_path_errors() {
    let dir = fixture();
    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "nodes", "docs/ghost.md"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("node not found"), "got: {stderr}");
}

/// Text output is a node id followed by its indented namespace/field metadata,
/// one node per block — legible without parsing JSON.
#[test]
fn text_format_is_node_then_indented_metadata() {
    let dir = fixture();
    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "nodes",
            "docs/a.md",
            "--namespace",
            "frontmatter",
            "--field",
            "purpose",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout,
        "docs/a.md\n  @frontmatter\n    purpose: explains a\n"
    );
}

/// Text output defaults without `--format` and separates node blocks with a blank
/// line.
#[test]
fn text_format_is_the_default_and_separates_nodes() {
    let dir = fixture();
    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "nodes",
            "docs/",
            "--namespace",
            "frontmatter",
            "--field",
            "status",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // a.md (status: draft) then a blank line then b.md.
    assert!(stdout.contains("docs/a.md\n"), "got: {stdout}");
    assert!(
        stdout.contains("status: draft\n\ndocs/b.md"),
        "blank line between node blocks, got: {stdout}"
    );
}
