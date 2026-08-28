# drft

A drift checker for linked files, built for LLMs and humans working in the same repo. It treats a directory of files as a dependency graph — files are nodes, links are edges — and flags what drifts when a dependency changes.

## Architecture

- **Crate name**: `drft-cli` (on crates.io)
- **Binary name**: `drft` (what users type)
- **npm package**: `drft` (wrapper, future)

Naming rule: "drift" spelled out refers only to the concept of structural drift. The tool is always `drft`.

## Language & stack

- Rust (2024 edition)
- `clap` (derive) for CLI parsing
- `serde` + `toml` for config/lockfile
- `serde_json` for JSON output
- `blake3` for content hashing (prefix: `b3:`)
- `pulldown-cmark` for markdown parsing (built-in parser)
- `saphyr` for YAML frontmatter parsing (marked AST → link line numbers + metadata)
- `ignore` for directory traversal (.gitignore-aware)
- `globset` for ignore/glob patterns
- `notify` for watch mode

## Commands

```bash
cargo build
cargo test
cargo clippy -- -D warnings
cargo fmt
dprint fmt             # formats markdown + TOML (CI runs dprint check)
cargo run -- check     # runs as `drft check`
```

## Conventions

- Error handling: `anyhow` for application errors, `thiserror` for typed library errors (add when needed)
- All output: diagnostics to stdout, progress/errors to stderr
- Exit codes: 0 (clean), 1 (violations), 2 (usage/config error)
- Lockfile (`drft.lock`): deterministic TOML, path-keyed nodes with a hash and nested per-edge target hashes, no version field, no timestamps. Lock is infrastructure, joined at check — not a graph.
- Config (`drft.toml`): TOML with `ignore`, `[graphs.*]`, and `[rules.*]` sections. Each config declares exactly one graph root; nested `drft.toml` files found while walking are ordinary files.
- Hashes use BLAKE3 with `b3:` prefix
- The substrate is a set of independent graphs, each a JGF graph of bare-path nodes. `fs` is the base graph: it walks every file, types it (`file`/`symlink`), and is the only graph drft auto-hashes. Cross-graph linkage is path coincidence, resolved at compose.
- Compose merges the set by path: each graph's metadata nests under `@<graph>`, with a `_graphs` provenance list. Resolution is namespace presence — a path with no `@fs` block is unresolved. `@` and `_` are compose-only reserved sigils; graph names are bare.
- Tests go in `tests/` (integration) and inline `#[cfg(test)]` modules (unit)
- Keep modules focused: one file per concern (sources, builders, graphs, compose, lock, rules, model, config, cli, util)
- Pipeline: `src/sources/` (bytes) → `src/builders/` (nodes + edges) → `src/graphs/` (per-graph build + auto-hash) → [`src/compose.rs`](src/compose.rs) (merge by path) → `src/rules/` (emit findings)

## Git workflow

Main is protected. All changes go through branches and pull requests — never push directly to main.

## Releasing

See [RELEASING.md](RELEASING.md). Releases go through a PR, then tag on main after merge.
