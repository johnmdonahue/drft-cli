//! The check orchestrator: run the v0.8 rules over the composed graph, apply
//! configured severity and per-rule ignore globs, and return the sorted
//! findings.
//!
//! Built-in rules are always on at their default `warn` severity; config can
//! promote a rule to `error`, silence it with `off`, or `ignore` subjects by
//! glob. Staleness rules run only when a lockfile is present.

use crate::config::{Config, RuleSeverity};
use crate::diagnostic::Finding;
use crate::lock::Lock;
use crate::model::Graph;
use crate::rules::{staleness, structural};

/// Evaluate all v0.8 rules and apply config. Staleness rules run only with a
/// lockfile; structural rules always run.
pub fn run(graph: &Graph, lock: Option<&Lock>, config: &Config) -> Vec<Finding> {
    let mut findings = Vec::new();
    if let Some(lock) = lock {
        findings.extend(staleness::evaluate(graph, lock));
    }
    findings.extend(structural::evaluate(graph));

    findings.retain_mut(|finding| {
        // Default severity is warn unless config overrides the rule.
        let severity = config
            .rules
            .get(&finding.name)
            .map(|r| r.severity)
            .unwrap_or(RuleSeverity::Warn);
        if severity == RuleSeverity::Off {
            return false;
        }
        if config.is_rule_ignored(&finding.name, &finding.subject) {
            return false;
        }
        finding.severity = severity;
        true
    });

    findings.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| a.subject.cmp(&b.subject))
            .then_with(|| a.message.cmp(&b.message))
    });
    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compose::compose;
    use crate::model::{Edge, GraphSet, Node};
    use serde_json::json;

    fn fs_node() -> Node {
        Node::new(
            json!({ "type": "file", "hash": "b3:x" })
                .as_object()
                .unwrap()
                .clone(),
        )
    }

    fn composed_unresolved() -> Graph {
        let mut fs = Graph::labeled("fs");
        fs.set_node("index.md", fs_node());
        fs.add_edge(Edge::new("index.md", "gone.md"));
        compose(&GraphSet::new(vec![fs]))
    }

    #[test]
    fn applies_default_warn_severity() {
        let graph = composed_unresolved();
        let findings = run(&graph, None, &Config::defaults());
        let unresolved = findings
            .iter()
            .find(|f| f.name == "unresolved-edge")
            .unwrap();
        assert_eq!(unresolved.severity, RuleSeverity::Warn);
    }

    #[test]
    fn config_promotes_to_error() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("drft.toml"),
            "[rules]\nunresolved-edge = \"error\"\n",
        )
        .unwrap();
        let config = Config::load(dir.path()).unwrap();
        let findings = run(&composed_unresolved(), None, &config);
        let unresolved = findings
            .iter()
            .find(|f| f.name == "unresolved-edge")
            .unwrap();
        assert_eq!(unresolved.severity, RuleSeverity::Error);
    }

    #[test]
    fn config_off_silences_rule() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("drft.toml"),
            "[rules]\nunresolved-edge = \"off\"\n",
        )
        .unwrap();
        let config = Config::load(dir.path()).unwrap();
        let findings = run(&composed_unresolved(), None, &config);
        assert!(!findings.iter().any(|f| f.name == "unresolved-edge"));
    }

    #[test]
    fn ignore_glob_drops_subject() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("drft.toml"),
            "[rules.unresolved-edge]\nignore = [\"index.md\"]\n",
        )
        .unwrap();
        let config = Config::load(dir.path()).unwrap();
        let findings = run(&composed_unresolved(), None, &config);
        assert!(!findings.iter().any(|f| f.name == "unresolved-edge"));
    }
}
