mod analysis;
mod cli;
mod config;
mod diagnostic;
mod discovery;
mod graph;
mod lockfile;
mod parsing;
mod rules;

use anyhow::{Context, Result};
use clap::Parser;
use std::path::{Path, PathBuf};

use cli::{Cli, ColorChoice, Commands, OutputFormat};
use config::{Config, RuleSeverity};
use diagnostic::Diagnostic;
use graph::build_graph;
use lockfile::{Lockfile, derive_manifest, read_lockfile, write_lockfile};
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
        Commands::Init => run_init(&root),
        Commands::Lock {
            check,
            manifest,
            no_manifest,
            recursive,
            max_depth,
        } => run_lock(
            &root,
            *check,
            manifest.as_deref(),
            *no_manifest,
            *recursive,
            *max_depth,
        ),
        Commands::Report { analyses } => run_report(&root, cli.format, analyses),
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

fn run_init(root: &Path) -> Result<i32> {
    let config_path = root.join("drft.toml");
    if config_path.exists() {
        anyhow::bail!("drft.toml already exists");
    }

    let content = r#"# drft.toml

# Glob patterns for files to exclude from discovery
ignore = []

# Manifest file — seals the scope, controlling visibility to parent scopes
# manifest = "README.md"

# Rules: "error", "warn", or "off"
[rules]
broken-link = "warn"
containment = "warn"
cycle = "warn"
directory-link = "warn"
encapsulation = "warn"
fragility = "off"
fragmentation = "off"
indirect-link = "off"
layer-violation = "off"
lockfile-outdated = "warn"
orphan = "off"
redundant-edge = "off"
stale = "warn"

# Per-rule path ignores (glob patterns)
# [ignore-rules]
# orphan = ["README.md", "index.md"]
# broken-link = ["drafts/*"]

# Custom rules (scripts that receive graph JSON on stdin, emit diagnostics as NDJSON)
# [custom-rules.my-rule]
# command = "./scripts/my-rule.sh"
# severity = "warn"
"#;

    std::fs::write(&config_path, content)
        .with_context(|| format!("failed to write {}", config_path.display()))?;

    Ok(0)
}

