use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;

use crate::config::Config;
use crate::discovery::{discover, find_child_scopes};
use crate::parsing::extract_links;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum NodeType {
    Document,
    Asset,
    External,
    Frontier,
    Virtual,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum EdgeType {
    Inline,
    Reference,
    Autolink,
    Image,
    Frontmatter,
    Wikilink,
}

#[derive(Debug, Clone)]
pub struct Node {
    pub path: String,
    pub node_type: NodeType,
    pub hash: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Edge {
    pub source: String,
    pub target: String,
    pub edge_type: EdgeType,
}

#[derive(Debug, Default)]
pub struct Graph {
    pub nodes: HashMap<String, Node>,
    pub edges: Vec<Edge>,
    pub forward: HashMap<String, Vec<usize>>,
    pub reverse: HashMap<String, Vec<usize>>,
    pub child_scopes: Vec<String>,
}

impl Graph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_node(&mut self, node: Node) {
        self.nodes.insert(node.path.clone(), node);
    }

    /// Returns true for Document and Asset nodes (excludes External, Frontier, Virtual).
    pub fn is_real_node(&self, path: &str) -> bool {
        self.nodes
            .get(path)
            .is_some_and(|n| matches!(n.node_type, NodeType::Document | NodeType::Asset))
    }

    pub fn add_edge(&mut self, edge: Edge) {
        let idx = self.edges.len();
        self.forward
            .entry(edge.source.clone())
            .or_default()
            .push(idx);
        self.reverse
            .entry(edge.target.clone())
            .or_default()
            .push(idx);
        self.edges.push(edge);
    }
}

/// Hash file contents with BLAKE3, returning `b3:<hex>`.
pub fn hash_bytes(content: &[u8]) -> String {
    format!("b3:{}", blake3::hash(content).to_hex())
}

/// Build a graph from the markdown files in `root`, using `config` for ignore patterns.
/// Computes BLAKE3 content hashes for all nodes.
pub fn build_graph(root: &Path, config: &Config) -> Result<Graph> {
    let files = discover(root, &config.ignore)?;
    let child_scopes = find_child_scopes(root)?;
    let mut graph = Graph::new();
    graph.child_scopes = child_scopes;
    let mut pending_edges = Vec::new();

    // Discover documents: read, hash, and extract links in one pass
    for file in &files {
        let file_path = root.join(file);
        let content = std::fs::read_to_string(&file_path)?;
        let hash = hash_bytes(content.as_bytes());

        graph.add_node(Node {
            path: file.clone(),
            node_type: NodeType::Document,
            hash: Some(hash),
        });

        let links = extract_links(&content);
        for link in links {
            if link.is_external {
                // External URLs are stored as-is, no path resolution
                pending_edges.push(Edge {
                    source: file.clone(),
                    target: link.target,
                    edge_type: link.link_type,
                });
            } else {
                let resolved = resolve_link(file, &link.target);
                pending_edges.push(Edge {
                    source: file.clone(),
                    target: resolved,
                    edge_type: link.link_type,
                });
            }
        }
    }

    // Create frontier nodes for child scopes
    let scope_prefixes: Vec<String> = graph.child_scopes.clone();
    for scope_dir in &scope_prefixes {
        let child_lock_path = root.join(scope_dir.trim_end_matches('/')).join("drft.lock");
        if let Ok(content) = std::fs::read(&child_lock_path) {
            let hash = hash_bytes(&content);
            graph.add_node(Node {
                path: scope_dir.clone(),
                node_type: NodeType::Frontier,
                hash: Some(hash),
            });
        }
    }

    // Build ignore set for filtering asset nodes
    let ignore_set = if config.ignore.is_empty() {
        None
    } else {
        let mut builder = globset::GlobSetBuilder::new();
        for pattern in &config.ignore {
            if let Ok(glob) = globset::Glob::new(pattern) {
                builder.add(glob);
            }
        }
        builder.build().ok()
    };

    // Create external, virtual, and asset nodes for non-document targets
    let mut implicit_edges = Vec::new();
    for edge in &pending_edges {
        if graph.nodes.contains_key(&edge.target) {
            continue;
        }
        if edge.target.starts_with("http://") || edge.target.starts_with("https://") {
            graph.add_node(Node {
                path: edge.target.clone(),
                node_type: NodeType::External,
                hash: None,
            });
            continue;
        }

        // Check if target is inside a child scope → virtual node
        let in_child_scope = scope_prefixes
            .iter()
            .find(|s| edge.target.starts_with(s.as_str()));
        if let Some(scope_prefix) = in_child_scope {
            let target_path = root.join(&edge.target);
            if target_path.is_file() {
                let content = std::fs::read(&target_path)?;
                let hash = hash_bytes(&content);
                graph.add_node(Node {
                    path: edge.target.clone(),
                    node_type: NodeType::Virtual,
                    hash: Some(hash),
                });
                // Implicit edge: virtual depends on frontier
                implicit_edges.push(Edge {
                    source: edge.target.clone(),
                    target: scope_prefix.clone(),
                    edge_type: EdgeType::Inline,
                });
            }
            continue;
        }

        // Skip ignored files — they should not become asset nodes
        if let Some(ref set) = ignore_set
            && set.is_match(&edge.target)
        {
            continue;
        }

        // Regular asset node
        let target_path = root.join(&edge.target);
        if target_path.is_file() {
            let content = std::fs::read(&target_path)?;
            let hash = hash_bytes(&content);
            graph.add_node(Node {
                path: edge.target.clone(),
                node_type: NodeType::Asset,
                hash: Some(hash),
            });
        }
    }

    // Add all edges (explicit + implicit) to the graph
    pending_edges.extend(implicit_edges);
    for edge in pending_edges {
        graph.add_edge(edge);
    }

    Ok(graph)
}

/// Normalize a relative path by resolving `.` and `..` components using Path APIs.
/// Does not touch the filesystem. Always returns forward-slash separated paths.
/// Preserves leading `..` that escape above the root — these indicate scope escape.
pub fn normalize_relative_path(path: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    for component in Path::new(path).components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                // Only pop if there's a normal component to pop (not a leading ..)
                if parts.last().is_some_and(|p| p != "..") {
                    parts.pop();
                } else {
                    parts.push("..".to_string());
                }
            }
            std::path::Component::Normal(c) => parts.push(c.to_string_lossy().to_string()),
            _ => {}
        }
    }
    parts.join("/")
}

