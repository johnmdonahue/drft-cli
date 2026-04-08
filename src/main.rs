mod cli;

use drft::analyses;
use drft::config;
use drft::diagnostic;
use drft::discovery;
use drft::graph;
use drft::lockfile;
use drft::metrics;
use drft::parsers;
use drft::rules;

use anyhow::{Context, Result};
use clap::Parser;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use cli::{Cli, ColorChoice, Commands, ConfigAction, OutputFormat};
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
        Commands::Init => run_init(&root),
        Commands::Config { action } => match action {
            ConfigAction::Show {
                recursive,
                max_depth,
            } => run_config_show(&root, cli.format, *recursive, *max_depth),
        },
        Commands::Lock {
            check,
            recursive,
            max_depth,
        } => run_lock(&root, *check, *recursive, *max_depth),
        Commands::Report { names } => run_report(&root, cli.format, names),
        Commands::Impact { files, parser } => {
            run_impact(&root, cli.format, files, parser.as_deref())
        }
        Commands::Parse { parser } => run_parse(&root, cli.format, parser.as_deref()),
        Commands::Graph {
            recursive,
            max_depth,
            dot,
            parser,
        } => run_graph(
            &root,
            cli.format,
            *recursive,
            *max_depth,
            *dot,
            parser.as_deref(),
        ),
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

# Which paths become File nodes (default: ["*.md"])
include = ["*.md"]

# Remove from the graph (also respects .gitignore)
# exclude = []

[parsers.markdown]
# files = ["*.md"]   # uncomment to restrict (receives all included files by default)

# [parsers.frontmatter]
# files = ["*.md"]   # frontmatter link extraction + metadata

[rules]
# All rules default to warn. Override only what you need.
# stale = "error"  # recommended for LLM workflows and CI

# [interface]
# files = ["overview.md", "api/*.md"]
"#;

    std::fs::write(&config_path, content)
        .with_context(|| format!("failed to write {}", config_path.display()))?;

    Ok(0)
}

fn run_config_show(
    root: &Path,
    format: OutputFormat,
    recursive: bool,
    max_depth: Option<usize>,
) -> Result<i32> {
    if recursive {
        match format {
            OutputFormat::Json => {
                let mut entries = Vec::new();
                collect_configs_recursive(root, ".", max_depth, &mut entries)?;
                println!("{}", serde_json::to_string_pretty(&entries)?);
            }
            OutputFormat::Text => {
                print_configs_recursive(root, ".", max_depth, true)?;
            }
        }
    } else {
        let config = Config::load(root)?;
        match format {
            OutputFormat::Json => {
                println!("{}", serde_json::to_string_pretty(&config)?);
            }
            OutputFormat::Text => {
                let toml_str = toml::to_string_pretty(&config)
                    .context("failed to serialize config as TOML")?;
                print!("{}", toml_str);
            }
        }
    }
    Ok(0)
}

fn normalize_label(root: &Path, child_dir: &Path) -> String {
    child_dir
        .strip_prefix(root)
        .unwrap_or(child_dir)
        .to_string_lossy()
        .replace('\\', "/")
}

fn collect_configs_recursive(
    root: &Path,
    label: &str,
    max_depth: Option<usize>,
    entries: &mut Vec<serde_json::Value>,
) -> Result<()> {
    let config = Config::load(root)?;
    let mut value = serde_json::to_value(&config)?;
    if let serde_json::Value::Object(ref mut map) = value {
        map.insert("path".to_string(), serde_json::json!(label));
    }
    entries.push(value);

    if max_depth == Some(0) {
        return Ok(());
    }

    let next_depth = max_depth.map(|d| d.saturating_sub(1));
    let child_graphs = find_lockable_graphs(root)?;
    for child_dir in &child_graphs {
        let relative = normalize_label(root, child_dir);
        let child_label = if label == "." {
            relative
        } else {
            format!("{label}/{relative}")
        };
        collect_configs_recursive(child_dir, &child_label, next_depth, entries)?;
    }
    Ok(())
}

