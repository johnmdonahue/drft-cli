mod common;
use common::drft_bin;
use serde_json::Value;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

/// A graph with body links (markdown) and a frontmatter `sources` derivation, so
/// edges carry both `@markdown` and `@frontmatter` metadata.
///
/// - `index.md` → `docs/a.md`, `docs/b.md`  (markdown body links)
/// - `docs/a.md` → `docs/b.md`              (markdown body link)
/// - `docs/a.md` → `index.md`               (frontmatter `sources`)
/// - `docs/b.md` has no outbound edges
fn fixture() -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(
        dir.path().join("index.md"),
        "# Index\n\n[a](docs/a.md) and [b](docs/b.md)\n",
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("docs")).unwrap();
    fs::write(
        dir.path().join("docs/a.md"),
        "---\nsources:\n  - ../index.md\n---\n\n[b](b.md)\n",
    )
    .unwrap();
    fs::write(dir.path().join("docs/b.md"), "# B\n").unwrap();
    dir
}

fn edges_json(dir: &Path, extra: &[&str]) -> Value {
    let mut args = vec!["-C", dir.to_str().unwrap(), "--format", "json", "edges"];
    args.extend_from_slice(extra);
    let output = drft_bin().args(&args).output().unwrap();
    assert!(
        output.status.success(),
        "drft edges failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("valid JSON")
}

/// The `source → target` pairs in a projection, in order.
fn pairs(v: &Value) -> Vec<(String, String)> {
    v["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["source"].as_str().unwrap().to_string(),
                e["target"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

/// Edges are matched on source: a file's projection is exactly the edges leaving
/// it, and nothing pointing at it or leaving another file.
#[test]
fn edges_match_on_source() {
    let dir = fixture();
    let v = edges_json(dir.path(), &["docs/a.md"]);
    let p = pairs(&v);
    assert!(p.contains(&("docs/a.md".into(), "docs/b.md".into())));
    assert!(p.contains(&("docs/a.md".into(), "index.md".into())));
    assert!(
        p.iter().all(|(s, _)| s == "docs/a.md"),
        "only edges leaving docs/a.md, got: {p:?}"
    );
}

/// With no selector, every edge in the graph is returned.
#[test]
fn no_selector_returns_all_edges() {
    let dir = fixture();
    let v = edges_json(dir.path(), &[]);
    let p = pairs(&v);
    for want in [
        ("index.md", "docs/a.md"),
        ("index.md", "docs/b.md"),
        ("docs/a.md", "docs/b.md"),
        ("docs/a.md", "index.md"),
    ] {
        assert!(
            p.contains(&(want.0.into(), want.1.into())),
            "missing edge {want:?}, got {p:?}"
        );
    }
}

/// A bare directory selects the edges leaving every node under it.
#[test]
fn subtree_selector_matches_sources_under_it() {
    let dir = fixture();
    let v = edges_json(dir.path(), &["docs/"]);
    let p = pairs(&v);
    // Both edges leaving docs/a.md; nothing from index.md (above docs/).
    assert!(p.contains(&("docs/a.md".into(), "docs/b.md".into())));
    assert!(p.contains(&("docs/a.md".into(), "index.md".into())));
    assert!(
        !p.iter().any(|(s, _)| s == "index.md"),
        "index.md is above docs/, got: {p:?}"
    );
}

/// `--namespace` filters the edge set and the metadata: an edge with no block for
/// the requested namespace drops out.
#[test]
fn namespace_filters_edge_set_and_metadata() {
    let dir = fixture();
    let v = edges_json(dir.path(), &["docs/a.md", "--namespace", "frontmatter"]);
    let p = pairs(&v);
    // Only the frontmatter-derived edge survives; the markdown body link drops.
    assert_eq!(p, vec![("docs/a.md".into(), "index.md".into())]);
    let meta = &v["edges"][0]["metadata"];
    assert!(meta.get("@frontmatter").is_some());
    assert!(
        meta.get("@markdown").is_none(),
        "restricted to the namespace"
    );
}

/// `--field` narrows the returned metadata to named keys within the namespace.
#[test]
fn field_narrows_metadata_within_namespace() {
    let dir = fixture();
    let v = edges_json(
        dir.path(),
        &["docs/a.md", "--namespace", "markdown", "--field", "lines"],
    );
    // The markdown edge to b.md, with only `lines` (not `raw`).
    let edge = v["edges"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["target"] == "docs/b.md")
        .expect("markdown edge to docs/b.md");
    let md = &edge["metadata"]["@markdown"];
    assert!(md.get("lines").is_some());
    assert!(md.get("raw").is_none(), "field narrows to lines only");
}

/// An unknown namespace is a typo: it errors (exit 2) and lists the declared graphs.
#[test]
fn unknown_namespace_errors_and_lists_declared() {
    let dir = fixture();
    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "edges",
            "--namespace",
            "bogus",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown namespace"), "got: {stderr}");
    assert!(
        stderr.contains("fs") && stderr.contains("markdown") && stderr.contains("frontmatter"),
        "declared graphs should be listed, got: {stderr}"
    );
}

/// A mistyped source path errors rather than reading as an empty answer.
#[test]
fn missing_source_errors() {
    let dir = fixture();
    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "edges", "docs/ghost.md"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("node not found"),
        "got: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A glob matching no source is a valid query with an empty result (exit 0),
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
            "edges",
            "nonesuch/**",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["total"], 0);
}

/// Text output is `source → target` followed by its indented namespace/field
/// metadata, legible without parsing JSON.
#[test]
fn text_format_is_source_arrow_target() {
    let dir = fixture();
    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "edges",
            "docs/a.md",
            "--namespace",
            "frontmatter",
            "--field",
            "lines",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout,
        "docs/a.md → index.md\n  @frontmatter\n    lines: [3]\n"
    );
}

/// An edge to a target with no defining node — an external URL, or a broken link —
/// is still projected: edges are matched on source, and the target need not resolve.
#[test]
fn external_and_unresolved_targets_are_projected() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(
        dir.path().join("a.md"),
        "[home](https://example.com) and [gone](./missing.md)\n",
    )
    .unwrap();
    let v = edges_json(dir.path(), &["a.md"]);
    let targets: Vec<String> = v["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["target"].as_str().unwrap().to_string())
        .collect();
    assert!(
        targets.iter().any(|t| t == "https://example.com"),
        "external URL is an edge target, got {targets:?}"
    );
    assert!(
        targets.iter().any(|t| t == "missing.md"),
        "unresolved link is an edge target, got {targets:?}"
    );
}

/// Multiple selectors union their source sets.
#[test]
fn multiple_selectors_union_sources() {
    let dir = fixture();
    let v = edges_json(dir.path(), &["index.md", "docs/a.md"]);
    let sources: Vec<String> = pairs(&v).into_iter().map(|(s, _)| s).collect();
    assert!(sources.iter().any(|s| s == "index.md"), "got {sources:?}");
    assert!(sources.iter().any(|s| s == "docs/a.md"), "got {sources:?}");
}
