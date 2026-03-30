use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;

use crate::config::Config;
use crate::discovery::{discover, find_child_scopes};
use crate::parsers;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum NodeType {
    Source,
    Resource,
    External,
    Graph,
}

/// Namespaced edge type in the format `parser:type`.
/// Validated on construction — accessors are infallible.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EdgeType {
    parser: String,
    link_type: String,
}

impl EdgeType {
    pub fn new(parser: impl Into<String>, link_type: impl Into<String>) -> Self {
        Self {
            parser: parser.into(),
            link_type: link_type.into(),
        }
    }

    #[allow(dead_code)]
    pub fn parser(&self) -> &str {
        &self.parser
    }

    #[allow(dead_code)]
    pub fn link_type(&self) -> &str {
        &self.link_type
    }
}

impl std::fmt::Display for EdgeType {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}:{}", self.parser, self.link_type)
    }
}

impl std::str::FromStr for EdgeType {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        let (parser, link_type) = s
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("edge type must be 'parser:type', got '{s}'"))?;
        Ok(Self::new(parser, link_type))
    }
}

impl serde::Serialize for EdgeType {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for EdgeType {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Node {
    pub path: String,
    pub node_type: NodeType,
    pub hash: Option<String>,
    /// If set, this node lives in a child graph (value is the child graph's path).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Edge {
    pub source: String,
    pub target: String,
    pub edge_type: EdgeType,
    /// True if created by graph builder (e.g., child-graph coupling edges), not parsed from content.
    #[allow(dead_code)]
    pub synthetic: bool,
}

#[derive(Debug, Default)]
pub struct Graph {
    pub nodes: HashMap<String, Node>,
    pub edges: Vec<Edge>,
    pub forward: HashMap<String, Vec<usize>>,
    pub reverse: HashMap<String, Vec<usize>>,
    pub child_scopes: Vec<String>,
    /// Resolved interface nodes from config (empty = open graph).
    pub interface: Vec<String>,
}

impl Graph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Child graphs (nodes with type Graph).
    #[allow(dead_code)]
    pub fn children(&self) -> impl Iterator<Item = &Node> {
        self.nodes
            .values()
            .filter(|n| n.node_type == NodeType::Graph)
    }

    /// Whether a node is part of this graph's interface.
    #[allow(dead_code)]
    pub fn is_interfaced(&self, path: &str) -> bool {
        self.interface.iter().any(|e| e == path)
    }

    pub fn add_node(&mut self, node: Node) {
        self.nodes.insert(node.path.clone(), node);
    }

    /// Returns true for Source and Resource nodes (excludes External and Graph).
    /// Used by structural analyses that operate only on file-backed nodes.
    pub fn is_file_node(&self, path: &str) -> bool {
        self.nodes
            .get(path)
            .is_some_and(|n| matches!(n.node_type, NodeType::Source | NodeType::Resource))
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

/// Build a graph from files in `root`, using configured parsers to extract links.
/// Computes BLAKE3 content hashes for all nodes.
pub fn build_graph(root: &Path, config: &Config) -> Result<Graph> {
    let all_files = discover(root, &config.ignore)?;
    let child_scopes = find_child_scopes(root, &config.ignore)?;
    let mut graph = Graph::new();
    graph.child_scopes = child_scopes;
    let mut pending_edges = Vec::new();

    // Build parser registry from config
    let parser_list = parsers::build_parsers(&config.parsers, config.config_dir.as_deref());

    // For each file, find matching parsers, run them, collect links
    for file in &all_files {
        // Find all parsers that match this file
        let matching: Vec<&dyn parsers::Parser> = parser_list
            .iter()
            .filter(|p| p.matches(file))
            .map(|p| p.as_ref())
            .collect();

        if matching.is_empty() {
            continue;
        }

        let file_path = root.join(file);
        let content = std::fs::read_to_string(&file_path)?;
        let hash = hash_bytes(content.as_bytes());

        graph.add_node(Node {
            path: file.clone(),
            node_type: NodeType::Source,
            hash: Some(hash),
            graph: None,
        });

        // Run all matching parsers
        for parser in &matching {
            let links = parser.parse(file, &content);
            for link in links {
                let edge_type = EdgeType::new(parser.name(), &link.link_type);
                if link.is_external {
                    pending_edges.push(Edge {
                        source: file.clone(),
                        target: link.target,
                        edge_type,
                        synthetic: false,
                    });
                } else {
                    let resolved = resolve_link(file, &link.target);
                    pending_edges.push(Edge {
                        source: file.clone(),
                        target: resolved,
                        edge_type,
                        synthetic: false,
                    });
                }
            }
        }
    }

    // Create Graph nodes for child scopes — any directory with drft.toml or drft.lock.
    // No hash: staleness within a child graph is the child's concern, not the parent's.
    let scope_prefixes: Vec<String> = graph.child_scopes.clone();
    for scope_dir in &scope_prefixes {
        graph.add_node(Node {
            path: scope_dir.clone(),
            node_type: NodeType::Graph,
            hash: None,
            graph: None,
        });
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

    // Create External, child-graph projection, and Resource nodes for non-Source targets
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
                graph: None,
            });
            continue;
        }