fn run_report(root: &Path, format: OutputFormat, analysis_filter: &[String]) -> Result<i32> {
    use analysis::Analysis;
    use analysis::betweenness::Betweenness;
    use analysis::bridges::Bridges as BridgesAnalysis;
    use analysis::connected_components::ConnectedComponents;
    use analysis::degree::Degree;
    use analysis::depth::Depth as DepthAnalysis;
    use analysis::edge_classification::{EdgeClassification, EdgeStatus};
    use analysis::graph_stats::GraphStats;
    use analysis::pagerank::PageRank;
    use analysis::scc::StronglyConnectedComponents;
    use analysis::scope_boundaries::ScopeBoundaries;
    use analysis::transitive_reduction::TransitiveReduction;

    let known_analyses = [
        "betweenness",
        "bridges",
        "connected-components",
        "degree",
        "depth",
        "edge-classification",
        "graph-stats",
        "pagerank",
        "scc",
        "scope-boundaries",
        "transitive-reduction",
    ];

    // Validate analysis names
    for name in analysis_filter {
        if !known_analyses.contains(&name.as_str()) {
            anyhow::bail!("unknown analysis: \"{name}\"");
        }
    }

    let scope_root = find_scope_root(root);
    let config = Config::load(&scope_root)?;
    let graph = build_graph(&scope_root, &config)?;

    let run_all = analysis_filter.is_empty();

    let mut results = serde_json::Map::new();

    if run_all || analysis_filter.iter().any(|a| a == "betweenness") {
        let bw = Betweenness;
        let result = bw.run(&graph, &scope_root);

        match format {
            OutputFormat::Json => {
                results.insert(bw.name().to_string(), serde_json::to_value(&result)?);
            }
            _ => {
                println!("=== betweenness ===");
                if result.nodes.is_empty() {
                    println!("no nodes");
                } else {
                    for nb in &result.nodes {
                        println!("{}  {:.4}", nb.node, nb.score);
                    }
                }
            }
        }
    }

    if run_all || analysis_filter.iter().any(|a| a == "bridges") {
        let br = BridgesAnalysis;
        let result = br.run(&graph, &scope_root);

        match format {
            OutputFormat::Json => {
                results.insert(br.name().to_string(), serde_json::to_value(&result)?);
            }
            _ => {
                println!("=== bridges ===");
                if result.cut_vertices.is_empty() && result.bridges.is_empty() {
                    println!("no cut vertices or bridges");
                } else {
                    println!(
                        "{} cut {}, {} {}",
                        result.cut_vertices.len(),
                        if result.cut_vertices.len() == 1 {
                            "vertex"
                        } else {
                            "vertices"
                        },
                        result.bridges.len(),
                        if result.bridges.len() == 1 {
                            "bridge"
                        } else {
                            "bridges"
                        }
                    );
                    for v in &result.cut_vertices {
                        println!("cut vertex: {v}");
                    }
                    for b in &result.bridges {
                        println!("bridge: {} \u{2194} {}", b.source, b.target);
                    }
                }
            }
        }
    }

    if run_all || analysis_filter.iter().any(|a| a == "connected-components") {
        let cc = ConnectedComponents;
        let result = cc.run(&graph, &scope_root);

        match format {
            OutputFormat::Json => {
                results.insert(cc.name().to_string(), serde_json::to_value(&result)?);
            }
            _ => {
                println!("=== connected-components ===");
                if result.component_count <= 1 {
                    println!("1 component (fully connected)");
                } else {
                    println!("{} components", result.component_count);
                    for c in &result.components {
                        println!(
                            "component {} ({} nodes): {}",
                            c.id,
                            c.members.len(),
                            c.members.join(", ")
                        );
                    }
                }
            }
        }
    }

    if run_all || analysis_filter.iter().any(|a| a == "depth") {
        let dep = DepthAnalysis;
        let result = dep.run(&graph, &scope_root);

        match format {
            OutputFormat::Json => {
                results.insert(dep.name().to_string(), serde_json::to_value(&result)?);
            }
            _ => {
                println!("=== depth ===");
                if result.nodes.is_empty() {
                    println!("no nodes");
                } else {
                    // Group by depth
                    let mut current_depth = None;
                    let mut current_nodes = Vec::new();
                    let mut current_has_cycle = false;

                    let flush = |depth: usize, nodes: &[String], has_cycle: bool| {
                        if has_cycle {
                            println!("depth {} (cyclic): {}", depth, nodes.join(", "));
                        } else {
                            println!("depth {}: {}", depth, nodes.join(", "));
                        }
                    };

                    for nd in &result.nodes {
                        if current_depth != Some(nd.depth) {
                            if let Some(d) = current_depth {
                                flush(d, &current_nodes, current_has_cycle);
                            }
                            current_depth = Some(nd.depth);
                            current_nodes.clear();
                            current_has_cycle = false;
                        }
                        current_nodes.push(nd.node.clone());
                        if nd.in_cycle {
                            current_has_cycle = true;
                        }
                    }
                    if let Some(d) = current_depth {
                        flush(d, &current_nodes, current_has_cycle);
                    }
                }
            }
        }
    }

    if run_all || analysis_filter.iter().any(|a| a == "edge-classification") {
        let ec = EdgeClassification;
        let result = ec.run(&graph, &scope_root);

        match format {
            OutputFormat::Json => {
                results.insert(ec.name().to_string(), serde_json::to_value(&result)?);
            }
            _ => {
                println!("=== edge-classification ===");
                let mut counts = std::collections::HashMap::new();
                for e in &result.edges {
                    let label = match &e.status {
                        EdgeStatus::Valid => "valid",
                        EdgeStatus::Broken => "broken",
                        EdgeStatus::Excluded => "excluded",
                        EdgeStatus::DirectoryTarget => "directory",
                        EdgeStatus::SymlinkTarget { .. } => "symlink",
                        EdgeStatus::External => "external",
                    };
                    *counts.entry(label).or_insert(0usize) += 1;
                }
                let total = result.edges.len();
                println!("{total} edges");
                for label in &[
                    "valid",
                    "broken",
                    "excluded",
                    "directory",
                    "symlink",
                    "external",
                ] {
                    if let Some(&count) = counts.get(label) {
                        println!("  {label}: {count}");
                    }
                }
            }
        }
    }

    if run_all || analysis_filter.iter().any(|a| a == "graph-stats") {
        let gs = GraphStats;
        let result = gs.run(&graph, &scope_root);

        match format {
            OutputFormat::Json => {
                results.insert(gs.name().to_string(), serde_json::to_value(&result)?);
            }
            _ => {
                println!("=== graph-stats ===");
                println!("nodes: {}", result.node_count);
                println!("edges: {}", result.edge_count);
                println!("density: {:.2}", result.density);
                match result.diameter {
                    Some(d) => println!("diameter: {d}"),
                    None => println!("diameter: - (disconnected)"),
                }
                match result.average_path_length {
                    Some(a) => println!("avg path length: {a:.1}"),
                    None => println!("avg path length: - (disconnected)"),
                }
            }
        }
    }

    if run_all || analysis_filter.iter().any(|a| a == "degree") {
        let deg = Degree;
        let result = deg.run(&graph, &scope_root);

        match format {
            OutputFormat::Json => {
                results.insert(deg.name().to_string(), serde_json::to_value(&result)?);
            }
            _ => {
                println!("=== degree ===");
                if result.nodes.is_empty() {
                    println!("no nodes");
                } else {
                    for nd in &result.nodes {
                        println!("{}  in:{}  out:{}", nd.node, nd.in_degree, nd.out_degree);
                    }
                }
            }
        }
    }

    if run_all || analysis_filter.iter().any(|a| a == "pagerank") {
        let pr = PageRank;
        let result = pr.run(&graph, &scope_root);

        match format {
            OutputFormat::Json => {
                results.insert(pr.name().to_string(), serde_json::to_value(&result)?);
            }
            _ => {
                println!("=== pagerank ===");
                if result.nodes.is_empty() {
                    println!("no nodes");
                } else {
                    if result.converged {
                        println!("converged in {} iterations", result.iterations);
                    } else {
                        println!("did not converge after {} iterations", result.iterations);
                    }
                    for np in &result.nodes {
                        println!("{}  {:.4}", np.node, np.score);
                    }
                }
            }
        }
    }

    if run_all || analysis_filter.iter().any(|a| a == "scc") {
        let scc = StronglyConnectedComponents;
        let result = scc.run(&graph, &scope_root);

        match format {
            OutputFormat::Json => {
                results.insert(scc.name().to_string(), serde_json::to_value(&result)?);
            }
            _ => {
                println!("=== scc ===");
                if result.nontrivial_count == 0 {
                    println!("no non-trivial SCCs (graph is acyclic)");
                } else {
                    println!(
                        "{} non-trivial {}",
                        result.nontrivial_count,
                        if result.nontrivial_count == 1 {
                            "SCC"
                        } else {
                            "SCCs"
                        }
                    );
                    for s in &result.sccs {
                        println!(
                            "scc {} ({} nodes): {}",
                            s.id,
                            s.members.len(),
                            s.members.join(", ")
                        );
                    }
                }
            }
        }
    }

    if run_all || analysis_filter.iter().any(|a| a == "scope-boundaries") {
        let sb = ScopeBoundaries;
        let result = sb.run(&graph, &scope_root);

        match format {
            OutputFormat::Json => {
                results.insert(sb.name().to_string(), serde_json::to_value(&result)?);
            }
            _ => {
                println!("=== scope-boundaries ===");
                println!("sealed: {}", if result.sealed { "yes" } else { "no" });
                if result.escapes.is_empty() && result.encapsulation_violations.is_empty() {
                    println!("no boundary crossings");
                } else {
                    for e in &result.escapes {
                        println!("escape: {} \u{2192} {}", e.source, e.target);
                    }
                    for v in &result.encapsulation_violations {
                        println!(
                            "encapsulation: {} \u{2192} {} (bypasses {}manifest)",
                            v.source, v.target, v.scope
                        );
                    }
                }
            }
        }
    }

    if run_all || analysis_filter.iter().any(|a| a == "transitive-reduction") {
        let tr = TransitiveReduction;
        let result = tr.run(&graph, &scope_root);

        match format {
            OutputFormat::Json => {
                results.insert(tr.name().to_string(), serde_json::to_value(&result)?);
            }
            _ => {
                if result.redundant_edges.is_empty() {
                    println!("=== transitive-reduction ===");
                    println!("no redundant edges");
                } else {
                    println!("=== transitive-reduction ===");
                    for re in &result.redundant_edges {
                        println!("{} \u{2192} {} (via {})", re.source, re.target, re.via);
                    }
                }
            }
        }
    }

    if matches!(format, OutputFormat::Json) {
        let output = serde_json::json!({ "analyses": results });
        println!("{}", serde_json::to_string_pretty(&output)?);
    }

    Ok(0)
}

