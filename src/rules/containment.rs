use crate::diagnostic::Diagnostic;
use crate::graph::Graph;
use crate::rules::Rule;
use std::path::Path;

pub struct ContainmentRule;

impl Rule for ContainmentRule {
    fn name(&self) -> &str {
        "containment"
    }

    fn evaluate(&self, graph: &Graph, root: &Path) -> Vec<Diagnostic> {
        // Containment only applies when a scope boundary exists (drft.lock)
        if !root.join("drft.lock").exists() {
            return vec![];
        }

        let mut diagnostics = Vec::new();

        for edge in &graph.edges {
            // Skip external URLs
            if edge.target.starts_with("http://") || edge.target.starts_with("https://") {
                continue;
            }

            // Check if the raw target (before normalization) escapes the scope.
            // After normalization, paths that escape resolve to something like "" or
            // the normalized form won't match a file inside the scope.
            // But we need to check the un-normalized edge: if the link text
            // included ../ that would go above root, it's a containment violation.
            //
            // We re-derive this by checking if the target path, when joined with root,
            // would land outside root.
            let joined = root.join(&edge.target);
            let canonical_root = match root.canonicalize() {
                Ok(p) => p,
                Err(_) => return diagnostics,
            };
            // For broken links the target may not exist, so we canonicalize the parent
            let target_parent = match joined.parent() {
                Some(p) if p.exists() => match p.canonicalize() {
                    Ok(cp) => cp,
                    Err(_) => continue,
                },
                _ => continue,
            };
            let target_canonical = target_parent.join(joined.file_name().unwrap_or_default());

            if !target_canonical.starts_with(&canonical_root) {
                diagnostics.push(Diagnostic {
                    rule: "containment".into(),
                    message: "links outside scope boundary".into(),
                    source: Some(edge.source.clone()),
                    target: Some(edge.target.clone()),
                    fix: Some(format!(
                        "link reaches outside the scope — move {} into the scope or remove the link from {}",
                        edge.target, edge.source
                    )),
                    ..Default::default()
                });
            }
        }

        diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Edge, EdgeType, Graph, Node, NodeType};
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn detects_escape() {
        let parent = TempDir::new().unwrap();
        let scope = parent.path().join("docs");
        fs::create_dir(&scope).unwrap();
        fs::write(scope.join("drft.lock"), "lockfile_version = 1\n").unwrap();
        fs::write(scope.join("index.md"), "").unwrap();
        fs::write(parent.path().join("README.md"), "").unwrap();

        let mut graph = Graph::new();
        graph.add_node(Node {
            path: "index.md".into(),
            node_type: NodeType::Document,
            hash: None,
        });
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "../README.md".into(),
            edge_type: EdgeType::Inline,
        });

        let rule = ContainmentRule;
        let diagnostics = rule.evaluate(&graph, &scope);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].rule, "containment");
        assert_eq!(diagnostics[0].target.as_deref(), Some("../README.md"));
    }

    #[test]
    fn no_violation_for_internal_link() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("drft.lock"), "lockfile_version = 1\n").unwrap();
        fs::write(dir.path().join("index.md"), "").unwrap();
        fs::write(dir.path().join("setup.md"), "").unwrap();

        let mut graph = Graph::new();
        graph.add_node(Node {
            path: "index.md".into(),
            node_type: NodeType::Document,
            hash: None,
        });
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "setup.md".into(),
            edge_type: EdgeType::Inline,
        });

        let rule = ContainmentRule;
        let diagnostics = rule.evaluate(&graph, dir.path());
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn vacuous_without_lockfile() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("index.md"), "").unwrap();

        let mut graph = Graph::new();
        graph.add_node(Node {
            path: "index.md".into(),
            node_type: NodeType::Document,
            hash: None,
        });
        graph.add_edge(Edge {
            source: "index.md".into(),
            target: "../escape.md".into(),
            edge_type: EdgeType::Inline,
        });

        let rule = ContainmentRule;
        let diagnostics = rule.evaluate(&graph, dir.path());
        assert!(
            diagnostics.is_empty(),
            "no lockfile means no boundary to enforce"
        );
    }
}
