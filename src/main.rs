mod cli;

use drft::compose;
use drft::config;
use drft::edges;
use drft::graphs;
use drft::impact;
use drft::lock;
use drft::nodes;
use drft::rules;

use anyhow::{Context, Result};
use clap::Parser;
use std::path::Path;

use cli::{Cli, ColorChoice, Commands, Depth, Direction, OutputFormat};
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
        Commands::Nodes {
            selectors,
            namespaces,
            fields,
        } => run_nodes(&root, cli.format, selectors, namespaces, fields),
        Commands::Edges {
            selectors,
            namespaces,
            fields,
        } => run_edges(&root, cli.format, selectors, namespaces, fields),
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

# Frontmatter link values resolve relative to the declaring file, the same way
# that file's markdown links do — from docs/guide.md, write ../src/lib.rs, not
# src/lib.rs. Add keys = ["sources"] to limit edges to named keys instead of
# every path-shaped value.
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
/// node's outbound edge target hashes. With paths, lock only those nodes (their
/// bytes and their outbound edge targets), merging into the existing lockfile.
///
/// A lock is an assertion that the locked state was reviewed, so the scoped form
/// exists to make that assertion narrow enough to be true — lock what you read,
/// not whatever happens to be stale.
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

    // Resolve every path before writing any of them. A typo in the third of five
    // must not leave the first two locked: a partial lock claims some files were
    // reviewed and drops the rest without saying so, which is worse than failing.
    // A path resolves against the graph or, when its file is gone, against the
    // lockfile — so a deleted node can be named to clear its `removed-node` finding.
    let nodes = paths
        .iter()
        .map(|p| resolve_lock_node(&composed, &existing, root, &graph_root, p))
        .collect::<Result<Vec<_>>>()?;

    // Make the lockfile reflect each named path's current state: re-snapshot a node
    // that carries content, and drop the entry for anything that no longer does —
    // a deleted file, or a path that has become a hash-less directory. Dropping the
    // entry for a reviewed deletion is how a `removed-node` finding is cleared.
    for node in nodes {
        match snapshot.nodes.get(&node) {
            Some(entry) => {
                existing.nodes.insert(node, entry.clone());
            }
            None => {
                existing.nodes.remove(&node);
            }
        }
    }
    lock::write(&graph_root, &existing)?;
    Ok(0)
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
    // The `.md` fallback is for a bare doc name (`guide` → `guide.md`). An argument
    // that already carries an extension names a specific file, so appending `.md`
    // would only invent a bogus `guide.md.md` candidate — and, pushed first, try it
    // ahead of the exact key.
    let add_md = Path::new(path).extension().is_none();
    let mut candidates = Vec::new();
    if let Some(key) = graph_key(root, graph_root, path) {
        if add_md {
            candidates.push(format!("{key}.md"));
        }
        candidates.push(key);
    }
    if add_md {
        candidates.push(format!("{path}.md"));
    }
    candidates.push(path.to_string());
    candidates
}

/// Build a "node not found" error, suggesting a key that ends with the given path
/// (the common "right file, wrong prefix" miss). The needle is normalized to match
/// the graph's forward-slash keys, and a single suggestion is only offered when the
/// suffix match is unambiguous — otherwise the candidates are listed, so an
/// arbitrary pick is never presented as "the" one.
fn not_found_error<'a>(keys: impl Iterator<Item = &'a String>, path: &str) -> anyhow::Error {
    let needle = drft::util::normalize_relative_path(path);
    let suffix = format!("/{needle}");
    let mut matches: Vec<&String> = keys
        .filter(|k| k.as_str() == needle || k.ends_with(&suffix))
        .collect();
    matches.sort();
    matches.dedup();
    match matches.as_slice() {
        [] => anyhow::anyhow!("node not found: \"{path}\""),
        [hit] => anyhow::anyhow!("node not found: \"{path}\" — did you mean \"{hit}\"?"),
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
            anyhow::anyhow!(
                "node not found: \"{path}\" — multiple matches: {}{more}",
                shown.join(", ")
            )
        }
    }
}

/// Resolve a user-supplied path to a node key in the composed graph.
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
    Err(not_found_error(composed.nodes.keys(), path))
}

