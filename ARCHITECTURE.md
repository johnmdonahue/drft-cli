# Architecture

drft treats a directory of files as a directed graph — files are nodes, links are edges — and validates the graph against configurable rules.

## Core model

```
Files                                 Graph
  index.md ──[link]──> setup.md         index.md ──→ setup.md
  index.md ──[link]──> config.md        index.md ──→ config.md
  setup.md ──[link]──> config.md        setup.md ──→ config.md
```

The graph is built in a single pass: discover files, run parsers, normalize links, resolve paths, classify nodes. The result is an adjacency-list `Graph` with forward and reverse indices for efficient traversal.

### Node types

| Type | Meaning |
|------|---------|
| `File` | A path matched by `include` (minus `exclude`). Hashed, tracked, sent to parsers. |
| `External` | Discovered via an edge — target not in `include`. Validated for existence, not tracked. Covers both file paths on disk and URIs. |
| `Graph` | A child graph (directory with its own `drft.toml` or `drft.lock`). A hypernode. |

Node classification is driven by `include`/`exclude`, not by parsers. The `include` patterns declare the graph's known universe of files. Everything outside is an exit.

### Edges

Edges are minimal — the relationship and provenance:

```rust
pub struct Edge {
    pub source: String,        // source file path
    pub target: String,        // node ID (fragment-stripped)
    pub link: Option<String>,  // original link when it differs (e.g., bar.md#heading)
    pub parser: String,        // which parser found this
}
```

`target` is always a node ID — you can join on it directly. `link` is present only when the original reference included a fragment. `parser` records provenance.

### Parser contract

Parsers extract link strings from source files — relative paths, URIs, whatever the format contains. The parser decides what constitutes a link; the [graph builder](docs/graph.md) handles everything after: URI detection (RFC 3986), fragment stripping, path resolution, and node classification.

## Pipeline

```
  Parsers          → raw link strings + metadata
  Graph builder    → normalized edges, classified nodes, filesystem properties
  Enrichment       → structural analyses (degree, SCC, bridges, pagerank, etc.)
  Rules            → diagnostics
```

Each layer's output feeds the next. Custom scripts (parsers, rules) receive the same data as built-in implementations.

- **[`src/parsers/`](src/parsers/README.md)** — link extraction and metadata. Each parser implements the `Parser` trait, receives File nodes, and returns link strings + optional metadata. Built-in (markdown, frontmatter) and script-based parsers share the same interface.
- **[`src/graph.rs`](src/graph.rs)** — normalization, path resolution, node classification, filesystem probing. See [docs/graph.md](docs/graph.md) for the full contract.
- **[`src/analyses/`](src/analyses/README.md)** — pure computation. Each analysis implements the `Analysis` trait and returns a typed result. No judgments, no formatting.
- **[`src/metrics.rs`](src/metrics.rs)** — scalar extraction from analysis results. Named `Metric` values.
- **[`src/rules/`](src/rules/README.md)** — diagnostic mapping. Each rule implements the `Rule` trait, receives the enriched graph, and emits `Diagnostic` structs. Rules are pure functions — no filesystem access, no config.

This separation means:
- Analyses are reusable. Multiple rules and metrics can consume the same analysis.
- Rules carry no computation. They filter and format analysis output into diagnostics.
- New rules can compose existing analyses (e.g., "high PageRank + cut vertex = critical fragility").

## Analyses

Each analysis implements:

```rust
pub trait Analysis {
    type Output: serde::Serialize;
    fn name(&self) -> &str;
    fn run(&self, ctx: &AnalysisContext) -> Self::Output;
}

pub struct AnalysisContext<'a> {
    pub graph: &'a Graph,
    pub root: &'a Path,
    pub config: &'a Config,
    pub lockfile: Option<&'a Lockfile>,
}
```

Pure analyses (SCC, PageRank, bridges) ignore root/config/lockfile. Stateful analyses (change-propagation, graph-boundaries) use them. Both produce reusable, serializable results.

### Pure (graph topology only)

