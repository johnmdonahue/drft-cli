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
use crate::config::{Config, compile_globs};
use crate::model::{Graph, GraphSet};
use crate::sources::{self, fs::SourceFile};
use crate::util::hash_bytes;

/// drft's own lockfile is never graph content — hashing it would be circular
/// (its bytes change every time it's written). The wiring layer always excludes
/// it from the `fs` walk.
const LOCKFILE_IGNORE: &str = "drft.lock";

/// Build the raw set of per-graph fragments for the graph rooted at `root`.
///
/// `fs` is implicit and always builds first — it owns the identity space. Each
/// configured text graph (`[graphs.*]`) then builds over the same fs walk's
/// content, scoped by its filter and labeled with the graph's name.
pub fn build_set(root: &Path, config: &Config) -> Result<GraphSet> {
    let mut ignore = config.ignore_patterns().to_vec();
    ignore.push(LOCKFILE_IGNORE.to_string());
    let files = sources::fs::walk(root, &ignore)?;

    let mut fs_graph = builders::fs::build(root, &files);
    auto_hash(&mut fs_graph, &files);

    let mut graphs = vec![fs_graph];

    // Decode each file's bytes once for the text builders. Non-UTF-8 files are
    // skipped (they have no text edges or metadata).
    let texts: Vec<(String, String)> = files
        .iter()
        .filter_map(|f| {
            f.bytes
                .as_ref()
                .and_then(|b| String::from_utf8(b.clone()).ok())
                .map(|text| (f.path.clone(), text))
        })
        .collect();

    for (name, graph) in &config.graphs {
        let filter = compile_globs(&graph.filter)?;
        match graph.builder.as_str() {
            "markdown" => graphs.push(builders::markdown::build(name, &texts, filter)),
            "frontmatter" => graphs.push(builders::frontmatter::build(name, &texts, filter)),
            // `fs` is the implicit base graph, already built above.
            "fs" => {}
            other => eprintln!("warn: unknown builder \"{other}\" for graph \"{name}\""),
        }
    }

    Ok(GraphSet::new(graphs))
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
        // fs is always the base graph (built first); defaults also enable markdown.
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
