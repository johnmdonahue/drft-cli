//! Staleness rules: the drift findings derived by joining the composed graph to
//! the lockfile. Staleness is computed locally — per node (`hash` vs locked) and
//! per edge (target hash vs locked) — with no recursive propagation, so a
//! dependency cycle can't loop or produce ambiguous staleness.
//!
//! Findings: `stale-node`, `stale-edge`, `new-edge`, `removed-edge`,
//! `removed-node`. A stale node subsumes its outbound `stale-edge` findings; a
//! removed node subsumes its `removed-edge` findings.

use std::collections::HashSet;

use crate::diagnostic::Finding;
use crate::lock::Lock;
use crate::model::{Graph, Node};
use crate::rules::{edge_provenance, provenance, short_hash};

/// Evaluate staleness findings for `graph` against `lock`.
pub fn evaluate(graph: &Graph, lock: &Lock) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut stale_nodes: HashSet<&str> = HashSet::new();

    // stale-node: a node's current hash differs from its locked hash.
    for (path, node) in &graph.nodes {
        if let (Some(current), Some(locked)) = (
            node.fs_hash(),
            lock.nodes.get(path).and_then(|n| n.hash.as_deref()),
        ) && current != locked
        {
            stale_nodes.insert(path.as_str());
            findings.push(Finding::warn(
                "stale-node",
                path,
                provenance(&node.metadata),
                format!(
                    "hash {} ≠ locked {}",
                    short_hash(current),
                    short_hash(locked)
                ),
            ));
        }
    }

    // Edge findings. Track current (source, target) pairs for removed-edge.
    let mut current_pairs: HashSet<(&str, &str)> = HashSet::new();
    for edge in &graph.edges {
        current_pairs.insert((edge.source.as_str(), edge.target.as_str()));

        let locked_target_hash = lock
            .nodes
            .get(&edge.source)
            .and_then(|n| n.edges.get(&edge.target));

        match locked_target_hash {
            // The edge is locked: compare the target's hash to derive staleness.
            Some(locked_hash) => {
                // A stale source subsumes its outbound stale-edge findings.
                if stale_nodes.contains(edge.source.as_str()) {
                    continue;
                }
                let current_target_hash = graph.nodes.get(&edge.target).and_then(Node::fs_hash);
                if let (Some(locked_hash), Some(current)) =
                    (locked_hash.as_deref(), current_target_hash)
                    && locked_hash != current
                {
                    findings.push(
                        Finding::warn(
                            "stale-edge",
                            &edge.source,
                            edge_provenance(edge),
                            format!(
                                "hash {} ≠ locked {}",
                                short_hash(current),
                                short_hash(locked_hash)
                            ),
                        )
                        .with_target(&edge.target)
                        .with_lines(edge.lines()),
                    );
                }
            }
            // new-edge: a current edge has no locked target hash.
            None => findings.push(
                Finding::warn(
                    "new-edge",
                    &edge.source,
                    edge_provenance(edge),
                    "not locked",
                )
                .with_target(&edge.target)
                .with_lines(edge.lines()),
            ),
        }
    }

    // removed-node and removed-edge: locked entries absent from the graph.
    for (path, locked_node) in &lock.nodes {
        match graph.nodes.get(path) {
            None => {
                // A removed node subsumes its removed-edge findings.
                findings.push(Finding::warn(
                    "removed-node",
                    path,
                    Vec::new(),
                    "node is no longer present",
                ));
            }
            Some(node) => {
                for target in locked_node.edges.keys() {
                    if !current_pairs.contains(&(path.as_str(), target.as_str())) {
                        findings.push(
                            Finding::warn(
                                "removed-edge",
                                path,
                                provenance(&node.metadata),
                                "edge no longer present",
                            )
                            .with_target(target),
                        );
                    }
                }
            }
        }
    }

    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compose::compose;
    use crate::model::{Edge, GraphSet, Node};
    use serde_json::json;

    fn fs_node(hash: &str) -> Node {
        Node::new(
            json!({ "type": "file", "hash": hash })
                .as_object()
                .unwrap()
                .clone(),
        )
    }

    /// fs fragment with `index.md -> setup.md`, then compose.
    fn composed_with(index_hash: &str, setup_hash: &str) -> Graph {
        let mut fs = Graph::labeled("fs");
        fs.set_node("index.md", fs_node(index_hash));
        fs.set_node("setup.md", fs_node(setup_hash));
        fs.add_edge(Edge::new("index.md", "setup.md"));
        compose(&GraphSet::new(vec![fs]))
    }

    fn names(findings: &[Finding]) -> Vec<(&str, &str)> {
        findings
            .iter()
            .map(|f| (f.name.as_str(), f.subject.as_str()))
            .collect()
    }

    #[test]
    fn edited_dependency_produces_stale_node_and_dependent_stale_edge() {
        let locked = Lock::from_composed(&composed_with("b3:idx", "b3:setup"));
        // setup.md edited: its hash changed.
        let current = composed_with("b3:idx", "b3:setup2");
        let findings = evaluate(&current, &locked);

        let n = names(&findings);
        assert!(n.contains(&("stale-node", "setup.md")), "got {n:?}");
        assert!(n.contains(&("stale-edge", "index.md")), "got {n:?}");
    }

    #[test]
    fn clean_graph_has_no_findings() {
        let composed = composed_with("b3:idx", "b3:setup");
        let locked = Lock::from_composed(&composed);
        assert!(evaluate(&composed, &locked).is_empty());
    }

    #[test]
    fn stale_source_subsumes_its_outbound_stale_edge() {
        // index.md edited AND setup.md edited: index is stale-node, so its
        // outbound stale-edge to setup is subsumed; setup is stale-node.
        let locked = Lock::from_composed(&composed_with("b3:idx", "b3:setup"));
        let current = composed_with("b3:idx2", "b3:setup2");
        let findings = evaluate(&current, &locked);
        let n = names(&findings);
        assert!(n.contains(&("stale-node", "index.md")));
        assert!(n.contains(&("stale-node", "setup.md")));
        assert!(
            !n.contains(&("stale-edge", "index.md")),
            "stale node subsumes its outbound stale-edge, got {n:?}"
        );
    }

    #[test]
    fn removed_node_subsumes_its_outbound_removed_edge() {
        // Lock index.md (with edge to setup.md) and setup.md; then remove the
        // source node index.md entirely.
        let locked = Lock::from_composed(&composed_with("b3:idx", "b3:setup"));
        let mut fs = Graph::labeled("fs");
        fs.set_node("setup.md", fs_node("b3:setup"));
        let current = compose(&GraphSet::new(vec![fs]));

        let findings = evaluate(&current, &locked);
        let n = names(&findings);
        assert!(n.contains(&("removed-node", "index.md")), "got {n:?}");
        assert!(
            !n.iter().any(|(name, _)| *name == "removed-edge"),
            "a removed source node subsumes its outbound removed-edge, got {n:?}"
        );
    }

    #[test]
    fn removed_edge_fires_when_source_survives() {
        // index.md survives but no longer links setup.md.
        let locked = Lock::from_composed(&composed_with("b3:idx", "b3:setup"));
        let mut fs = Graph::labeled("fs");
        fs.set_node("index.md", fs_node("b3:idx"));
        fs.set_node("setup.md", fs_node("b3:setup"));
        let current = compose(&GraphSet::new(vec![fs]));

        let findings = evaluate(&current, &locked);
        assert!(names(&findings).contains(&("removed-edge", "index.md")));
    }

    #[test]
    fn new_edge_when_unlocked_edge_appears() {
        let locked = Lock::from_composed(&composed_with("b3:idx", "b3:setup"));
        // index.md gains a new edge to extra.md.
        let mut fs = Graph::labeled("fs");
        fs.set_node("index.md", fs_node("b3:idx"));
        fs.set_node("setup.md", fs_node("b3:setup"));
        fs.set_node("extra.md", fs_node("b3:extra"));
        fs.add_edge(Edge::new("index.md", "setup.md"));
        fs.add_edge(Edge::new("index.md", "extra.md"));
        let current = compose(&GraphSet::new(vec![fs]));

        let findings = evaluate(&current, &locked);
        assert!(names(&findings).contains(&("new-edge", "index.md")));
    }
}
