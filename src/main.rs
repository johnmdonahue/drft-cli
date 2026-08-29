mod cli;

use drft::compose;
use drft::config;
use drft::edges;
use drft::graphs;
use drft::hints::{Hint, Hints};
use drft::impact;
use drft::lock;
use drft::nodes;
use drft::projection;
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

/// Hints ride stderr, so the terminal check is on stderr rather than stdout — a
/// piped result with an attached terminal still gets color. Unlike [`use_color`]
/// this ignores `--format`: what it governs is always a text hint, never JSON.
fn use_color_stderr(choice: ColorChoice) -> bool {
    match choice {
        ColorChoice::Always => true,
        ColorChoice::Never => false,
        ColorChoice::Auto => std::io::IsTerminal::is_terminal(&std::io::stderr()),
    }
}

/// Rendered bytes at which a projection is worth flagging — roughly 16k tokens,
/// where reading the graph starts competing for context with the task the
/// reading was meant to serve.
///
/// Measured after rendering, so it is exact and costs nothing; estimating a
/// projection's size *before* building it is a different problem.
const LARGE_PROJECTION_BYTES: usize = 64 * 1024;

/// The move that narrows a read verb: it takes a selector and both filters.
const NARROW_WITH_SELECTOR: &str =
    "narrow it with a selector, or with --namespace / --field to project fewer keys per node";

/// The move for the whole-graph verbs, which take neither — `drft graph` and
/// `drft impact` have no selector to scope, so pointing at one would be a `next`
/// the reader cannot act on.
const NARROW_WITH_READ_VERB: &str = "read a slice instead — `drft nodes <selector>` or `drft edges <selector>` \
     project the same graph scoped to what you need";

/// Flag a projection large enough to crowd its reader. Reports both numbers:
/// a byte count alone is opaque, and a node count alone does not say how much
/// output it became. `next` varies by command — the flags that narrow a read
/// verb do not exist on `graph` or `impact`.
fn large_projection_hint(count: usize, bytes: usize, next: &str) -> Option<Hint> {
    (bytes >= LARGE_PROJECTION_BYTES).then(|| {
        Hint::new(
            "large-projection",
            format!("{count} nodes rendered to {}KB of output", bytes / 1024),
        )
        .with_next(next)
    })
}

/// Attach the run's hints to a result document under `hints`, and record that
/// they reached the reader. The key is always present, empty included, so a
/// consumer can read `.hints[]` without a guard.
fn attach_hints(document: &mut serde_json::Value, hints: &mut Hints) -> Result<()> {
    let obj = document
        .as_object_mut()
        .context("a result document must be a JSON object to carry hints")?;
    obj.insert("hints".to_string(), serde_json::to_value(&*hints)?);
    hints.mark_delivered();
    Ok(())
}

/// Print a JSON result document carrying the run's hints, raising the
/// large-projection hint when the rendered document is big enough to warrant it.
///
/// The size is measured on the document as it would print, hints included — the
/// bytes a reader actually pays for. Adding the large-projection hint grows the
/// document a little past what was measured, which is why the threshold is a
/// rough budget rather than a promise.
fn print_json_document(
    mut document: serde_json::Value,
    count: usize,
    next: &str,
    hints: &mut Hints,
) -> Result<()> {
    attach_hints(&mut document, hints)?;
    let rendered = serde_json::to_string_pretty(&document)?;
    match large_projection_hint(count, rendered.len(), next) {
        Some(hint) => {
            hints.push(hint);
            attach_hints(&mut document, hints)?;
            println!("{}", serde_json::to_string_pretty(&document)?);
        }
        None => println!("{rendered}"),
    }
    Ok(())
}

/// Print a text projection, raising the large-projection hint on the rendered
/// size. The hint itself lands on stderr after the command returns, so the
/// result reads first and a pipe carries only the projection.
fn print_text_projection(text: &str, count: usize, next: &str, hints: &mut Hints) {
    if let Some(hint) = large_projection_hint(count, text.len(), next) {
        hints.push(hint);
    }
    print!("{text}");
}

/// Load the config, folding its load-time advisories into the run's hints.
fn load_config(graph_root: &Path, hints: &mut Hints) -> Result<Config> {
    let mut config = Config::load(graph_root)?;
    hints.extend(std::mem::take(&mut config.hints));
    Ok(config)
}

