mod common;
use common::drft_bin;
use std::fs;
use tempfile::TempDir;

/// Validate that single-graph JSON output complies with JGF v2.0 schema.
/// See https://jsongraphformat.info/v2.0/json-graph-schema.json
#[test]
fn graph_json_follows_jgf() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), "").unwrap();
    fs::write(dir.path().join("index.md"), "[setup](setup.md)").unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "graph"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");

    // Root: must have "graph" key and nothing else
    let root = v.as_object().expect("root is object");
    assert!(root.contains_key("graph"), "root must contain 'graph'");
    for key in root.keys() {
        assert!(
            key == "graph",
            "root has unexpected key '{key}' (JGF allows only 'graph' or 'graphs')"
        );
    }

    let graph = root["graph"].as_object().expect("graph is object");
    let allowed_graph_keys = [
        "id", "label", "directed", "type", "metadata", "nodes", "edges",
    ];
    for key in graph.keys() {
        assert!(
            allowed_graph_keys.contains(&key.as_str()),
            "graph has unexpected key '{key}'"
        );
    }
    assert!(graph["directed"].as_bool().unwrap());

    // Nodes: map of { "label"?, "metadata"? }
    let nodes = graph["nodes"].as_object().expect("nodes is object");
    assert!(nodes.contains_key("index.md"));
    assert!(nodes.contains_key("setup.md"));
    let allowed_node_keys = ["label", "metadata"];
    for (path, node) in nodes {
        let node = node
            .as_object()
            .unwrap_or_else(|| panic!("node '{path}' is object"));
        for key in node.keys() {
            assert!(
                allowed_node_keys.contains(&key.as_str()),
                "node '{path}' has unexpected key '{key}'"
            );
        }
    }
    // Node metadata should carry type and included fields
    assert!(
        nodes["index.md"]["metadata"].get("type").is_some(),
        "node metadata should carry a type field"
    );
    assert_eq!(
        nodes["index.md"]["metadata"]["type"], "file",
        "included markdown file should have type 'file'"
    );
    assert_eq!(
        nodes["index.md"]["metadata"]["included"], true,
        "discovered file should be included"
    );

    // Edges: array of { "source", "target", "id"?, "relation"?, "directed"?, "label"?, "metadata"? }
    let edges = graph["edges"].as_array().expect("edges is array");
    assert!(!edges.is_empty());
    let allowed_edge_keys = [
        "id", "source", "target", "relation", "directed", "label", "metadata",
    ];
    for (i, edge) in edges.iter().enumerate() {
        let edge = edge
            .as_object()
            .unwrap_or_else(|| panic!("edge {i} is object"));
        assert!(edge.contains_key("source"), "edge {i} missing 'source'");
        assert!(edge.contains_key("target"), "edge {i} missing 'target'");
        for key in edge.keys() {
            assert!(
                allowed_edge_keys.contains(&key.as_str()),
                "edge {i} has unexpected key '{key}'"
            );
        }
        // relation is reserved for future semantic relationships
        if let Some(relation) = edge.get("relation") {
            assert!(relation.is_string(), "edge {i} relation is string");
        }
        // parser provenance and internal flag are in metadata
        if let Some(meta) = edge.get("metadata") {
            assert!(meta.is_object(), "edge {i} metadata is object");
        }
    }
}

/// Directory traversal: ../ targets are referenced nodes with included=false.
#[test]
fn traversal_parent_escape_not_included() {
    let outer = TempDir::new().unwrap();
    let graph_root = outer.path().join("project");
    fs::create_dir(&graph_root).unwrap();
    fs::write(graph_root.join("drft.toml"), "").unwrap();
    fs::write(graph_root.join("index.md"), "[escape](../outside.md)").unwrap();
    fs::write(outer.path().join("outside.md"), "secret content").unwrap();

    let output = drft_bin()
        .args([
            "-C",
            graph_root.to_str().unwrap(),
            "graph",
            "--format",
            "json",
        ])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");

    let nodes = v["graph"]["nodes"].as_object().unwrap();
    if let Some(escape_node) = nodes.get("../outside.md") {
        assert_eq!(
            escape_node["metadata"]["included"], false,
            "escape target should not be included"
        );
    }
}

