//! Graph wiring: build each configured graph independently into its own
//! bare-path namespace, producing the raw [`GraphSet`] (the substrate).
//! Composition into a single graph is a separate projection (see
//! [`crate::compose`]).
//!
//! This layer is also the **adoption seam** where drft auto-hashes: sources and
//! builders never compute hashes; drft does, once per node, from the source
//! bytes.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::Result;
use serde_json::Value;

use crate::builders;
use crate::config::{Config, compile_globs};
use crate::diagnostic::Finding;
use crate::hints::{Hint, Hints};
use crate::model::{Graph, GraphSet};
use crate::sources::{
    self,
    fs::{NodeKind, SourceFile},
};
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
pub fn build_set(
    root: &Path,
    config: &Config,
    hints: &mut Hints,
    findings: &mut Vec<Finding>,
) -> Result<GraphSet> {
    let mut ignore = config.ignore_patterns().to_vec();
    ignore.push(LOCKFILE_IGNORE.to_string());
    let files = sources::fs::walk(root, &ignore)?;

    build_from_files(root, config, hints, findings, &files)
}

fn build_from_files(
    root: &Path,
    config: &Config,
    hints: &mut Hints,
    findings: &mut Vec<Finding>,
    files: &[SourceFile],
) -> Result<GraphSet> {
    let mut fs_graph = builders::fs::build(root, files);
    auto_hash(&mut fs_graph, files);

    let mut graphs = vec![fs_graph];

    // Decode each file's bytes once for the text builders. Non-UTF-8 files stay
    // in the fs graph, with their raw-byte hash, and are reported below when a
    // configured text graph would otherwise have read them.
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
    // Here rather than in the frontmatter parser, because a mark costs a file its
    // headings too: the markdown library declines to open a metadata block on a
    // line that does not start with the fence, so a marked file's frontmatter
    // renders as a setext heading. A strip inside the frontmatter parser never
    // reaches the copy the markdown parser is handed.
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
    let mut invalid_text_paths = BTreeSet::new();
    let texts: Vec<(String, String)> = files
        .iter()
        .filter_map(|f| {
            let bytes = f.bytes.as_ref()?;
            match std::str::from_utf8(bytes) {
                Ok(text) => {
                    let text = text.trim_start_matches('\u{feff}');
                    Some((f.path.clone(), text.to_string()))
                }
                Err(_) => {
                    if f.kind == NodeKind::File {
                        invalid_text_paths.insert(f.path.clone());
                    }
                    None
                }
            }
        })
        .collect();

    let mut unreadable_text: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for (name, graph) in &config.graphs {
        let graph_files = compile_globs(&graph.files)?;
        for path in &invalid_text_paths {
            let matches_graph = graph_files
                .as_ref()
                .is_none_or(|patterns| patterns.is_match(path));
            if matches_graph {
                unreadable_text
                    .entry(path.clone())
                    .or_default()
                    .insert(format!("@{name}"));
            }
        }
        match graph.parser.as_str() {
            "markdown" => graphs.push(builders::markdown::build(name, &texts, graph_files)),
            "frontmatter" => {
                let parser_files = graph_files.clone();
                let fragment = builders::frontmatter::build(
                    name,
                    &texts,
                    graph_files,
                    graph.edge_keys.clone(),
                    findings,
                );
                // Use walked regular files before decoding, and raw parser
                // findings before configured severity or ignores. A hidden
                // failure still makes speculative spelling advice misleading.
                let provenance = format!("@{name}");
                let failed_frontmatter: BTreeSet<&str> = findings
                    .iter()
                    .filter(|finding| {
                        finding.name == "unreadable-frontmatter"
                            && finding.graphs.contains(&provenance)
                    })
                    .map(|finding| finding.subject.as_str())
                    .collect();
                let mut candidates = 0;
                let mut failures = 0;
                for file in files.iter().filter(|file| {
                    file.kind == NodeKind::File
                        && parser_files
                            .as_ref()
                            .is_none_or(|patterns| patterns.is_match(&file.path))
                }) {
                    candidates += 1;
                    // Missing bytes currently have no construction finding.
                    // They still establish that this candidate was unreadable.
                    if file.bytes.is_none()
                        || invalid_text_paths.contains(&file.path)
                        || failed_frontmatter.contains(file.path.as_str())
                    {
                        failures += 1;
                    }
                }
                if !graph.edge_keys.is_empty() && fragment.edges.is_empty() {
                    let keys = render_keys(&graph.edge_keys);
                    let (message, next) = if failures > 0 && failures == candidates {
                        (
                            format!(
                                "declares {keys} but yielded no edges; matched files could not be read"
                            ),
                            "repair the unreadable files matched by this graph, then rerun",
                        )
                    } else if failures > 0 {
                        (
                            format!(
                                "declares {keys} but yielded no edges; some matched files could not be read"
                            ),
                            "repair the unreadable files matched by this graph, then rerun; if no edges remain, check the declared keys, files globs, ignore patterns, and string values",
                        )
                    } else if candidates > 0 {
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
                            // No candidates also covers an unwritten corpus or
                            // files excluded by the walk's ignore patterns.
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

    findings.extend(unreadable_text.into_iter().map(|(path, graphs)| {
        Finding::warn(
            "unreadable-text",
            path,
            graphs.into_iter().collect(),
            "file is not valid UTF-8, so matched text graphs could not read its edges or metadata",
        )
    }));

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

    fn hint_fixture(files: &[SourceFile], findings: &mut Vec<Finding>) -> Hint {
        let mut config = Config::defaults();
        config.graphs.clear();
        config.graphs.insert(
            "fm".into(),
            crate::config::GraphConfig {
                parser: "frontmatter".into(),
                files: vec!["*.md".into()],
                edge_keys: vec!["sources".into()],
            },
        );
        let mut hints = Hints::default();
        build_from_files(Path::new("."), &config, &mut hints, findings, files).unwrap();
        hints.as_slice()[0].clone()
    }

    fn source(path: &str, kind: NodeKind, bytes: Option<&[u8]>) -> SourceFile {
        SourceFile {
            path: path.into(),
            kind,
            bytes: bytes.map(Vec::from),
        }
    }

    #[test]
    fn zero_edge_hint_partitions_each_read_failure_before_decoding() {
        for bytes in [
            None,
            Some(&b"\xff"[..]),
            Some(&b"---\nsources: [\n---\n"[..]),
        ] {
            let mut findings = Vec::new();
            let hint = hint_fixture(&[source("bad.md", NodeKind::File, bytes)], &mut findings);
            assert_eq!(
                hint.next.as_deref(),
                Some("repair the unreadable files matched by this graph, then rerun")
            );
            assert!(hint.message.contains("matched files could not be read"));
            if bytes.is_none() {
                assert!(findings.is_empty(), "absent bytes add no diagnostic");
            }
            let mixed = hint_fixture(
                &[
                    source("bad.md", NodeKind::File, bytes),
                    source("plain.md", NodeKind::File, Some(b"no metadata")),
                ],
                &mut Vec::new(),
            );
            assert!(
                mixed
                    .message
                    .contains("some matched files could not be read")
            );
            assert_eq!(
                mixed.next.as_deref(),
                Some(
                    "repair the unreadable files matched by this graph, then rerun; if no edges remain, check the declared keys, files globs, ignore patterns, and string values"
                )
            );
        }
    }

    #[test]
    fn zero_edge_hint_uses_only_regular_files_matching_this_graph() {
        let hint = hint_fixture(
            &[
                source("folder.md", NodeKind::Dir, None),
                source("link.md", NodeKind::Symlink, None),
                source("other.txt", NodeKind::File, None),
            ],
            &mut Vec::new(),
        );
        assert!(hint.message.contains("no file was read"));
        assert!(hint.next.unwrap().contains("globs"));
        let hint = hint_fixture(
            &[
                source("bad.md", NodeKind::File, None),
                source("folder.md", NodeKind::Dir, None),
                source("link.md", NodeKind::Symlink, None),
                source("other.txt", NodeKind::File, Some(b"readable")),
            ],
            &mut Vec::new(),
        );
        assert_eq!(
            hint.next.as_deref(),
            Some("repair the unreadable files matched by this graph, then rerun")
        );
    }

    #[test]
    fn zero_edge_hint_requires_exact_graph_read_evidence() {
        for (rule, graph) in [
            ("unreadable-frontmatter", "@other"),
            ("stale-node", "@fm"),
            ("unresolved-edge", "@fm"),
        ] {
            let mut findings = vec![Finding::warn(
                rule,
                "plain.md",
                vec![graph.into()],
                "fixture",
            )];
            let hint = hint_fixture(
                &[source("plain.md", NodeKind::File, Some(b"plain"))],
                &mut findings,
            );
            assert!(hint.next.unwrap().contains("spelling"), "{rule} {graph}");
        }
        let hint = hint_fixture(&[], &mut Vec::new());
        assert!(hint.message.contains("no file was read"));
    }

    #[test]
    fn fs_graph_has_typed_hashed_nodes() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("drft.toml"), "").unwrap();
        fs::write(dir.path().join("index.md"), "# Index").unwrap();
        let config = Config::defaults();

        let set = build_set(dir.path(), &config, &mut Hints::default(), &mut Vec::new()).unwrap();
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

        let set = build_set(
            &root,
            &Config::defaults(),
            &mut Hints::default(),
            &mut Vec::new(),
        )
        .unwrap();
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

        let set = build_set(
            dir.path(),
            &Config::defaults(),
            &mut Hints::default(),
            &mut Vec::new(),
        )
        .unwrap();
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
