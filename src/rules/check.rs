//! The check orchestrator: run the v0.8 rules over the composed graph, apply
//! configured severity and per-rule ignore globs, and return the sorted
//! findings.
//!
//! Built-in rules are always on at their default `warn` severity; config can
//! promote a rule to `error`, silence it with `off`, or `ignore` subjects by
//! glob. Staleness rules run only against a usable baseline; when there is none,
//! `no-baseline` says so once instead.

use crate::config::{Config, RuleSeverity};
use crate::diagnostic::Finding;
use crate::lock::Lock;
use crate::model::Graph;
use crate::rules::{staleness, structural};

/// Evaluate all v0.8 rules and apply config. Staleness rules run only against a
/// usable baseline; structural rules always run.
///
/// `build_findings` are raised while constructing the graph, where the condition
/// disappears from the built shape. They include parser diagnostics and files a
/// configured text graph could not decode. They join the derived findings before
/// severity and ignore globs are applied, so config promotes, silences and scopes
/// them like any other rule.
pub fn run(
    graph: &Graph,
    lock: Option<&Lock>,
    config: &Config,
    build_findings: Vec<Finding>,
) -> Vec<Finding> {
    let mut findings = build_findings;

    // A baseline that does not exist and a baseline with no entries are the same
    // fact: nothing to compare against. Both used to leave `check` silent, so a
    // clean run and an absent baseline were indistinguishable — exit 0 either way,
    // no finding either way.
    //
    // This is a finding rather than a hint on purpose. Hints never change an exit
    // code, so a hint-only answer leaves an automated caller exactly as blind as
    // it was. As a rule it defaults to `warn` — the first `check` of a new repo
    // stays quiet — and a repo that wants the missing baseline to fail its run
    // promotes it to `error` like any other rule.
    // Say nothing when there is nothing a baseline could have covered: a graph of
    // directories alone is consistent with having no lockfile.
    let anything_to_cover = !Lock::from_composed(graph).nodes.is_empty();
    let empty_baseline = lock.is_some_and(|lock| lock.nodes.is_empty());
    if anything_to_cover && (lock.is_none() || empty_baseline) {
        // The message says what is true of the run rather than guessing why.
        // `lock` is `None` for a file that is absent and for one that could not be
        // parsed — the latter carries plenty of entries, so "no lock entries"
        // would be false, while `unparseable-lock` on the same run says something
        // different and correct.
        findings.push(Finding::warn(
            "no-baseline",
            "drft.lock",
            Vec::new(),
            "no usable baseline, so no file is checked for drift",
        ));
    }

    // An empty lockfile still runs the staleness rules; an absent one does not.
    //
    // For the message the two are one fact. For the gate they are not. Absent is
    // the ordinary state of a repo that has never locked, and reporting one
    // finding per file there would bury the quick start. Empty means a baseline
    // was established and then emptied — every lockable node really is unlocked,
    // and that is the state worth failing on. Skipping the rules there disarmed
    // them all: a repo gating on `new-edge` stopped failing, and `unlocked-node`
    // fired zero times in the one state where it is true of every node.
    if let Some(lock) = lock {
        findings.extend(staleness::evaluate(graph, lock));
    }
    findings.extend(structural::evaluate(graph, &config.anchor_namespaces()));

    apply_policy(findings, config)
}

