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
    //
    // A leading byte-order mark is dropped here, which is the one place that
    // serves every text parser at once. A frontmatter block opens with `---` at
    // offset 0, and a BOM ahead of it means no parser recognizes the block: the
    // file loses its metadata and its declared edges, the markdown parser is
    // handed the block as body text, and nothing reports any of it.
    //
    // Here rather than in the `fs` source, because that is what `auto_hash`
    // hashes — stripping there moves every BOM-carrying file's hash, reports
    // `stale-node` on files nobody edited, and stops drft's `b3:` being the
    // file's blake3. Here rather than in the frontmatter parser, because
    // `parsed_block` returns an offset into this text and the markdown parser
    // masks with it; skipping three bytes without adjusting that offset leaves
    // the closing fence partly unmasked. Removing bytes at offset 0 removes no
    // newline, so line numbers are unaffected.
    let texts: Vec<(String, String)> = files
        .iter()
        .filter_map(|f| {
            f.bytes
                .as_ref()
                .and_then(|b| std::str::from_utf8(b).ok())
                .map(|text| {
                    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
                    (f.path.clone(), text.to_string())
                })
        })
        .collect();

    for (name, graph) in &config.graphs {
        let files = compile_globs(&graph.files)?;
        match graph.parser.as_str() {
            "markdown" => graphs.push(builders::markdown::build(name, &texts, files)),
            "frontmatter" => graphs.push(builders::frontmatter::build(
                name,
                &texts,
                files,
                graph.keys.clone(),
            )),
            // Parser names are validated at config load (`KNOWN_PARSERS`); an
            // unknown parser cannot reach here.
            other => unreachable!("unvalidated parser \"{other}\""),
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
        // fs is always the base graph, built first regardless of config.
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

    #[cfg(unix)]
    #[test]
    fn inroot_symlink_node_is_not_hashed() {
        // A symlink is untrackable even when its target is in-graph: it carries no
        // hash. Staleness reaches it through the edge to the (hashed) target.
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("drft.toml"), "").unwrap();
        fs::write(dir.path().join("real.md"), "content").unwrap();
        std::os::unix::fs::symlink(dir.path().join("real.md"), dir.path().join("alias.md"))
            .unwrap();

        let set = build_set(dir.path(), &Config::defaults()).unwrap();
        let nodes = &set.graphs[0].nodes;
        assert_eq!(
            nodes["alias.md"].metadata["type"],
            Value::String("symlink".into())
        );
        assert!(
            nodes["alias.md"].metadata.get("hash").is_none(),
            "in-root symlink must not be hashed"
        );
        assert!(
            nodes["real.md"].metadata["hash"]
                .as_str()
                .unwrap()
                .starts_with("b3:"),
            "the real target is still hashed"
        );
    }
}