fn main() {
    // Owned here rather than in `try_main` so the error envelope below can carry
    // whatever was raised before the failure — a run-level advisory is often what
    // explains it.
    let mut hints = Hints::default();
    let code = match try_main(&mut hints) {
        Ok(code) => code,
        Err(e) => {
            // A parse/usage error happens before clap resolves `--format`, so
            // scan the raw args to decide whether to emit a JSON error envelope.
            if wants_json_output() {
                let err = serde_json::json!({
                    "error": format!("{e:#}"),
                    "exit_code": 2,
                    "hints": &hints,
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

fn try_main(hints: &mut Hints) -> Result<i32> {
    let cli = Cli::parse();

    let root = match &cli.directory {
        Some(dir) => std::fs::canonicalize(dir)
            .with_context(|| format!("cannot access directory: {}", dir.display()))?,
        None => std::env::current_dir()?,
    };

    let result = match &cli.command {
        Commands::Init => run_init(&root),
        Commands::Lock { paths, all } => run_lock(&root, cli.format, paths, *all, hints),
        Commands::Impact {
            paths,
            depth,
            direction,
        } => run_impact(&root, cli.format, paths, *depth, *direction, hints),
        Commands::Graph { raw } => run_graph(&root, *raw, cli.format, hints),
        Commands::Nodes {
            selectors,
            namespaces,
            fields,
        } => run_nodes(&root, cli.format, selectors, namespaces, fields, hints),
        Commands::Edges {
            selectors,
            namespaces,
            fields,
        } => run_edges(&root, cli.format, selectors, namespaces, fields, hints),
        Commands::Check => run_check(&root, cli.format, cli.color, hints),
    };

    // A hint that reached nobody is worse than no hint, so delivery is tracked
    // rather than inferred: a command that printed a result document embedded its
    // hints there and is done. Everything else has to route them somewhere.
    //
    // Three commands land here. `init` and `lock` print no document at all;
    // `drft graph --format json` prints one whose root is exactly `graph`, a JGF
    // document rather than drft's own envelope, where a sibling key would cost
    // the translatability the format was chosen for.
    //
    // In JSON that leaves a stderr envelope, matching the shape the error path
    // already uses, so a consumer parsing stderr as JSON keeps working. The error
    // path itself is left alone: `main` folds the hints into its envelope, and
    // emitting here would print them twice.
    if !hints.is_empty() && !hints.delivered() && result.is_ok() {
        match cli.format {
            OutputFormat::Json => {
                let envelope = serde_json::json!({ "hints": &*hints });
                eprintln!("{}", serde_json::to_string(&envelope)?);
            }
            OutputFormat::Text => {
                let colorize = use_color_stderr(cli.color);
                for hint in hints.as_slice() {
                    eprintln!(
                        "{}",
                        if colorize {
                            hint.format_text_color()
                        } else {
                            hint.format_text()
                        }
                    );
                }
            }
        }
    }

    result
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
/// bytes and their outbound edge targets), merging into the existing lockfile;
/// with `--all`, lock every node.
///
/// A lock is an assertion that the locked state was reviewed, so the scoped form
/// exists to make that assertion narrow enough to be true — lock what you read,
/// not whatever happens to be stale.
///
/// The whole-graph lock has to be named. Zero paths is also what the shell hands
/// over when a command substitution matches nothing, so inferring "every node"
/// from an empty argument list would turn a scoped invocation into a whole-graph
/// assertion silently, in a file that outlives the session.
fn run_lock(
    root: &Path,
    format: OutputFormat,
    paths: &[String],
    all: bool,
    hints: &mut Hints,
) -> Result<i32> {
    // Both guards are pure argv, so they answer before any graph work.
    if paths.is_empty() && !all {
        anyhow::bail!(
            "no paths given — name the paths you reviewed (`drft lock <path>...`), \
             or pass `--all` to lock every node. An empty argument list is also \
             what a shell substitution that matched nothing produces, so it is \
             not read as the whole graph."
        );
    }
    if !paths.is_empty() && all {
        anyhow::bail!(
            "`--all` locks every node, so it cannot be combined with paths — \
             drop `--all` to lock only the paths you named, or drop the paths \
             to lock the whole graph."
        );
    }

    let graph_root = find_graph_root(root);
    let config = load_config(&graph_root, hints)?;
    let set = graphs::build_set(&graph_root, &config)?;
    let composed = compose::compose(&set);
    let snapshot = lock::Lock::from_composed(&composed);

    if all {
        let locked: Vec<String> = snapshot.nodes.keys().cloned().collect();
        lock::write(&graph_root, &snapshot)?;
        report_lock(format, &locked, &[], hints)?;
        return Ok(0);
    }

    // `read` returns `None` for a lockfile that is absent and for one that cannot
    // be parsed. Defaulting both to an empty baseline meant a scoped lock over a
    // corrupt lockfile silently replaced the whole file with only the paths named:
    // every other entry was gone, and the nodes behind them became unlocked leaves
    // whose loss no rule reports. Absent is the ordinary pre-lock state and is
    // fine; unreadable is not something a scoped lock can preserve, so it refuses.
    let read = lock::read(&graph_root, hints)?;
    if read.is_none() && lock::exists(&graph_root) {
        anyhow::bail!(
            "drft.lock exists but could not be parsed, so locking named paths \
             would replace the whole baseline with just those paths. Restore the \
             file from version control, or run `drft lock --all` to rebuild it \
             from the current tree — which asserts every node was reviewed."
        );
    }
    let mut existing = read.unwrap_or_default();

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
    //
    // A directory resolves to a real node but never carries a lock entry, so it
    // reaches the `None` arm and removes a key that was never there. That used to
    // be the whole of `drft lock <dir>`: exit 0, no output, nothing written. Say
    // so rather than letting silence read as success.
    let mut locked = Vec::new();
    let mut dropped = Vec::new();
    for node in nodes {
        match snapshot.nodes.get(&node) {
            Some(entry) => {
                existing.nodes.insert(node.clone(), entry.clone());
                locked.push(node);
            }
            None => {
                if composed
                    .nodes
                    .get(&node)
                    .and_then(drft::model::Node::fs_type)
                    == Some("directory")
                {
                    let beneath = composed
                        .nodes
                        .keys()
                        .filter(|k| k.starts_with(&format!("{node}/")))
                        .count();
                    hints.push(
                        Hint::new(
                            "directory-lock",
                            format!(
                                "is a directory, which carries no content to snapshot — the {beneath} {} beneath it {} not locked",
                                if beneath == 1 { "node" } else { "nodes" },
                                if beneath == 1 { "was" } else { "were" },
                            ),
                        )
                        .at(&node)
                        .with_next("name the files you reviewed, or pass `--all`"),
                    );
                    continue;
                }
                if existing.nodes.remove(&node).is_some() {
                    dropped.push(node);
                }
            }
        }
    }

    // Write only when something changed. Writing unconditionally meant that
    // `drft lock <dir>` in a repo that had never been locked created a valid,
    // parseable, zero-entry lockfile — a baseline covering nothing, produced by a
    // command that reported success, which made every staleness rule a no-op while
    // the file's presence made it look established.
    if !locked.is_empty() || !dropped.is_empty() {
        lock::write(&graph_root, &existing)?;
    }
    report_lock(format, &locked, &dropped, hints)?;
    Ok(0)
}

/// Report what a lock actually wrote.
///
/// `lock` printed nothing at all until this landed, so a caller could not tell a
/// lock that covered five files from one that covered none without reading
/// `drft.lock` by hand. The count is what makes the difference observable; naming
/// the nodes is what makes a resolution the caller did not expect — a bare name
/// that matched a file in another directory — visible at the moment it happens
/// rather than at the next `check`.
fn report_lock(
    format: OutputFormat,
    locked: &[String],
    dropped: &[String],
    hints: &mut Hints,
) -> Result<()> {
    match format {
        OutputFormat::Json => print_json_document(
            serde_json::json!({ "locked": locked, "dropped": dropped }),
            locked.len() + dropped.len(),
            "name fewer paths",
            hints,
        ),
        OutputFormat::Text => {
            fn plural<'a>(n: usize, one: &'a str, many: &'a str) -> &'a str {
                if n == 1 { one } else { many }
            }
            let mut line = format!(
                "locked {} {}",
                locked.len(),
                plural(locked.len(), "node", "nodes")
            );
            if !dropped.is_empty() {
                line.push_str(&format!(
                    ", dropped {} {}",
                    dropped.len(),
                    plural(dropped.len(), "entry", "entries")
                ));
            }
            println!("{line}");
            for node in locked {
                println!("  locked  {node}");
            }
            for node in dropped {
                println!("  dropped {node}");
            }
            Ok(())
        }
    }
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
        // The exact key comes first. Offering `{key}.md` ahead of it meant that
        // with both `docs/` and `docs.md` present, `drft lock docs` snapshotted
        // `docs.md` — a durable "this was reviewed" claim against a file the
        // caller never named, written silently. A path the caller spelled out
        // and that exists is never a worse answer than one invented from it.
        candidates.push(key.clone());
        if add_md {
            candidates.push(format!("{key}.md"));
        }
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

fn run_graph(root: &Path, raw: bool, format: OutputFormat, hints: &mut Hints) -> Result<i32> {
    let graph_root = find_graph_root(root);
    let config = load_config(&graph_root, hints)?;
    let set = graphs::build_set(&graph_root, &config)?;

    // `--raw` dumps the per-graph fragment set — a JSON structure with no text
    // projection — so it is JSON-only and ignores `--format`. The composed views
    // below honor it.
    if raw {
        // Distinct paths, not a per-fragment sum: the same file appears in every
        // graph that matched it, and counting it twice would make the same hint
        // name mean something different here than everywhere else.
        let count = set
            .graphs
            .iter()
            .flat_map(|g| g.nodes.keys())
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        let rendered = serde_json::to_string_pretty(&set)?;
        if let Some(hint) = large_projection_hint(count, rendered.len(), NARROW_WITH_READ_VERB) {
            hints.push(hint);
        }
        println!("{rendered}");
        return Ok(0);
    }

    let composed = compose::compose(&set);
    let node_count = composed.nodes.len();
    match format {
        OutputFormat::Json => {
            let rendered = serde_json::to_string_pretty(&composed.into_document())?;
            if let Some(hint) =
                large_projection_hint(node_count, rendered.len(), NARROW_WITH_READ_VERB)
            {
                hints.push(hint);
            }
            println!("{rendered}");
        }
        OutputFormat::Text => {
            // The whole composed graph as text: every node's metadata, then every
            // edge. `# nodes` / `# edges` headers keep the two sections legible
            // for a model reading the graph without parsing JSON. Reuses the same
            // per-node/per-edge rendering as `drft nodes` and `drft edges`.
            let keys = resolve_selectors(&composed, root, &graph_root, &[], hints)?;
            let node_text = nodes::format_text(&nodes::project(&composed, &keys, &[], &[]));
            let edge_text = edges::format_text(&edges::project(&composed, None, &[], &[]));
            let text = projection::join_sections(&[("nodes", &node_text), ("edges", &edge_text)]);
            print_text_projection(&text, node_count, NARROW_WITH_READ_VERB, hints);
        }
    }
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
    hints: &mut Hints,
) -> Result<i32> {
    let graph_root = find_graph_root(root);
    let config = load_config(&graph_root, hints)?;
    let composed = compose::compose(&graphs::build_set(&graph_root, &config)?);

    // Validate namespaces up front: a typo must error, not read as an empty
    // answer. Normalize to the `@<graph>` keys the projection matches on.
    let requested_ns = resolve_namespaces(&config, namespaces)?;

    let keys = resolve_selectors(&composed, root, &graph_root, selectors, hints)?;
    let projected = nodes::project(&composed, &keys, &requested_ns, fields);

    match format {
        OutputFormat::Json => {
            let output = serde_json::json!({
                "total": projected.len(),
                "nodes": projected,
            });
            print_json_document(output, projected.len(), NARROW_WITH_SELECTOR, hints)?;
        }
        OutputFormat::Text => {
            // One compact block per node — id, indented namespaces, fields — so a
            // model can read it for grounding without parsing JSON.
            print_text_projection(
                &nodes::format_text(&projected),
                projected.len(),
                NARROW_WITH_SELECTOR,
                hints,
            );
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
    hints: &mut Hints,
) -> Result<i32> {
    let graph_root = find_graph_root(root);
    let config = load_config(&graph_root, hints)?;
    let composed = compose::compose(&graphs::build_set(&graph_root, &config)?);

    let requested_ns = resolve_namespaces(&config, namespaces)?;
    // Edges match on source, so a selector resolves to the source node set. No
    // selector means every edge — passed as `None` so it never rides on the node
    // set, keeping the "every edge" guarantee independent of that coupling.
    let sources = if selectors.is_empty() {
        None
    } else {
        Some(resolve_selectors(
            &composed,
            root,
            &graph_root,
            selectors,
            hints,
        )?)
    };
    let projected = edges::project(&composed, sources.as_deref(), &requested_ns, fields);

    match format {
        OutputFormat::Json => {
            let output = serde_json::json!({
                "total": projected.len(),
                "edges": projected,
            });
            print_json_document(output, projected.len(), NARROW_WITH_SELECTOR, hints)?;
        }
        OutputFormat::Text => {
            // One compact block per edge — `source → target`, then its metadata.
            print_text_projection(
                &edges::format_text(&projected),
                projected.len(),
                NARROW_WITH_SELECTOR,
                hints,
            );
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
    hints: &mut Hints,
) -> Result<Vec<String>> {
    if selectors.is_empty() {
        return Ok(composed.nodes.keys().cloned().collect());
    }

    let mut keys = std::collections::BTreeSet::new();
    // Selectors dedupe, so their hints do too — a repeated selector is one
    // mistake, and saying it twice reads as two.
    let mut seen = std::collections::BTreeSet::new();
    for selector in selectors {
        if !seen.insert(selector) {
            continue;
        }
        keys.extend(resolve_selector(
            composed, root, graph_root, selector, hints,
        )?);
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
    hints: &mut Hints,
) -> Result<Vec<String>> {
    if has_glob_meta(selector) {
        let matched = glob_match_keys(composed, selector)?;
        // A pattern that matches nothing is a legitimate query result, so it stays
        // an empty answer rather than an error — but an empty answer reads exactly
        // like a clean one, which is what the hint is for.
        if matched.is_empty() {
            hints.push(
                Hint::new("zero-match-selector", "matched no nodes")
                    .at(selector)
                    .with_next(
                        "check the pattern against node keys — `*` stops at a path \
                         separator, `**` crosses it",
                    ),
            );
        }
        return Ok(matched);
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
    // A real directory with nothing below it in the graph: a true empty result,
    // and the one case above that does not error. Say so rather than let it pass
    // for a clean projection of nothing.
    if matched.is_empty() {
        hints.push(
            Hint::new(
                "zero-match-selector",
                "is a directory with no files in the graph",
            )
            .at(selector),
        );
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
    hints: &mut Hints,
) -> Result<i32> {
    let graph_root = find_graph_root(root);
    let config = load_config(&graph_root, hints)?;
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
            print_json_document(output, impacted.len(), NARROW_WITH_READ_VERB, hints)?;
        }
        OutputFormat::Text => {
            if impacted.is_empty() {
                println!("no dependents found");
            } else {
                let text = impacted
                    .iter()
                    .map(|i| {
                        format!(
                            "{} (via {}, depth {}, radius {})\n",
                            i.location(),
                            i.via,
                            i.depth,
                            i.impact_radius
                        )
                    })
                    .collect::<String>();
                print_text_projection(&text, impacted.len(), NARROW_WITH_READ_VERB, hints);
            }
        }
    }

    Ok(0)
}

/// Check the composed graph against the lockfile, reporting drift and structural
/// findings. Errors exit 1, warnings exit 0.
fn run_check(
    root: &Path,
    format: OutputFormat,
    color: ColorChoice,
    hints: &mut Hints,
) -> Result<i32> {
    let graph_root = find_graph_root(root);
    let config = load_config(&graph_root, hints)?;
    let set = graphs::build_set(&graph_root, &config)?;
    let composed = compose::compose(&set);
    let lock = lock::read(&graph_root, hints)?;

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
            // Findings, not a projection, so no size hint — `check` reports what
            // the graph says rather than handing back a slice of it.
            let mut output = serde_json::json!({
                "diagnostics": findings,
                "summary": { "errors": errors, "warnings": warnings },
            });
            attach_hints(&mut output, hints)?;
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
