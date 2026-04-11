use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;

use crate::config::Config;
use crate::discovery::discover;
use crate::parsers;

/// Check if a target string is a valid URI.
///
/// Uses the `url` crate (WHATWG URL Standard) for parsing, then filters to
/// URIs that either have authority (`://`) or use a known opaque scheme.
/// Without this filter, any `word:stuff` passes WHATWG parsing — e.g.,
/// YAML values like `name: foo` would be treated as URIs with scheme `name`.
pub fn is_uri(target: &str) -> bool {
    match url::Url::parse(target) {
        Ok(url) => {
            if url.has_authority() {
                return true;
            }
            matches!(
                url.scheme(),
                "mailto" | "tel" | "data" | "urn" | "javascript"
            )
        }
        Err(_) => false,
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum NodeType {
    File,
    Directory,
    External,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Node {
    pub path: String,
    pub node_type: NodeType,
    pub hash: Option<String>,
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
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct TargetProperties {
    pub is_symlink: bool,
    pub is_directory: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symlink_target: Option<String>,
}

#[derive(Debug, Default)]
pub struct Graph {
    pub nodes: HashMap<String, Node>,
    pub edges: Vec<Edge>,
    pub forward: HashMap<String, Vec<usize>>,
    pub reverse: HashMap<String, Vec<usize>>,
    /// Filesystem properties of edge targets, keyed by node identity (fragment-stripped).
    pub target_properties: HashMap<String, TargetProperties>,
}

impl Graph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_node(&mut self, node: Node) {
        self.nodes.insert(node.path.clone(), node);
    }

    /// Returns true for File nodes (excludes External and Directory).
    pub fn is_file_node(&self, path: &str) -> bool {
        self.nodes
            .get(path)
            .is_some_and(|n| n.node_type == NodeType::File)
    }

    /// Returns true when both endpoints are File nodes. Excludes edges that
    /// touch External (URI) nodes, Directory (existence-only) leaves, or
    /// dangling targets missing from the graph. Analyses that reason about
    /// density, reachability, and similar on-graph properties use this to
    /// scope to the file-to-file subgraph.
    pub fn is_internal_edge(&self, edge: &Edge) -> bool {
        self.is_file_node(&edge.source) && self.is_file_node(&edge.target)
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

    /// Create a new graph containing only edges from the specified parsers.
    /// All nodes are preserved. Adjacency maps are rebuilt for the filtered edge set.
    pub fn filter_by_parsers(&self, parsers: &[String]) -> Graph {
        let mut filtered = Graph {
            nodes: self.nodes.clone(),
            target_properties: self.target_properties.clone(),
            ..Default::default()
        };

        for edge in &self.edges {
            if parsers.iter().any(|p| p == &edge.parser) {
                filtered.add_edge(edge.clone());
            }
        }

        filtered
    }
}

/// Hash file contents with BLAKE3, returning `b3:<hex>`.
pub fn hash_bytes(content: &[u8]) -> String {
    format!("b3:{}", blake3::hash(content).to_hex())
}

/// Returns true if `target_path` resolves to a location within `canonical_root`.
/// Uses canonicalization to resolve symlinks and normalize paths.
/// Returns false if the path doesn't exist or escapes the root.
fn is_within_root(target_path: &Path, canonical_root: &Path) -> bool {
    target_path
        .canonicalize()
        .is_ok_and(|canonical| canonical.starts_with(canonical_root))
}

/// Build a graph from files in `root`.
///
/// 1. Discover File nodes via `include`/`exclude` — hash raw bytes for all.
/// 2. Read text content for parser input (graceful skip for binary files).
/// 3. Run parsers to extract edges.
/// 4. Edge targets become File/Directory/External nodes based on what they
///    resolve to on disk. File targets outside `include` are hashed and
///    tracked like any other File node.
pub fn build_graph(root: &Path, config: &Config) -> Result<Graph> {
    let canonical_root = root.canonicalize()?;
    let included_files = discover(root, &config.include, &config.exclude)?;
    let mut graph = Graph::new();
    let mut pending_edges = Vec::new();

    // 1. Create File nodes for everything in include — hash raw bytes.
    //    Separately read text content for files parsers will need.
    let mut file_text: HashMap<String, String> = HashMap::new(); // path → text content

    for file in &included_files {
        let file_path = root.join(file);

        // Safety: don't read files that resolve outside the graph root (e.g. symlinks).
        // Still create the node so it's visible, but warn.
        if !is_within_root(&file_path, &canonical_root) {
            eprintln!(
                "warn: included file '{file}' resolves outside the graph root and was not read"
            );
            graph.add_node(Node {
                path: file.clone(),
                node_type: NodeType::File,
                hash: None,
                metadata: HashMap::new(),
            });
            continue;
        }

        let raw = std::fs::read(&file_path)?;
        let hash = hash_bytes(&raw);

        graph.add_node(Node {
            path: file.clone(),
            node_type: NodeType::File,
            hash: Some(hash),
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

    // 4. Edge-driven node creation.
    //    Every edge target not already in the graph is classified by what it
    //    resolves to on disk. No child-graph machinery — this is a flat graph.
    for edge in &pending_edges {
        if graph.nodes.contains_key(&edge.target) {
            continue;
        }

        // URIs → External (not on filesystem)
        if is_uri(&edge.target) {
            graph.add_node(Node {
                path: edge.target.clone(),
                node_type: NodeType::External,
                hash: None,
                metadata: HashMap::new(),
            });
            continue;
        }

        // Safety: targets that lexically escape the graph root (../, absolute
        // paths) get a hash-less File node — no filesystem access. Prevents
        // directory traversal.
        let escapes_root =
            edge.target.starts_with("../") || edge.target == ".." || edge.target.starts_with('/');
        if escapes_root {
            graph.add_node(Node {
                path: edge.target.clone(),
                node_type: NodeType::File,
                hash: None,
                metadata: HashMap::new(),
            });
            continue;
        }

        let target_path = root.join(&edge.target);

        // Safety: verify the resolved path stays within root before any
        // filesystem access. Catches symlinks pointing outside the graph.
        if target_path.exists() && !is_within_root(&target_path, &canonical_root) {
            graph.add_node(Node {
                path: edge.target.clone(),
                node_type: NodeType::File,
                hash: None,
                metadata: HashMap::new(),
            });
            continue;
        }

        // Directory on disk → existence-only Directory leaf (no hash, no
        // outbound edges — just validates the link target).
        if target_path.is_dir() {
            graph.add_node(Node {
                path: edge.target.clone(),
                node_type: NodeType::Directory,
                hash: None,
                metadata: HashMap::new(),
            });
            continue;
        }

        // File on disk → hashed File node. Out-of-include file targets become
        // first-class nodes just like included files.
        if target_path.is_file() {
            let hash = std::fs::read(&target_path).ok().map(|c| hash_bytes(&c));
            graph.add_node(Node {
                path: edge.target.clone(),
                node_type: NodeType::File,
                hash,
                metadata: HashMap::new(),
            });
        }
        // If doesn't exist: no node created. dangling-edge rule handles this.
    }

    // Probe filesystem properties for non-URI edge targets (stored per-target, not per-edge).
    // Only probe targets within the graph root — no filesystem access for escaped targets.
    for edge in &pending_edges {
        if is_uri(&edge.target) || graph.target_properties.contains_key(&edge.target) {
            continue;
        }
        let target_path = root.join(&edge.target);
        if !is_within_root(&target_path, &canonical_root) {
            continue;
        }
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

    // Add all edges to the graph.
    for edge in pending_edges {
        graph.add_edge(edge);
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

    pub fn make_enriched(graph: Graph) -> crate::analyses::EnrichedGraph {
        crate::analyses::enrich_graph(
            graph,
            std::path::Path::new("."),
            &crate::config::Config::defaults(),
            None,
        )
    }

    pub fn make_enriched_with_root(
        graph: Graph,
        root: &std::path::Path,
    ) -> crate::analyses::EnrichedGraph {
        crate::analyses::enrich_graph(graph, root, &crate::config::Config::defaults(), None)
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
            metadata: HashMap::new(),
        });
        g.add_node(Node {
            path: "b.md".into(),
            node_type: NodeType::File,
            hash: None,
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
        assert!(!is_uri("path/with:colon.md"));
    }

    #[test]
    fn is_uri_rejects_bare_schemes() {
        // WHATWG parses any `word:stuff` as a valid URL, so we require
        // authority (://) or a known opaque scheme (mailto, tel, etc.)
        assert!(!is_uri("name: foo bar bazz"));
        assert!(!is_uri("status: draft"));
        assert!(!is_uri("title: My Document"));
        assert!(!is_uri("name:foo"));
        assert!(!is_uri("x:y"));
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

    #[test]
    fn filter_by_single_parser() {
        let mut g = Graph::new();
        g.add_node(test_helpers::make_node("a.md"));
        g.add_node(test_helpers::make_node("b.md"));
        g.add_node(test_helpers::make_node("c.md"));
        g.add_edge(Edge {
            source: "a.md".into(),
            target: "b.md".into(),
            link: None,
            parser: "markdown".into(),
        });
        g.add_edge(Edge {
            source: "a.md".into(),
            target: "c.md".into(),
            link: None,
            parser: "frontmatter".into(),
        });

        let filtered = g.filter_by_parsers(&["frontmatter".into()]);
        assert_eq!(filtered.edges.len(), 1);
        assert_eq!(filtered.edges[0].target, "c.md");
        assert_eq!(filtered.edges[0].parser, "frontmatter");
    }

    #[test]
    fn filter_preserves_all_nodes() {
        let mut g = Graph::new();
        g.add_node(test_helpers::make_node("a.md"));
        g.add_node(test_helpers::make_node("b.md"));
        g.add_edge(Edge {
            source: "a.md".into(),
            target: "b.md".into(),
            link: None,
            parser: "markdown".into(),
        });

        let filtered = g.filter_by_parsers(&["frontmatter".into()]);
        assert_eq!(filtered.nodes.len(), 2);
        assert!(filtered.nodes.contains_key("a.md"));
        assert!(filtered.nodes.contains_key("b.md"));
        assert!(filtered.edges.is_empty());
    }

    #[test]
    fn filter_rebuilds_adjacency_maps() {
        let mut g = Graph::new();
        g.add_node(test_helpers::make_node("a.md"));
        g.add_node(test_helpers::make_node("b.md"));
        g.add_node(test_helpers::make_node("c.md"));
        g.add_edge(Edge {
            source: "a.md".into(),
            target: "b.md".into(),
            link: None,
            parser: "markdown".into(),
        });
        g.add_edge(Edge {
            source: "a.md".into(),
            target: "c.md".into(),
            link: None,
            parser: "frontmatter".into(),
        });

        let filtered = g.filter_by_parsers(&["frontmatter".into()]);
        assert_eq!(filtered.forward["a.md"], vec![0]);
        assert_eq!(filtered.reverse["c.md"], vec![0]);
        assert!(!filtered.reverse.contains_key("b.md"));
    }

    #[test]
    fn filter_by_multiple_parsers() {
        let mut g = Graph::new();
        g.add_node(test_helpers::make_node("a.md"));
        g.add_node(test_helpers::make_node("b.md"));
        g.add_node(test_helpers::make_node("c.md"));
        g.add_edge(Edge {
            source: "a.md".into(),
            target: "b.md".into(),
            link: None,
            parser: "markdown".into(),
        });
        g.add_edge(Edge {
            source: "a.md".into(),
            target: "c.md".into(),
            link: None,
            parser: "frontmatter".into(),
        });

        let filtered = g.filter_by_parsers(&["markdown".into(), "frontmatter".into()]);
        assert_eq!(filtered.edges.len(), 2);
    }

    #[test]
    fn filter_empty_parsers_removes_all_edges() {
        let mut g = Graph::new();
        g.add_node(test_helpers::make_node("a.md"));
        g.add_node(test_helpers::make_node("b.md"));
        g.add_edge(test_helpers::make_edge("a.md", "b.md"));

        let filtered = g.filter_by_parsers(&[]);
        assert!(filtered.edges.is_empty());
        assert_eq!(filtered.nodes.len(), 2);
    }
}
