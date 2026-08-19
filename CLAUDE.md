# drft

A drift checker for linked files, built for LLMs and humans working in the same repo. It treats a directory of files as a dependency graph — files are nodes, links are edges — and flags what drifts when a dependency changes.

## Dogfooding

This repo runs drft on itself (`drft.toml` at root). A PreToolUse hook runs `drft impact <file> --format json` before every `.md` and `.rs` file edit.

### Before you start: map the blast radius

**Run `drft impact <files> --format json` on the files you plan to change before writing any code.** This is navigation, not verification. The output shows the files that name the ones you're about to touch — docs, examples, READMEs that reference the module, configs that name a rule. Use this to build the task list: impacted files are tasks, not afterthoughts. On wide changes (core types, rule renames, config schema), this step prevents a cleanup pass at the end.

`impact` reports one hop by default. Each result carries a `radius` — the count of nodes behind it — so when a hit turns out to restate your change, widen with `--depth <n>`, or `--depth all` for the full reachable set on a rename sweep.

### During editing: respond to the hook

**When the hook reports impacted files, STOP.** The output lists every file that transitively depends on the file you just edited. These are files whose content may be out of date because of your change. Read each one and check whether it still accurately reflects the source it depends on.

**Reviewing impacted files means reading them.** Check all content that could be affected by your change — not just prose, but code examples, JSON snippets, data structures, command invocations, and any content that mirrors or describes the file you changed. A doc that links to a source file is making a promise that its content reflects that source. When the source changes, verify the promise still holds.

Do not suppress warnings by removing links, ignoring paths, or disabling rules — fix the root cause (create missing files, fix broken references, restructure links).

**Lock only what you reviewed, by name.** A lock asserts the locked state was read and is correct, so the assertion has to stay narrow enough to be true:

- **You may** run `drft lock <path>...` for paths you edited this session, once you have reviewed their impacted dependents. Name every path.
- **Never run bare `drft lock`.** It clears staleness across the whole graph, including files you never opened and work someone else has in flight. That launders an unreviewed state into a reviewed one, and the record that it was never read is gone.
- **Never lock staleness you did not cause.** If `drft check` reports findings you cannot account for, stop and say so — those are the user's to inspect.

The one bare lock that is correct is **regenerating a baseline**: no `drft.lock` exists, or a release requires every lockfile to be rewritten (0.8.0, 0.9.0, and 0.10.0 each did). There is no prior baseline to preserve, so nothing is being laundered. Ask first, and say that is what you are doing — it is the call the rule otherwise forbids.

**A `stale-edge` clears from the declaring side.** An edge's recorded target hash lives on the file that wrote the link, so locking a changed source clears its own `stale-node` and leaves every inbound `stale-edge` standing. Those clear when you lock the dependent — the file whose promise you actually checked. Scoped locking therefore cannot wave off the review it exists to record.

A file with **no dependents** has no promise to check. Its staleness is bookkeeping — lock it and move on.

The workflow: impact upfront → plan includes dependents → edit → hook fires → review impacts inline → `drft lock <the files you touched>` → commit.

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

## Scratch files

`.scratch/` is a gitignored working-tree directory for ephemeral design notes, exploration, and planning docs. Files there are not versioned and are not visible to anyone who clones the repo.

Put ephemeral notes — design explorations, implementation plans, scratch thinking — in `.scratch/`, not in the repo root or alongside source files. This keeps the tracked tree clean and prevents accidental commits of working notes.

Never reference `.scratch/` files from durable artifacts — commit messages, PR descriptions, code comments, CHANGELOG, or any checked-in doc. A reader who clones the repo cannot follow the link. If anything in a scratch doc needs to be durable, graduate it to a non-ignored location (docs, PR body, or a code comment) before landing the work.

## Releasing

See [RELEASING.md](RELEASING.md). Releases go through a PR, then tag on main after merge.
