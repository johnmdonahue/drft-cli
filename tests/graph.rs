mod common;
use common::drft_bin;
use std::fs;
use tempfile::TempDir;

fn graph_json(dir: &std::path::Path) -> serde_json::Value {
    graph_json_args(dir, &[])
}

fn graph_json_args(dir: &std::path::Path, extra: &[&str]) -> serde_json::Value {
    let mut args = vec!["-C", dir.to_str().unwrap(), "graph"];
    args.extend_from_slice(extra);
    let output = drft_bin().args(&args).output().unwrap();
    assert!(
        output.status.success(),
        "drft graph failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("valid JSON")
}

/// The composed graph is valid JGF: a single `graph` envelope with bare-path
/// nodes whose metadata nests `fs` facts under `@fs` plus `_graphs` provenance.
#[test]
fn graph_emits_composed_jgf() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(dir.path().join("index.md"), "# Index").unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup").unwrap();

    let v = graph_json(dir.path());

    // Root: exactly one JGF document key, "graph".
    let root = v.as_object().expect("root is object");
    assert_eq!(root.keys().collect::<Vec<_>>(), vec!["graph"]);

    let graph = &v["graph"];
    assert_eq!(graph["directed"], serde_json::json!(true));
    // Composed graph is unlabeled (label belongs to raw fragments).
    assert!(graph.get("label").is_none());

    let nodes = graph["nodes"].as_object().unwrap();
    // fs walks every file, including drft.toml.
    for path in ["index.md", "setup.md", "drft.toml"] {
        assert!(nodes.contains_key(path), "missing node {path}");
    }

    // Bare-path node carries @fs metadata (type + hash) and _graphs.
    let meta = &nodes["index.md"]["metadata"];
    assert_eq!(meta["@fs"]["type"], serde_json::json!("file"));
    assert!(
        meta["@fs"]["hash"].as_str().unwrap().starts_with("b3:"),
        "fs node should be auto-hashed"
    );
    assert_eq!(meta["_graphs"], serde_json::json!(["@fs"]));

    // No markdown/frontmatter edges yet — fs is node-only (no symlinks here).
    assert!(graph["edges"].as_array().unwrap().is_empty());
}

/// Node keys serialize in sorted order for deterministic output.
#[test]
fn graph_nodes_are_sorted() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(dir.path().join("z.md"), "z").unwrap();
    fs::write(dir.path().join("a.md"), "a").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "graph"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let a = stdout.find("\"a.md\"").unwrap();
    let z = stdout.find("\"z.md\"").unwrap();
    assert!(a < z, "node keys should be sorted");
}

/// A symlink within the graph root becomes a symlink-typed node with an edge to
/// its resolved target.
#[cfg(unix)]
#[test]
fn graph_symlink_within_root_emits_edge() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(dir.path().join("real.md"), "real").unwrap();
    std::os::unix::fs::symlink(dir.path().join("real.md"), dir.path().join("alias.md")).unwrap();

    let v = graph_json(dir.path());
    let nodes = v["graph"]["nodes"].as_object().unwrap();
    assert_eq!(nodes["alias.md"]["metadata"]["@fs"]["type"], "symlink");

    let edges = v["graph"]["edges"].as_array().unwrap();
    let edge = edges
        .iter()
        .find(|e| e["source"] == "alias.md")
        .expect("symlink edge");
    assert_eq!(edge["target"], "real.md");
    assert_eq!(edge["metadata"]["_graphs"], serde_json::json!(["@fs"]));
}

/// A symlink whose canonical target is outside the graph root is typed as a
/// symlink but carries no hash (its content is intentionally not read).
#[cfg(unix)]
#[test]
fn graph_symlink_escaping_root_is_not_hashed() {
    let outer = TempDir::new().unwrap();
    let root = outer.path().join("project");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(outer.path().join("secret.md"), "secret").unwrap();
    std::os::unix::fs::symlink(outer.path().join("secret.md"), root.join("trap.md")).unwrap();

    let v = graph_json(&root);
    let trap = &v["graph"]["nodes"]["trap.md"]["metadata"]["@fs"];
    assert_eq!(trap["type"], "symlink");
    assert!(
        trap.get("hash").is_none(),
        "escaping symlink must not be hashed"
    );
}

/// Markdown body links become composed edges to their resolved targets.
#[test]
fn graph_markdown_links_become_edges() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup").unwrap();

    let v = graph_json(dir.path());
    let edges = v["graph"]["edges"].as_array().unwrap();
    let edge = edges
        .iter()
        .find(|e| e["source"] == "index.md" && e["target"] == "setup.md")
        .expect("markdown edge index.md -> setup.md");
    assert_eq!(
        edge["metadata"]["_graphs"],
        serde_json::json!(["@markdown"])
    );
}

/// Frontmatter contributes node metadata under `@frontmatter` and link edges;
/// an edge declared by both markdown and frontmatter is deduped with merged
/// provenance.
#[test]
fn graph_frontmatter_metadata_and_edge_dedup() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(
        dir.path().join("doc.md"),
        "---\ntitle: Doc\nsources:\n  - target.md\n---\n\n[t](target.md)",
    )
    .unwrap();
    fs::write(dir.path().join("target.md"), "# Target").unwrap();

    let v = graph_json(dir.path());

    // @frontmatter metadata nests on the node, with merged _graphs provenance.
    let meta = &v["graph"]["nodes"]["doc.md"]["metadata"];
    assert_eq!(meta["@frontmatter"]["title"], "Doc");
    assert_eq!(meta["_graphs"], serde_json::json!(["@frontmatter", "@fs"]));

    // The shared edge is deduped: one edge, both graphs in _graphs.
    let edges = v["graph"]["edges"].as_array().unwrap();
    let shared: Vec<_> = edges
        .iter()
        .filter(|e| e["source"] == "doc.md" && e["target"] == "target.md")
        .collect();
    assert_eq!(
        shared.len(),
        1,
        "edge should be deduped by (source, target)"
    );
    assert_eq!(
        shared[0]["metadata"]["_graphs"],
        serde_json::json!(["@frontmatter", "@markdown"])
    );
}

/// `--raw` emits the unmerged set: one labeled fragment per graph, bare paths,
/// no `@` namespacing.
#[test]
fn graph_raw_emits_the_set() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(
        dir.path().join("doc.md"),
        "---\ntitle: Doc\n---\n\n[t](target.md)",
    )
    .unwrap();
    fs::write(dir.path().join("target.md"), "# Target").unwrap();

    let v = graph_json_args(dir.path(), &["--raw"]);
    let graphs = v["graphs"].as_array().expect("raw set has 'graphs' array");
    // fs is built first; the configured text graphs follow in name order.
    let labels: Vec<&str> = graphs
        .iter()
        .map(|g| g["label"].as_str().unwrap())
        .collect();
    assert_eq!(labels, vec!["fs", "frontmatter", "markdown"]);

    // Bare paths, no @ namespacing in the raw view.
    let fs_graph = &graphs[0];
    assert!(fs_graph["nodes"]["doc.md"]["metadata"]["type"] == "file");
    assert!(fs_graph["nodes"]["doc.md"]["metadata"].get("@fs").is_none());

    // frontmatter carries the bare metadata block; markdown is edge-only.
    let frontmatter = graphs.iter().find(|g| g["label"] == "frontmatter").unwrap();
    let markdown = graphs.iter().find(|g| g["label"] == "markdown").unwrap();
    assert_eq!(frontmatter["nodes"]["doc.md"]["metadata"]["title"], "Doc");
    assert!(markdown["nodes"].as_object().unwrap().is_empty());
}
