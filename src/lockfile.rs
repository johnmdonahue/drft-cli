use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

use crate::graph::Graph;

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct Lockfile {
    pub lockfile_version: u32,
    #[serde(default)]
    pub nodes: BTreeMap<String, LockfileNode>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct LockfileNode {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
}

impl Lockfile {
    /// Convert an in-memory Graph to a Lockfile.
    /// Nodes are stored in a BTreeMap (sorted by path).
    /// Edges are not stored — edge changes are detected via content hashes.
    pub fn from_graph(graph: &Graph) -> Self {
        let mut nodes = BTreeMap::new();
        for (path, node) in &graph.nodes {
            if !node.included {
                continue;
            }
            nodes.insert(
                path.clone(),
                LockfileNode {
                    hash: node.hash.clone(),
                },
            );
        }

        Lockfile {
            lockfile_version: 2,
            nodes,
        }
    }

    /// Serialize to deterministic TOML string.
    pub fn to_toml(&self) -> Result<String> {
        toml::to_string_pretty(self).context("failed to serialize lockfile")
    }

    /// Deserialize from TOML string.
    pub fn from_toml(content: &str) -> Result<Self> {
        let lockfile: Self = toml::from_str(content).context("failed to parse lockfile")?;
        Ok(lockfile)
    }
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
    use crate::graph::Node;
    use std::collections::HashMap;
    use tempfile::TempDir;

    fn make_graph() -> Graph {
        let mut g = Graph::new();
        g.add_node(Node {
            path: "index.md".into(),
            node_type: Some(crate::graph::NodeType::File),
            included: true,
            hash: Some("b3:aaa".into()),
            metadata: HashMap::new(),
        });
        g.add_node(Node {
            path: "setup.md".into(),
            node_type: Some(crate::graph::NodeType::File),
            included: true,
            hash: Some("b3:bbb".into()),
            metadata: HashMap::new(),
        });
        g
    }

    #[test]
    fn from_graph_produces_sorted_nodes() {
        let lf = Lockfile::from_graph(&make_graph());
        let keys: Vec<&String> = lf.nodes.keys().collect();
        assert_eq!(keys, vec!["index.md", "setup.md"]);
    }

    #[test]
    fn roundtrip_toml() {
        let lf = Lockfile::from_graph(&make_graph());
        let toml_str = lf.to_toml().unwrap();
        let parsed = Lockfile::from_toml(&toml_str).unwrap();
        assert_eq!(lf, parsed);
    }

    #[test]
    fn deterministic_output() {
        let lf = Lockfile::from_graph(&make_graph());
        let a = lf.to_toml().unwrap();
        let b = lf.to_toml().unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn write_and_read() {
        let dir = TempDir::new().unwrap();
        let lf = Lockfile::from_graph(&make_graph());
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
    fn no_edges_in_lockfile() {
        let lf = Lockfile::from_graph(&make_graph());
        let toml_str = lf.to_toml().unwrap();
        assert!(
            !toml_str.contains("[[edges]]"),
            "lockfile v2 should not contain edges"
        );
    }

    #[test]
    fn parses_current_lockfile_version() {
        let toml = "lockfile_version = 2\n";
        let result = Lockfile::from_toml(toml);
        assert!(result.is_ok());
    }
}
