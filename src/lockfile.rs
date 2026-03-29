use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

use crate::graph::{EdgeType, Graph, NodeType};

const SUPPORTED_LOCKFILE_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct Manifest {
    pub file: String,
    pub nodes: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct Lockfile {
    pub lockfile_version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manifest: Option<Manifest>,
    #[serde(default)]
    pub nodes: BTreeMap<String, LockfileNode>,
    #[serde(default)]
    pub edges: Vec<LockfileEdge>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct LockfileNode {
    #[serde(rename = "type")]
    pub node_type: NodeType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockfileEdge {
    pub source: String,
    pub target: String,
    #[serde(rename = "type")]
    pub edge_type: EdgeType,
}

impl Lockfile {
    /// Convert an in-memory Graph to a Lockfile.
    /// Nodes are stored in a BTreeMap (sorted by path).
    /// Edges are sorted by (source, target, type).
    pub fn from_graph(graph: &Graph, manifest: Option<Manifest>) -> Self {
        let mut nodes = BTreeMap::new();
        for (path, node) in &graph.nodes {
            nodes.insert(
                path.clone(),
                LockfileNode {
                    node_type: node.node_type,
                    hash: node.hash.clone(),
                },
            );
        }

        let mut edges: Vec<LockfileEdge> = graph
            .edges
            .iter()
            .filter(|e| {
                // Only include edges whose target is a known node
                graph.nodes.contains_key(&e.target)
            })
            .map(|e| LockfileEdge {
                source: e.source.clone(),
                target: e.target.clone(),
                edge_type: e.edge_type,
            })
            .collect();

        edges.sort_by(|a, b| {
            a.source
                .cmp(&b.source)
                .then_with(|| a.target.cmp(&b.target))
                .then_with(|| a.edge_type.cmp(&b.edge_type))
        });

        // Deduplicate edges
        edges.dedup_by(|a, b| {
            a.source == b.source && a.target == b.target && a.edge_type == b.edge_type
        });

        Lockfile {
            lockfile_version: 1,
            manifest,
            nodes,
            edges,
        }
    }

    /// Serialize to deterministic TOML string.
    pub fn to_toml(&self) -> Result<String> {
        toml::to_string_pretty(self).context("failed to serialize lockfile")
    }

    /// Deserialize from TOML string. Rejects lockfiles with unsupported versions.
    pub fn from_toml(content: &str) -> Result<Self> {
        let lockfile: Self = toml::from_str(content).context("failed to parse lockfile")?;
        if lockfile.lockfile_version > SUPPORTED_LOCKFILE_VERSION {
            anyhow::bail!(
                "drft.lock version {} is not supported (max supported: {}). upgrade drft to read this lockfile",
                lockfile.lockfile_version,
                SUPPORTED_LOCKFILE_VERSION
            );
        }
        Ok(lockfile)
    }
}

/// Derive a manifest from the graph: the manifest file + its in-scope outbound link targets.
pub fn derive_manifest(graph: &Graph, manifest_file: &str) -> Result<Manifest> {
    if !graph.nodes.contains_key(manifest_file) {
        anyhow::bail!("manifest file \"{manifest_file}\" not found in scope");
    }

    let mut nodes = vec![manifest_file.to_string()];

    // Collect outbound link targets from the manifest file that are within the scope
    if let Some(edge_indices) = graph.forward.get(manifest_file) {
        for &idx in edge_indices {
            let target = &graph.edges[idx].target;
            // Only include targets that are nodes in this scope (not external, not outside)
            if let Some(node) = graph.nodes.get(target.as_str())
                && !matches!(node.node_type, crate::graph::NodeType::External)
                && !nodes.contains(target)
            {
                nodes.push(target.clone());
            }
        }
    }

    nodes.sort();
    Ok(Manifest {
        file: manifest_file.to_string(),
        nodes,
    })
}

/// Read `drft.lock` from the given root directory.
/// Returns `Ok(None)` if the file doesn't exist.
pub fn read_lockfile(root: &Path) -> Result<Option<Lockfile>> {
    let path = root.join("drft.lock");
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let lockfile = Lockfile::from_toml(&content)?;
    Ok(Some(lockfile))
}

/// Write `drft.lock` atomically using temp file + rename.
pub fn write_lockfile(root: &Path, lockfile: &Lockfile) -> Result<()> {
    let content = lockfile.to_toml()?;
    let lock_path = root.join("drft.lock");
    let tmp_path = root.join("drft.lock.tmp");

    std::fs::write(&tmp_path, &content)
        .with_context(|| format!("failed to write {}", tmp_path.display()))?;

    std::fs::rename(&tmp_path, &lock_path).with_context(|| {
        // Clean up temp file on rename failure
        let _ = std::fs::remove_file(&tmp_path);
        format!(
            "failed to rename {} to {}",
            tmp_path.display(),
            lock_path.display()
        )
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Edge, Graph, Node};
    use tempfile::TempDir;

    fn make_graph() -> Graph {
        let mut g = Graph::new();
        g.add_node(Node {
            path: "index.md".into(),
            node_type: NodeType::Document,
            hash: Some("b3:aaa".into()),
        });
        g.add_node(Node {
            path: "setup.md".into(),
            node_type: NodeType::Document,
            hash: Some("b3:bbb".into()),
        });
        g.add_edge(Edge {
            source: "index.md".into(),
            target: "setup.md".into(),
            edge_type: EdgeType::Inline,
        });
        g
    }

    #[test]
    fn from_graph_produces_sorted_nodes() {
        let lf = Lockfile::from_graph(&make_graph(), None);
        let keys: Vec<&String> = lf.nodes.keys().collect();
        assert_eq!(keys, vec!["index.md", "setup.md"]);
    }

    #[test]
    fn roundtrip_toml() {
        let lf = Lockfile::from_graph(&make_graph(), None);
        let toml_str = lf.to_toml().unwrap();
        let parsed = Lockfile::from_toml(&toml_str).unwrap();
        assert_eq!(lf, parsed);
    }

    #[test]
    fn deterministic_output() {
        let lf = Lockfile::from_graph(&make_graph(), None);
        let a = lf.to_toml().unwrap();
        let b = lf.to_toml().unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn write_and_read() {
        let dir = TempDir::new().unwrap();
        let lf = Lockfile::from_graph(&make_graph(), None);
        write_lockfile(dir.path(), &lf).unwrap();
        let read_back = read_lockfile(dir.path()).unwrap().unwrap();
        assert_eq!(lf, read_back);
    }

    #[test]
    fn read_missing_returns_none() {
        let dir = TempDir::new().unwrap();
        assert!(read_lockfile(dir.path()).unwrap().is_none());
    }

    #[test]
    fn filters_edges_to_unknown_nodes() {
        let mut g = Graph::new();
        g.add_node(Node {
            path: "index.md".into(),
            node_type: NodeType::Document,
            hash: Some("b3:aaa".into()),
        });
        // Edge to non-existent node (broken link)
        g.add_edge(Edge {
            source: "index.md".into(),
            target: "gone.md".into(),
            edge_type: EdgeType::Inline,
        });

        let lf = Lockfile::from_graph(&g, None);
        assert!(
            lf.edges.is_empty(),
            "broken link edges should be filtered out"
        );
    }

    #[test]
    fn rejects_future_lockfile_version() {
        let toml = "lockfile_version = 99\n";
        let result = Lockfile::from_toml(toml);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("not supported"),
            "error should mention version: {err}"
        );
    }

    #[test]
    fn accepts_current_lockfile_version() {
        let toml = "lockfile_version = 1\n";
        let result = Lockfile::from_toml(toml);
        assert!(result.is_ok());
    }
}
