use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;

use crate::config::Config;
use crate::discovery::{discover, find_child_graphs};
use crate::parsers;

/// Check if a target string is a URI (has a scheme per RFC 3986).
/// A scheme is `[a-zA-Z][a-zA-Z0-9+.-]*:` — e.g., `http:`, `mailto:`, `ftp:`, `tel:`.
pub fn is_uri(target: &str) -> bool {
    let bytes = target.as_bytes();
    if bytes.is_empty() || !bytes[0].is_ascii_alphabetic() {
        return false;
    }
    for &b in &bytes[1..] {
        if b == b':' {
            return true;
        }
        if !b.is_ascii_alphanumeric() && b != b'+' && b != b'-' && b != b'.' {
            return false;
        }
    }
    false
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum NodeType {
    File,
    External,
    Graph,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Node {
    pub path: String,
    pub node_type: NodeType,
    pub hash: Option<String>,
    /// If set, this node lives in a child graph (value is the child graph's path).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph: Option<String>,
    /// Structured metadata from parsers, keyed by parser name.
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct Edge {
    pub source: String,
    /// Node identity — always matches a key in `graph.nodes` (or is a dangling target).
    /// Fragment-stripped: `bar.md`, not `bar.md#heading`.
    pub target: String,
    /// Original link when it differs from target (e.g., `bar.md#heading`).
    /// Absent when the link resolved to exactly the node ID.
    pub link: Option<String>,
    /// Which parser discovered this edge (provenance).
    pub parser: String,
}

/// Filesystem properties of an edge target, probed during graph building.
/// Stored per-target on the Graph, not per-edge.
#[derive(Debug, Clone, Default)]
pub struct TargetProperties {
    pub is_symlink: bool,
    pub is_directory: bool,
    pub symlink_target: Option<String>,
}

#[derive(Debug, Default)]
pub struct Graph {
    pub nodes: HashMap<String, Node>,
    pub edges: Vec<Edge>,
    pub forward: HashMap<String, Vec<usize>>,
    pub reverse: HashMap<String, Vec<usize>>,
    pub child_graphs: Vec<String>,
    /// Resolved interface nodes from config (empty = open graph).
    pub interface: Vec<String>,
    /// Filesystem properties of edge targets, keyed by node identity (fragment-stripped).
    pub target_properties: HashMap<String, TargetProperties>,
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

    /// Returns true for File nodes (excludes External and Graph).
    /// Used by structural analyses that operate only on declared file-backed nodes.
    pub fn is_file_node(&self, path: &str) -> bool {
        self.nodes
            .get(path)
            .is_some_and(|n| n.node_type == NodeType::File)
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

    /// Get filesystem properties for an edge target.
    pub fn target_props(&self, target: &str) -> Option<&TargetProperties> {
        self.target_properties.get(target)
    }
}

/// Hash file contents with BLAKE3, returning `b3:<hex>`.
pub fn hash_bytes(content: &[u8]) -> String {
    format!("b3:{}", blake3::hash(content).to_hex())
}

/// Build a graph from files in `root`.
///
/// 1. Discover File nodes via `include`/`exclude` — hash raw bytes for all.
/// 2. Read text content for parser input (graceful skip for binary files).
/// 3. Run parsers to extract edges.
/// 4. Edge targets outside `include` become External nodes (not tracked).
pub fn build_graph(root: &Path, config: &Config) -> Result<Graph> {
    let included_files = discover(root, &config.include, &config.exclude)?;
    let child_graphs = find_child_graphs(root, &config.exclude)?;
    let mut graph = Graph::new();
    graph.child_graphs = child_graphs;
    let mut pending_edges = Vec::new();

    // 1. Create File nodes for everything in include — hash raw bytes.
    //    Separately read text content for files parsers will need.
    let mut file_text: HashMap<String, String> = HashMap::new(); // path → text content

    for file in &included_files {
        let file_path = root.join(file);
        let raw = std::fs::read(&file_path)?;
        let hash = hash_bytes(&raw);

        graph.add_node(Node {
            path: file.clone(),
            node_type: NodeType::File,
            hash: Some(hash),
            graph: None,
            metadata: HashMap::new(),
        });

        // Try to read as text for parser input — binary files just won't have text
        if let Ok(text) = String::from_utf8(raw) {
            file_text.insert(file.clone(), text);
        }
    }

    // 2. Build parser registry and determine which files each parser receives
    let parser_list = parsers::build_parsers(&config.parsers, config.config_dir.as_deref(), root);
    let mut parser_files: Vec<Vec<String>> = vec![Vec::new(); parser_list.len()];

    for file in &included_files {
        for (i, parser) in parser_list.iter().enumerate() {
            if parser.matches(file) {
                parser_files[i].push(file.clone());
            }
        }
    }

    // 3. Run each parser in batch mode
    for (i, parser) in parser_list.iter().enumerate() {
        let files: Vec<(&str, &str)> = parser_files[i]
            .iter()
            .filter_map(|path| {
                file_text
                    .get(path)
                    .map(|content| (path.as_str(), content.as_str()))
            })
            .collect();

        if files.is_empty() {
            continue;
        }

        let batch_results = parser.parse_batch(&files);

        for (file, result) in batch_results {
            // Attach metadata to node if parser returned it
            if let Some(metadata) = result.metadata
                && let Some(node) = graph.nodes.get_mut(&file)
            {
                node.metadata.insert(parser.name().to_string(), metadata);
            }

            for link in result.links {
                let normalized = match normalize_link_target(&link) {
                    Some(n) => n,
                    None => continue, // filtered (empty, anchor-only)
                };

                let target = if is_uri(&normalized.target) {
                    normalized.target
                } else {
                    resolve_link(&file, &normalized.target)
                };
                // link carries the full original when it has a fragment
                let link = normalized.fragment.map(|frag| format!("{target}{frag}"));
                pending_edges.push(Edge {
                    source: file.clone(),
                    target,
                    link,
                    parser: parser.name().to_string(),
                });
            }
        }
    }

    // 4. Create Graph nodes for child graphs
    let graph_prefixes: Vec<String> = graph.child_graphs.clone();
    for graph_dir in &graph_prefixes {
        graph.add_node(Node {
            path: graph_dir.clone(),
            node_type: NodeType::Graph,
            hash: None,
            graph: None,
            metadata: HashMap::new(),
        });
    }

    // 5. Classify edge targets not already in the graph.
    //    edge.target is already the node identity (fragment-stripped).
    //    - URIs → External
    //    - Child graph files (exist on disk) → External with graph field
    //    - Files on disk outside include → External
    //    - Doesn't exist / is directory → no node (dangling-edge / directory-edge candidates)
    let mut implicit_edges = Vec::new();
    for edge in &pending_edges {
        if graph.nodes.contains_key(&edge.target) {
            continue;
        }

        // URIs → External
        if is_uri(&edge.target) {
            graph.add_node(Node {
                path: edge.target.clone(),
                node_type: NodeType::External,
                hash: None,
                graph: None,
                metadata: HashMap::new(),
            });
            continue;
        }

        // Target inside a child graph → External with graph field (if file exists)
        let in_child_graph = graph_prefixes
            .iter()
            .find(|s| edge.target.starts_with(s.as_str()));
        if let Some(graph_prefix) = in_child_graph {
            let target_path = root.join(&edge.target);
            if target_path.is_file() {
                graph.add_node(Node {
                    path: edge.target.clone(),
                    node_type: NodeType::External,
                    hash: None,
                    graph: Some(graph_prefix.clone()),
                    metadata: HashMap::new(),
                });
                // Synthetic coupling edge: child-graph file → Graph node
                implicit_edges.push(Edge {
                    source: edge.target.clone(),
                    target: graph_prefix.clone(),
                    link: None,
                    parser: edge.parser.clone(),
                });
            }
            continue;
        }

        // File exists on disk but not in include → External (validated, not tracked)
        let target_path = root.join(&edge.target);
        if target_path.is_file() {
            graph.add_node(Node {
                path: edge.target.clone(),
                node_type: NodeType::External,
                hash: None,
                graph: None,
                metadata: HashMap::new(),
            });
        }
        // If doesn't exist or is a directory: no node created.
        // dangling-edge and directory-edge rules handle these cases.
    }

    // Probe filesystem properties for non-URI edge targets (stored per-target, not per-edge)
    pending_edges.extend(implicit_edges);
    for edge in &pending_edges {
        if is_uri(&edge.target) || graph.target_properties.contains_key(&edge.target) {
            continue;
        }
        let target_path = root.join(&edge.target);
        let is_symlink = target_path.is_symlink();
        let is_directory = target_path.is_dir();
        let symlink_target = if is_symlink {
            std::fs::read_link(&target_path)
                .ok()
                .map(|p| p.to_string_lossy().to_string())
        } else {
            None
        };
        graph.target_properties.insert(
            edge.target.clone(),
            TargetProperties {
                is_symlink,
                is_directory,
                symlink_target,
            },
        );
    }

    // Add all edges (explicit + implicit) to the graph
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
/// Preserves leading `..` that escape above the root — these indicate graph escape.
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

/// Normalized link target: the node identity and optional fragment metadata.
struct NormalizedTarget {
    /// The target path or URI with fragment stripped (used for node identity).
    target: String,
    /// The fragment portion (e.g., `#heading`), if any. Preserved as edge metadata.
    fragment: Option<String>,
}

/// Normalize a raw link target from a parser.
/// Returns None for targets that should be filtered (empty, anchor-only with no file target).
/// Strips fragments for node identity but preserves them as metadata.
fn normalize_link_target(raw: &str) -> Option<NormalizedTarget> {
    let target = raw.trim();
    if target.is_empty() {
        return None;
    }

    // Anchor-only links (#heading) have no file target — drop them
    if target.starts_with('#') {
        return None;
    }

    // Split target and fragment at the first #
    let (base, fragment) = match target.find('#') {
        Some(idx) => (&target[..idx], Some(target[idx..].to_string())),
        None => (target, None),
    };

    // After stripping fragment, if nothing remains, drop
    if base.is_empty() {
        return None;
    }

    Some(NormalizedTarget {
        target: base.to_string(),
        fragment,
    })
}

/// Resolve a link target relative to a source file, producing a path relative to the graph root.
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
            node_type: NodeType::File,
            hash: None,
            graph: None,
            metadata: HashMap::new(),
        }
    }

    pub fn make_edge(source: &str, target: &str) -> Edge {
        Edge {
            source: source.into(),
            target: target.into(),
            link: None,
            parser: "markdown".into(),
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
            node_type: NodeType::File,
            hash: None,
            graph: None,
            metadata: HashMap::new(),
        });
        g.add_node(Node {
            path: "b.md".into(),
            node_type: NodeType::File,
            hash: None,
            graph: None,
            metadata: HashMap::new(),
        });
        g.add_edge(Edge {
            source: "a.md".into(),
            target: "b.md".into(),
            link: None,
            parser: "markdown".into(),
        });
        assert_eq!(g.forward["a.md"], vec![0]);
        assert_eq!(g.reverse["b.md"], vec![0]);
        assert!(!g.forward.contains_key("b.md"));
    }

    #[test]
    fn fragment_edge_resolves_to_node() {
        let mut g = Graph::new();
        g.add_node(test_helpers::make_node("a.md"));
        g.add_node(test_helpers::make_node("b.md"));
        g.add_edge(Edge {
            source: "a.md".into(),
            target: "b.md".into(),
            link: Some("b.md#heading".into()),
            parser: "markdown".into(),
        });
        // target is the node ID
        assert_eq!(g.edges[0].target, "b.md");
        // reference carries the full original
        assert_eq!(g.edges[0].link.as_deref(), Some("b.md#heading"));
        // reverse map works directly on target
        assert_eq!(g.reverse["b.md"], vec![0]);
    }

    #[test]
    fn is_uri_detects_schemes() {
        assert!(is_uri("http://example.com"));
        assert!(is_uri("https://example.com"));
        assert!(is_uri("mailto:user@example.com"));
        assert!(is_uri("ftp://files.example.com"));
        assert!(is_uri("tel:+1234567890"));
        assert!(is_uri("ssh://git@github.com"));
        assert!(is_uri("custom+scheme://foo"));
    }

    #[test]
    fn is_uri_rejects_paths() {
        assert!(!is_uri("setup.md"));
        assert!(!is_uri("./relative/path.md"));
        assert!(!is_uri("../parent.md"));
        assert!(!is_uri("#heading"));
        assert!(!is_uri(""));
        assert!(!is_uri("path/with:colon.md")); // colon after slash = not a scheme
    }

    #[test]
    fn normalize_strips_fragment() {
        let n = normalize_link_target("file.md#heading").unwrap();
        assert_eq!(n.target, "file.md");
        assert_eq!(n.fragment.as_deref(), Some("#heading"));
    }

    #[test]
    fn normalize_strips_uri_fragment() {
        let n = normalize_link_target("https://example.com/page#section").unwrap();
        assert_eq!(n.target, "https://example.com/page");
        assert_eq!(n.fragment.as_deref(), Some("#section"));
    }

    #[test]
    fn normalize_drops_anchor_only() {
        assert!(normalize_link_target("#heading").is_none());
    }

    #[test]
    fn normalize_drops_empty() {
        assert!(normalize_link_target("").is_none());
        assert!(normalize_link_target("  ").is_none());
    }

    #[test]
    fn normalize_preserves_mailto() {
        let n = normalize_link_target("mailto:user@example.com").unwrap();
        assert_eq!(n.target, "mailto:user@example.com");
        assert!(n.fragment.is_none());
    }
}
