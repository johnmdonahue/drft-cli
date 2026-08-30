# Contributing to drft

## Naming

The crate is `drft-cli` on crates.io and npm; the binary users type is `drft`.

Spelled out, "drift" refers only to the concept — what a file does when its
source changes underneath it. The tool is always `drft`. This holds in prose,
identifiers, and commit messages alike, so that searching for either word finds
what you meant.

## Development setup

```bash
git clone https://github.com/johnmdonahue/drft-cli.git
cd drft-cli

cargo build
cargo test
cargo run -- check -C examples/simple
```

`dprint fmt` formats markdown and TOML; CI runs `dprint check`.

## Stack

Rust, 2024 edition. The dependencies that shape the design:

- `clap` (derive) for CLI parsing
- `serde` + `toml` for the config and lockfile, `serde_json` for JSON output
- `blake3` for content hashing, written with a `b3:` prefix
- `pulldown-cmark` for markdown, `saphyr` for YAML frontmatter — the marked AST
  is what gives links their line numbers
- `ignore` for the gitignore-aware walk, `globset` for ignore and glob patterns
- `notify` for watch mode

Graph algorithms are not on that list, and deliberately — see the design
principles below.

## Code style

- Run `cargo fmt` and `dprint fmt` before committing
- Run `cargo clippy -- -D warnings` (must pass cleanly)
- `anyhow` for application errors, `thiserror` for typed library errors
- Write diagnostics to stdout, errors to stderr. Run-level advisories are `hints`:
  a key on the JSON result document, or — for a command that prints no document —
  a JSON envelope on stderr; text output puts them on stderr after the result
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

- **[`src/sources/`](src/sources/fs.rs)** — deliver `(path, bytes)`. `fs` is the gitignore-aware walk, and the only source.
- **[`src/builders/`](src/builders/mod.rs)** — turn bytes into a per-graph JGF fragment. `fs` types each entry — file, symlink, or directory — and emits symlink edges; `markdown`/`frontmatter` are text builders that wrap the parsers in [`src/parsers/`](src/parsers/mod.rs).
- **[`src/graphs/`](src/graphs/mod.rs)** — wire each graph, auto-hash, and decode bytes to text for the text builders. Hashing is drft's job, done once per node at this seam — sources and builders never hash. The decode runs after hashing and is where text is normalized, so a normalization there cannot move a node's hash; doing it in a source would, since a source's bytes are what gets hashed.
- **[`src/compose.rs`](src/compose.rs)** — merge the set by path, nest metadata under `@<graph>`, stamp `_graphs` provenance, dedup edges. The only module that knows about more than one graph.
- **[`src/rules/`](src/rules/check.rs)** — findings over the composed graph. `staleness.rs` joins the lockfile; `structural.rs` reads shape; `check.rs` applies config and severity, and owns the findings about the run itself rather than about a node — the ones that have to fire when there is no lockfile to join.

Core types live in [`src/model.rs`](src/model.rs); path/URI/hash helpers in [`src/util.rs`](src/util.rs); the lockfile in [`src/lock.rs`](src/lock.rs).

## Adding a rule

1. Add the finding to `src/rules/staleness.rs` (lock-derived) or `src/rules/structural.rs` (shape-derived); return `Finding`s. A finding about the run rather than about a node — one that has to fire when there is no lockfile to join — belongs in `check.rs` instead, because `staleness.rs` only runs against a usable baseline.
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

## Git workflow

`main` is protected. Every change goes through a branch and a pull request —
never push directly to main.

## Releasing

See [RELEASING.md](RELEASING.md). A release goes through a PR, then a tag on main
after the merge.
