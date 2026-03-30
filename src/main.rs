// TODO: Remove these allows after Phase 6 re-wires the analysis/metrics pipeline
#[allow(dead_code)]
mod analyses;
mod cli;
mod config;
mod diagnostic;
mod discovery;
mod graph;
mod lockfile;
#[allow(dead_code)]
mod metrics;
mod parsers;
mod rules;

use anyhow::{Context, Result};
use clap::Parser;
use std::path::{Path, PathBuf};

use analyses::Analysis;
use cli::{Cli, ColorChoice, Commands, OutputFormat};
use config::{Config, RuleSeverity};
use diagnostic::Diagnostic;
use graph::build_graph;
use lockfile::{Lockfile, write_lockfile};
use rules::all_rules;

fn use_color(choice: ColorChoice, format: OutputFormat) -> bool {
    // Never colorize JSON output
    if matches!(format, OutputFormat::Json) {
        return false;
    }
    match choice {
        ColorChoice::Always => true,
        ColorChoice::Never => false,
        ColorChoice::Auto => std::io::IsTerminal::is_terminal(&std::io::stdout()),
    }
}

fn main() {
    // Pre-check if JSON format was requested (for structured error output)
    let wants_json =
        std::env::args().any(|a| a == "json") && std::env::args().any(|a| a == "--format");

    let code = match try_main() {
        Ok(code) => code,
        Err(e) => {
            if wants_json {
                let err = serde_json::json!({
                    "error": format!("{e:#}"),
                    "exit_code": 2,
                });
                eprintln!("{}", serde_json::to_string(&err).unwrap());
            } else {
                eprintln!("error: {e:#}");
            }
            2
        }
    };
    std::process::exit(code);
}

fn try_main() -> Result<i32> {
    let cli = Cli::parse();

    let root = match &cli.directory {
        Some(dir) => std::fs::canonicalize(dir)
            .with_context(|| format!("cannot access directory: {}", dir.display()))?,
        None => std::env::current_dir()?,
    };

    match &cli.command {
        Commands::Init {
            interface_from,
            no_interface,
        } => run_init(&root, interface_from.as_deref(), *no_interface),
        Commands::Lock {
            check,
            recursive,
            max_depth,
        } => run_lock(&root, *check, *recursive, *max_depth),
        Commands::Analysis { analyses } => run_analysis(&root, cli.format, analyses),
        Commands::Metrics => run_metrics(&root, cli.format),
        Commands::Impact { files } => run_impact(&root, cli.format, files),
        Commands::Graph {
            recursive,
            max_depth,
        } => run_graph(&root, cli.format, *recursive, *max_depth),
        Commands::Check {
            rules: rule_filter,
            recursive,
            max_depth,
            watch,
        } => {
            if *watch {
                run_check_watch(
                    &root,
                    cli.format,
                    cli.color,
                    rule_filter,
                    *recursive,
                    *max_depth,
                )
            } else {
                run_check(
                    &root,
                    cli.format,
                    cli.color,
                    rule_filter,
                    *recursive,
                    *max_depth,
                )
            }
        }
    }
}

fn run_init(root: &Path, interface_from: Option<&str>, no_interface: bool) -> Result<i32> {
    let config_path = root.join("drft.toml");
    if config_path.exists() {
        anyhow::bail!("drft.toml already exists");
    }

    let mut content = String::from(
        r#"# drft.toml

# Glob patterns for files to exclude from discovery
ignore = []

[parsers]
markdown = true

[rules]
broken-link = "warn"
cycle = "warn"
stale = "error"
"#,
    );

    // Derive interface from a file's outbound links
    if let Some(file) = interface_from {
        let config = Config::defaults();
        let graph = build_graph(root, &config)?;

        if !graph.nodes.contains_key(file) {
            anyhow::bail!("file \"{file}\" not found in graph");
        }

        let mut nodes = Vec::new();
        if let Some(edge_indices) = graph.forward.get(file) {
            for &idx in edge_indices {
                let target = &graph.edges[idx].target;
                if graph.nodes.contains_key(target.as_str()) && !nodes.contains(target) {
                    nodes.push(target.clone());
                }
            }
        }
        nodes.sort();

        content.push_str("\n[interface]\nnodes = [");
        if nodes.is_empty() {
            content.push(']');
        } else {
            content.push('\n');
            for node in &nodes {
                content.push_str(&format!("  \"{node}\",\n"));
            }
            content.push(']');
        }
        content.push('\n');
    } else if !no_interface {
        content.push_str("\n# [interface]\n# nodes = [\"overview.md\", \"api/*.md\"]\n");
    }

    std::fs::write(&config_path, &content)
        .with_context(|| format!("failed to write {}", config_path.display()))?;

    Ok(0)
}

