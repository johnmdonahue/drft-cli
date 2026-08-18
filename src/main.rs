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
        Commands::Lock { paths } => run_lock(&root, paths),
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
/// node's outbound edge target hashes. With no paths, lock the whole graph. With
/// paths, lock only those nodes (their bytes and outbound edge targets), merging
/// into the existing lockfile — and drop the entry for a path that is locked but
/// no longer in the graph, which is how a `removed-node` finding is reviewed and
/// cleared. Resolution is tolerant: paths that resolve are written, unresolved
/// paths are reported, and the command exits non-zero only if some path missed.
fn run_lock(root: &Path, paths: &[String]) -> Result<i32> {
    let graph_root = find_graph_root(root);
    let config = Config::load(&graph_root)?;
    let set = graphs::build_set(&graph_root, &config)?;
    let composed = compose::compose(&set);
    let snapshot = lock::Lock::from_composed(&composed);

    if paths.is_empty() {
        lock::write(&graph_root, &snapshot)?;
        return Ok(0);
    }

    let mut existing = lock::read(&graph_root)?.unwrap_or_default();
    let mut changed = false;
    let mut misses = Vec::new();

    for path in paths {
        match resolve_lock_target(&composed, &existing, root, &graph_root, path) {
            Some(LockTarget::Live(node)) => match snapshot.nodes.get(&node) {
                Some(entry) => {
                    existing.nodes.insert(node, entry.clone());
                    changed = true;
                }
                // A directory (or other hash-less, edge-less node) carries nothing
                // that can drift, so it is never a lock entry — nothing to lock.
                None => eprintln!("nothing to lock: {node} (no content to snapshot)"),
            },
            Some(LockTarget::Removed(node)) => {
                existing.nodes.remove(&node);
                changed = true;
                eprintln!("unlocked removed node: {node}");
            }
            None => misses.push(path.as_str()),
        }
    }

    if changed {
        lock::write(&graph_root, &existing)?;
    }

    for path in &misses {
        eprintln!("{}", not_found_message(&composed, Some(&existing), path));
    }
    Ok(if misses.is_empty() { 0 } else { 2 })
}

/// What a lock path resolves to: a node still in the graph (re-snapshot it), or a
/// node present only in the lockfile because its file is gone (drop it — the
/// reviewed-deletion case that clears a `removed-node` finding).
enum LockTarget {
    Live(String),
    Removed(String),
}

/// Candidate node keys for a user-supplied path, most-specific first.
///
/// Nodes are keyed by graph-root-relative path, but the argument is relative to
/// the current directory (`root`) like any other CLI path — so it is resolved
/// against `root`, normalized, and made relative to `graph_root` first. This
/// makes lookup cwd-agnostic: the same file resolves whether given
/// project-relative from a subdirectory or root-relative from the top. A `.md`
/// suffix is offered as a fallback, and the raw argument is tried last for back
/// compatibility.
fn node_candidates(root: &Path, graph_root: &Path, path: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    if let Some(key) = graph_key(root, graph_root, path) {
        candidates.push(format!("{key}.md"));
        candidates.push(key);
    }
    candidates.push(format!("{path}.md"));
    candidates.push(path.to_string());
    candidates
}

/// Resolve a user-supplied path to a node key in the composed graph. On a miss,
/// a node whose key ends with the argument is suggested — but only when the
/// suffix match is unambiguous (otherwise the candidates are listed, so an
/// arbitrary pick is never presented as "the" one).
fn resolve_node(
    composed: &drft::model::Graph,
    root: &Path,
    graph_root: &Path,
    path: &str,
) -> Result<String> {
    for candidate in node_candidates(root, graph_root, path) {
        if composed.nodes.contains_key(&candidate) {
            return Ok(candidate);
        }
    }
    anyhow::bail!("{}", not_found_message(composed, None, path))
}

/// Resolve a lock path to a live node or a removed one. A live node (still in the
/// graph) wins over a lockfile entry, so re-locking a tracked file updates it
/// rather than dropping it; a path present only in the lockfile resolves to
/// `Removed`. `None` means the path matched neither.
fn resolve_lock_target(
    composed: &drft::model::Graph,
    existing: &lock::Lock,
    root: &Path,
    graph_root: &Path,
    path: &str,
) -> Option<LockTarget> {
    let candidates = node_candidates(root, graph_root, path);
    for candidate in &candidates {
        if composed.nodes.contains_key(candidate) {
            return Some(LockTarget::Live(candidate.clone()));
        }
    }
    for candidate in &candidates {
        if existing.nodes.contains_key(candidate) {
            return Some(LockTarget::Removed(candidate.clone()));
        }
    }
    None
}

/// Build the "node not found" message. Suggests nodes whose key ends with the
/// given path (the common "right file, wrong prefix" miss), searching the graph
/// and — when provided — the lockfile, so a mistyped removed node still gets a
/// suggestion. Normalizes the needle to match the graph's forward-slash keys and
/// only presents a single suggestion when it's unambiguous.
fn not_found_message(
    composed: &drft::model::Graph,
    lock: Option<&lock::Lock>,
    path: &str,
) -> String {
    let needle = drft::util::normalize_relative_path(path);
    let suffix = format!("/{needle}");
    let mut matches: Vec<&String> = composed
        .nodes
        .keys()
        .chain(lock.into_iter().flat_map(|l| l.nodes.keys()))
        .filter(|k| k.as_str() == needle || k.ends_with(&suffix))
        .collect();
    matches.sort();
    matches.dedup();
    match matches.as_slice() {
        [] => format!("node not found: \"{path}\""),
        [hit] => format!("node not found: \"{path}\" — did you mean \"{hit}\"?"),
        many => {
            let shown = many
                .iter()
                .take(5)
                .map(|k| format!("\"{k}\""))
                .collect::<Vec<_>>();
            let more = if many.len() > shown.len() {
                format!(", and {} more", many.len() - shown.len())
            } else {
                String::new()
            };
            format!(
                "node not found: \"{path}\" — multiple matches: {}{more}",
                shown.join(", ")
            )
        }
    }
}

/// Resolve `arg` (relative to the current directory `root`, or absolute) to a
/// graph-root-relative node key. Normalization is lexical — `.`/`..` are
/// resolved without touching the filesystem, so symlink node identities are
/// preserved. Returns `None` when the path resolves outside `graph_root`.
fn graph_key(root: &Path, graph_root: &Path, arg: &str) -> Option<String> {
    let candidate = Path::new(arg);
    let abs = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        root.join(candidate)
    };
    let abs = drft::util::normalize_relative_path(&abs.to_string_lossy());
    let graph_root = drft::util::normalize_relative_path(&graph_root.to_string_lossy());
    let rel = Path::new(&abs).strip_prefix(&graph_root).ok()?;
    let key = rel.to_string_lossy().replace('\\', "/");
    (!key.is_empty()).then_some(key)
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

/// List nodes that transitively depend on `paths` (a structural query; `paths`
/// is required by the CLI).
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

    let seeds: Vec<String> = paths
        .iter()
        .map(|p| resolve_node(&composed, root, &graph_root, p))
        .collect::<Result<_>>()?;

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
                        i.location(),
                        i.via,
                        i.depth,
                        i.impact_radius
                    );
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