fn run_lock(
    root: &Path,
    check_mode: bool,
    manifest_flag: Option<&str>,
    no_manifest: bool,
    recursive: bool,
    max_depth: Option<usize>,
) -> Result<i32> {
    if manifest_flag.is_some() && no_manifest {
        anyhow::bail!("cannot use --manifest and --no-manifest together");
    }

    // Recursive: lock child scopes bottom-up first
    if recursive && max_depth != Some(0) {
        let next_depth = max_depth.map(|d| d.saturating_sub(1));
        let child_scopes = find_lockable_scopes(root)?;
        for child_dir in &child_scopes {
            let code = run_lock(child_dir, check_mode, None, false, true, next_depth)?;
            if check_mode && code != 0 {
                return Ok(code);
            }
        }
    }

    // Lock this scope
    lock_scope(root, check_mode, manifest_flag, no_manifest)
}

fn lock_scope(
    root: &Path,
    check_mode: bool,
    manifest_flag: Option<&str>,
    no_manifest: bool,
) -> Result<i32> {
    let config = Config::load(root)?;
    let graph = build_graph(root, &config)?;

    // Resolve manifest: CLI flag > config > existing lockfile
    let manifest = if no_manifest {
        None
    } else if let Some(file) = manifest_flag {
        Some(derive_manifest(&graph, file)?)
    } else if let Some(ref file) = config.manifest {
        Some(derive_manifest(&graph, file)?)
    } else {
        // Preserve existing manifest (re-derive nodes from current graph)
        match read_lockfile(root)? {
            Some(existing) => match existing.manifest {
                Some(m) => Some(derive_manifest(&graph, &m.file)?),
                None => None,
            },
            None => None,
        }
    };

    let lockfile = Lockfile::from_graph(&graph, manifest);

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

fn run_graph(
    root: &Path,
    format: OutputFormat,
    recursive: bool,
    max_depth: Option<usize>,
) -> Result<i32> {
    let scope_root = find_scope_root(root);

    match format {
        OutputFormat::Dot => {
            let graphs = collect_jgf_graphs(&scope_root, ".", recursive, max_depth)?;
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
            let graphs = collect_jgf_graphs(&scope_root, ".", recursive, max_depth)?;

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

struct ScopeGraph {
    id: String,
    nodes: Vec<(String, graph::NodeType, Option<String>)>, // path, type, hash
    edges: Vec<(String, String, graph::EdgeType)>,
}

/// Collect JGF graph(s) from a scope and optionally its children.
fn collect_jgf_graphs(
    root: &Path,
    id: &str,
    recursive: bool,
    max_depth: Option<usize>,
) -> Result<Vec<ScopeGraph>> {
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
        .map(|e| (e.source.clone(), e.target.clone(), e.edge_type))
        .collect();

    let mut graphs = vec![ScopeGraph {
        id: id.to_string(),
        nodes,
        edges,
    }];

    if recursive && max_depth != Some(0) {
        let next_depth = max_depth.map(|d| d.saturating_sub(1));
        for child_scope in &g.child_scopes {
            let child_dir = root.join(child_scope.trim_end_matches('/'));
            let child_id = if id == "." {
                child_scope.trim_end_matches('/').to_string()
            } else {
                format!(
                    "{}/{}",
                    id.trim_end_matches('/'),
                    child_scope.trim_end_matches('/')
                )
            };
            let child_graphs = collect_jgf_graphs(&child_dir, &child_id, true, next_depth)?;
            graphs.extend(child_graphs);
        }
    }

    Ok(graphs)
}

fn run_impact(root: &Path, format: OutputFormat, files: &[String]) -> Result<i32> {
    let scope_root = find_scope_root(root);
    let config = Config::load(&scope_root)?;
    let graph = build_graph(&scope_root, &config)?;

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
    // Validate rule names (built-in + custom from config)
    let scope_root = find_scope_root(root);
    let root_config = Config::load(&scope_root)?;
    let available_rules = all_rules();
    let mut known_names: Vec<&str> = available_rules.iter().map(|r| r.name()).collect();
    let custom_names: Vec<String> = root_config.custom_rules.keys().cloned().collect();
    known_names.extend(custom_names.iter().map(|s| s.as_str()));
    for name in rule_filter {
        if !known_names.contains(&name.as_str()) {
            anyhow::bail!("unknown rule: \"{name}\"");
        }
    }

    // Scope resolution: walk up to find nearest ancestor with drft.lock
    let scope_root = find_scope_root(root);

    let mut diagnostics = check_scope(&scope_root, rule_filter, None, recursive, max_depth)?;

    diagnostics.sort_by(|a, b| {
        a.scope
            .cmp(&b.scope)
            .then_with(|| a.rule.cmp(&b.rule))
            .then_with(|| a.source.cmp(&b.source))
            .then_with(|| a.target.cmp(&b.target))
            .then_with(|| a.node.cmp(&b.node))
    });

    let colorize = use_color(color, format);
    let mut current_scope: Option<&Option<String>> = None;
    for d in &diagnostics {
        match format {
            OutputFormat::Text | OutputFormat::Dot => {
                // Print scope header when scope changes
                if current_scope != Some(&d.scope) {
                    if let Some(scope) = &d.scope {
                        if current_scope.is_some() {
                            println!();
                        }
                        if colorize {
                            println!("\x1b[1m[{scope}]\x1b[0m");
                        } else {
                            println!("[{scope}]");
                        }
                    }
                    current_scope = Some(&d.scope);
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

    let scope_root = find_scope_root(root);

    // Initial run
    print!("\x1b[2J\x1b[H"); // clear screen
    let _ = run_check(root, format, color, rule_filter, recursive, max_depth);
    eprintln!("\n\x1b[2m--- watching for changes (ctrl-c to stop) ---\x1b[0m");

    let (tx, rx) = mpsc::channel();
    let mut debouncer = new_debouncer(Duration::from_millis(500), tx)?;

    notify::Watcher::watch(
        debouncer.watcher(),
        &scope_root,
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

/// Find subdirectories that are lockable scopes (have drft.lock or drft.toml).
/// Returns absolute paths, sorted, shallowest first. Does not recurse past scope boundaries.
/// Respects .gitignore.
fn find_lockable_scopes(root: &Path) -> Result<Vec<PathBuf>> {
    let mut scopes = Vec::new();
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

        // Skip if inside an already-found scope
        let inside_existing = found.iter().any(|s| entry.path().starts_with(s));
        if inside_existing {
            continue;
        }

        let has_lock = entry.path().join("drft.lock").exists();
        let has_config = entry.path().join("drft.toml").exists();
        if has_lock || has_config {
            found.push(entry.path().to_path_buf());
            scopes.push(entry.path().to_path_buf());
        }
    }

    scopes.sort();
    Ok(scopes)
}

/// Walk up from `start` to find the nearest ancestor directory with `drft.lock`.
/// If none found, returns `start`.
fn find_scope_root(start: &Path) -> std::path::PathBuf {
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

/// Check a single scope and optionally recurse into child scopes.
fn check_scope(
    root: &Path,
    rule_filter: &[String],
    scope_prefix: Option<&str>,
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

        let mut findings = rule.evaluate(&graph, root);
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
            if scope_prefix.is_some() {
                d.scope = scope_prefix.map(|s| s.to_string());
            }
        }
        diagnostics.extend(findings);
    }

    // Run custom rules (respecting --rule filter)
    let run_custom = if rule_filter.is_empty() {
        !config.custom_rules.is_empty()
    } else {
        config
            .custom_rules
            .keys()
            .any(|name| rule_filter.iter().any(|f| f == name))
    };
    if run_custom {
        let mut custom_findings = rules::custom::run_custom_rules(&graph, root, &config);
        // Filter to only requested custom rules if --rule is set
        if !rule_filter.is_empty() {
            custom_findings.retain(|d| rule_filter.iter().any(|f| f == &d.rule));
        }
        // Apply ignore-rules filtering to custom rule diagnostics too
        custom_findings.retain(|d| {
            let paths: Vec<&str> = [d.source.as_deref(), d.target.as_deref(), d.node.as_deref()]
                .into_iter()
                .flatten()
                .collect();
            !paths.iter().any(|p| config.is_rule_ignored(&d.rule, p))
        });
        for d in &mut custom_findings {
            if scope_prefix.is_some() {
                d.scope = scope_prefix.map(|s| s.to_string());
            }
        }
        diagnostics.extend(custom_findings);
    }

    // Recursively check child scopes if --recursive
    if recursive && max_depth != Some(0) {
        let next_depth = max_depth.map(|d| d.saturating_sub(1));
        for child_scope in &graph.child_scopes {
            let child_dir = root.join(child_scope.trim_end_matches('/'));
            let child_prefix = match scope_prefix {
                Some(parent) => format!("{parent}/{}", child_scope.trim_end_matches('/')),
                None => child_scope.trim_end_matches('/').to_string(),
            };
            let child_diagnostics = check_scope(
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
