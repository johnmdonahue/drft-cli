use drft::{
    config::{Config, RuleSeverity},
    diagnostic::Finding,
    impact::{self, Direction},
    lock::Lock,
    model::{Edge, Graph, Node},
};
use serde_json::json;

fn graph(edges: &[(&str, &str)], files: &[&str]) -> Graph {
    let mut graph = Graph::labeled("composed");
    for file in files {
        graph.set_node(
            *file,
            Node::new(
                json!({"@fs": {"type": "file", "hash": "b3:x"}, "@markdown": {"anchors": []}})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        );
    }
    for (source, target) in edges {
        graph.add_edge(Edge::with_metadata(
            *source,
            *target,
            json!({"@markdown": {"occurrences": [{"link": "encoded#missing", "line": 4}]}})
                .as_object()
                .unwrap()
                .clone(),
        ));
    }
    graph
}
fn config() -> Config {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("drft.toml"),
        "[graphs.markdown]\nparser = \"markdown\"\n",
    )
    .unwrap();
    Config::load(dir.path()).unwrap()
}
fn run(
    graph: &Graph,
    seeds: &[&str],
    direction: Direction,
    depth: Option<usize>,
    lock: Option<&Lock>,
) -> Vec<Finding> {
    let seeds: Vec<_> = seeds.iter().map(|s| s.to_string()).collect();
    let impacted = impact::compute(graph, &seeds, direction, depth);
    impact::diagnostics(
        graph,
        &seeds,
        direction,
        depth,
        &impacted,
        lock,
        &config(),
        vec![],
    )
}
fn pairs(findings: &[Finding]) -> Vec<(&str, Option<&str>)> {
    findings
        .iter()
        .map(|f| (f.subject.as_str(), f.target.as_deref()))
        .collect()
}
#[test]
fn direction_and_strict_expansion_boundary() {
    let g = graph(&[("a", "b"), ("b", "c"), ("c", "d")], &["a", "b", "c", "d"]);
    assert!(run(&g, &["b"], Direction::Both, Some(0), None).is_empty());
    assert_eq!(
        pairs(&run(&g, &["b"], Direction::Inbound, Some(1), None)),
        vec![("a", Some("b#missing"))]
    );
    assert_eq!(
        pairs(&run(&g, &["b"], Direction::Outbound, Some(1), None)),
        vec![("b", Some("c#missing"))]
    );
    assert_eq!(run(&g, &["b"], Direction::Both, Some(1), None).len(), 2);
    assert_eq!(run(&g, &["b"], Direction::Outbound, Some(2), None).len(), 2);
    assert_eq!(run(&g, &["b"], Direction::Both, None, None).len(), 3);
}
#[test]
fn all_inspected_edges_include_cycles_alternate_paths_and_seed_pairs() {
    let g = graph(
        &[
            ("a", "b"),
            ("a", "c"),
            ("b", "c"),
            ("c", "a"),
            ("b", "d"),
            ("d", "c"),
        ],
        &["a", "b", "c", "d"],
    );
    assert_eq!(run(&g, &["a", "b"], Direction::Both, None, None).len(), 6);
    // c is reachable in one step despite an alternate two-step path through d.
    assert_eq!(
        run(&g, &["a", "b"], Direction::Outbound, Some(2), None).len(),
        6
    );
}
#[test]
fn underlying_identity_preserves_literal_hash_and_ignores_uri() {
    let g = graph(
        &[
            ("source", "literal#file"),
            ("source", "missing#file"),
            ("source", "https://example.test/#anchor"),
        ],
        &["source", "literal#file"],
    );
    assert_eq!(
        pairs(&run(
            &g,
            &["literal#file"],
            Direction::Inbound,
            Some(1),
            None
        )),
        vec![("source", Some("literal#file#missing"))]
    );
    let f = run(&g, &["source"], Direction::Outbound, None, None);
    assert_eq!(f.len(), 2);
    assert!(
        f.iter()
            .any(|f| f.name == "unresolved-edge" && f.target.as_deref() == Some("missing#file"))
    );
}
#[test]
fn historical_pairs_use_current_frontier_and_deduplicate_removed_nodes() {
    let old = graph(
        &[
            ("gone", "seed"),
            ("gone", "second"),
            ("ancestor", "gone"),
            ("live", "seed"),
            ("seed", "old-target"),
        ],
        &["gone", "seed", "second", "ancestor", "live", "old-target"],
    );
    let lock = Lock::from_composed(&old);
    let current = graph(&[], &["seed", "second", "live"]);
    let inbound = run(
        &current,
        &["seed", "second"],
        Direction::Inbound,
        None,
        Some(&lock),
    );
    assert_eq!(
        pairs(&inbound),
        vec![("live", Some("seed")), ("gone", None)]
    );
    assert_eq!(
        pairs(&run(
            &current,
            &["seed"],
            Direction::Outbound,
            None,
            Some(&lock)
        )),
        vec![("seed", Some("old-target"))]
    );
    assert_eq!(
        run(&current, &["seed"], Direction::Both, None, Some(&lock)).len(),
        3
    );
    assert!(run(&current, &["seed"], Direction::Both, Some(0), Some(&lock)).is_empty());
    assert!(impact::compute(&current, &["seed".into()], Direction::Both, None).is_empty());
}
#[test]
fn historical_edge_at_depth_boundary_is_excluded() {
    let lock = Lock::from_composed(&graph(
        &[("seed", "next"), ("next", "gone")],
        &["seed", "next", "gone"],
    ));
    let current = graph(&[("seed", "next")], &["seed", "next"]);
    assert!(
        !run(
            &current,
            &["seed"],
            Direction::Outbound,
            Some(1),
            Some(&lock)
        )
        .iter()
        .any(|f| f.name == "removed-edge")
    );
    assert!(
        run(
            &current,
            &["seed"],
            Direction::Outbound,
            Some(2),
            Some(&lock)
        )
        .iter()
        .any(|f| f.name == "removed-edge")
    );
}
#[test]
fn isolated_seed_has_no_bookkeeping_with_missing_empty_or_stale_baseline() {
    let g = graph(&[], &["seed"]);
    for lock in [None, Some(Lock::default()), Some(Lock::from_composed(&g))] {
        assert!(run(&g, &["seed"], Direction::Both, None, lock.as_ref()).is_empty());
    }
}
#[test]
fn construction_is_global_at_every_direction_and_depth_and_uses_check_policy() {
    let g = graph(&[], &["seed"]);
    for direction in [Direction::Inbound, Direction::Outbound, Direction::Both] {
        for depth in [Some(0), Some(1), None] {
            let findings = impact::diagnostics(
                &g,
                &["seed".into()],
                direction,
                depth,
                &[],
                None,
                &Config::defaults(),
                vec![Finding::warn(
                    "unreadable-frontmatter",
                    "disconnected",
                    vec![],
                    "failure",
                )],
            );
            assert_eq!(findings.len(), 1);
        }
    }
    for (rule, expected) in [
        ("\"off\"", None),
        ("\"error\"", Some(RuleSeverity::Error)),
        ("\"warn\"", Some(RuleSeverity::Warn)),
        ("{severity = \"error\", ignore = [\"disconnected\"]}", None),
    ] {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("drft.toml"),
            format!("[rules]\nunreadable-frontmatter = {rule}\n"),
        )
        .unwrap();
        let config = Config::load(dir.path()).unwrap();
        let build = vec![Finding::warn(
            "unreadable-frontmatter",
            "disconnected",
            vec![],
            "failure",
        )];
        let actual = impact::diagnostics(
            &g,
            &["seed".into()],
            Direction::Inbound,
            None,
            &[],
            None,
            &config,
            build.clone(),
        );
        assert_eq!(actual.first().map(|f| f.severity), expected);
        let check = drft::rules::check::run(&g, None, &config, build);
        assert_eq!(
            check
                .iter()
                .find(|f| f.name == "unreadable-frontmatter")
                .map(|f| f.severity),
            expected
        );
    }
}
#[test]
fn same_rule_subject_uses_target_tiebreak() {
    let g = graph(&[("source", "z"), ("source", "a")], &["source"]);
    assert_eq!(
        pairs(&run(&g, &["source"], Direction::Outbound, None, None)),
        vec![("source", Some("a")), ("source", Some("z"))]
    );
}
