# Architecture

drft treats a directory of markdown files as a directed graph — files are nodes, links are edges — and provides structural analysis, health metrics, and configurable rule enforcement.

## Core model

```
Markdown files                     Graph
  index.md ──[link]──> setup.md      index.md ──→ setup.md
  index.md ──[link]──> config.md     index.md ──→ config.md
  setup.md ──[link]──> config.md     setup.md ──→ config.md
```

The graph is built in a single pass: discover files, read content, extract links, resolve paths, classify nodes. The result is an adjacency-list `Graph` with forward and reverse indices for efficient traversal.

### Node types

| Type | Meaning |
|------|---------|
| `Document` | Markdown file inside the scope |
| `Asset` | Non-markdown file linked to (images, data) |
| `External` | HTTP/HTTPS URL |
| `Frontier` | Child scope boundary (directory with its own `drft.lock`) |
| `Virtual` | File inside a child scope referenced from the parent |

### Edge types

`Inline`, `Reference`, `Autolink`, `Image`, `Frontmatter`, `Wikilink` — corresponding to the markdown link syntax that created the edge.

## Three-layer evaluation

```
┌─────────────────────────────────────────────────────────────┐
│  Analysis                            src/analyses/          │
│  Computes structural properties. Returns typed structs.     │
│  No judgments, no thresholds.                               │
│                                                             │
│  drft report                                                │
├─────────────────────────────────────────────────────────────┤
│  Metric                              src/metrics/           │
│  Extracts named scalar values from analysis results.        │
│  Grouped by quality dimension (Zaveri taxonomy).            │
│                                                             │
│  drft report --metrics                                      │
├─────────────────────────────────────────────────────────────┤
│  Rule                                src/rules/             │
│  Thin wrapper: runs an analysis, maps results to            │
│  diagnostics with severity and fix suggestions.             │
│                                                             │
│  drft check                                                 │
└─────────────────────────────────────────────────────────────┘
```

Each layer has its own directory and concerns:

- **`src/analyses/`** — pure computation. Each analysis implements the `Analysis` trait, takes a `Graph`, returns a typed result. No judgments, no formatting.
- **`src/metrics/`** — scalar extraction. Each dimension module reads from analysis results and produces named `Metric` values. No graph traversal, no I/O.
- **`src/rules/`** — diagnostic mapping. Each rule instantiates an analysis, runs it, and filters/formats the output into `Diagnostic` structs with severity and fix suggestions.

This separation means:
- Analyses are reusable. Multiple rules and metrics can consume the same analysis.
- Rules carry no computation. They filter and format analysis output into diagnostics.
- Metrics are independent. Adding a new metric doesn't touch analysis code.
- New rules can compose existing analyses (e.g., "high PageRank + cut vertex = critical fragility").

## Analyses

Each analysis implements:

```rust
pub trait Analysis {
    type Output: serde::Serialize;
    fn name(&self) -> &str;
    fn run(&self, graph: &Graph, root: &Path) -> Self::Output;
}
```

The 12 analyses fall into three tiers based on what they need:

### Tier 0 — Foundational (graph topology only)