| Analysis | What it computes | Key output |
|----------|-----------------|------------|
| [`degree`](docs/analyses/degree.md) | In-degree and out-degree per node | `Vec<NodeDegree>` |
| [`scc`](docs/analyses/scc.md) | Strongly connected components (Tarjan's) | Non-trivial SCCs, node-to-SCC map |
| [`connected-components`](docs/analyses/connected-components.md) | Weakly connected components (BFS, undirected) | Component membership |
| [`depth`](docs/analyses/depth.md) | Topological layer from roots, with cycle handling | Layer assignments |
| [`graph-stats`](docs/analyses/graph-stats.md) | Node/edge count, density, diameter, avg path length | Summary statistics |
| [`bridges`](docs/analyses/bridges.md) | Cut vertices and bridge edges (Tarjan's, undirected) | Critical nodes and edges |
| [`transitive-reduction`](docs/analyses/transitive-reduction.md) | Transitively redundant edges | Per-edge BFS |
| [`betweenness`](docs/analyses/betweenness.md) | Betweenness centrality (Brandes' algorithm) | Centrality scores |
| [`impact-radius`](docs/analyses/impact-radius.md) | Transitive dependent count, blast zone depth | Radius per node |
| [`pagerank`](docs/analyses/pagerank.md) | PageRank scores (power iteration, d=0.85) | Rank scores |

### Stateful (graph + lockfile/config)

| Analysis | What it computes | External state |
|----------|-----------------|---------------|
| [`graph-boundaries`](docs/analyses/graph-boundaries.md) | Encapsulation violations, containment escapes | Child `drft.toml` interfaces |
| [`change-propagation`](docs/analyses/change-propagation.md) | Content changes, transitive staleness, node additions/removals | Lockfile hash comparison |

## Metrics

Metrics extract named scalar values from analysis results. They are flat -- no dimension grouping, no taxonomy. Each metric is derived from a specific analysis.

Metrics live in [`src/metrics.rs`](src/metrics.rs) as a single module. The `compute_metrics()` function takes pre-computed analysis results (via `AnalysisInputs`) and returns `Vec<Metric>`. Analyses run unconditionally during graph enrichment; metrics are derived from their outputs.

Each `Metric` carries a `MetricKind` (`Ratio`, `Count`, or `Score`) that indicates how to interpret and normalize the value.

## Rules

Each rule implements:

```rust
pub trait Rule {
    fn name(&self) -> &str;
    fn evaluate(&self, ctx: &RuleContext) -> Vec<Diagnostic>;
}

pub struct RuleContext<'a> {
    pub graph: &'a EnrichedGraph,
    pub options: Option<&'a toml::Value>,
}
```

Rules are pure functions over data. The enriched graph carries the graph plus all pre-computed analysis results (degree, SCC, bridges, etc.). Rules read what they need — no re-computation, no filesystem access, no config. Per-rule options from `[rules.<name>.options]` are passed through for rules that accept structured configuration.

| Rule | Reads from | Default severity |
|------|-----------|-----------------|
| `boundary-violation` | `graph_boundaries` | warn |
| `dangling-edge` | target properties | warn |
| `directed-cycle` | `scc` | warn |
| `directory-edge` | target properties | warn |
| `encapsulation-violation` | `graph_boundaries` | warn |
| `fragility` | `bridges` | warn |
| `fragmentation` | `connected_components` | warn |
| `layer-violation` | `depth` | warn |
| `orphan-node` | `degree` | warn |
| `redundant-edge` | `transitive_reduction` | warn |
| `schema-violation` | node metadata + options | warn |
| `stale` | `change_propagation` | warn |
| `symlink-edge` | target properties | warn |

All rules default to `warn` for immediate discoverability. Override to `error` for CI enforcement or `off` to suppress.

## Graph nesting

Any directory is a graph. A child directory with its own `drft.toml` or `drft.lock` appears as a node of type `Graph` in the parent, and file discovery stops at that boundary. This nesting is recursive.

- A graph with `[interface]` in its `drft.toml` enforces encapsulation: only interface nodes can be linked to from parent graphs.
- A graph without `[interface]` is open -- anything can link into it.
- **Child graphs** appear as `Graph` nodes in the parent. Files inside them that are linked from the parent appear as External nodes with a `graph` field.
- `drft check --recursive` and `drft lock --recursive` traverse the tree.

## Lockfile

`drft.lock` is a deterministic TOML snapshot of the graph's node set and content hashes. All File nodes are hashed via BLAKE3 (raw bytes). It enables:

- **Staleness detection** — compare current hashes to locked hashes.
- **Change propagation** — BFS from changed nodes through reverse edges to find transitively stale dependents.
- **Structural drift detection** — node additions and removals since last lock.

Edges are not stored in the lockfile. If a file's links change, its content hash changes. Graph nodes are hashed against the child's resolved `[interface]` section, so internal changes behind a stable interface don't trigger parent staleness.

## Commands

See [Commands](README.md#commands) in the README.

## Config

`drft.toml` controls:

```toml
include = ["*.md", "*.rs"]      # which paths become File nodes (default: ["*.md"])
exclude = ["drafts/*"]          # remove from the graph (also respects .gitignore)

[interface]
nodes = ["overview.md"]         # public interface nodes (enables encapsulation)

[parsers.markdown]              # built-in parser, all defaults

[parsers.frontmatter]           # built-in parser for YAML frontmatter
files = ["*.md"]

[parsers.tsx]                   # custom parser (has command)
files = ["*.tsx"]
command = "./scripts/parse-tsx.sh"

[rules]
dangling-edge = "warn"          # "error", "warn", or "off"
orphan-node = "off"

[rules.orphan-node]             # expanded: severity + ignore
severity = "warn"
ignore = ["README.md"]

[rules.my-check]                # custom rule (has command)
command = "./scripts/check.sh"
severity = "warn"

[rules.my-check.options]        # rule-specific options (passed through)
threshold = 5
```

Parsers and rules use the same config pattern. Shorthand (`markdown = true`, `directed-cycle = "warn"`) for the common case. Table form (`[parsers.tsx]`, `[rules.orphan-node]`) for options or custom scripts. Both support an `options` sub-table for arbitrary structured data passed through to the parser or rule. The `command` field is the discriminant — present means custom, absent means built-in.

Rules are evaluated at the configured severity. `--rule <name>` on the command line overrides `off` to `warn` for on-demand checks without config changes.

## Module layout

- [`src/README.md`](src/README.md) — source module index
- [`tests/README.md`](tests/README.md) — integration test index
- [`benches/README.md`](benches/README.md) — benchmark index

## Adding a new analysis

1. Create `src/analyses/<name>.rs` with a struct implementing `Analysis`. Define the output type and implement `run()` taking `&AnalysisContext`.
2. Add `pub mod <name>` to [`src/analyses/mod.rs`](src/analyses/mod.rs). Add the name to `all_analysis_names()`, add a field to `EnrichedGraph`, and wire it in `enrich_graph()`.
3. Register in the report command's `all_analyses` list in [`src/main.rs`](src/main.rs).
4. If it powers a rule: create `src/rules/<name>.rs`, register in `all_rules()`, add default severity in [`src/config.rs`](src/config.rs), add to the `drft init` template.
5. Add unit tests in the analysis module, integration tests in [`tests/`](tests/README.md).
6. Document in `docs/analyses/<name>.md` and update [`docs/analyses/README.md`](docs/analyses/README.md).

## Adding a new metric

Add the metric extraction to [`src/metrics.rs`](src/metrics.rs) inside `compute_metrics()`. The metric reads from analysis results and returns a `Metric` with name, value, and kind. It automatically appears in `drft report` output. Add the metric name to `all_metric_names()`.

## Design principles

- **Analyses describe shape, rules judge correctness.** An analysis says "this edge is transitively redundant." A rule says "that's a warning."
- **Three directories, three concerns.** `parsers/` extracts links, `analyses/` computes properties, `rules/` emits diagnostics. No layer reaches into another's concern.
- **Parsers emit, the graph normalizes.** Parsers return raw strings. The graph builder handles URI detection, fragment stripping, path resolution, node classification. Parser authors don't bake in assumptions.
- **No new dependencies for algorithms.** All graph algorithms (Tarjan's SCC, Brandes' betweenness, PageRank, BFS) are implemented in `std` only. File graphs are small enough that O(V*E) is fine.
- **Deterministic output.** All results are sorted. No timestamps in lockfiles. Same input always produces same output.
- **Explicit node filtering.** Each analysis declares which node types it operates on. No shared default, no hidden filter. File nodes for most structural analyses; Graph nodes added for boundary analyses.
