mod cli;

use drft::analyses;
use drft::compose;
use drft::config;
use drft::discovery;
use drft::graph;
use drft::graphs;
use drft::lock;
use drft::lockfile;
use drft::metrics;
use drft::parsers;
use drft::rules;

use anyhow::{Context, Result};
use clap::Parser;
use std::path::Path;

use cli::{Cli, ColorChoice, Commands, ConfigAction, OutputFormat};
use config::{Config, RuleSeverity};

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
            ConfigAction::Show => run_config_show(&root, cli.format),
        },
        Commands::Lock => run_lock(&root),
        Commands::Report { names } => run_report(&root, cli.format, names),
        Commands::Impact { files, parser } => {
            run_impact(&root, cli.format, files, parser.as_deref())
        }
        Commands::Parse { parser } => run_parse(&root, cli.format, parser.as_deref()),
        Commands::Graph { raw } => run_graph(&root, *raw),
        Commands::Check => run_check(&root, cli.format, cli.color),
    }
}

fn run_init(root: &Path) -> Result<i32> {
    let config_path = root.join("drft.toml");
    if config_path.exists() {
        anyhow::bail!("drft.toml already exists");
    }

    let content = r#"# drft.toml

# Which paths become nodes (default: ["**/*.md"])
include = ["**/*.md"]

# Remove from the graph (also respects .gitignore)
# exclude = []

[parsers.markdown]
# files = ["**/*.md"]   # uncomment to restrict (receives all included files by default)

# [parsers.frontmatter]
# files = ["**/*.md"]   # frontmatter link extraction + metadata

[rules]
# All rules default to warn. Override only what you need.
# stale = "error"  # recommended for LLM workflows and CI
"#;

    std::fs::write(&config_path, content)
        .with_context(|| format!("failed to write {}", config_path.display()))?;

    Ok(0)
}

fn run_config_show(root: &Path, format: OutputFormat) -> Result<i32> {
    let config = Config::load(root)?;
    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&config)?);
        }
        OutputFormat::Text => {
            let toml_str =
                toml::to_string_pretty(&config).context("failed to serialize config as TOML")?;
            print!("{}", toml_str);
        }
    }
    Ok(0)
}

/// Snapshot the composed graph into `drft.lock`: node content hashes and each
/// node's outbound edge target hashes.
fn run_lock(root: &Path) -> Result<i32> {
    let graph_root = find_graph_root(root);
    let config = Config::load(&graph_root)?;
    let set = graphs::build_set(&graph_root, &config)?;
    let composed = compose::compose(&set);
    lock::write(&graph_root, &lock::Lock::from_composed(&composed))?;
    Ok(0)
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

/// Emit the graph as JGF. The default composed view (`{"graph": {...}}`) merges
/// the raw per-graph set by path, nesting each graph's metadata under its
/// `@<graph>` namespace with `_graphs` provenance. `--raw` emits the unmerged
/// set (`{"graphs": [...]}`), JGF's multi-graph form.
fn run_graph(root: &Path, raw: bool) -> Result<i32> {
    let graph_root = find_graph_root(root);
    let config = Config::load(&graph_root)?;
    let set = graphs::build_set(&graph_root, &config)?;

    let json = if raw {
        serde_json::to_string_pretty(&set)?
    } else {
        serde_json::to_string_pretty(&compose::compose(&set).into_document())?
    };
    println!("{json}");
    Ok(0)
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
        if graph.nodes.get(file.as_str()).is_some_and(|n| n.included) {
            seeds.push(file.clone());
        } else {
            let with_ext = format!("{file}.md");
            if graph
                .nodes
                .get(with_ext.as_str())
                .is_some_and(|n| n.included)
            {
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

/// Check the composed graph against the lockfile, reporting drift and structural
/// findings. Errors exit 1, warnings exit 0.
fn run_check(root: &Path, format: OutputFormat, color: ColorChoice) -> Result<i32> {
    let graph_root = find_graph_root(root);
    let config = Config::load(&graph_root)?;
    let set = graphs::build_set(&graph_root, &config)?;
    let composed = compose::compose(&set);
    let lock = lock::read(&graph_root)?;

    let findings = rules::check::run(&composed, lock.as_ref(), &config);

    let colorize = use_color(color, format);
    match format {
        OutputFormat::Text => {
            for f in &findings {
                if colorize {
                    println!("{}", f.format_text_color());
                } else {
                    println!("{}", f.format_text());
                }
            }
        }
        OutputFormat::Json => {
            let errors = findings
                .iter()
                .filter(|f| f.severity == RuleSeverity::Error)
                .count();
            let warnings = findings
                .iter()
                .filter(|f| f.severity == RuleSeverity::Warn)
                .count();
            let output = serde_json::json!({
                "diagnostics": findings,
                "summary": { "errors": errors, "warnings": warnings },
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
    }

    let has_errors = findings.iter().any(|f| f.severity == RuleSeverity::Error);
    Ok(if has_errors { 1 } else { 0 })
}

/// Walk up from `start` to find the nearest ancestor directory with `drft.toml`.
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
