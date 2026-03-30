# Architecture

drft treats a directory of files as a directed graph — files are nodes, links are edges — and provides structural analysis, health metrics, and configurable rule enforcement.

## Core model

```
Files                                 Graph
  index.md ──[link]──> setup.md         index.md ──→ setup.md
  index.md ──[link]──> config.md        index.md ──→ config.md
  setup.md ──[link]──> config.md        setup.md ──→ config.md
```

The graph is built in a single pass: discover files, match parsers, parse links, resolve paths, classify nodes. The result is an adjacency-list `Graph` with forward and reverse indices for efficient traversal.

Parsers define what a "link" is. Markdown is the default parser, but any file type can be parsed via configurable built-in or script-based parsers. The same graph algorithms apply regardless of what parsers built the graph.

### Node types

| Type | Meaning |
|------|---------|
| `Source` | A file a parser ran on. Can have outbound edges. |
| `Resource` | A file linked to but not parsed. Inbound edges only. |
| `External` | A URI-scheme link targeting a resource outside the filesystem. |
| `Graph` | A child graph (directory with its own `drft.toml` or `drft.lock`). A hypernode. |

Source/Resource classification is parser-driven, not file-type-driven. A `.md` file is a Source if the markdown parser is enabled, otherwise a Resource if something links to it. Enable a parser and its matched files become Sources.

### Edge types

Edge types are namespaced identifiers in the format `parser:type`. Each parser defines its own vocabulary of link types.

The built-in markdown parser produces: `markdown:inline`, `markdown:reference`, `markdown:autolink`, `markdown:image`, `markdown:frontmatter`, `markdown:wikilink`.

Custom parsers define their own types -- e.g., `tsx:import` for `import X from './path'`.

The type is represented as a validated `EdgeType` newtype, not a bare string:

```rust
pub struct EdgeType {
    parser: String,
    link_type: String,
}
```

## Pipeline

```
┌─────────────────────────────────────────────────────────────┐
│  Parsing                             src/parsers/           │
│  Discovers files, parses links from matched file types.     │
│  Pluggable: built-in (markdown) + custom scripts.           │
│                                                             │
│  Produces the Graph.                                        │
├═════════════════════════════════════════════════════════════╡
│  Analysis                            src/analyses/          │
│  Computes structural properties and contract checks.        │
│  fn(&AnalysisContext) → Output.                             │
├─────────────────────────────────────────────────────────────┤
│  Metrics                             src/metrics.rs         │
│  Extracts named scalar values from analysis results.        │
└─────────────────────────────────────────────────────────────┘

Evaluated State = Graph + all analysis results + all metrics
    │
    ├── Rules (built-in and script-based, via Rule trait)
    └── drft check output
```

The pipeline: **Parsing → Graph → Analyses → Metrics → Rules**. Each layer's output feeds the next. Custom scripts (parsers, rules) receive the same data as built-in implementations.

Each layer has its own directory and concerns:

- **`src/parsers/`** — link extraction. Each parser implements the `Parser` trait, matches files by glob, and emits `RawLink` results. Built-in (markdown) and script-based parsers share the same interface.
- **`src/analyses/`** — pure computation. Each analysis implements the `Analysis` trait, takes an `AnalysisContext`, returns a typed result. No judgments, no formatting.
- **`src/metrics.rs`** — scalar extraction. Reads from analysis results and produces named `Metric` values. No graph traversal, no I/O.
- **`src/rules/`** — diagnostic mapping. Each rule implements the `Rule` trait, receives a `RuleContext` with full evaluated state, and emits `Diagnostic` structs with severity and fix suggestions.

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
| [`pagerank`](docs/analyses/pagerank.md) | PageRank scores (power iteration, d=0.85) | Rank scores |

### Stateful (graph + lockfile/config)

