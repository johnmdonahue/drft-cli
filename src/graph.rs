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

#[derive(Debug, Clone, serde::Serialize)]
pub struct Node {
    pub path: String,
    /// What kind of entity: file, directory, symlink, uri. None when stat failed (broken link).
    #[serde(rename = "type")]
    pub node_type: Option<NodeType>,
    /// Whether this node matched include patterns — drft reads, hashes, and manages included nodes.
    pub included: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    /// Structured metadata from parsers, keyed by parser name.
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, serde_json::Value>,
}

/// What kind of filesystem entity this node represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeType {
    File,
    Directory,
    Symlink,
    Uri,
}

#[derive(Debug, Clone)]
pub struct Edge {
    pub source: String,
    /// Node identity — always matches a key in `graph.nodes`. Fragment-stripped.
    pub target: String,
    /// Original link when it differs from target (e.g., `bar.md#heading`).
    pub link: Option<String>,
    /// Which parser discovered this edge (provenance).
    pub parser: String,
}

#[derive(Debug, Default)]
pub struct Graph {
    pub nodes: HashMap<String, Node>,
    pub edges: Vec<Edge>,
    pub forward: HashMap<String, Vec<usize>>,
    pub reverse: HashMap<String, Vec<usize>>,
}

impl Graph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_node(&mut self, node: Node) {
        self.nodes.insert(node.path.clone(), node);
    }

    /// Returns true when the edge target is an included node in the graph.
    pub fn is_internal_edge(&self, edge: &Edge) -> bool {
        self.nodes.get(&edge.target).is_some_and(|n| n.included)
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

    /// Iterate over nodes that match include patterns.
    pub fn included_nodes(&self) -> impl Iterator<Item = (&String, &Node)> {
        self.nodes.iter().filter(|(_, n)| n.included)
    }

    /// Create a new graph containing only edges from the specified parsers.
    /// All nodes are preserved. Adjacency maps are rebuilt for the filtered edge set.
    pub fn filter_by_parsers(&self, parsers: &[String]) -> Graph {
        let mut filtered = Graph {
            nodes: self.nodes.clone(),
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

/// Intermediate edge representation used before classification.
struct PendingEdge {
    source: String,
    target: String,
    link: Option<String>,
    parser: String,
}

/// Build a graph from files in `root`.
///
/// 1. Discover included nodes via `include`/`exclude` — hash raw bytes.
/// 2. Run parsers to extract links.
/// 3. Create referenced nodes for all edge targets not already in the graph.
/// 4. Create filesystem edges for symlinks.
pub fn build_graph(root: &Path, config: &Config) -> Result<Graph> {
    let canonical_root = root.canonicalize()?;
    let included_files = discover(root, &config.include, &config.exclude)?;
    let include_globs = crate::config::compile_globs(&config.include)?;
    let exclude_globs = crate::config::compile_globs(&config.exclude)?;
    let mut graph = Graph::new();
    let mut pending_edges: Vec<PendingEdge> = Vec::new();

    // 1. Create nodes for everything in include — hash raw bytes.
    //    Files that resolve outside the graph root (via symlink) get a
    //    hash-less node — content is intentionally not read.
    let mut file_text: HashMap<String, String> = HashMap::new();

    for file in &included_files {
        let file_path = root.join(file);
        let is_symlink = file_path
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink());

        if is_symlink {
            match file_path.canonicalize() {
                Err(_) => {
                    eprintln!("warn: symlink '{file}' could not be resolved — skipping");
                    continue;
                }
                Ok(canonical) => {
                    let should_hash = canonical.starts_with(&canonical_root)
                        && canonical.strip_prefix(&canonical_root).is_ok_and(|rel| {
                            let rel_str = rel.to_string_lossy().replace('\\', "/");
                            include_globs
                                .as_ref()
                                .is_some_and(|set| set.is_match(rel_str.as_str()))
                        });

                    if should_hash {
                        let raw = std::fs::read(&file_path)?;
                        let hash = hash_bytes(&raw);
                        graph.add_node(Node {
                            path: file.clone(),
                            node_type: Some(NodeType::Symlink),
                            included: true,
                            hash: Some(hash),
                            metadata: HashMap::new(),
                        });
                        if let Ok(text) = String::from_utf8(raw) {
                            file_text.insert(file.clone(), text);
                        }
                    } else {
                        graph.add_node(Node {
                            path: file.clone(),
                            node_type: Some(NodeType::Symlink),
                            included: true,
                            hash: None,
                            metadata: HashMap::new(),
                        });
                    }
                }
            }
            continue;
        }

        let raw = std::fs::read(&file_path)?;
        let hash = hash_bytes(&raw);

        graph.add_node(Node {
            path: file.clone(),
            node_type: Some(NodeType::File),
            included: true,
            hash: Some(hash),
            metadata: HashMap::new(),
        });

        if let Ok(text) = String::from_utf8(raw) {
            file_text.insert(file.clone(), text);
        }
    }

    // 2. Build parser registry and determine which files each parser receives.
    let parser_list = parsers::build_parsers(&config.parsers, config.config_dir.as_deref(), root);
    let mut parser_files: Vec<Vec<String>> = vec![Vec::new(); parser_list.len()];

    for file in &included_files {
        for (i, parser) in parser_list.iter().enumerate() {
            if parser.matches(file) {
                parser_files[i].push(file.clone());
            }
        }
    }

    // 3. Run each parser in batch mode.
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
            if let Some(metadata) = result.metadata
                && let Some(node) = graph.nodes.get_mut(&file)
            {
                node.metadata.insert(parser.name().to_string(), metadata);
            }

            for link in result.links {
                let normalized = match normalize_link_target(&link) {
                    Some(n) => n,
                    None => continue,
                };

                let target = if is_uri(&normalized.target) {
                    normalized.target
                } else {
                    resolve_link(&file, &normalized.target)
                };
                let link = normalized.fragment.map(|frag| format!("{target}{frag}"));
                pending_edges.push(PendingEdge {
                    source: file.clone(),
                    target,
                    link,
                    parser: parser.name().to_string(),
                });
            }
        }
    }

    // 4. Create referenced nodes for edge targets not already in the graph.
    //    Stat non-URI targets within root to determine their type.
    for pending in &pending_edges {
        if graph.nodes.contains_key(&pending.target) {
            continue;
        }
        if is_uri(&pending.target) {
            graph.add_node(Node {
                path: pending.target.clone(),
                node_type: Some(NodeType::Uri),
                included: false,
                hash: None,
                metadata: HashMap::new(),
            });
            continue;
        }
        // Stat non-URI targets to determine type. Skip absolute paths and
        // paths escaping root — don't probe outside the graph root.
        let escapes_root = Path::new(&pending.target)
            .components()
            .next()
            .is_some_and(|c| matches!(c, std::path::Component::ParentDir));
        let is_absolute = Path::new(&pending.target).is_absolute();
        let node_type = if is_absolute || escapes_root {
            None
        } else {
            let target_path = root.join(&pending.target);
            target_path.symlink_metadata().ok().map(|m| {
                if m.file_type().is_symlink() {
                    NodeType::Symlink
                } else if m.is_dir() {
                    NodeType::Directory
                } else {
                    NodeType::File
                }
            })
        };
        // A target that matches include && !exclude is included even if it
        // doesn't exist on disk — that's a broken link in scope.
        // Paths escaping the root or absolute paths are never included.
        let matches_include = !escapes_root
            && !is_absolute
            && include_globs
                .as_ref()
                .is_some_and(|set| set.is_match(&pending.target));
        let matches_exclude = exclude_globs
            .as_ref()
            .is_some_and(|set| set.is_match(&pending.target));
        let included = matches_include && !matches_exclude;
        graph.add_node(Node {
            path: pending.target.clone(),
            node_type,
            included,
            hash: None,
            metadata: HashMap::new(),
        });
    }

    // 5. Create filesystem edges for symlinks in include.
    //    A symlink is a link — model it as an edge with "filesystem" provenance.
    for file in &included_files {
        let file_path = root.join(file);
        if !file_path
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
        {
            continue;
        }
        if let Ok(link_target) = std::fs::read_link(&file_path) {
            // If the symlink target is absolute, resolve it relative to root.
            // If it's outside root, canonicalize and make relative if possible.
            let resolved = if link_target.is_absolute() {
                match link_target.canonicalize() {
                    Ok(canonical) => match canonical.strip_prefix(&canonical_root) {
                        Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
                        Err(_) => continue, // symlink escapes root — skip filesystem edge
                    },
                    Err(_) => continue, // broken absolute symlink — skip
                }
            } else {
                resolve_link(file, &link_target.to_string_lossy())
            };
            // Skip filesystem edges for targets that escape root
            let resolved_escapes = Path::new(&resolved)
                .components()
                .next()
                .is_some_and(|c| matches!(c, std::path::Component::ParentDir));
            if resolved_escapes {
                continue;
            }
            // Ensure the resolved target is a node
            if !graph.nodes.contains_key(&resolved) {
                let resolved_path = root.join(&resolved);
                let node_type = resolved_path.symlink_metadata().ok().map(|m| {
                    if m.file_type().is_symlink() {
                        NodeType::Symlink
                    } else if m.is_dir() {
                        NodeType::Directory
                    } else {
                        NodeType::File
                    }
                });
                graph.add_node(Node {
                    path: resolved.clone(),
                    node_type,
                    included: false,
                    hash: None,
                    metadata: HashMap::new(),
                });
            }
            graph.add_edge(Edge {
                source: file.clone(),
                target: resolved,
                link: None,
                parser: "filesystem".into(),
            });
        }
    }

    // 6. Attach pending edges.
    for pending in pending_edges {
        graph.add_edge(Edge {
            source: pending.source,
            target: pending.target,
            link: pending.link,
            parser: pending.parser,
        });
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
            node_type: Some(NodeType::File),
            included: true,
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
        g.add_node(test_helpers::make_node("a.md"));
        g.add_node(test_helpers::make_node("b.md"));
        g.add_edge(test_helpers::make_edge("a.md", "b.md"));
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

    fn edge_with_parser(source: &str, target: &str, parser: &str) -> Edge {
        Edge {
            source: source.into(),
            target: target.into(),
            link: None,
            parser: parser.into(),
        }
    }

    #[test]
    fn filter_by_single_parser() {
        let mut g = Graph::new();
        g.add_node(test_helpers::make_node("a.md"));
        g.add_node(test_helpers::make_node("b.md"));
        g.add_node(test_helpers::make_node("c.md"));
        g.add_edge(edge_with_parser("a.md", "b.md", "markdown"));
        g.add_edge(edge_with_parser("a.md", "c.md", "frontmatter"));

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
        g.add_edge(edge_with_parser("a.md", "b.md", "markdown"));

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
        g.add_edge(edge_with_parser("a.md", "b.md", "markdown"));
        g.add_edge(edge_with_parser("a.md", "c.md", "frontmatter"));

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
        g.add_edge(edge_with_parser("a.md", "b.md", "markdown"));
        g.add_edge(edge_with_parser("a.md", "c.md", "frontmatter"));

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
