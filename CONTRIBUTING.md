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

See the `examples/` directory for sample projects used in manual testing. Each is its own graph with a `drft.toml`.

## Codebase structure

### Pipeline

```
sources/   → (path, bytes)
builders/  → per-graph nodes + edges (fs types; markdown/frontmatter parse)
graphs/    → per-graph build + auto-hash → the raw set
compose.rs → merge the set by path → the composed graph
rules/     → findings (drft check)
```

The substrate is a **set of independent graphs**; composition is a projection over it. Each layer's output feeds the next.

- **[`src/sources/`](src/sources/fs.rs)** — deliver `(path, bytes)`. v0.8 ships `fs`, the gitignore-aware walk.
- **[`src/builders/`](src/builders/mod.rs)** — turn bytes into a per-graph JGF fragment. `fs` types each entry — file, symlink, or directory — and emits symlink edges; `markdown`/`frontmatter` are text builders that wrap the parsers in [`src/parsers/`](src/parsers/mod.rs).
- **[`src/graphs/`](src/graphs/mod.rs)** — wire each graph and auto-hash. Hashing is drft's job, done once per node at this seam — sources and builders never hash.
- **[`src/compose.rs`](src/compose.rs)** — merge the set by path, nest metadata under `@<graph>`, stamp `_graphs` provenance, dedup edges. The only module that knows about more than one graph.
- **[`src/rules/`](src/rules/check.rs)** — findings over the composed graph. `staleness.rs` joins the lockfile; `structural.rs` reads shape; `check.rs` applies config and severity.

Core types live in [`src/model.rs`](src/model.rs); path/URI/hash helpers in [`src/util.rs`](src/util.rs); the lockfile in [`src/lock.rs`](src/lock.rs).

## Adding a rule

1. Add the finding to `src/rules/staleness.rs` (lock-derived) or `src/rules/structural.rs` (shape-derived); return `Finding`s.
2. Add the rule name to `BUILTIN_RULES` in [`src/config.rs`](src/config.rs) so configured severities don't warn as unknown.
3. Add unit tests in the rule module and an integration test in `tests/`.
4. Document it in the [rules reference](docs/rules/README.md).

## Adding a parser

Parsers are built in — there's no plugin mechanism.

1. Add the parser in `src/parsers/<name>.rs` (a content interpreter) and a builder in `src/builders/<name>.rs` that turns its output into a graph fragment.
2. Add the name to `KNOWN_PARSERS` in [`src/config.rs`](src/config.rs) and a dispatch arm in [`src/graphs/mod.rs`](src/graphs/mod.rs).
3. Add unit tests in the parser and builder modules and an integration test in `tests/`.
4. Document it in the [parsers reference](docs/parsers/README.md).

## Design principles

- **The substrate is the set of graphs; composition is a projection.** `drft graph --raw` is the honest source of truth; the composed view is derived. Never bake the merge into the substrate.
- **Auto-hashing is drft's job.** Sources and builders don't compute hashes — drft does, once per node, at the adoption seam (`graphs/`).
- **Lock is infrastructure, not a graph.** The graph carries current observations only; the lockfile is joined at check to derive staleness.
- **No new dependencies for algorithms.** Graph algorithms (BFS, Brandes' betweenness) are implemented in `std` only. File graphs are small enough that O(V*E) is fine.
- **Deterministic output.** Results are sorted. No timestamps or version fields in the lockfile. Same input always produces the same output.