fn run_lock(
    root: &Path,
    check_mode: bool,
    recursive: bool,
    max_depth: Option<usize>,
) -> Result<i32> {
    // Recursive: lock child graphs bottom-up first
    if recursive && max_depth != Some(0) {
        let next_depth = max_depth.map(|d| d.saturating_sub(1));
        let child_graphs = find_lockable_graphs(root)?;
        for child_dir in &child_graphs {
            let code = run_lock(child_dir, check_mode, true, next_depth)?;
            if check_mode && code != 0 {
                return Ok(code);
            }
        }
    }

    // Lock this graph
    lock_graph(root, check_mode)
}

fn lock_graph(root: &Path, check_mode: bool) -> Result<i32> {
    let config = Config::load(root)?;
    let graph = build_graph(root, &config)?;

    let lockfile = Lockfile::from_graph(&graph);

    if check_mode {
        let lock_path = root.join("drft.lock");
        if !lock_path.exists() {
            eprintln!("drft.lock not found");
            return Ok(1);
        }

        let new_content = lockfile.to_toml()?;
        let existing_content = std::fs::read_to_string(&lock_path)
            .with_context(|| format!("failed to read {}", lock_path.display()))?;

        if new_content == existing_content {
            Ok(0)
        } else {
            eprintln!("drft.lock is out of date");
            Ok(1)
        }
    } else {
        write_lockfile(root, &lockfile)?;
        Ok(0)
    }
}

fn run_analysis(root: &Path, format: OutputFormat, filter: &[String]) -> Result<i32> {
    let config = Config::load(root)?;
    let graph = build_graph(root, &config)?;
    let lockfile = lockfile::read_lockfile(root)?;
    let ctx = analyses::AnalysisContext {
        graph: &graph,
        root,
        config: &config,
        lockfile: lockfile.as_ref(),
    };

    // Run all analyses, serialize results to JSON values
    let all: Vec<(&str, serde_json::Value)> = vec![
        ("degree", serde_json::to_value(analyses::degree::Degree.run(&ctx))?),
        ("scc", serde_json::to_value(analyses::scc::StronglyConnectedComponents.run(&ctx))?),
        ("connected-components", serde_json::to_value(analyses::connected_components::ConnectedComponents.run(&ctx))?),
        ("depth", serde_json::to_value(analyses::depth::Depth.run(&ctx))?),
        ("graph-stats", serde_json::to_value(analyses::graph_stats::GraphStats.run(&ctx))?),
        ("bridges", serde_json::to_value(analyses::bridges::Bridges.run(&ctx))?),
        ("transitive-reduction", serde_json::to_value(analyses::transitive_reduction::TransitiveReduction.run(&ctx))?),
        ("betweenness", serde_json::to_value(analyses::betweenness::Betweenness.run(&ctx))?),
        ("pagerank", serde_json::to_value(analyses::pagerank::PageRank.run(&ctx))?),
        ("graph-boundaries", serde_json::to_value(analyses::graph_boundaries::GraphBoundaries.run(&ctx))?),
        ("change-propagation", serde_json::to_value(analyses::change_propagation::ChangePropagation.run(&ctx))?),
    ];

    let results: Vec<_> = if filter.is_empty() {
        all
    } else {
        all.into_iter()
            .filter(|(name, _)| filter.iter().any(|f| f == name))
            .collect()
    };

    if results.is_empty() && !filter.is_empty() {
        anyhow::bail!("no matching analyses: {}", filter.join(", "));
    }

    match format {
        OutputFormat::Json => {
            let map: serde_json::Map<String, serde_json::Value> = results
                .iter()
                .map(|(name, value)| (name.to_string(), value.clone()))
                .collect();
            println!("{}", serde_json::to_string_pretty(&map)?);
        }
        OutputFormat::Text | OutputFormat::Dot => {
            for (name, result) in &results {
                println!("=== {name} ===");
                println!("{}", serde_json::to_string_pretty(result)?);
                println!();
            }
        }
    }

    Ok(0)
}

