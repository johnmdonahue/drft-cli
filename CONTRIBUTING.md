# Contributing to drft

## Development setup

```bash
git clone https://github.com/johnmdonahue/drft-cli.git
cd drft-cli

cargo build
cargo test
cargo run -- check -C examples/simple
```

## Code style

- Run `cargo fmt` before committing
- Run `dprint fmt` before committing (formats markdown and TOML; CI runs `dprint check`)
- Run `cargo clippy -- -D warnings` (must pass cleanly)
- Write diagnostics to stdout, errors to stderr
- Exit codes: 0 clean, 1 violations, 2 usage error

## Testing

Unit tests are inline (`#[cfg(test)]` modules). Integration tests are in `tests/` and run the binary as a subprocess against temp directories.

```bash
cargo test                    # all tests
cargo test scenario_5         # specific test
```

## Examples

See the [examples](examples/README.md) for sample projects used in manual testing.

## Codebase structure

### Pipeline

```
Parsers          → raw link strings + metadata
Graph builder    → normalized edges, classified targets, filesystem properties
Enrichment       → structural analyses (degree, SCC, bridges, pagerank, etc.)
Rules            → diagnostics
```

Each layer's output feeds the next. Custom parsers and rules receive the same data as built-in implementations.

- **`src/parsers/`** — link extraction and metadata. Each parser implements the `Parser` trait, receives File nodes, and returns link strings + optional metadata. Built-in (markdown, frontmatter) and custom parsers share the same interface.
- **[`src/graph.rs`](src/graph.rs)** — normalization, path resolution, edge classification, filesystem probing. See [docs/graph.md](docs/graph.md) for the full contract.
- **`src/analyses/`** — pure computation. Each analysis implements the `Analysis` trait and returns a typed result. No judgments, no formatting.
- **[`src/metrics.rs`](src/metrics.rs)** — scalar extraction from analysis results. Named `Metric` values with a `MetricKind` (`Ratio`, `Count`, or `Score`).
- **`src/rules/`** — diagnostic mapping. Each rule implements the `Rule` trait, receives the enriched graph, and emits `Diagnostic` structs. Rules are pure functions — no filesystem access, no config.

## Adding a new analysis

1. Create `src/analyses/<name>.rs` with a struct implementing `Analysis`. Define the output type and implement `run()` taking `&AnalysisContext`.
2. Add `pub mod <name>` to [`src/analyses/mod.rs`](src/analyses/mod.rs). Add the name to `all_analysis_names()`, add a field to `EnrichedGraph`, and wire it in `enrich_graph()`.
3. Register in the report command's `all_analyses` list in [`src/main.rs`](src/main.rs).
4. If it powers a rule: create `src/rules/<name>.rs`, register in `all_rules()`, add default severity in [`src/config.rs`](src/config.rs), add to the `drft init` template.
5. Add unit tests in the analysis module, integration tests in `tests/`.
6. Document in `docs/analyses/<name>.md` and update [`docs/analyses/README.md`](docs/analyses/README.md).

## Adding a new metric

Add the metric extraction to [`src/metrics.rs`](src/metrics.rs) inside `compute_metrics()`. The metric reads from analysis results and returns a `Metric` with name, value, and kind. It automatically appears in `drft report` output. Add the metric name to `all_metric_names()`.

## Design principles

- **Analyses describe shape, rules judge correctness.** An analysis says "this edge is transitively redundant." A rule says "that's a warning."
- **Three directories, three concerns.** `parsers/` extracts links, `analyses/` computes properties, `rules/` emits diagnostics. No layer reaches into another's concern.
- **Parsers emit, the graph normalizes.** Parsers return raw strings. The graph builder handles URI detection, fragment stripping, path resolution, node classification. Parser authors don't bake in assumptions.
- **No new dependencies for algorithms.** All graph algorithms (Tarjan's SCC, Brandes' betweenness, PageRank, BFS) are implemented in `std` only. File graphs are small enough that O(V*E) is fine.
- **Deterministic output.** All results are sorted. No timestamps in lockfiles. Same input always produces same output.
- **Explicit scope filtering.** Structural analyses operate on included nodes and internal edges (`graph.is_internal_edge()` checks whether the target is an included node). Edges to non-included nodes don't participate in structural analysis.