/// Resolve a lock path to a node key present in the graph or, when its file is
/// gone, in the existing lockfile. A live node re-snapshots; a key that survives
/// only in the lock is a deletion under review, and naming it drops the entry.
/// This is what lets a `removed-node` finding be cleared by locking the vanished
/// path — the reviewed-deletion case the graph alone cannot resolve. Suggestions
/// on a miss search both the graph and the lockfile, so a mistyped deleted path
/// still gets one.
fn resolve_lock_node(
    composed: &drft::model::Graph,
    existing: &lock::Lock,
    root: &Path,
    graph_root: &Path,
    path: &str,
) -> Result<String> {
    for candidate in node_candidates(root, graph_root, path) {
        if composed.nodes.contains_key(&candidate) || existing.nodes.contains_key(&candidate) {
            return Ok(candidate);
        }
    }
    Err(not_found_error(
        composed.nodes.keys().chain(existing.nodes.keys()),
        path,
    ))
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

/// Project the composed graph's nodes and their metadata, scoped by `selectors`
/// and narrowed by `namespaces`/`fields`. A reader: expanding a selector to many
/// nodes is expected and has no side effect.
fn run_nodes(
    root: &Path,
    format: OutputFormat,
    selectors: &[String],
    namespaces: &[String],
    fields: &[String],
) -> Result<i32> {
    let graph_root = find_graph_root(root);
    let config = Config::load(&graph_root)?;
    let composed = compose::compose(&graphs::build_set(&graph_root, &config)?);

    // Validate namespaces up front: a typo must error, not read as an empty
    // answer. Normalize to the `@<graph>` keys the projection matches on.
    let requested_ns = resolve_namespaces(&config, namespaces)?;

    let keys = resolve_selectors(&composed, root, &graph_root, selectors)?;
    let projected = nodes::project(&composed, &keys, &requested_ns, fields);

    match format {
        OutputFormat::Json => {
            let output = serde_json::json!({
                "total": projected.len(),
                "nodes": projected,
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        OutputFormat::Text => {
            // One compact block per node — id, indented namespaces, fields — so a
            // model can read it for grounding without parsing JSON.
            print!("{}", nodes::format_text(&projected));
        }
    }

    Ok(0)
}

/// Project the composed graph's edges, matched on source: the selector picks source
/// nodes, and every edge leaving them is returned — the outbound one-hop view. A
/// reader, so expanding a selector to many sources is expected and has no side
/// effect. With no selector, every edge is projected.
fn run_edges(
    root: &Path,
    format: OutputFormat,
    selectors: &[String],
    namespaces: &[String],
    fields: &[String],
) -> Result<i32> {
    let graph_root = find_graph_root(root);
    let config = Config::load(&graph_root)?;
    let composed = compose::compose(&graphs::build_set(&graph_root, &config)?);

    let requested_ns = resolve_namespaces(&config, namespaces)?;
    // Edges match on source, so the selector resolves to the source node set.
    let sources = resolve_selectors(&composed, root, &graph_root, selectors)?;
    let projected = edges::project(&composed, &sources, &requested_ns, fields);

    match format {
        OutputFormat::Json => {
            let output = serde_json::json!({
                "total": projected.len(),
                "edges": projected,
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        OutputFormat::Text => {
            // One compact block per edge — `source → target`, then its metadata.
            print!("{}", edges::format_text(&projected));
        }
    }

    Ok(0)
}

/// Validate each requested namespace against the declared graphs (`fs` plus every
/// `[graphs.*]`) and normalize it to its `@<graph>` metadata key, deduped in the
/// order given. An unknown namespace is a typo — error, listing the declared
/// graphs — so it never reads as an empty answer.
fn resolve_namespaces(config: &Config, namespaces: &[String]) -> Result<Vec<String>> {
    // `fs` plus every configured graph, sorted for a deterministic error listing.
    let declared: std::collections::BTreeSet<&str> = config
        .graphs
        .keys()
        .map(String::as_str)
        .chain(std::iter::once("fs"))
        .collect();

    let mut normalized: Vec<String> = Vec::new();
    for name in namespaces {
        let bare = name.strip_prefix('@').unwrap_or(name);
        if !declared.contains(bare) {
            anyhow::bail!(
                "unknown namespace \"{name}\" — declared graphs: {}",
                declared.iter().copied().collect::<Vec<_>>().join(", ")
            );
        }
        let key = nodes::normalize_namespace(name);
        if !normalized.contains(&key) {
            normalized.push(key);
        }
    }
    Ok(normalized)
}

/// Resolve the positional selectors to node keys, sorted and deduped. With none,
/// the whole node set. Each selector is a globset pattern over node keys, a bare
/// directory (its recursive subtree), or an exact path — reusing `node_candidates`
/// for the exact, cwd-aware resolution `impact`/`lock` already use.
fn resolve_selectors(
    composed: &drft::model::Graph,
    root: &Path,
    graph_root: &Path,
    selectors: &[String],
) -> Result<Vec<String>> {
    if selectors.is_empty() {
        return Ok(composed.nodes.keys().cloned().collect());
    }

    let mut keys = std::collections::BTreeSet::new();
    for selector in selectors {
        keys.extend(resolve_selector(composed, root, graph_root, selector)?);
    }
    Ok(keys.into_iter().collect())
}

/// Resolve one selector to matching node keys.
///
/// A glob selector matches its pattern against node keys, graph-root-relative like
/// `drft.toml`'s `files`/`ignore`; an empty match is a legitimate query result. A
/// selector with no glob metacharacters is resolved cwd-aware: an exact file
/// resolves to itself, and a bare directory expands to its recursive subtree
/// (`docs/` ⇒ `docs/**`) — the same set the glob spelling names, so there is no
/// wrong spelling. An explicit path that matches nothing is a likely typo and
/// errors with a suggestion, rather than reading as empty.
fn resolve_selector(
    composed: &drft::model::Graph,
    root: &Path,
    graph_root: &Path,
    selector: &str,
) -> Result<Vec<String>> {
    if has_glob_meta(selector) {
        return glob_match_keys(composed, selector);
    }

    let mut matched: Vec<String> = Vec::new();
    let prefix = graph_key(root, graph_root, selector);

    // Does the exact key (no `.md` fallback) name a directory node? Checking this
    // before the fallback is what keeps a `docs.md` file from shadowing a `docs`
    // directory when both exist. A trailing slash also declares a directory
    // outright. Either way, a directory is represented by the subtree below, not the
    // bare directory node — so `docs`, `docs/`, and `docs/**` name one set.
    let names_dir = prefix
        .as_deref()
        .and_then(|key| composed.nodes.get(key))
        .is_some_and(|node| node.fs_type() == Some("directory"));
    let is_dir = names_dir || selector.ends_with('/');

    // A non-directory selector resolves to an exact node, with the `.md` fallback
    // impact/lock use, most-specific first. A file resolves to itself.
    if !is_dir {
        for candidate in node_candidates(root, graph_root, selector) {
            if composed.nodes.contains_key(&candidate) {
                matched.push(candidate);
                break;
            }
        }
    }

    // Directory ⇒ recursive subtree (`docs/` ⇒ `docs/**`). `graph_key` normalizes
    // the selector to a graph-root-relative prefix the same way exact resolution does.
    if let Some(prefix) = &prefix {
        for key in glob_match_keys(composed, &format!("{prefix}/**"))? {
            if !matched.contains(&key) {
                matched.push(key);
            }
        }
    }

    // Nothing resolved is a likely typo — error with a suggestion — unless the
    // selector named a real directory that simply has no descendants, which is a
    // legitimate empty result.
    if matched.is_empty() && !names_dir {
        return Err(not_found_error(composed.nodes.keys(), selector));
    }
    Ok(matched)
}

/// Whether a selector carries glob metacharacters — the signal that switches it
/// from an exact/subtree path to a pattern matched against node keys.
fn has_glob_meta(selector: &str) -> bool {
    selector.contains(['*', '?', '[', ']', '{', '}'])
}

/// Match one glob pattern against the composed graph's node keys, graph-root-
/// relative like `drft.toml`'s `files`/`ignore`. Node keys iterate sorted, so the
/// result is sorted; an empty match is a legitimate reader result.
fn glob_match_keys(composed: &drft::model::Graph, pattern: &str) -> Result<Vec<String>> {
    let set = drft::config::compile_globs(std::slice::from_ref(&pattern.to_string()))?
        .expect("a single non-empty pattern compiles to a set");
    Ok(composed
        .nodes
        .keys()
        .filter(|k| set.is_match(k.as_str()))
        .cloned()
        .collect())
}

/// List nodes that transitively depend on `paths` (a structural query; `paths`
/// is required by the CLI).
fn run_impact(
    root: &Path,
    format: OutputFormat,
    paths: &[String],
    depth: Depth,
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
    let impacted = impact::compute(&composed, &seeds, dir, depth.max_hops());

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
