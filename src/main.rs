mod cli;

use drft::compose;
use drft::config;
use drft::graphs;
use drft::impact;
use drft::lock;
use drft::rules;

use anyhow::{Context, Result};
use clap::Parser;
use std::path::Path;

use cli::{Cli, ColorChoice, Commands, Direction, OutputFormat};
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
    let code = match try_main() {
        Ok(code) => code,
        Err(e) => {
            // A parse/usage error happens before clap resolves `--format`, so
            // scan the raw args to decide whether to emit a JSON error envelope.
            if wants_json_output() {
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

/// Detect `--format json` (or `--format=json`) from the raw args, before clap
/// parses. Only the value of `--format` counts — a bare `json` elsewhere (e.g. a
/// directory named `json`) does not.
fn wants_json_output() -> bool {
    let mut args = std::env::args();
    while let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix("--format=") {
            return value == "json";
        }
        if arg == "--format" {
            return args.next().as_deref() == Some("json");
        }
    }
    false
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
        Commands::Lock { path } => run_lock(&root, path.as_deref()),
        Commands::Impact {
            paths,
            depth,
            direction,
        } => run_impact(&root, cli.format, paths, *depth, *direction),
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

# The graph root is this directory. fs walks every file under it, except
# .gitignore matches and these globs.
ignore = ["target/**", "node_modules/**"]

# Graphs. fs is implicit and always built. Declare the parsers you want — there
# are no defaults, so this file is the complete set.
[graphs.markdown]
parser = "markdown"
files = ["**/*.md"]

[graphs.frontmatter]
parser = "frontmatter"
files = ["**/*.md"]

[rules]
# Built-in rules default to warn. Promote for CI:
# stale-node = "error"
# stale-edge = "error"
"#;

    std::fs::write(&config_path, content)
        .with_context(|| format!("failed to write {}", config_path.display()))?;

    Ok(0)
}

/// Snapshot the composed graph into `drft.lock`: node content hashes and each
/// node's outbound edge target hashes. With a path, lock only that node (its
/// bytes and its outbound edge targets), merging into the existing lockfile.
fn run_lock(root: &Path, path: Option<&str>) -> Result<i32> {
    let graph_root = find_graph_root(root);
    let config = Config::load(&graph_root)?;
    let set = graphs::build_set(&graph_root, &config)?;
    let composed = compose::compose(&set);
    let snapshot = lock::Lock::from_composed(&composed);

    match path {
        None => lock::write(&graph_root, &snapshot)?,
        Some(path) => {
            let node = resolve_node(&composed, path)?;
            let mut existing = lock::read(&graph_root)?.unwrap_or_default();
            let entry = snapshot
                .nodes
                .get(&node)
                .expect("resolved node is in the snapshot")
                .clone();
            existing.nodes.insert(node, entry);
            lock::write(&graph_root, &existing)?;
        }
    }
    Ok(0)
}

/// Resolve a user-supplied path to a node in the composed graph, trying a
/// `.md` suffix as a fallback. Errors if no matching node exists.
fn resolve_node(composed: &drft::model::Graph, path: &str) -> Result<String> {
    if composed.nodes.contains_key(path) {
        return Ok(path.to_string());
    }
    let with_ext = format!("{path}.md");
    if composed.nodes.contains_key(&with_ext) {
        return Ok(with_ext);
    }
    anyhow::bail!("node not found: \"{path}\"")
}

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

/// List nodes transitively impacted by a change to `paths`. With no paths,
/// seeds are the stale source nodes derived from the lockfile.
fn run_impact(
    root: &Path,
    format: OutputFormat,
    paths: &[String],
    depth: Option<usize>,
    direction: Direction,
) -> Result<i32> {
    let graph_root = find_graph_root(root);
    let config = Config::load(&graph_root)?;
    let composed = compose::compose(&graphs::build_set(&graph_root, &config)?);

    let seeds: Vec<String> = if paths.is_empty() {
        stale_sources(&graph_root, &composed)?
    } else {
        paths
            .iter()
            .map(|p| resolve_node(&composed, p))
            .collect::<Result<_>>()?
    };

    let dir = match direction {
        Direction::Inbound => impact::Direction::Inbound,
        Direction::Outbound => impact::Direction::Outbound,
        Direction::Both => impact::Direction::Both,
    };
    let impacted = impact::compute(&composed, &seeds, dir, depth);

    match format {
        OutputFormat::Json => {
            let output = serde_json::json!({
                "seeds": seeds,
                "total": impacted.len(),
                "impacted": impacted,
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        OutputFormat::Text => {
            if impacted.is_empty() {
                println!("no dependents found");
            } else {
                for i in &impacted {
                    println!(
                        "{} (via {}, depth {}, radius {})",
                        i.node, i.via, i.depth, i.impact_radius
                    );
                }
            }
        }
    }

    Ok(0)
}

/// Stale source nodes: the subjects of the `stale-node` findings the staleness
/// rule derives against the lockfile — so impact's seeds and `drft check` share
/// one definition of "stale." Requires a lockfile.
fn stale_sources(graph_root: &Path, composed: &drft::model::Graph) -> Result<Vec<String>> {
    let lock = lock::read(graph_root)?.ok_or_else(|| {
        anyhow::anyhow!("no paths given and no drft.lock to derive stale sources from")
    })?;
    let seeds = rules::staleness::evaluate(composed, &lock)
        .into_iter()
        .filter(|f| f.name == "stale-node")
        .map(|f| f.subject)
        .collect();
    Ok(seeds)
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