| Analysis | What it computes | Key output |
|----------|-----------------|------------|
| `degree` | In-degree and out-degree per node | `Vec<NodeDegree>` |
| `scc` | Strongly connected components (Tarjan's) | Non-trivial SCCs, node-to-SCC map |
| `connected-components` | Weakly connected components (BFS, undirected) | Component membership |
| `edge-classification` | Target status per edge (valid, broken, directory, symlink, external) | `Vec<ClassifiedEdge>` |

### Tier 1 — Derived (compose from Tier 0 or independent graph algorithms)

| Analysis | What it computes | Dependencies |
|----------|-----------------|--------------|
| `depth` | Topological layer from roots, with cycle handling | Calls `scc` internally |
| `graph-stats` | Node/edge count, density, diameter, avg path length | All-pairs BFS |
| `bridges` | Cut vertices and bridge edges (Tarjan's, undirected) | Independent |
| `transitive-reduction` | Transitively redundant edges | Per-edge BFS |
| `betweenness` | Betweenness centrality (Brandes' algorithm) | Independent |
| `pagerank` | PageRank scores (power iteration, d=0.85) | Independent |

### Tier 2 — External state (graph + filesystem/lockfile)

| Analysis | What it computes | External data |
|----------|-----------------|---------------|
| `scope-boundaries` | Scope escapes and encapsulation violations | Child lockfile manifests |
| `change-propagation` | Direct changes and transitive staleness | Lockfile hash comparison |

## Metrics

Metrics live in `src/metrics/`, organized by Zaveri quality dimension — one file per dimension:

| Module | Dimension | What it measures | Example metrics |
|--------|-----------|-----------------|-----------------|
| `completeness.rs` | completeness | Are all expected connections present? | `orphan_ratio`, `dead_link_rate`, `island_ratio` |
| `consistency.rs` | consistency | Is the structure well-formed? | `component_count`, `fragmentation_index`, `density`, `cyclomatic_complexity` |
| `conciseness.rs` | conciseness | Is the structure free of redundancy? | `redundant_edge_ratio` |
| `timeliness.rs` | timeliness | Is the content current? | `directly_changed_count`, `transitively_stale_count` |

Each dimension module exports an `extract()` function that reads from an `AnalysisResults` struct (containing all 12 analysis outputs) and returns `Vec<Metric>`. The `collect_all()` function in `metrics/mod.rs` calls all four extractors.

Each `Metric` carries a `MetricKind` (`Ratio`, `Count`, or `Score`) that indicates how to interpret and normalize the value.

## Rules

Each rule implements:

```rust
pub trait Rule {
    fn name(&self) -> &str;
    fn evaluate(&self, graph: &Graph, root: &Path) -> Vec<Diagnostic>;
}
```

Every rule follows the same pattern: instantiate an analysis, call `.run()`, filter/map the result into `Diagnostic` structs.

| Rule | Analysis it consumes | Default severity |
|------|---------------------|-----------------|
| `broken-link` | `edge-classification` | warn |
| `containment` | `scope-boundaries` | warn |
| `cycle` | `scc` | warn |
| `directory-link` | `edge-classification` | warn |
| `encapsulation` | `scope-boundaries` | warn |
| `fragility` | `bridges` | off |
| `fragmentation` | `connected-components` | off |
| `indirect-link` | `edge-classification` | off |
| `layer-violation` | `depth` | off |
| `lockfile-outdated` | (inline) | warn |
| `orphan` | `degree` | off |
| `redundant-edge` | `transitive-reduction` | off |
| `stale` | `change-propagation` | warn |

Rules default to `off` when they report structural insights (fragility, fragmentation, orphan, etc.) vs. `warn` when they report likely errors (broken links, cycles, staleness).

## Scopes

A **scope** is a directory with a `drft.lock` file. Scopes create partitions in the graph:

- A **sealed scope** (has lockfile) enforces containment: no `../` links escape.
- A **manifest** (declared in `drft lock --manifest`) defines the scope's public interface. External edges must target manifest nodes.
- **Child scopes** appear as `Frontier` nodes in the parent graph. Files inside them appear as `Virtual` nodes.
- Scopes can be nested. `drft check --recursive` and `drft lock --recursive` traverse the tree.

## Lockfile

`drft.lock` is a deterministic TOML snapshot of the graph state: node hashes (BLAKE3), edges, and optional manifest. It enables:

- **Staleness detection** — compare current hashes to locked hashes.
- **Change propagation** — BFS from changed nodes through reverse edges to find transitively stale dependents.
- **Scope sealing** — lockfile existence means the scope has a boundary.

## Commands

| Command | Purpose |
|---------|---------|
| `drft init` | Create `drft.toml` with default config |
| `drft check` | Run rules, emit diagnostics. Exit 0 (clean) or 1 (violations). |
| `drft lock` | Snapshot current graph state to `drft.lock` |
| `drft report` | Run analyses, output structured results (text or JSON) |
| `drft report --metrics` | Extract scalar health metrics from all analyses |
| `drft graph` | Export the dependency graph (JSON Graph Format) |
| `drft impact <files>` | Show transitive dependents of given files |

## Config

`drft.toml` controls:

```toml
ignore = ["drafts/*"]           # glob patterns to exclude from discovery

[rules]
broken-link = "warn"            # "error", "warn", or "off"
orphan = "off"

[ignore-rules]
orphan = ["README.md"]          # suppress specific rules for specific paths

[custom-rules.my-check]
command = "./scripts/check.sh"  # external script receiving graph JSON on stdin
severity = "warn"
```

Rules are evaluated at the configured severity. `--rule <name>` on the command line overrides `off` to `warn` for on-demand checks without config changes.

## Module layout

```
src/
├── main.rs          Command dispatch, report/check/lock/graph/impact runners
├── cli.rs           Clap-derived CLI definition
├── config.rs        Config loading, defaults, inheritance
├── graph.rs         Graph, Node, Edge types; graph construction from filesystem
├── discovery.rs     .gitignore-aware file discovery
├── parsing.rs       Markdown AST → link extraction (pulldown-cmark)
├── lockfile.rs      Lockfile read/write, manifest derivation
├── diagnostic.rs    Diagnostic struct, text/JSON formatting
├── analyses/
│   ├── mod.rs       Analysis trait
│   ├── degree.rs
│   ├── scc.rs
│   ├── connected_components.rs
│   ├── depth.rs
│   ├── graph_stats.rs
│   ├── bridges.rs
│   ├── betweenness.rs
│   ├── pagerank.rs
│   ├── transitive_reduction.rs
│   ├── edge_classification.rs
│   ├── scope_boundaries.rs
│   └── change_propagation.rs
├── metrics/
│   ├── mod.rs       Metric/MetricKind types, AnalysisResults, collect_all()
│   ├── completeness.rs
│   ├── consistency.rs
│   ├── conciseness.rs
│   └── timeliness.rs
├── rules/
│   ├── mod.rs       Rule trait, all_rules() registry
│   ├── broken_link.rs
│   ├── containment.rs
│   ├── cycle.rs
│   ├── directory_link.rs
│   ├── encapsulation.rs
│   ├── fragility.rs
│   ├── fragmentation.rs
│   ├── indirect_link.rs
│   ├── layer_violation.rs
│   ├── lockfile_outdated.rs
│   ├── orphan.rs
│   ├── redundant_edge.rs
│   ├── stale.rs
│   └── custom.rs
tests/
└── scenarios.rs     Integration tests (62 scenarios)
docs/
└── analyses/        Per-analysis conceptual documentation
```

## Adding a new analysis

1. Create `src/analyses/<name>.rs` with a struct implementing `Analysis`. Define the output type and implement `run()`.
2. Add `pub mod <name>` to `src/analyses/mod.rs`.
3. Add a dispatch block in `run_report()` in `src/main.rs`.
4. If it powers a rule: create `src/rules/<name>.rs`, register in `all_rules()`, add default severity in `config.rs`, add to the `drft init` template.
5. Add unit tests in the analysis module, integration tests in `tests/scenarios.rs`.
6. Document in `docs/analyses/<name>.md` and update `docs/analyses/README.md`.

## Adding a new metric

Add the metric extraction to the appropriate dimension file in `src/metrics/` (completeness, consistency, conciseness, or timeliness). The metric reads from `AnalysisResults` and returns a `Metric` with name, value, kind, and dimension. It automatically appears in `drft report --metrics` output.

If the metric needs a new analysis result, add the field to `AnalysisResults` in `src/metrics/mod.rs` and update `run_metrics()` in `src/main.rs` to provide it.

## Design principles

- **Analyses describe shape, rules judge correctness.** An analysis says "this edge is transitively redundant." A rule says "that's a warning."
- **Three directories, three concerns.** `analyses/` computes, `metrics/` extracts scalars, `rules/` emits diagnostics. No layer reaches into another's concern.
- **No new dependencies for algorithms.** All graph algorithms (Tarjan's SCC, Brandes' betweenness, PageRank, BFS) are implemented in `std` only. Markdown repos are small enough that O(V*E) is fine.
- **Deterministic output.** All results are sorted. No timestamps in lockfiles. Same input always produces same output.
- **Real nodes only.** Most analyses filter to Document and Asset nodes via `Graph::is_real_node()`, excluding synthetic nodes (External, Frontier, Virtual) that represent boundaries rather than content.
