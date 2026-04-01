# drft

A structural integrity checker for linked file systems. Treats a directory of files as a dependency graph — files are nodes, links are edges — and validates the graph against configurable rules.

## Dogfooding

This repo runs drft on itself (`drft.toml` at root). A PreToolUse hook runs `drft impact <file> --format json` before every `.md` and `.rs` file edit.

### Before you start: map the blast radius

**Run `drft impact <files> --format json` on the files you plan to change before writing any code.** This is navigation, not verification. The output shows every file that transitively depends on the files you're about to touch — docs, examples, READMEs that reference the module, configs that name a rule. Use this to build the task list: impacted files are tasks, not afterthoughts. On wide changes (core types, rule renames, config schema), this step prevents a cleanup pass at the end.

### During editing: respond to the hook

**When the hook reports impacted files, STOP.** The output lists every file that transitively depends on the file you just edited. These are files whose content may be out of date because of your change. Read each one and check whether it still accurately reflects the source it depends on.

**Reviewing impacted files means reading them.** Check all content that could be affected by your change — not just prose, but code examples, JSON snippets, data structures, command invocations, and any content that mirrors or describes the file you changed. A doc that links to a source file is making a promise that its content reflects that source. When the source changes, verify the promise still holds.

Do not suppress warnings by removing links, ignoring paths, or disabling rules — fix the root cause (create missing files, fix broken references, restructure links).

**Never run `drft lock` unless the user explicitly asks.** Locking acknowledges that the current state has been reviewed — it is the user's decision, not the agent's. Running lock unprompted clears staleness signals the user may want to inspect. If you believe a lock is warranted, say so and let the user decide.

The workflow: impact upfront → plan includes dependents → edit → hook fires → review impacts inline → user runs `drft lock` when satisfied → commit.

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
- `serde_yml` for YAML frontmatter metadata extraction
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
- Lockfile (`drft.lock`): TOML v2 format, nodes + hashes only (no edges), fully deterministic, no timestamps
- Config (`drft.toml`): TOML, unified `[parsers]` and `[rules]` sections, `[interface]` for graph boundary
- Hashes use BLAKE3 with `b3:` prefix
- Edges carry the parser name as provenance (e.g., `"parser": "markdown"`)
- Node types: `File` (matched by `include`, hashed, tracked, parsed), `Directory` (directory on disk, `is_graph` when `drft.toml` exists, hashed via `drft.lock`), `External` (discovered via edge, hashed when on disk)
- Parsers are configurable via `[parsers]` — built-in (markdown, frontmatter) or custom (`command` field)
- Tests go in `tests/` (integration) and inline `#[cfg(test)]` modules (unit)
- Keep modules focused: one file per concern (discovery, parsers, graph, analyses, metrics, rules, lockfile, config, cli)
- Pipeline: `src/parsers/` (parse links) → [`src/graph.rs`](src/graph.rs) (build graph) → `src/analyses/` (compute properties) → [`src/metrics.rs`](src/metrics.rs) (extract scalars) → `src/rules/` (emit diagnostics)

## Git workflow

Main is protected. All changes go through branches and pull requests — never push directly to main.

## Releasing

See [RELEASING.md](RELEASING.md). Releases go through a PR, then tag on main after merge.
