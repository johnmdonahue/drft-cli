# drft

A structural integrity checker for markdown directories. Treats a directory of markdown files as a dependency graph — files are nodes, links are edges — and validates the graph against configurable rules.

## Architecture

- **Crate name**: `drft-cli` (on crates.io)
- **Binary name**: `drft` (what users type)
- **npm package**: `drft` (wrapper, future)

Naming rule: "drift" spelled out refers only to the concept of markdown drift. The tool is always `drft`.

## Language & stack

- Rust (2021 edition)
- `clap` (derive) for CLI parsing
- `serde` + `toml` for config/lockfile
- `serde_json` for JSON output
- `blake3` for content hashing (prefix: `b3:`)
- `pulldown-cmark` for markdown AST parsing
- `ignore` for directory traversal (.gitignore-aware)
- `globset` for ignore patterns
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
- Lockfile (`drft.lock`): TOML, fully deterministic, no timestamps
- Config (`drft.toml`): TOML, no inheritance/cascading
- Hashes use BLAKE3 with `b3:` prefix
- Edges store source type: `inline`, `reference`, `autolink`, `image`, `frontmatter`, `wikilink`
- Tests go in `tests/` (integration) and inline `#[cfg(test)]` modules (unit)
- Keep modules focused: one file per concern (discovery, parsing, graph, rules, lockfile, config, cli)