/// Apply shared severity, subject ignores, subsumption, and deterministic ordering.
pub fn apply_policy(mut findings: Vec<Finding>, config: &Config) -> Vec<Finding> {
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

    // An unlocked node subsumes its outbound `new-edge` findings: the node having
    // no baseline is the one fact behind every one of them.
    //
    // Decided here rather than where the findings are derived, and only over what
    // survived the filter above. Subsuming earlier meant that silencing
    // `unlocked-node` — with `off`, or with the `ignore` glob the rules reference
    // recommends for a source tree — also dropped the `new-edge` findings it was
    // standing in for, so a node configured to be quieter went completely dark and
    // lost coverage that predates this rule.
    // Subsuming must not weaken the run. A `warn` `unlocked-node` standing in for
    // an `error` `new-edge` would turn exit 1 into exit 0 — a repo gating CI on
    // `new-edge` would stop failing without anything saying so, which is the shape
    // of failure this whole change exists to remove. So a finding is only
    // subsumed by one at least as severe as itself.
    let subsuming: std::collections::HashMap<String, RuleSeverity> = findings
        .iter()
        .filter(|f| f.name == "unlocked-node")
        .map(|f| (f.subject.clone(), f.severity))
        .collect();
    if !subsuming.is_empty() {
        // Only Warn and Error can reach here — the filter above dropped every Off
        // finding — but the Off arm is written out rather than assumed, so the
        // rule stays true if that filter ever moves.
        let at_least_as_severe = |standing: RuleSeverity, subsumed: RuleSeverity| match standing {
            RuleSeverity::Error => true,
            RuleSeverity::Warn => subsumed != RuleSeverity::Error,
            RuleSeverity::Off => false,
        };
        findings.retain(|f| {
            f.name != "new-edge"
                || !subsuming
                    .get(&f.subject)
                    .is_some_and(|standing| at_least_as_severe(*standing, f.severity))
        });
    }

    // Lines before message, so several findings on one subject read in the order
    // a reader would walk the file rather than in fragment byte order.
    findings.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| a.subject.cmp(&b.subject))
            .then_with(|| a.lines.cmp(&b.lines))
            .then_with(|| a.target.cmp(&b.target))
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
        node("b3:x")
    }

    fn node(hash: &str) -> Node {
        Node::new(
            json!({ "type": "file", "hash": hash })
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
        let findings = run(&graph, None, &Config::defaults(), Vec::new());
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
        let findings = run(&composed_unresolved(), None, &config, Vec::new());
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
        let findings = run(&composed_unresolved(), None, &config, Vec::new());
        assert!(!findings.iter().any(|f| f.name == "unresolved-edge"));
    }

    #[test]
    fn global_rule_ignore_suppresses_group_but_keeps_transitive_signal() {
        use crate::lock::Lock;
        // Locked state: yours.md links vendor/x.md, whose hash is then changed.
        let mut locked = Graph::labeled("fs");
        locked.set_node("yours.md", node("b3:yours"));
        locked.set_node("vendor/x.md", node("b3:old"));
        locked.add_edge(Edge::new("yours.md", "vendor/x.md"));
        let lock = Lock::from_composed(&compose(&GraphSet::new(vec![locked])));

        // Current state: vendor/x.md changed; yours.md unchanged.
        let mut current = Graph::labeled("fs");
        current.set_node("yours.md", node("b3:yours"));
        current.set_node("vendor/x.md", node("b3:new"));
        current.add_edge(Edge::new("yours.md", "vendor/x.md"));
        let composed = compose(&GraphSet::new(vec![current]));

        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("drft.toml"),
            "[rules]\nignore = [\"vendor/**\"]\n",
        )
        .unwrap();
        let config = Config::load(dir.path()).unwrap();

        let findings = run(&composed, Some(&lock), &config, Vec::new());
        // The group file's own staleness is suppressed (subject is in the group).
        assert!(
            !findings
                .iter()
                .any(|f| f.name == "stale-node" && f.subject == "vendor/x.md"),
            "vendor stale-node should be suppressed, got: {findings:?}"
        );
        // But your file's transitive staleness survives — its subject is yours.md,
        // which the group ignore does not match.
        assert!(
            findings
                .iter()
                .any(|f| f.name == "stale-edge" && f.subject == "yours.md"),
            "your transitive stale-edge should remain, got: {findings:?}"
        );
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
        let findings = run(&composed_unresolved(), None, &config, Vec::new());
        assert!(!findings.iter().any(|f| f.name == "unresolved-edge"));
    }
}
