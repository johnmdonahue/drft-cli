//! The `fs` builder: the base graph. Emits a node per entry, typed by stat, plus
//! a symlink edge per symlink. It does not hash — drft auto-hashes at the wiring
//! seam (see [`crate::graphs`]).

use std::path::{Component, Path};

use serde_json::Value;

use crate::model::{Edge, Graph, Metadata, Node};
use crate::sources::fs::{NodeKind, SourceFile};

/// Build the `fs` graph fragment from walked source entries. Each entry becomes
/// a bare-path node carrying its `type` (`file`, `symlink`, or `directory`);
/// each symlink also contributes an edge to its resolved target within the graph
/// root. Directory nodes carry no edge — containment is implicit in path
/// structure.
pub fn build(root: &Path, files: &[SourceFile]) -> Graph {
    let mut graph = Graph::labeled("fs");

    for file in files {
        let node_type = match file.kind {
            NodeKind::Symlink => "symlink",
            NodeKind::Dir => "directory",
            NodeKind::File => "file",
        };
        let mut meta = Metadata::new();
        meta.insert("type".into(), Value::String(node_type.into()));
        graph.set_node(file.path.clone(), Node::new(meta));

        if file.kind == NodeKind::Symlink
            && let Some(target) = symlink_target(root, &file.path)
        {
            graph.add_edge(Edge::new(file.path.clone(), target));
        }
    }

    graph
}

/// Resolve a symlink at `path` to a graph-relative target within `root`.
/// Returns `None` for broken links or targets that escape the root.
fn symlink_target(root: &Path, path: &str) -> Option<String> {
    let abs = root.join(path);
    let link = std::fs::read_link(&abs).ok()?;

    let resolved = if link.is_absolute() {
        let canonical_root = root.canonicalize().ok()?;
        link.canonicalize()
            .ok()?
            .strip_prefix(&canonical_root)
            .ok()?
            .to_string_lossy()
            .replace('\\', "/")
    } else {
        crate::util::resolve_link(path, &link.to_string_lossy())
    };

    let escapes_root = Path::new(&resolved)
        .components()
        .next()
        .is_some_and(|c| matches!(c, Component::ParentDir));
    if escapes_root {
        return None;
    }

    Some(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn source(path: &str, bytes: &str) -> SourceFile {
        SourceFile {
            path: path.into(),
            kind: NodeKind::File,
            bytes: Some(bytes.as_bytes().to_vec()),
        }
    }

    #[test]
    fn types_regular_files() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.md"), "a").unwrap();
        let graph = build(dir.path(), &[source("a.md", "a")]);
        assert_eq!(graph.label.as_deref(), Some("fs"));
        let node = &graph.nodes["a.md"];
        assert_eq!(node.metadata["type"], Value::String("file".into()));
        // The builder does not hash.
        assert!(node.metadata.get("hash").is_none());
        assert!(graph.edges.is_empty());
    }

    #[test]
    fn types_directories() {
        let dir = TempDir::new().unwrap();
        let entry = SourceFile {
            path: "guides".into(),
            kind: NodeKind::Dir,
            bytes: None,
        };
        let graph = build(dir.path(), &[entry]);
        let node = &graph.nodes["guides"];
        assert_eq!(node.metadata["type"], Value::String("directory".into()));
        // Directories are never hashed and contribute no edge.
        assert!(node.metadata.get("hash").is_none());
        assert!(graph.edges.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_within_root_emits_edge() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("real.md"), "r").unwrap();
        std::os::unix::fs::symlink(dir.path().join("real.md"), dir.path().join("alias.md"))
            .unwrap();

        let alias = SourceFile {
            kind: NodeKind::Symlink,
            ..source("alias.md", "r")
        };
        let files = vec![alias, source("real.md", "r")];
        let graph = build(dir.path(), &files);
        assert_eq!(
            graph.nodes["alias.md"].metadata["type"],
            Value::String("symlink".into())
        );
        assert_eq!(graph.edges.len(), 1);
        assert_eq!(graph.edges[0].source, "alias.md");
        assert_eq!(graph.edges[0].target, "real.md");
    }
}