fn run_metrics(root: &Path, format: OutputFormat) -> Result<i32> {
    let config = Config::load(root)?;
    let graph = build_graph(root, &config)?;
    let lockfile = lockfile::read_lockfile(root)?;
    let ctx = analyses::AnalysisContext {
        graph: &graph,
        root,
        config: &config,
        lockfile: lockfile.as_ref(),
    };

    // Run the analyses we need for metrics
    let degree = analyses::degree::Degree.run(&ctx);
    let scc = analyses::scc::StronglyConnectedComponents.run(&ctx);
    let cc = analyses::connected_components::ConnectedComponents.run(&ctx);
    let stats = analyses::graph_stats::GraphStats.run(&ctx);
    let bridges = analyses::bridges::Bridges.run(&ctx);
    let reduction = analyses::transitive_reduction::TransitiveReduction.run(&ctx);
    let change = analyses::change_propagation::ChangePropagation.run(&ctx);
    let pagerank = analyses::pagerank::PageRank.run(&ctx);

    use metrics::{Metric, MetricKind};
    let mut all_metrics: Vec<Metric> = Vec::new();

    // Connectivity
    let total_nodes = graph.nodes.values().filter(|n| graph.is_file_node(&n.path)).count() as f64;
    if total_nodes > 0.0 {
        let orphans = degree.nodes.iter().filter(|n| n.in_degree == 0).count() as f64;
        all_metrics.push(Metric { name: "orphan_ratio".into(), value: orphans / total_nodes, kind: MetricKind::Ratio, dimension: "connectivity".into() });

        let islands = cc.components.iter().filter(|c| c.members.len() == 1).count() as f64;
        all_metrics.push(Metric { name: "island_ratio".into(), value: islands / total_nodes, kind: MetricKind::Ratio, dimension: "connectivity".into() });
    }

    // Complexity
    all_metrics.push(Metric { name: "component_count".into(), value: cc.component_count as f64, kind: MetricKind::Count, dimension: "complexity".into() });
    all_metrics.push(Metric { name: "density".into(), value: stats.density, kind: MetricKind::Ratio, dimension: "complexity".into() });
    all_metrics.push(Metric { name: "cyclomatic_complexity".into(), value: (graph.edges.len() as f64 - graph.nodes.len() as f64 + cc.components.len() as f64), kind: MetricKind::Count, dimension: "complexity".into() });
    if let Some(d) = stats.diameter {
        all_metrics.push(Metric { name: "diameter".into(), value: d as f64, kind: MetricKind::Count, dimension: "complexity".into() });
    }
    if let Some(avg) = stats.average_path_length {
        all_metrics.push(Metric { name: "average_path_length".into(), value: avg, kind: MetricKind::Score, dimension: "complexity".into() });
    }

    // Conciseness
    let total_edges = graph.edges.len() as f64;
    if total_edges > 0.0 {
        all_metrics.push(Metric { name: "redundant_edge_ratio".into(), value: reduction.redundant_edges.len() as f64 / total_edges, kind: MetricKind::Ratio, dimension: "conciseness".into() });
    }

    // Resilience
    all_metrics.push(Metric { name: "bridge_count".into(), value: bridges.bridges.len() as f64, kind: MetricKind::Count, dimension: "resilience".into() });
    all_metrics.push(Metric { name: "cut_node_count".into(), value: bridges.cut_vertices.len() as f64, kind: MetricKind::Count, dimension: "resilience".into() });

    // Freshness
    if change.has_lockfile {
        all_metrics.push(Metric { name: "directly_changed_count".into(), value: change.directly_changed.len() as f64, kind: MetricKind::Count, dimension: "freshness".into() });
        all_metrics.push(Metric { name: "transitively_stale_count".into(), value: change.transitively_stale.len() as f64, kind: MetricKind::Count, dimension: "freshness".into() });
        if total_nodes > 0.0 {
            let stale = (change.directly_changed.len() + change.transitively_stale.len()) as f64;
            all_metrics.push(Metric { name: "stale_ratio".into(), value: stale / total_nodes, kind: MetricKind::Ratio, dimension: "freshness".into() });
        }
    }

    // PageRank concentration
    if !pagerank.nodes.is_empty() {
        let scores: Vec<f64> = pagerank.nodes.iter().map(|n| n.score).collect();
        let max = scores.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        all_metrics.push(Metric { name: "max_pagerank".into(), value: max, kind: MetricKind::Score, dimension: "complexity".into() });
    }

    // SCC
    all_metrics.push(Metric { name: "cycle_count".into(), value: scc.nontrivial_count as f64, kind: MetricKind::Count, dimension: "complexity".into() });

    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&all_metrics)?);
        }
        OutputFormat::Text | OutputFormat::Dot => {
            for m in &all_metrics {
                let val = if m.value == m.value.floor() {
                    format!("{}", m.value as i64)
                } else {
                    format!("{:.4}", m.value)
                };
                println!("{:<30} {:>10}  ({})", m.name, val, m.dimension);
            }
        }
    }

    Ok(0)
}

