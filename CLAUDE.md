# drft

A structural integrity checker for linked file systems. Treats a directory of files as a dependency graph — files are nodes, links are edges — and validates the graph against configurable rules.

## Dogfooding

This repo runs drft on itself (`drft.toml` at root). A PostToolUse hook runs `drft impact <file> --format json` after every `.md` and `.rs` file edit.

**When planning changes, run `drft impact <files> --format json` on the files you intend to modify.** This shows the transitive dependency chain *before* you start editing. Document impacted files in the plan so the implementation accounts for downstream effects.

**When the hook reports impacted files, STOP.** The output lists every file that transitively depends on the file you just edited. These are files whose content may now be out of date because of your change. Read each one and check whether it still accurately reflects the source it depends on.

**Reviewing impacted files means reading them.** Check all content that could be affected by your change — not just prose, but code examples, JSON snippets, data structures, command invocations, and any content that mirrors or describes the file you changed. A doc that links to a source file is making a promise that its content reflects that source. When the source changes, verify the promise still holds.

Do not suppress warnings by removing links, ignoring paths, or disabling rules — fix the root cause (create missing files, fix broken references, restructure links). Do not run `drft lock` to clear staleness without first reviewing the impacted files.

The workflow: edit → drft impact fires → read impacted dependents → fix impacts → drft lock (only after review) → commit.

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

## Releasing

See [RELEASING.md](RELEASING.md). Main is protected — releases go through a PR, then tag on main after merge.
