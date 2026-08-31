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
use crate::hints::{Hint, Hints};
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
pub fn build_set(root: &Path, config: &Config, hints: &mut Hints) -> Result<GraphSet> {
    let mut ignore = config.ignore_patterns().to_vec();
    ignore.push(LOCKFILE_IGNORE.to_string());
    let files = sources::fs::walk(root, &ignore)?;

    let mut fs_graph = builders::fs::build(root, &files);
    auto_hash(&mut fs_graph, &files);

    let mut graphs = vec![fs_graph];

    // Decode each file's bytes once for the text builders. Non-UTF-8 files are
    // skipped (they have no text edges or metadata).
    //
    // Leading byte-order marks are dropped here, which is the one place that
    // serves every text parser at once. A frontmatter block opens with `---` at
    // offset 0, and a mark ahead of it means no parser recognizes the block: the
    // file loses its metadata and its declared edges, the markdown parser is
    // handed the block as body text, and nothing reports any of it.
    //
    // *Marks*, plural: a tool re-marking an already-marked file writes two, and
    // stripping one would leave that file failing exactly as it did before, just
    // as quietly. Accommodating one mark and silently dropping a file with two is
    // the incoherent position.
    //
    // Here rather than in the `fs` source, because that is what `auto_hash`
    // hashes — stripping there moves every mark-carrying file's hash, reports
    // `stale-node` on files nobody edited, and stops drft's `b3:` being the
    // file's blake3.
    //
    // Here rather than in the frontmatter parser for two reasons, and the second
    // is the one that decides it. `parsed_block` returns an offset into this text
    // and the markdown parser masks with it, so skipping the mark's bytes without
    // adjusting that offset leaves the closing fence partly unmasked. More
    // simply: a mark costs a file its headings too, and a strip inside the
    // frontmatter parser never reaches the copy the markdown parser is handed.
    //
    // That this is the *only* such place is the property to preserve. Normalizing
    // inside each text parser instead is output-identical today and stays green,
    // and it silently gives a parser added later no strip at all.
    //
    // What keeps this away from the hash is that the decode builds an owned copy
    // and never touches `files` — not the fact that it runs after `auto_hash`.
    // Reordering the two statements changes nothing; moving the normalization
    // into a source changes every mark-carrying file's hash.
    //
    // Removing bytes at offset 0 removes no newline, so line numbers are
    // unaffected. Byte offsets into the original file are not: everything here
    // is an offset into the stripped text, which is what every consumer of this
    // text uses. And a U+FEFF that is genuinely content rather than a mark is
    // indistinguishable at offset 0, so a file opening with one loses it from the
    // text while keeping it in the hash.
    let texts: Vec<(String, String)> = files
        .iter()
        .filter_map(|f| {
            f.bytes
                .as_ref()
                .and_then(|b| std::str::from_utf8(b).ok())
                .map(|text| {
                    let text = text.trim_start_matches('\u{feff}');
                    (f.path.clone(), text.to_string())
                })
        })
        .collect();

    for (name, graph) in &config.graphs {
        let files = compile_globs(&graph.files)?;
        match graph.parser.as_str() {
            "markdown" => graphs.push(builders::markdown::build(name, &texts, files)),
            "frontmatter" => {
                let parser_files = files.clone();
                let fragment =
                    builders::frontmatter::build(name, &texts, files, graph.edge_keys.clone());
                // Declaring keys states an expectation the corpus can fail to
                // meet, and every way of failing it produces a graph tracking
                // nothing while the config says otherwise, at exit 0.
                //
                // Two ways, and the message says which, because the remedy
                // differs: the graph's globs reached no file at all, or they
                // reached files and no value sat under a declared key.
                //
                // Declaring *no* keys is not this state: a frontmatter graph may
                // exist purely to seed node metadata, and a graph that is as
                // intended has nothing to report.
                //
                // There is deliberately no exemption for a repository with
                // nothing in it yet. One was tried and it swallowed a misspelled
                // `files` glob — a graph reaching no file looks identical whether
                // the globs are wrong or the files are unwritten, so exempting
                // the second hides the first. The first message covers both, and
                // names the two remedies rather than assuming which applies.
                let matched_any = texts.iter().any(|(path, _)| match &parser_files {
                    Some(set) => set.is_match(path),
                    None => true,
                });
                if !graph.edge_keys.is_empty() && fragment.edges.is_empty() {
                    let keys = render_keys(&graph.edge_keys);
                    let (message, next) = if matched_any {
                        (
                            // "Nothing yielded an edge" rather than "no value was
                            // found": a number, a boolean, an empty string or an
                            // empty list under a declared key is a value that is
                            // present and still names no target, and saying it was
                            // not found contradicts the `@frontmatter` block the
                            // same run prints.
                            format!(
                                "declares {keys} but nothing under {} yielded an edge",
                                if graph.edge_keys.len() == 1 {
                                    "it"
                                } else {
                                    "any of them"
                                }
                            ),
                            // Every reason a declared key can come up empty, for
                            // the same reason the other branch names all of its
                            // own: reaching some file is not reaching the right
                            // one, an `ignore` pattern can drop exactly the file
                            // that carried the derivations, and only a string
                            // names a target.
                            "check the spelling against the frontmatter the files carry, the graph's `files` globs and any `ignore` patterns, and that the values are strings",
                        )
                    } else {
                        (
                            // "No file was read" rather than "the globs matched
                            // nothing": a file the globs do reach is dropped here
                            // if it is ignored, unreadable, or not UTF-8 text, and
                            // this seam cannot tell those apart. Naming only the
                            // globs sent the reader to a correct one.
                            format!("declares {keys}, but no file was read for this graph"),
                            "check the `files` globs, any `ignore` patterns, and that the matched files are readable UTF-8 text",
                        )
                    };
                    hints.push(
                        Hint::new("edge-keys-matched-nothing", message)
                            .at(format!("graphs.{name}"))
                            .with_next(next.to_string()),
                    );
                }
                graphs.push(fragment)
            }
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

/// Render a key list for a hint message: `` `a` ``, `` `a` and `b` ``, or
/// `` `a`, `b` and `c` ``.
fn render_keys(keys: &[String]) -> String {
    let quoted: Vec<String> = keys.iter().map(|k| format!("`{k}`")).collect();
    match quoted.split_last() {
        None => String::new(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
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

        let set = build_set(dir.path(), &config, &mut Hints::default()).unwrap();
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

        let set = build_set(&root, &Config::defaults(), &mut Hints::default()).unwrap();
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

        let set = build_set(dir.path(), &Config::defaults(), &mut Hints::default()).unwrap();
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