| Analysis | What it computes | External state |
|----------|-----------------|---------------|
| [`graph-boundaries`](docs/analyses/graph-boundaries.md) | Encapsulation violations, containment escapes | Child `drft.toml` interfaces |
| [`change-propagation`](docs/analyses/change-propagation.md) | Content changes, transitive staleness, node additions/removals | Lockfile hash comparison |

## Metrics

Metrics extract named scalar values from analysis results. They are flat -- no dimension grouping, no taxonomy. Each metric is derived from a specific analysis.

Metrics live in `src/metrics.rs` as a single module. The `compute_metrics()` function takes pre-computed analysis results (via `AnalysisInputs`) and returns `Vec<Metric>`. Analyses are run by the `report` command; metrics are derived from their outputs.

Each `Metric` carries a `MetricKind` (`Ratio`, `Count`, or `Score`) that indicates how to interpret and normalize the value.

## Rules

Each rule implements:

```rust
pub trait Rule {
    fn name(&self) -> &str;
    fn default_severity(&self) -> Severity;
    fn evaluate(&self, ctx: &RuleContext) -> Vec<Diagnostic>;
}

pub struct RuleContext<'a> {
    pub graph: &'a Graph,
    pub analyses: &'a AnalysisResults,
    pub metrics: &'a [Metric],
    pub config: &'a Config,
    pub lockfile: Option<&'a Lockfile>,
}
```

| Rule | Source | Default severity |
|------|-------|-----------------|
| `broken-link` | graph | warn |
| `containment` | analysis: `graph-boundaries` | warn |
| `cycle` | analysis: `scc` | warn |
| `directory-link` | graph | warn |
| `encapsulation` | analysis: `graph-boundaries` | warn |
| `fragility` | analysis: `bridges` | off |
| `fragmentation` | analysis: `connected-components` | off |
| `indirect-link` | graph | off |
| `layer-violation` | analysis: `depth` | off |
| `orphan` | analysis: `degree` | off |
| `redundant-edge` | analysis: `transitive-reduction` | off |
| `stale` | analysis: `change-propagation` | error |

Rules default to `off` when they report structural insights (fragility, fragmentation, orphan, etc.) vs. `warn`/`error` when they report likely errors (broken links, cycles, staleness).

## Graph nesting

Any directory is a graph. A child directory with its own `drft.toml` or `drft.lock` appears as a node of type `Graph` in the parent, and file discovery stops at that boundary. This nesting is recursive.

- A graph with `[interface]` in its `drft.toml` enforces encapsulation: only interface nodes can be linked to from parent graphs.
- A graph without `[interface]` is open -- anything can link into it.
- **Child graphs** appear as `Graph` nodes in the parent. Files inside them that are linked from the parent appear as Source or Resource nodes with a `graph` field.
- `drft check --recursive` and `drft lock --recursive` traverse the tree.

## Lockfile

`drft.lock` is a deterministic TOML snapshot of the graph's node set and content hashes. All file-backed nodes (Source, Resource, Graph) are hashed via BLAKE3. It enables:

- **Staleness detection** — compare current hashes to locked hashes.
- **Change propagation** — BFS from changed nodes through reverse edges to find transitively stale dependents.
- **Structural drift detection** — node additions and removals since last lock.

Edges are not stored in the lockfile. If a file's links change, its content hash changes. Graph nodes are hashed against the child's resolved `[interface]` section, so internal changes behind a stable interface don't trigger parent staleness.

## Commands

| Command | Purpose |
|---------|---------|
| `drft init` | Create `drft.toml` with default config |
| `drft check` | Run rules, emit diagnostics. Exit 0 (clean) or 1 (violations). |
| `drft lock` | Snapshot current graph state to `drft.lock` |
| `drft graph` | Export the dependency graph (JSON Graph Format) |
| `drft impact <files>` | Show transitive dependents of given files |
| `drft report [names]` | [unstable] Run graph analyses and health metrics |

## Config

`drft.toml` controls:

