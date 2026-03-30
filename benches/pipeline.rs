use criterion::{criterion_group, criterion_main, Criterion};
use drft::analyses::{self, Analysis, AnalysisContext};
use drft::config::Config;
use drft::graph::build_graph;
use drft::lockfile;
use drft::rules::{self, Rule, RuleContext};
use std::path::Path;

/// Benchmark against the repo itself (98 nodes, 113 edges).
fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn bench_graph_construction(c: &mut Criterion) {
    let root = repo_root();
    let config = Config::load(root).unwrap();

    c.bench_function("build_graph", |b| {
        b.iter(|| build_graph(root, &config).unwrap());
    });
}

fn bench_analyses(c: &mut Criterion) {
    let root = repo_root();
    let config = Config::load(root).unwrap();
    let graph = build_graph(root, &config).unwrap();
    let lockfile = lockfile::read_lockfile(root).unwrap();
    let ctx = AnalysisContext {
        graph: &graph,
        root,
        config: &config,
        lockfile: lockfile.as_ref(),
    };

    let mut group = c.benchmark_group("analyses");

    group.bench_function("degree", |b| {
        b.iter(|| analyses::degree::Degree.run(&ctx));
    });
    group.bench_function("scc", |b| {
        b.iter(|| analyses::scc::StronglyConnectedComponents.run(&ctx));
    });
    group.bench_function("connected_components", |b| {
        b.iter(|| analyses::connected_components::ConnectedComponents.run(&ctx));
    });
    group.bench_function("depth", |b| {
        b.iter(|| analyses::depth::Depth.run(&ctx));
    });
    group.bench_function("graph_stats", |b| {
        b.iter(|| analyses::graph_stats::GraphStats.run(&ctx));
    });
    group.bench_function("bridges", |b| {
        b.iter(|| analyses::bridges::Bridges.run(&ctx));
    });
    group.bench_function("transitive_reduction", |b| {
        b.iter(|| analyses::transitive_reduction::TransitiveReduction.run(&ctx));
    });
    group.bench_function("betweenness", |b| {
        b.iter(|| analyses::betweenness::Betweenness.run(&ctx));
    });
    group.bench_function("pagerank", |b| {
        b.iter(|| analyses::pagerank::PageRank.run(&ctx));
    });
    group.bench_function("graph_boundaries", |b| {
        b.iter(|| analyses::graph_boundaries::GraphBoundaries.run(&ctx));
    });
    group.bench_function("change_propagation", |b| {
        b.iter(|| analyses::change_propagation::ChangePropagation.run(&ctx));
    });

    group.finish();
}

fn bench_rules(c: &mut Criterion) {
    let root = repo_root();
    let config = Config::load(root).unwrap();
    let graph = build_graph(root, &config).unwrap();
    let ctx = RuleContext {
        graph: &graph,
        root,
        config: &config,
        lockfile: None,
    };

    c.bench_function("all_rules", |b| {
        b.iter(|| {
            let all = rules::all_rules();
            let mut diagnostics = Vec::new();
            for rule in &all {
                diagnostics.extend(rule.evaluate(&ctx));
            }
            diagnostics
        });
    });
}

fn bench_full_check(c: &mut Criterion) {
    let root = repo_root();

    c.bench_function("full_check", |b| {
        b.iter(|| {
            let config = Config::load(root).unwrap();
            let graph = build_graph(root, &config).unwrap();
            let ctx = RuleContext {
                graph: &graph,
                root,
                config: &config,
                lockfile: None,
            };
            let all = rules::all_rules();
            let mut diagnostics = Vec::new();
            for rule in &all {
                diagnostics.extend(rule.evaluate(&ctx));
            }
            diagnostics
        });
    });
}

criterion_group!(
    benches,
    bench_graph_construction,
    bench_analyses,
    bench_rules,
    bench_full_check,
);
criterion_main!(benches);