fn print_configs_recursive(
    root: &Path,
    label: &str,
    max_depth: Option<usize>,
    first: bool,
) -> Result<()> {
    let config = Config::load(root)?;
    if !first {
        println!();
    }
    println!("# {}", label);
    let toml_str = toml::to_string_pretty(&config).context("failed to serialize config as TOML")?;
    print!("{}", toml_str);

    if max_depth == Some(0) {
        return Ok(());
    }

    let next_depth = max_depth.map(|d| d.saturating_sub(1));
    let child_graphs = find_lockable_graphs(root)?;
    for child_dir in &child_graphs {
        let relative = normalize_label(root, child_dir);
        let child_label = if label == "." {
            relative
        } else {
            format!("{label}/{relative}")
        };
        print_configs_recursive(child_dir, &child_label, next_depth, false)?;
    }
    Ok(())
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

fn run_report(root: &Path, format: OutputFormat, filter: &[String]) -> Result<i32> {
    // Validate filter names
    let analysis_names = analyses::all_analysis_names();
    let metric_names = metrics::all_metric_names();
    for name in filter {
        if !analysis_names.contains(&name.as_str()) && !metric_names.contains(&name.as_str()) {
            let mut valid: Vec<&str> = analysis_names.to_vec();
            valid.extend_from_slice(metric_names);
            valid.sort();
            anyhow::bail!(
                "unknown report name \"{name}\"\n\nValid names:\n  {}",
                valid.join("\n  ")
            );
        }
    }

    let config = Config::load(root)?;
    let lockfile = lockfile::read_lockfile(root)?;
    let enriched = analyses::enrich(root, &config, lockfile.as_ref())?;

    // Determine which analyses and metrics to output
    let show = |name: &str| filter.is_empty() || filter.iter().any(|f| f == name);
    let want_any_metrics =
        filter.is_empty() || filter.iter().any(|f| metric_names.contains(&f.as_str()));

    // Serialize requested analyses from enriched graph
    let all_analyses: Vec<(&str, serde_json::Value)> = vec![
        ("betweenness", serde_json::to_value(&enriched.betweenness)?),
        ("bridges", serde_json::to_value(&enriched.bridges)?),
        (
            "change-propagation",
            serde_json::to_value(&enriched.change_propagation)?,
        ),
        (
            "connected-components",
            serde_json::to_value(&enriched.connected_components)?,
        ),
        ("degree", serde_json::to_value(&enriched.degree)?),
        ("depth", serde_json::to_value(&enriched.depth)?),
        (
            "graph-boundaries",
            serde_json::to_value(&enriched.graph_boundaries)?,
        ),
        ("graph-stats", serde_json::to_value(&enriched.graph_stats)?),
        (
            "impact-radius",
            serde_json::to_value(&enriched.impact_radius)?,
        ),
        ("pagerank", serde_json::to_value(&enriched.pagerank)?),
        ("scc", serde_json::to_value(&enriched.scc)?),
        (
            "transitive-reduction",
            serde_json::to_value(&enriched.transitive_reduction)?,
        ),
    ];

    let output_analyses: Vec<_> = all_analyses
        .into_iter()
        .filter(|(name, _)| show(name))
        .collect();

    // Compute metrics from enriched graph
    let output_metrics: Vec<_> = if want_any_metrics {
        let inputs = metrics::AnalysisInputs {
            degree: &enriched.degree,
            scc: &enriched.scc,
            connected_components: &enriched.connected_components,
            graph_stats: &enriched.graph_stats,
            bridges: &enriched.bridges,
            transitive_reduction: &enriched.transitive_reduction,
            change_propagation: &enriched.change_propagation,
            pagerank: &enriched.pagerank,
        };
        metrics::compute_metrics(&inputs, &enriched.graph)
            .into_iter()
            .filter(|m| show(&m.name))
            .collect()
    } else {
        Vec::new()
    };

    // Find requested names that produced no output (conditional metrics
    // like diameter or freshness may not be available for every graph).
    let missing_names: Vec<&str> = if filter.is_empty() {
        Vec::new()
    } else {
        let produced: std::collections::HashSet<&str> = output_analyses
            .iter()
            .map(|(name, _)| *name)
            .chain(output_metrics.iter().map(|m| m.name.as_str()))
            .collect();
        filter
            .iter()
            .map(|s| s.as_str())
            .filter(|name| !produced.contains(name))
            .collect()
    };

    if !missing_names.is_empty() {
        eprintln!(
            "note: no results for {} \u{2014} some metrics are only available when the graph meets certain conditions (e.g. lockfile present, graph connected)",
            missing_names.join(", ")
        );
    }

    match format {
        OutputFormat::Json => {
            let mut map = serde_json::Map::new();
            for (name, value) in &output_analyses {
                map.insert(name.to_string(), value.clone());
            }
            for m in &output_metrics {
                map.insert(
                    m.name.clone(),
                    serde_json::json!({
                        "value": m.value,
                        "kind": m.kind,
                        "dimension": m.dimension,
                    }),
                );
            }
            for name in &missing_names {
                map.insert(name.to_string(), serde_json::Value::Null);
            }
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::Value::Object(map))?
            );
        }
        OutputFormat::Text => {
            for (name, result) in &output_analyses {
                println!("=== {name} ===");
                println!("{}", serde_json::to_string_pretty(result)?);
                println!();
            }
            for m in &output_metrics {
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

fn run_parse(root: &Path, format: OutputFormat, parser_filter: Option<&str>) -> Result<i32> {
    let graph_root = find_graph_root(root);
    let config = Config::load(&graph_root)?;

    // Validate --parser filter
    if let Some(name) = parser_filter
        && !config.parsers.contains_key(name)
    {
        anyhow::bail!(
            "unknown parser \"{name}\" (available: {})",
            config
                .parsers
                .keys()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    // 1. Discover files
    let included_files = discovery::discover(&graph_root, &config.include, &config.exclude)?;

    // 2. Read text content (binary files skipped)
    let mut file_text: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for file in &included_files {
        if let Ok(text) = std::fs::read_to_string(graph_root.join(file)) {
            file_text.insert(file.clone(), text);
        }
    }

    // 3. Build parser registry, apply --parser filter
    let parser_list =
        parsers::build_parsers(&config.parsers, config.config_dir.as_deref(), &graph_root);

    // 4. Route files to parsers and run
    let mut edge_results: Vec<serde_json::Value> = Vec::new();
    let mut metadata_results: Vec<serde_json::Value> = Vec::new();

    for parser in &parser_list {
        if let Some(name) = parser_filter
            && parser.name() != name
        {
            continue;
        }

        let files: Vec<(&str, &str)> = included_files
            .iter()
            .filter(|f| parser.matches(f))
            .filter_map(|f| {
                file_text
                    .get(f)
                    .map(|content| (f.as_str(), content.as_str()))
            })
            .collect();

        if files.is_empty() {
            continue;
        }

        let batch_results = parser.parse_batch(&files);

        for (file, result) in batch_results {
            for link in result.links {
                edge_results.push(serde_json::json!({
                    "parser": parser.name(),
                    "file": file,
                    "link": link,
                }));
            }
            if let Some(metadata) = result.metadata {
                metadata_results.push(serde_json::json!({
                    "parser": parser.name(),
                    "file": file,
                    "metadata": metadata,
                }));
            }
        }
    }

    // Sort for deterministic output
    edge_results.sort_by(|a, b| {
        a["parser"]
            .as_str()
            .cmp(&b["parser"].as_str())
            .then_with(|| a["file"].as_str().cmp(&b["file"].as_str()))
            .then_with(|| a["link"].as_str().cmp(&b["link"].as_str()))
    });
    metadata_results.sort_by(|a, b| {
        a["parser"]
            .as_str()
            .cmp(&b["parser"].as_str())
            .then_with(|| a["file"].as_str().cmp(&b["file"].as_str()))
    });

    match format {
        OutputFormat::Json => {
            let mut output = serde_json::json!({
                "edges": edge_results,
                "total": edge_results.len(),
            });
            if !metadata_results.is_empty() {
                output["metadata"] = serde_json::json!(metadata_results);
            }
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        _ => {
            if edge_results.is_empty() && metadata_results.is_empty() {
                println!("no edges found");
            } else {
                for edge in &edge_results {
                    let parser = edge["parser"].as_str().unwrap_or("");
                    let file = edge["file"].as_str().unwrap_or("");
                    let reference = edge["link"].as_str().unwrap_or("");
                    println!("{file} → {reference} ({parser})");
                }
                for meta in &metadata_results {
                    let parser = meta["parser"].as_str().unwrap_or("");
                    let file = meta["file"].as_str().unwrap_or("");
                    println!("{file} [metadata:{parser}]");
                }
                eprintln!(
                    "\n{} edges, {} metadata",
                    edge_results.len(),
                    metadata_results.len()
                );
            }
        }
    }

    Ok(0)
}

fn run_graph(
    root: &Path,
    _format: OutputFormat,
    recursive: bool,
    max_depth: Option<usize>,
    dot: bool,
    parser: Option<&str>,
) -> Result<i32> {
    let graph_root = find_graph_root(root);

    if dot {
        let graphs = collect_jgf_graphs(&graph_root, ".", recursive, max_depth, parser)?;
        println!("digraph {{");
        let mut all_nodes = Vec::new();
        let mut all_edges = Vec::new();
        for g in &graphs {
            let prefix = if g.id == "." { "" } else { g.id.as_str() };
            for NodeExport { path, .. } in &g.nodes {
                let full = if prefix.is_empty() {
                    path.clone()
                } else {
                    format!("{prefix}{path}")
                };
                all_nodes.push(full);
            }
            for (source, target, _, _) in &g.edges {
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
    } else {
        // JSON Graph Format (JGF)
        let graphs = collect_jgf_graphs(&graph_root, ".", recursive, max_depth, parser)?;

        let jgf_graphs: Vec<serde_json::Value> = graphs
            .iter()
            .map(|g| {
                let mut nodes = serde_json::Map::new();
                let mut sorted: Vec<&NodeExport> = g.nodes.iter().collect();
                sorted.sort_by(|a, b| a.path.cmp(&b.path));
                // Build a lookup for included status to compute edge.internal
                let included: std::collections::HashSet<&str> = g
                    .nodes
                    .iter()
                    .filter(|n| n.included)
                    .map(|n| n.path.as_str())
                    .collect();
                for n in &*sorted {
                    let mut meta = serde_json::Map::new();
                    meta.insert("type".into(), serde_json::json!(n.node_type));
                    if let Some(h) = &n.hash {
                        meta.insert("hash".into(), serde_json::json!(h));
                    }
                    if let Some(gr) = &n.graph {
                        meta.insert("graph".into(), serde_json::json!(gr));
                    }
                    if n.is_graph {
                        meta.insert("is_graph".into(), serde_json::json!(true));
                    }
                    meta.insert("included".into(), serde_json::json!(n.included));
                    let mut metadata_keys: Vec<&String> = n.metadata.keys().collect();
                    metadata_keys.sort();
                    for key in metadata_keys {
                        meta.insert(key.clone(), n.metadata[key].clone());
                    }
                    nodes.insert(n.path.clone(), serde_json::json!({ "metadata": meta }));
                }

                let mut edges: Vec<serde_json::Value> = g
                    .edges
                    .iter()
                    .map(|(source, target, reference, parser)| {
                        let internal = included.contains(source.as_str())
                            && included.contains(target.as_str());
                        let mut meta = serde_json::Map::new();
                        meta.insert("parser".into(), serde_json::json!(parser));
                        meta.insert("internal".into(), serde_json::json!(internal));
                        if let Some(r) = reference {
                            meta.insert("link".into(), serde_json::json!(r));
                        }
                        serde_json::json!({
                            "source": source,
                            "target": target,
                            "metadata": meta,
                        })
                    })
                    .collect();
                edges.sort_by(|a, b| {
                    a["source"]
                        .as_str()
                        .cmp(&b["source"].as_str())
                        .then_with(|| a["target"].as_str().cmp(&b["target"].as_str()))
                });

                let mut graph_meta = serde_json::Map::new();
                if !g.interface.is_empty() {
                    graph_meta.insert("interface".into(), serde_json::json!(g.interface));
                }
                if !g.target_properties.is_empty() {
                    let sorted: std::collections::BTreeMap<&String, &graph::TargetProperties> =
                        g.target_properties.iter().collect();
                    graph_meta.insert("target_properties".into(), serde_json::json!(sorted));
                }

                let mut graph_obj = serde_json::Map::new();
                graph_obj.insert("id".into(), serde_json::json!(g.id));
                graph_obj.insert("directed".into(), serde_json::json!(true));
                if !graph_meta.is_empty() {
                    graph_obj.insert("metadata".into(), serde_json::Value::Object(graph_meta));
                }
                graph_obj.insert("nodes".into(), serde_json::json!(nodes));
                graph_obj.insert("edges".into(), serde_json::json!(edges));
                serde_json::Value::Object(graph_obj)
            })
            .collect();

        let output = if jgf_graphs.len() == 1 {
            serde_json::json!({ "graph": jgf_graphs[0] })
        } else {
            serde_json::json!({ "graphs": jgf_graphs })
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    }

    Ok(0)
}

struct NodeExport {
    path: String,
    node_type: graph::NodeType,
    hash: Option<String>,
    graph: Option<String>,
    is_graph: bool,
    included: bool,
    metadata: HashMap<String, serde_json::Value>,
}

struct GraphExport {
    id: String,
    nodes: Vec<NodeExport>,
    edges: Vec<(String, String, Option<String>, String)>, // source, target (node ID), reference, parser
    interface: Vec<String>,
    target_properties: HashMap<String, graph::TargetProperties>,
}

/// Collect JGF graph(s) from a graph root and optionally its children.
fn collect_jgf_graphs(
    root: &Path,
    id: &str,
    recursive: bool,
    max_depth: Option<usize>,
    parser: Option<&str>,
) -> Result<Vec<GraphExport>> {
    let config = Config::load(root)?;
    let g = build_graph(root, &config)?;
    let g = if let Some(name) = parser {
        if !config.parsers.contains_key(name) {
            let mut available: Vec<&str> = config.parsers.keys().map(|s| s.as_str()).collect();
            available.sort();
            anyhow::bail!(
                "unknown parser \"{name}\" (available: {})",
                available.join(", ")
            );
        }
        g.filter_by_parsers(&[name.to_string()])
    } else {
        g
    };

    let nodes: Vec<NodeExport> = g
        .nodes
        .iter()
        .map(|(path, node)| NodeExport {
            path: path.clone(),
            node_type: node.node_type,
            hash: node.hash.clone(),
            graph: node.graph.clone(),
            is_graph: node.is_graph,
            included: node.included,
            metadata: node.metadata.clone(),
        })
        .collect();

    let edges: Vec<(String, String, Option<String>, String)> = g
        .edges
        .iter()
        .filter(|e| g.nodes.contains_key(&e.target))
        .map(|e| {
            (
                e.source.clone(),
                e.target.clone(),
                e.link.clone(),
                e.parser.clone(),
            )
        })
        .collect();

    let interface = g.interface.clone();
    let target_properties = g.target_properties.clone();

    let mut graphs = vec![GraphExport {
        id: id.to_string(),
        nodes,
        edges,
        interface,
        target_properties,
    }];

    if recursive && max_depth != Some(0) {
        let next_depth = max_depth.map(|d| d.saturating_sub(1));
        for child_graph in &g.child_graphs {
            let child_dir = root.join(child_graph);
            let child_id = if id == "." {
                child_graph.clone()
            } else {
                format!("{id}/{child_graph}")
            };
            let sub_graphs = collect_jgf_graphs(&child_dir, &child_id, true, next_depth, parser)?;
            graphs.extend(sub_graphs);
        }
    }

    Ok(graphs)
}

fn run_impact(
    root: &Path,
    format: OutputFormat,
    files: &[String],
    parser: Option<&str>,
) -> Result<i32> {
    let graph_root = find_graph_root(root);
    let config = Config::load(&graph_root)?;
    let lockfile = lockfile::read_lockfile(&graph_root)?;
    let graph = graph::build_graph(&graph_root, &config)?;
    let graph = if let Some(name) = parser {
        if !config.parsers.contains_key(name) {
            let mut available: Vec<&str> = config.parsers.keys().map(|s| s.as_str()).collect();
            available.sort();
            anyhow::bail!(
                "unknown parser \"{name}\" (available: {})",
                available.join(", ")
            );
        }
        graph.filter_by_parsers(&[name.to_string()])
    } else {
        graph
    };
    let enriched = analyses::enrich_graph(graph, &graph_root, &config, lockfile.as_ref());
    let graph = &enriched.graph;

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

    // BFS: walk reverse edges to find all transitive dependents, tracking depth
    let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut queue: std::collections::VecDeque<(String, usize)> = std::collections::VecDeque::new();

    for seed in &seeds {
        visited.insert(seed.clone());
        queue.push_back((seed.clone(), 0));
    }

    // (node, via, depth)
    let mut impacted: Vec<(String, String, usize)> = Vec::new();

    while let Some((node, depth)) = queue.pop_front() {
        if let Some(edge_indices) = graph.reverse.get(node.as_str()) {
            for &idx in edge_indices {
                let dependent = &graph.edges[idx].source;
                if !visited.contains(dependent.as_str()) {
                    visited.insert(dependent.clone());
                    let next_depth = depth + 1;
                    impacted.push((dependent.clone(), node.clone(), next_depth));
                    queue.push_back((dependent.clone(), next_depth));
                }
            }
        }
    }

    // Build lookup maps for enrichment data
    let radius_map: std::collections::HashMap<
        &str,
        &drft::analyses::impact_radius::ImpactRadiusNode,
    > = enriched
        .impact_radius
        .nodes
        .iter()
        .map(|n| (n.node.as_str(), n))
        .collect();
    let betweenness_map: std::collections::HashMap<&str, f64> = enriched
        .betweenness
        .nodes
        .iter()
        .map(|n| (n.node.as_str(), n.score))
        .collect();

    // Sort by review priority: high-radius nodes at shallow depth first
    impacted.sort_by(|a, b| {
        let a_radius = radius_map.get(a.0.as_str()).map(|n| n.radius).unwrap_or(0);
        let b_radius = radius_map.get(b.0.as_str()).map(|n| n.radius).unwrap_or(0);
        let a_betweenness = betweenness_map.get(a.0.as_str()).copied().unwrap_or(0.0);
        let b_betweenness = betweenness_map.get(b.0.as_str()).copied().unwrap_or(0.0);

        // Priority: impact_radius / depth + betweenness (higher = more important = sorts first)
        let a_priority = (a_radius as f64) / (a.2 as f64) + a_betweenness;
        let b_priority = (b_radius as f64) / (b.2 as f64) + b_betweenness;
        b_priority
            .partial_cmp(&a_priority)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });

    match format {
        OutputFormat::Json => {
            let output = serde_json::json!({
                "files": seeds,
                "total": impacted.len(),
                "impacted": impacted.iter().map(|(node, via, depth)| {
                    let radius = radius_map.get(node.as_str()).map(|n| n.radius).unwrap_or(0);
                    let betweenness = betweenness_map.get(node.as_str()).copied().unwrap_or(0.0);
                    serde_json::json!({
                        "node": node,
                        "via": via,
                        "depth": depth,
                        "impact_radius": radius,
                        "betweenness": betweenness,
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
                for (node, via, depth) in &impacted {
                    let radius = radius_map.get(node.as_str()).map(|n| n.radius).unwrap_or(0);
                    println!("{node} (via {via}, depth {depth}, radius {radius})");
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
    // Validate rule names (built-in + custom rules from config)
    let graph_root = find_graph_root(root);
    let root_config = Config::load(&graph_root)?;
    let available_rules = all_rules();
    let mut known_names: Vec<&str> = available_rules.iter().map(|r| r.name()).collect();
    let custom_names: Vec<String> = root_config
        .custom_rules()
        .map(|(name, _)| name.to_string())
        .collect();
    known_names.extend(custom_names.iter().map(|s| s.as_str()));
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
            OutputFormat::Text => {
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

/// Find subdirectories that are lockable graphs (have drft.toml).
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

        if entry.path().join("drft.toml").exists() {
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
        if current.join("drft.toml").exists() {
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
    let lockfile = lockfile::read_lockfile(root)?;
    let base_graph = graph::build_graph(root, &config)?;

    // Collect distinct parser filter sets needed by rules that will actually run
    // (respects both --rule filter and severity=off).
    let mut parser_sets: Vec<Vec<String>> = Vec::new();
    for rule in all_rules() {
        if !rule_filter.is_empty() && !rule_filter.iter().any(|f| f == rule.name()) {
            continue;
        }
        if rule_filter.is_empty() && config.rule_severity(rule.name()) == RuleSeverity::Off {
            continue;
        }
        let parsers = config.rule_parsers(rule.name()).to_vec();
        if !parsers.is_empty() && !parser_sets.contains(&parsers) {
            parser_sets.push(parsers);
        }
    }
    // Also collect from custom rules
    for (name, rule_config) in config.custom_rules() {
        if !rule_filter.is_empty() && !rule_filter.iter().any(|f| f == name) {
            continue;
        }
        if rule_filter.is_empty() && rule_config.severity == RuleSeverity::Off {
            continue;
        }
        let parsers = rule_config.parsers.clone();
        if !parsers.is_empty() && !parser_sets.contains(&parsers) {
            parser_sets.push(parsers);
        }
    }

    // Build filtered enriched graphs BEFORE consuming the base graph.
    let mut filtered_cache: std::collections::HashMap<Vec<String>, analyses::EnrichedGraph> =
        std::collections::HashMap::new();
    for parser_set in &parser_sets {
        let filtered = base_graph.filter_by_parsers(parser_set);
        let enriched = analyses::enrich_graph(filtered, root, &config, lockfile.as_ref());
        filtered_cache.insert(parser_set.clone(), enriched);
    }

    // Now consume the base graph for the unfiltered enriched graph.
    let base_enriched = analyses::enrich_graph(base_graph, root, &config, lockfile.as_ref());

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

        let rule_parsers = config.rule_parsers(rule.name());
        let enriched = if rule_parsers.is_empty() {
            &base_enriched
        } else {
            filtered_cache.get(rule_parsers).unwrap()
        };

        let rule_ctx = rules::RuleContext {
            graph: enriched,
            options: config.rule_options(rule.name()),
        };
        let mut findings = rule.evaluate(&rule_ctx);
        findings.retain(|d| {
            let paths: Vec<&str> = [d.source.as_deref(), d.target.as_deref(), d.node.as_deref()]
                .into_iter()
                .flatten()
                .collect();
            // files: scope which nodes the rule evaluates (all paths must be in scope)
            let in_scope = paths.is_empty()
                || paths
                    .iter()
                    .any(|p| config.is_rule_in_scope(rule.name(), p));
            // ignore: exclude specific nodes from diagnostics
            let ignored = paths.iter().any(|p| config.is_rule_ignored(rule.name(), p));
            in_scope && !ignored
        });
        for d in &mut findings {
            d.severity = severity;
            if graph_prefix.is_some() {
                d.graph = graph_prefix.map(|s| s.to_string());
            }
        }
        diagnostics.extend(findings);
    }

    // Run custom rules individually (respecting --rule filter and per-rule parser scoping)
    let config_dir = config.config_dir.as_deref().unwrap_or(root);
    for (rule_name, rule_config) in config.custom_rules() {
        if !rule_filter.is_empty() && !rule_filter.iter().any(|f| f == rule_name) {
            continue;
        }
        let enriched = if rule_config.parsers.is_empty() {
            &base_enriched
        } else {
            filtered_cache.get(&rule_config.parsers).unwrap()
        };
        match rules::custom::run_one(rule_name, rule_config, enriched, root, config_dir) {
            Ok(mut findings) => {
                // Apply per-rule files/ignore filtering
                findings.retain(|d| {
                    let paths: Vec<&str> =
                        [d.source.as_deref(), d.target.as_deref(), d.node.as_deref()]
                            .into_iter()
                            .flatten()
                            .collect();
                    let in_scope = paths.is_empty()
                        || paths.iter().any(|p| config.is_rule_in_scope(rule_name, p));
                    let ignored = paths.iter().any(|p| config.is_rule_ignored(rule_name, p));
                    in_scope && !ignored
                });
                for d in &mut findings {
                    if graph_prefix.is_some() {
                        d.graph = graph_prefix.map(|s| s.to_string());
                    }
                }
                diagnostics.extend(findings);
            }
            Err(e) => {
                eprintln!("warn: custom rule \"{rule_name}\" failed: {e}");
                diagnostics.push(Diagnostic {
                    rule: rule_name.to_string(),
                    severity: rule_config.severity,
                    message: format!("custom rule failed: {e}"),
                    fix: Some(format!(
                        "custom rule \"{rule_name}\" failed to execute — check the command path and script"
                    )),
                    graph: graph_prefix.map(|s| s.to_string()),
                    ..Default::default()
                });
            }
        }
    }

    // Recursively check child graphs if --recursive
    if recursive && max_depth != Some(0) {
        let next_depth = max_depth.map(|d| d.saturating_sub(1));
        for child_graph in &base_enriched.graph.child_graphs {
            let child_dir = root.join(child_graph);
            let child_prefix = match graph_prefix {
                Some(parent) => format!("{parent}/{child_graph}"),
                None => child_graph.clone(),
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