fn run_graph(
    root: &Path,
    format: OutputFormat,
    recursive: bool,
    max_depth: Option<usize>,
) -> Result<i32> {
    let graph_root = find_graph_root(root);

    match format {
        OutputFormat::Dot => {
            let graphs = collect_jgf_graphs(&graph_root, ".", recursive, max_depth)?;
            println!("digraph {{");
            let mut all_nodes = Vec::new();
            let mut all_edges = Vec::new();
            for g in &graphs {
                let prefix = if g.id == "." { "" } else { g.id.as_str() };
                for (path, _, _) in &g.nodes {
                    let full = if prefix.is_empty() {
                        path.clone()
                    } else {
                        format!("{prefix}{path}")
                    };
                    all_nodes.push(full);
                }
                for (source, target, _) in &g.edges {
                    let fs = if prefix.is_empty() {
                        source.clone()
                    } else {
                        format!("{prefix}{source}")
                    };
                    let ft = if prefix.is_empty() {
                        target.clone()
                    } else {
                        format!("{prefix}{target}")
                    };
                    all_edges.push((fs, ft));
                }
            }
            all_nodes.sort();
            all_nodes.dedup();
            for path in &all_nodes {
                println!("  \"{path}\"");
            }
            all_edges.sort();
            all_edges.dedup();
            for (source, target) in &all_edges {
                println!("  \"{source}\" -> \"{target}\"");
            }
            println!("}}");
        }
        _ => {
            // JSON Graph Format (JGF)
            let graphs = collect_jgf_graphs(&graph_root, ".", recursive, max_depth)?;

            let jgf_graphs: Vec<serde_json::Value> = graphs
                .iter()
                .map(|g| {
                    let mut nodes = serde_json::Map::new();
                    let mut sorted: Vec<&(String, graph::NodeType, Option<String>)> =
                        g.nodes.iter().collect();
                    sorted.sort_by(|a, b| a.0.cmp(&b.0));
                    for (path, node_type, hash) in sorted {
                        let mut meta = serde_json::Map::new();
                        meta.insert("type".into(), serde_json::json!(node_type));
                        if let Some(h) = hash {
                            meta.insert("hash".into(), serde_json::json!(h));
                        }
                        nodes.insert(path.clone(), serde_json::json!({ "metadata": meta }));
                    }

                    let mut edges: Vec<serde_json::Value> = g
                        .edges
                        .iter()
                        .map(|(source, target, edge_type)| {
                            serde_json::json!({
                                "source": source,
                                "target": target,
                                "relation": edge_type,
                            })
                        })
                        .collect();
                    edges.sort_by(|a, b| {
                        a["source"]
                            .as_str()
                            .cmp(&b["source"].as_str())
                            .then_with(|| a["target"].as_str().cmp(&b["target"].as_str()))
                    });

                    serde_json::json!({
                        "id": g.id,
                        "directed": true,
                        "nodes": nodes,
                        "edges": edges,
                    })
                })
                .collect();

            let output = if jgf_graphs.len() == 1 {
                serde_json::json!({ "graph": jgf_graphs[0] })
            } else {
                serde_json::json!({ "graphs": jgf_graphs })
            };
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
    }

    Ok(0)
}

struct GraphExport {
    id: String,
    nodes: Vec<(String, graph::NodeType, Option<String>)>, // path, type, hash
    edges: Vec<(String, String, graph::EdgeType)>,
}

/// Collect JGF graph(s) from a graph root and optionally its children.
fn collect_jgf_graphs(
    root: &Path,
    id: &str,
    recursive: bool,
    max_depth: Option<usize>,
) -> Result<Vec<GraphExport>> {
    let config = Config::load(root)?;
    let g = build_graph(root, &config)?;

    let nodes: Vec<(String, graph::NodeType, Option<String>)> = g
        .nodes
        .iter()
        .map(|(path, node)| (path.clone(), node.node_type, node.hash.clone()))
        .collect();

    let edges: Vec<(String, String, graph::EdgeType)> = g
        .edges
        .iter()
        .filter(|e| g.nodes.contains_key(&e.target))
        .map(|e| (e.source.clone(), e.target.clone(), e.edge_type.clone()))
        .collect();

    let mut graphs = vec![GraphExport {
        id: id.to_string(),
        nodes,
        edges,
    }];

    if recursive && max_depth != Some(0) {
        let next_depth = max_depth.map(|d| d.saturating_sub(1));
        for child_graph in &g.child_graphs {
            let child_dir = root.join(child_graph.trim_end_matches('/'));
            let child_id = if id == "." {
                child_graph.trim_end_matches('/').to_string()
            } else {
                format!(
                    "{}/{}",
                    id.trim_end_matches('/'),
                    child_graph.trim_end_matches('/')
                )
            };
            let sub_graphs = collect_jgf_graphs(&child_dir, &child_id, true, next_depth)?;
            graphs.extend(sub_graphs);
        }
    }

    Ok(graphs)
}

fn run_impact(root: &Path, format: OutputFormat, files: &[String]) -> Result<i32> {
    let graph_root = find_graph_root(root);
    let config = Config::load(&graph_root)?;
    let graph = build_graph(&graph_root, &config)?;

    // Resolve file args (try with .md extension if not found)
    let mut seeds: Vec<String> = Vec::new();
    for file in files {
        if graph.nodes.contains_key(file.as_str()) {
            seeds.push(file.clone());
        } else {
            let with_ext = format!("{file}.md");
            if graph.nodes.contains_key(with_ext.as_str()) {
                seeds.push(with_ext);
            } else {
                anyhow::bail!("node not found: \"{file}\"");
            }
        }
    }

    // BFS: walk reverse edges to find all transitive dependents
    let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut queue: std::collections::VecDeque<String> = std::collections::VecDeque::new();

    // Seeds are the starting points — don't include them in the output
    for seed in &seeds {
        visited.insert(seed.clone());
        queue.push_back(seed.clone());
    }

    let mut impacted: Vec<(String, String)> = Vec::new(); // (node, via)

    while let Some(node) = queue.pop_front() {
        if let Some(edge_indices) = graph.reverse.get(node.as_str()) {
            for &idx in edge_indices {
                let dependent = &graph.edges[idx].source;
                if !visited.contains(dependent.as_str()) {
                    visited.insert(dependent.clone());
                    impacted.push((dependent.clone(), node.clone()));
                    queue.push_back(dependent.clone());
                }
            }
        }
    }

    impacted.sort();

    match format {
        OutputFormat::Json => {
            let output = serde_json::json!({
                "files": seeds,
                "total": impacted.len(),
                "impacted": impacted.iter().map(|(node, via)| {
                    serde_json::json!({
                        "node": node,
                        "via": via,
                        "fix": format!("{via} may change — review {node} to ensure it still accurately reflects {via}")
                    })
                }).collect::<Vec<_>>()
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        _ => {
            if impacted.is_empty() {
                println!("no dependents found");
            } else {
                for (node, via) in &impacted {
                    println!("{node} (via {via})");
                }
            }
        }
    }

    Ok(0)
}

fn run_check(
    root: &Path,
    format: OutputFormat,
    color: ColorChoice,
    rule_filter: &[String],
    recursive: bool,
    max_depth: Option<usize>,
) -> Result<i32> {
    // Validate rule names (built-in + script rules from config)
    let graph_root = find_graph_root(root);
    let root_config = Config::load(&graph_root)?;
    let available_rules = all_rules();
    let mut known_names: Vec<&str> = available_rules.iter().map(|r| r.name()).collect();
    let script_names: Vec<String> = root_config
        .script_rules()
        .map(|(name, _)| name.to_string())
        .collect();
    known_names.extend(script_names.iter().map(|s| s.as_str()));
    for name in rule_filter {
        if !known_names.contains(&name.as_str()) {
            anyhow::bail!("unknown rule: \"{name}\"");
        }
    }

    let mut diagnostics = check_graph(&graph_root, rule_filter, None, recursive, max_depth)?;

    diagnostics.sort_by(|a, b| {
        a.graph
            .cmp(&b.graph)
            .then_with(|| a.rule.cmp(&b.rule))
            .then_with(|| a.source.cmp(&b.source))
            .then_with(|| a.target.cmp(&b.target))
            .then_with(|| a.node.cmp(&b.node))
    });

    let colorize = use_color(color, format);
    let mut current_graph: Option<&Option<String>> = None;
    for d in &diagnostics {
        match format {
            OutputFormat::Text | OutputFormat::Dot => {
                // Print graph header when graph changes
                if current_graph != Some(&d.graph) {
                    if let Some(g) = &d.graph {
                        if current_graph.is_some() {
                            println!();
                        }
                        if colorize {
                            println!("\x1b[1m[{g}]\x1b[0m");
                        } else {
                            println!("[{g}]");
                        }
                    }
                    current_graph = Some(&d.graph);
                }
                if colorize {
                    println!("{}", d.format_text_color());
                } else {
                    println!("{}", d.format_text());
                }
            }
            OutputFormat::Json => {} // handled below as envelope
        }
    }

    let has_errors = diagnostics
        .iter()
        .any(|d| d.severity == RuleSeverity::Error);
    let has_warnings = diagnostics.iter().any(|d| d.severity == RuleSeverity::Warn);

    if matches!(format, OutputFormat::Json) {
        let status = if has_errors {
            "error"
        } else if has_warnings {
            "warn"
        } else {
            "clean"
        };
        let error_count = diagnostics
            .iter()
            .filter(|d| d.severity == RuleSeverity::Error)
            .count();
        let warn_count = diagnostics
            .iter()
            .filter(|d| d.severity == RuleSeverity::Warn)
            .count();
        let output = serde_json::json!({
            "status": status,
            "total": diagnostics.len(),
            "errors": error_count,
            "warnings": warn_count,
            "diagnostics": diagnostics,
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    }

    Ok(if has_errors { 1 } else { 0 })
}

fn run_check_watch(
    root: &Path,
    format: OutputFormat,
    color: ColorChoice,
    rule_filter: &[String],
    recursive: bool,
    max_depth: Option<usize>,
) -> Result<i32> {
    use notify_debouncer_mini::{DebouncedEventKind, new_debouncer};
    use std::sync::mpsc;
    use std::time::Duration;

    let graph_root = find_graph_root(root);

    // Initial run
    print!("\x1b[2J\x1b[H"); // clear screen
    let _ = run_check(root, format, color, rule_filter, recursive, max_depth);
    eprintln!("\n\x1b[2m--- watching for changes (ctrl-c to stop) ---\x1b[0m");

    let (tx, rx) = mpsc::channel();
    let mut debouncer = new_debouncer(Duration::from_millis(500), tx)?;

    notify::Watcher::watch(
        debouncer.watcher(),
        &graph_root,
        notify::RecursiveMode::Recursive,
    )?;

    loop {
        match rx.recv() {
            Ok(Ok(events)) => {
                // Only re-run if relevant files changed (md, toml, lock)
                let relevant = events.iter().any(|e| {
                    if e.kind != DebouncedEventKind::Any {
                        return false;
                    }
                    let path = e.path.to_string_lossy();
                    path.ends_with(".md") || path.ends_with(".toml") || path.ends_with(".lock")
                });
                if !relevant {
                    continue;
                }

                print!("\x1b[2J\x1b[H"); // clear screen
                let _ = run_check(root, format, color, rule_filter, recursive, max_depth);
                eprintln!("\n\x1b[2m--- watching for changes (ctrl-c to stop) ---\x1b[0m");
            }
            Ok(Err(e)) => {
                eprintln!("watch error: {e}");
            }
            Err(_) => break, // channel closed
        }
    }

    Ok(0)
}

/// Find subdirectories that are lockable graphs (have drft.lock or drft.toml).
/// Returns absolute paths, sorted, shallowest first. Does not recurse past graph boundaries.
/// Respects .gitignore.
fn find_lockable_graphs(root: &Path) -> Result<Vec<PathBuf>> {
    let mut graphs = Vec::new();
    let mut found: Vec<PathBuf> = Vec::new();
    let root_owned = root.to_path_buf();

    let walker = ignore::WalkBuilder::new(root)
        .follow_links(true)
        .sort_by_file_name(|a, b| a.cmp(b))
        .filter_entry(move |entry| {
            entry.file_type().is_some_and(|ft| ft.is_dir()) && entry.path() != root_owned
                || entry.path() == root_owned
        })
        .build();

    for entry in walker.filter_map(|e| e.ok()) {
        if !entry.file_type().is_some_and(|ft| ft.is_dir()) || entry.path() == root {
            continue;
        }

        // Skip if inside an already-found graph
        let inside_existing = found.iter().any(|s| entry.path().starts_with(s));
        if inside_existing {
            continue;
        }

        let has_lock = entry.path().join("drft.lock").exists();
        let has_config = entry.path().join("drft.toml").exists();
        if has_lock || has_config {
            found.push(entry.path().to_path_buf());
            graphs.push(entry.path().to_path_buf());
        }
    }

    graphs.sort();
    Ok(graphs)
}

/// Walk up from `start` to find the nearest ancestor directory with `drft.lock`.
/// If none found, returns `start`.
fn find_graph_root(start: &Path) -> std::path::PathBuf {
    let mut current = start.to_path_buf();
    loop {
        if current.join("drft.lock").exists() {
            return current;
        }
        if !current.pop() {
            return start.to_path_buf();
        }
    }
}

/// Check a single graph and optionally recurse into child graphs.
fn check_graph(
    root: &Path,
    rule_filter: &[String],
    graph_prefix: Option<&str>,
    recursive: bool,
    max_depth: Option<usize>,
) -> Result<Vec<Diagnostic>> {
    let config = Config::load(root)?;
    let graph = build_graph(root, &config)?;

    let mut diagnostics = Vec::new();

    for rule in all_rules() {
        let severity = if !rule_filter.is_empty() {
            if !rule_filter.iter().any(|f| f == rule.name()) {
                continue;
            }
            let configured = config.rule_severity(rule.name());
            if configured == RuleSeverity::Off {
                RuleSeverity::Warn
            } else {
                configured
            }
        } else {
            let severity = config.rule_severity(rule.name());
            if severity == RuleSeverity::Off {
                continue;
            }
            severity
        };

        let rule_ctx = rules::RuleContext {
            graph: &graph,
            root,
            config: &config,
            lockfile: None,
        };
        let mut findings = rule.evaluate(&rule_ctx);
        findings.retain(|d| {
            // Check all path fields against ignore-rules
            let paths: Vec<&str> = [d.source.as_deref(), d.target.as_deref(), d.node.as_deref()]
                .into_iter()
                .flatten()
                .collect();
            !paths.iter().any(|p| config.is_rule_ignored(rule.name(), p))
        });
        for d in &mut findings {
            d.severity = severity;
            if graph_prefix.is_some() {
                d.graph = graph_prefix.map(|s| s.to_string());
            }
        }
        diagnostics.extend(findings);
    }

    // Run script rules (respecting --rule filter)
    let has_script_rules = config.script_rules().next().is_some();
    let run_script = if rule_filter.is_empty() {
        has_script_rules
    } else {
        config
            .script_rules()
            .any(|(name, _)| rule_filter.iter().any(|f| f == name))
    };
    if run_script {
        let mut script_findings = rules::script::run_script_rules(&graph, root, &config);
        // Filter to only requested rules if --rule is set
        if !rule_filter.is_empty() {
            script_findings.retain(|d| rule_filter.iter().any(|f| f == &d.rule));
        }
        // Apply per-rule ignore filtering to script rule diagnostics
        script_findings.retain(|d| {
            let paths: Vec<&str> = [d.source.as_deref(), d.target.as_deref(), d.node.as_deref()]
                .into_iter()
                .flatten()
                .collect();
            !paths.iter().any(|p| config.is_rule_ignored(&d.rule, p))
        });
        for d in &mut script_findings {
            if graph_prefix.is_some() {
                d.graph = graph_prefix.map(|s| s.to_string());
            }
        }
        diagnostics.extend(script_findings);
    }

    // Recursively check child graphs if --recursive
    if recursive && max_depth != Some(0) {
        let next_depth = max_depth.map(|d| d.saturating_sub(1));
        for child_graph in &graph.child_graphs {
            let child_dir = root.join(child_graph.trim_end_matches('/'));
            let child_prefix = match graph_prefix {
                Some(parent) => format!("{parent}/{}", child_graph.trim_end_matches('/')),
                None => child_graph.trim_end_matches('/').to_string(),
            };
            let child_diagnostics = check_graph(
                &child_dir,
                rule_filter,
                Some(&child_prefix),
                recursive,
                next_depth,
            )?;
            diagnostics.extend(child_diagnostics);
        }
    }

    Ok(diagnostics)
}
