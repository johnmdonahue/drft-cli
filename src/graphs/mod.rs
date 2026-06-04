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
use globset::GlobSet;
use serde_json::Value;

use crate::builders;
use crate::config::{Config, compile_globs};
use crate::graph::hash_bytes;
use crate::model::{Graph, GraphSet};
use crate::sources::{self, fs::SourceFile};

/// The default file filter for the text graphs (`markdown`, `frontmatter`) when
/// the config does not scope them. fs walks every file, so the text builders
/// must scope themselves to markdown.
const DEFAULT_TEXT_FILTER: &str = "**/*.md";

/// Build the raw set of per-graph fragments for the graph rooted at `root`.
///
/// `fs` always builds first — it owns the identity space. The `markdown` and
/// `frontmatter` text graphs build when their builder is enabled in config
/// (until the config reshape, this reads the `[parsers.*]` blocks). The `fs`
/// source walks the tree once; its content feeds the text builders.
pub fn build_set(root: &Path, config: &Config) -> Result<GraphSet> {
    let files = sources::fs::walk(root, config.ignore_patterns())?;

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

    if let Some(parser) = config.parsers.get("markdown") {
        let filter = text_filter(&parser.files)?;
        graphs.push(builders::markdown::build(&texts, filter));
    }
    if let Some(parser) = config.parsers.get("frontmatter") {
        let filter = text_filter(&parser.files)?;
        graphs.push(builders::frontmatter::build(&texts, filter));
    }

    Ok(GraphSet::new(graphs))
}

/// Compile a text graph's file filter, defaulting to markdown files when the
/// config leaves it unscoped.
fn text_filter(files: &Option<Vec<String>>) -> Result<Option<GlobSet>> {
    match files {
        Some(patterns) => compile_globs(patterns),
        None => compile_globs(&[DEFAULT_TEXT_FILTER.to_string()]),
    }
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
