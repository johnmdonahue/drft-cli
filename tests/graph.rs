mod common;
use common::drft_bin;
use std::fs;
use tempfile::TempDir;

fn graph_json(dir: &std::path::Path) -> serde_json::Value {
    let output = drft_bin()
        .args(["-C", dir.to_str().unwrap(), "graph"])
        .output()
        .unwrap();
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
    fs::write(dir.path().join("drft.toml"), "").unwrap();
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
    fs::write(dir.path().join("drft.toml"), "").unwrap();
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
    fs::write(dir.path().join("drft.toml"), "").unwrap();
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
    fs::write(root.join("drft.toml"), "").unwrap();
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
