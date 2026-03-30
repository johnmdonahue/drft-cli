use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

use crate::graph::{Graph, NodeType};

const SUPPORTED_LOCKFILE_VERSION: u32 = 2;

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct LockfileInterface {
    pub nodes: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct Lockfile {
    pub lockfile_version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interface: Option<LockfileInterface>,
    #[serde(default)]
    pub nodes: BTreeMap<String, LockfileNode>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct LockfileNode {
    #[serde(rename = "type")]
    pub node_type: NodeType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph: Option<String>,
}

impl Lockfile {
    /// Convert an in-memory Graph to a Lockfile.
    /// Nodes are stored in a BTreeMap (sorted by path).
    /// Edges are not stored — edge changes are detected via content hashes.
    pub fn from_graph(graph: &Graph) -> Self {
        let mut nodes = BTreeMap::new();
        for (path, node) in &graph.nodes {
            nodes.insert(
                path.clone(),
                LockfileNode {
                    node_type: node.node_type,
                    hash: node.hash.clone(),
                    graph: node.graph.clone(),
                },
            );
        }

        let interface = if graph.interface.is_empty() {
            None
        } else {
            Some(LockfileInterface {
                nodes: graph.interface.clone(),
            })
        };

        Lockfile {
            lockfile_version: 2,
            interface,
            nodes,
        }
    }

    /// Serialize to deterministic TOML string.
    pub fn to_toml(&self) -> Result<String> {
        toml::to_string_pretty(self).context("failed to serialize lockfile")
    }

    /// Deserialize from TOML string. Rejects v1 lockfiles with a migration message.
    pub fn from_toml(content: &str) -> Result<Self> {
        // Quick version check before full parse
        #[derive(Deserialize)]
        struct VersionOnly {
            lockfile_version: u32,
        }
        let version_check: VersionOnly =
            toml::from_str(content).context("failed to parse lockfile")?;

        if version_check.lockfile_version < 2 {
            anyhow::bail!(
                "drft.lock is v{} format — delete it and run `drft lock` to upgrade to v2",
                version_check.lockfile_version
            );
        }
        if version_check.lockfile_version > SUPPORTED_LOCKFILE_VERSION {
            anyhow::bail!(
                "drft.lock version {} is not supported (max supported: {}). upgrade drft to read this lockfile",
                version_check.lockfile_version,
                SUPPORTED_LOCKFILE_VERSION
            );
        }

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
    use crate::graph::{Node, NodeType};
    use tempfile::TempDir;

    fn make_graph() -> Graph {
        let mut g = Graph::new();
        g.add_node(Node {
            path: "index.md".into(),
            node_type: NodeType::Source,
            hash: Some("b3:aaa".into()),
            graph: None,
        });
        g.add_node(Node {
            path: "setup.md".into(),
            node_type: NodeType::Source,
            hash: Some("b3:bbb".into()),
            graph: None,
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
    fn stores_interface_when_present() {
        let mut g = make_graph();
        g.interface = vec!["index.md".to_string()];
        let lf = Lockfile::from_graph(&g);
        assert!(lf.interface.is_some());
        assert_eq!(lf.interface.unwrap().nodes, vec!["index.md"]);
    }

    #[test]
    fn no_interface_when_empty() {
        let g = make_graph();
        let lf = Lockfile::from_graph(&g);
        assert!(lf.interface.is_none());
    }

    #[test]
    fn rejects_v1_lockfile() {
        let toml = "lockfile_version = 1\n";
        let result = Lockfile::from_toml(toml);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("delete it"), "should suggest deletion: {err}");
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
        let toml = "lockfile_version = 2\n";
        let result = Lockfile::from_toml(toml);
        assert!(result.is_ok());
    }
}