```toml
ignore = ["drafts/*"]           # glob patterns to exclude from discovery

[interface]
nodes = ["overview.md"]         # public interface nodes (enables encapsulation)

[parsers]
markdown = true                 # built-in parser, all defaults

[parsers.tsx]                   # custom parser (has command)
glob = "*.tsx"
command = "./scripts/parse-tsx.sh"

[rules]
broken-link = "warn"            # "error", "warn", or "off"
orphan = "off"

[rules.orphan]                  # expanded: severity + ignore
severity = "warn"
ignore = ["README.md"]

[rules.my-check]                # custom rule (has command)
command = "./scripts/check.sh"
severity = "warn"
```

Parsers and rules use the same config pattern. Shorthand (`markdown = true`, `cycle = "warn"`) for the common case. Table form (`[parsers.tsx]`, `[rules.orphan]`) for options or custom scripts. The `command` field is the discriminant -- present means custom, absent means built-in.

Rules are evaluated at the configured severity. `--rule <name>` on the command line overrides `off` to `warn` for on-demand checks without config changes.

## Module layout

See [`src/README.md`](src/README.md) for the full module index.

```
src/
├── main.rs          Command dispatch
├── cli.rs           Clap-derived CLI definition
├── config.rs        Config loading, defaults
├── graph.rs         Graph, Node, Edge, EdgeType types; construction
├── discovery.rs     .gitignore-aware file discovery
├── lockfile.rs      Lockfile read/write
├── diagnostic.rs    Diagnostic struct, text/JSON formatting
├── parsers/
│   ├── mod.rs       Parser trait, registry, RawLink type
│   ├── markdown.rs  Built-in markdown parser
│   └── script.rs    Script-based parser runner
├── analyses/
│   ├── mod.rs       Analysis trait, AnalysisContext
│   ├── degree.rs
│   ├── scc.rs
│   ├── connected_components.rs
│   ├── depth.rs
│   ├── graph_stats.rs
│   ├── bridges.rs
│   ├── betweenness.rs
│   ├── pagerank.rs
│   ├── transitive_reduction.rs
│   ├── graph_boundaries.rs
│   └── change_propagation.rs
├── metrics.rs       Metric type, MetricKind, compute_metrics()
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
│   ├── orphan.rs
│   ├── redundant_edge.rs
│   ├── stale.rs
│   └── script.rs    Script-based rule runner
tests/
└── scenarios.rs     Integration tests
docs/
└── analyses/        Per-analysis conceptual documentation
```

## Adding a new analysis

1. Create `src/analyses/<name>.rs` with a struct implementing `Analysis`. Define the output type and implement `run()` taking `&AnalysisContext`.
2. Add `pub mod <name>` to `src/analyses/mod.rs`.
3. If it powers a rule: create `src/rules/<name>.rs`, register in `all_rules()`, add default severity in `config.rs`, add to the `drft init` template.
4. Add unit tests in the analysis module, integration tests in `tests/scenarios.rs`.
5. Document in `docs/analyses/<name>.md` and update `docs/analyses/README.md`.

## Adding a new metric

Add the metric extraction to `src/metrics.rs` inside `compute_metrics()`. The metric reads from analysis results and returns a `Metric` with name, value, and kind. It automatically appears in `drft report` output. Add the metric name to `all_metric_names()`.

## Design principles

- **Analyses describe shape, rules judge correctness.** An analysis says "this edge is transitively redundant." A rule says "that's a warning."
- **Three directories, three concerns.** `parsers/` extracts links, `analyses/` computes properties, `rules/` emits diagnostics. No layer reaches into another's concern.
- **No new dependencies for algorithms.** All graph algorithms (Tarjan's SCC, Brandes' betweenness, PageRank, BFS) are implemented in `std` only. File graphs are small enough that O(V*E) is fine.
- **Deterministic output.** All results are sorted. No timestamps in lockfiles. Same input always produces same output.
- **Explicit node filtering.** Each analysis declares which node types it operates on. No shared default, no hidden filter. Source and Resource for most structural analyses; Graph nodes added for boundary analyses.
