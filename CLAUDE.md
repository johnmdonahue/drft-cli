# drft

A structural integrity checker for linked file systems. Treats a directory of files as a dependency graph — files are nodes, links are edges — and validates the graph against configurable rules.

## Dogfooding

This repo runs drft on itself (`drft.toml` at root). A PostToolUse hook runs `drft check --format json` after every `.md` file edit.

**When the hook reports diagnostics, STOP.** Read the output. If files are stale, identify the impacted dependents and review whether they need updating. If links are broken, fix them before continuing. Do not batch up drft warnings and address them later — each diagnostic is a signal that the edit you just made has structural consequences.

Do not suppress warnings by removing links, ignoring paths, or disabling rules — fix the root cause (create missing files, fix broken references, restructure links). Do not run `drft lock` to clear staleness without first reviewing the impacted files.

The workflow: edit → drft check fires → review diagnostics → fix impacts → drft lock (only after review) → commit.

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
- `ignore` for directory traversal (.gitignore-aware)
- `globset` for ignore/glob patterns
- `notify` for watch mode

## Commands

```bash
cargo build
cargo test
cargo clippy -- -D warnings
cargo fmt
cargo run -- check    # runs as `drft check`
```

## Conventions

- Error handling: `anyhow` for application errors, `thiserror` for typed library errors (add when needed)
- All output: diagnostics to stdout, progress/errors to stderr
- Exit codes: 0 (clean), 1 (violations), 2 (usage/config error)
- Lockfile (`drft.lock`): TOML v2 format, nodes + hashes only (no edges), fully deterministic, no timestamps
- Config (`drft.toml`): TOML, unified `[parsers]` and `[rules]` sections, `[interface]` for graph boundary
- Hashes use BLAKE3 with `b3:` prefix
- Edge types are namespaced by parser: `markdown:inline`, `markdown:frontmatter`, `tsx:import`, etc.
- Node types: `Source` (parser ran on it), `Resource` (linked to, not parsed), `External` (URI), `Graph` (child graph)
- Parsers are configurable via `[parsers]` — built-in (markdown) or script-based (`command` field)
- Tests go in `tests/` (integration) and inline `#[cfg(test)]` modules (unit)
- Keep modules focused: one file per concern (discovery, parsers, graph, analyses, metrics, rules, lockfile, config, cli)
- Pipeline: `src/parsers/` (parse links) → `src/graph.rs` (build graph) → `src/analyses/` (compute properties) → `src/metrics.rs` (extract scalars) → `src/rules/` (emit diagnostics)