/// Resolve a link target relative to a source file, producing a path relative to the scope root.
/// Uses Path::join for correct platform-aware path handling.
pub fn resolve_link(source_file: &str, raw_target: &str) -> String {
    let source_path = Path::new(source_file);
    let source_dir = source_path.parent().unwrap_or(Path::new(""));
    let joined = source_dir.join(raw_target);
    normalize_relative_path(&joined.to_string_lossy())
}

#[cfg(test)]
pub mod test_helpers {
    use super::*;

    pub fn make_node(path: &str) -> Node {
        Node {
            path: path.into(),
            node_type: NodeType::Document,
            hash: None,
        }
    }

    pub fn make_edge(source: &str, target: &str) -> Edge {
        Edge {
            source: source.into(),
            target: target.into(),
            edge_type: EdgeType::Inline,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_simple() {
        assert_eq!(normalize_relative_path("a/b/c"), "a/b/c");
    }

    #[test]
    fn normalize_dot() {
        assert_eq!(normalize_relative_path("./a/./b"), "a/b");
    }

    #[test]
    fn normalize_dotdot() {
        assert_eq!(normalize_relative_path("a/b/../c"), "a/c");
    }

    #[test]
    fn normalize_preserves_leading_dotdot() {
        assert_eq!(normalize_relative_path("../a"), "../a");
    }

    #[test]
    fn normalize_deep_escape() {
        assert_eq!(normalize_relative_path("../../a"), "../../a");
    }

    #[test]
    fn normalize_escape_after_descent() {
        // guides/../../README.md -> ../README.md (one level above root)
        assert_eq!(
            normalize_relative_path("guides/../../README.md"),
            "../README.md"
        );
    }

    #[test]
    fn resolve_same_dir() {
        assert_eq!(resolve_link("index.md", "setup.md"), "setup.md");
    }

    #[test]
    fn resolve_subdir() {
        assert_eq!(
            resolve_link("guides/intro.md", "setup.md"),
            "guides/setup.md"
        );
    }

    #[test]
    fn resolve_parent() {
        assert_eq!(resolve_link("guides/intro.md", "../config.md"), "config.md");
    }

    #[test]
    fn graph_adjacency() {
        let mut g = Graph::new();
        g.add_node(Node {
            path: "a.md".into(),
            node_type: NodeType::Document,
            hash: None,
        });
        g.add_node(Node {
            path: "b.md".into(),
            node_type: NodeType::Document,
            hash: None,
        });
        g.add_edge(Edge {
            source: "a.md".into(),
            target: "b.md".into(),
            edge_type: EdgeType::Inline,
        });
        assert_eq!(g.forward["a.md"], vec![0]);
        assert_eq!(g.reverse["b.md"], vec![0]);
        assert!(!g.forward.contains_key("b.md"));
    }
}