        // Check if target is inside a child scope → Source/Resource with graph field
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
                    node_type: NodeType::Resource,
                    hash: Some(hash),
                    graph: Some(scope_prefix.clone()),
                });
                // Synthetic coupling edge: child-graph node → Graph node
                implicit_edges.push(Edge {
                    source: edge.target.clone(),
                    target: scope_prefix.clone(),
                    edge_type: edge.edge_type.clone(),
                    synthetic: true,
                });
            }
            continue;
        }

        // Skip ignored files — they should not become Resource nodes
        if let Some(ref set) = ignore_set
            && set.is_match(&edge.target)
        {
            continue;
        }

        // Regular Resource node
        let target_path = root.join(&edge.target);
        if target_path.is_file() {
            let content = std::fs::read(&target_path)?;
            let hash = hash_bytes(&content);
            graph.add_node(Node {
                path: edge.target.clone(),
                node_type: NodeType::Resource,
                hash: Some(hash),
                graph: None,
            });
        }
    }

    // Add all edges (explicit + implicit) to the graph
    pending_edges.extend(implicit_edges);
    for edge in pending_edges {
        graph.add_edge(edge);
    }

    // Resolve interface from config
    if let Some(ref iface) = config.interface {
        let mut resolved = Vec::new();
        for pattern in &iface.nodes {
            if let Ok(glob) = globset::Glob::new(pattern) {
                let matcher = glob.compile_matcher();
                for path in graph.nodes.keys() {
                    if matcher.is_match(path) {
                        resolved.push(path.clone());
                    }
                }
            } else {
                // Treat as literal path
                if graph.nodes.contains_key(pattern) {
                    resolved.push(pattern.clone());
                }
            }
        }
        resolved.sort();
        resolved.dedup();
        graph.interface = resolved;
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
            node_type: NodeType::Source,
            hash: None,
            graph: None,
        }
    }

    pub fn make_edge(source: &str, target: &str) -> Edge {
        Edge {
            source: source.into(),
            target: target.into(),
            edge_type: EdgeType::new("markdown", "inline"),
            synthetic: false,
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
            node_type: NodeType::Source,
            hash: None,
            graph: None,
        });
        g.add_node(Node {
            path: "b.md".into(),
            node_type: NodeType::Source,
            hash: None,
            graph: None,
        });
        g.add_edge(Edge {
            source: "a.md".into(),
            target: "b.md".into(),
            edge_type: EdgeType::new("markdown", "inline"),
            synthetic: false,
        });
        assert_eq!(g.forward["a.md"], vec![0]);
        assert_eq!(g.reverse["b.md"], vec![0]);
        assert!(!g.forward.contains_key("b.md"));
    }
}
