//! Graph wiring: build each configured graph independently into its own
//! bare-path namespace, producing the raw [`GraphSet`] (the substrate).
//! Composition into a single graph is a separate projection (see
//! [`crate::compose`]).
//!
//! This layer is also the **adoption seam** where drft auto-hashes: sources and
//! builders never compute hashes; drft does, once per node, from the source
//! bytes.

use std::path::Path;

use anyhow::Result;
use serde_json::Value;

use crate::builders;
use crate::config::Config;
use crate::graph::hash_bytes;
use crate::model::{Graph, GraphSet};
use crate::sources::{self, fs::SourceFile};

/// Build the raw set of per-graph fragments for the graph rooted at `root`.
///
/// v0.8 ships only the `fs` graph. The `fs` source walks the tree once; the
/// `fs` builder types each file and emits symlink edges; drft auto-hashes the
/// file content into each node's `hash`.
pub fn build_set(root: &Path, config: &Config) -> Result<GraphSet> {
    let files = sources::fs::walk(root, config.ignore_patterns())?;
    let mut fs_graph = builders::fs::build(root, &files);
    auto_hash(&mut fs_graph, &files);
    Ok(GraphSet::new(vec![fs_graph]))
}

/// drft's job: hash each node's source bytes into its `hash` metadata. Applied
/// to the `fs` graph, the one v0.8 graph whose nodes carry content.
fn auto_hash(graph: &mut Graph, files: &[SourceFile]) {
    for file in files {
        if let Some(bytes) = &file.bytes
            && let Some(node) = graph.nodes.get_mut(&file.path)
        {
            node.metadata
                .insert("hash".into(), Value::String(hash_bytes(bytes)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn fs_graph_has_typed_hashed_nodes() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("drft.toml"), "").unwrap();
        fs::write(dir.path().join("index.md"), "# Index").unwrap();
        let config = Config::defaults();

        let set = build_set(dir.path(), &config).unwrap();
        assert_eq!(set.graphs.len(), 1);
        let fs_graph = &set.graphs[0];
        assert_eq!(fs_graph.label.as_deref(), Some("fs"));

        let node = &fs_graph.nodes["index.md"];
        assert_eq!(node.metadata["type"], Value::String("file".into()));
        assert!(
            node.metadata["hash"].as_str().unwrap().starts_with("b3:"),
            "node should be auto-hashed"
        );
    }

    #[cfg(unix)]
    #[test]
    fn escaping_symlink_node_has_no_hash() {
        let outer = TempDir::new().unwrap();
        let root = outer.path().join("project");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("drft.toml"), "").unwrap();
        fs::write(outer.path().join("secret.md"), "secret").unwrap();
        std::os::unix::fs::symlink(outer.path().join("secret.md"), root.join("trap.md")).unwrap();

        let set = build_set(&root, &Config::defaults()).unwrap();
        let trap = &set.graphs[0].nodes["trap.md"];
        assert_eq!(trap.metadata["type"], Value::String("symlink".into()));
        assert!(
            trap.metadata.get("hash").is_none(),
            "escaping symlink must not be hashed"
        );
    }
}