/// Directory traversal: symlink whose canonical target is outside include has hash=None.
#[test]
fn traversal_symlink_escape_not_hashed() {
    let outer = TempDir::new().unwrap();
    let graph_root = outer.path().join("project");
    fs::create_dir(&graph_root).unwrap();
    fs::write(graph_root.join("drft.toml"), "").unwrap();
    fs::write(graph_root.join("index.md"), "[link](trap.md)").unwrap();

    fs::write(outer.path().join("secret.md"), "secret content").unwrap();

    #[cfg(unix)]
    std::os::unix::fs::symlink(outer.path().join("secret.md"), graph_root.join("trap.md")).unwrap();
    #[cfg(not(unix))]
    {
        return;
    }

    let output = drft_bin()
        .args([
            "-C",
            graph_root.to_str().unwrap(),
            "graph",
            "--format",
            "json",
        ])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");

    let nodes = v["graph"]["nodes"].as_object().unwrap();
    if let Some(trap_node) = nodes.get("trap.md") {
        assert!(
            trap_node["metadata"].get("hash").is_none() || trap_node["metadata"]["hash"].is_null(),
            "symlink-escaped target should not be hashed"
        );
    }
}

/// Directory traversal: absolute path target is a referenced node with included=false.
#[test]
fn traversal_absolute_path_not_included() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), "").unwrap();
    fs::write(dir.path().join("index.md"), "[root](/etc/hosts)").unwrap();

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "graph",
            "--format",
            "json",
        ])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");

    let nodes = v["graph"]["nodes"].as_object().unwrap();
    for (path, node) in nodes {
        if path.contains("etc") || path.starts_with('/') {
            assert_eq!(
                node["metadata"]["included"], false,
                "absolute path target should not be included, found: {path}"
            );
        }
    }
}

/// Directory traversal: nested ../ that escapes root is a referenced node with included=false.
#[test]
fn traversal_nested_escape_not_included() {
    let outer = TempDir::new().unwrap();
    let graph_root = outer.path().join("project");
    let sub = graph_root.join("sub");
    fs::create_dir_all(&sub).unwrap();
    fs::write(graph_root.join("drft.toml"), "").unwrap();
    fs::write(sub.join("doc.md"), "[escape](../../outside.md)").unwrap();
    fs::write(outer.path().join("outside.md"), "secret").unwrap();

    let output = drft_bin()
        .args([
            "-C",
            graph_root.to_str().unwrap(),
            "graph",
            "--format",
            "json",
        ])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");

    let nodes = v["graph"]["nodes"].as_object().unwrap();
    for (path, node) in nodes {
        if path.contains("outside") {
            assert_eq!(
                node["metadata"]["included"], false,
                "nested escape target should not be included, found: {path}"
            );
        }
    }
}

#[test]
fn graph_parser_filter_reduces_edges() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "[parsers.markdown]\n[parsers.frontmatter]\n",
    )
    .unwrap();
    // frontmatter links to docs/setup.md, markdown links to config.md — different targets
    let docs = dir.path().join("docs");
    fs::create_dir(&docs).unwrap();
    fs::write(
        dir.path().join("index.md"),
        "---\nsources:\n  - docs/setup.md\n---\n[config](config.md)",
    )
    .unwrap();
    fs::write(docs.join("setup.md"), "# Setup").unwrap();
    fs::write(dir.path().join("config.md"), "# Config").unwrap();

    // Without filter: both markdown and frontmatter edges
    let all = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "graph"])
        .output()
        .unwrap();
    let all_json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&all.stdout)).unwrap();
    let all_edges = all_json["graph"]["edges"].as_array().unwrap();
    assert_eq!(all_edges.len(), 2);

    // With filter: only frontmatter edges
    let filtered = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "graph",
            "--parser",
            "frontmatter",
        ])
        .output()
        .unwrap();
    let filtered_json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&filtered.stdout)).unwrap();
    let filtered_edges = filtered_json["graph"]["edges"].as_array().unwrap();
    assert_eq!(filtered_edges.len(), 1);
    assert_eq!(filtered_edges[0]["metadata"]["parser"], "frontmatter");
}

#[test]
fn graph_unknown_parser_errors() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), "").unwrap();
    fs::write(dir.path().join("index.md"), "# Index").unwrap();

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "graph",
            "--parser",
            "nonexistent",
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown parser \"nonexistent\""));
    assert!(stderr.contains("available:"));
}
