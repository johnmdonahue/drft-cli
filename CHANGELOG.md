# Changelog

All notable changes to drft are documented here.

## 0.5.0 (2026-03-31)

drft.toml as the sole graph marker — simpler mental model, no ordering constraints.

### Breaking changes

- **Require `drft.toml` to run** — `drft check`, `drft lock`, etc. exit 2 without a config file instead of silently applying defaults.
- **Child graph discovery by `drft.toml` only** — bare `drft.lock` no longer marks a graph boundary.
- **Directory nodes hashed from `drft.toml`** — parent tracks child config for staleness; no dependency on child lockfiles.
- **`Graph` node type replaced by `Directory`** — `NodeType::Graph` is now `NodeType::Directory` with an `is_graph` boolean. JSON output, lockfiles, and custom rule input use `"directory"` instead of `"graph"`.
- **Node `graph` field semantics expanded** — indicates graph membership: `"."` (local), `".."` (escape), child graph name, or `null` (not on filesystem). Replaces the previous "set only for child graph nodes" convention.
- **`[interface] nodes` renamed to `[interface] files`** — config and lockfile both changed. New `ignore` field for excluding interface paths.
- **`directory-edge` rule replaced by `untrackable-target`** — the config key changed; `directory-edge` is no longer recognized.
- **Child graph paths normalized** — no trailing slash (`"research"` not `"research/"`).
- **"Script" terminology renamed to "custom"** — "script parsers" and "script rules" are now "custom parsers" and "custom rules" in docs and source.

### New features

- **`drft config show`** — display the resolved configuration (defaults filled in). Supports `--format json` and `--recursive`.
- **Per-rule `files` scoping** — `[rules.<name>] files = ["docs/**"]` restricts which nodes a rule evaluates, complementing the existing `ignore` field.

### Fixes

- **Directory staleness detection** — `compute_current_hash` was hashing `drft.lock` while `build_graph` hashed `drft.toml`, causing child graphs to always appear stale.
- **Interface file promotion** — now honors child's `exclude` and interface `ignore` patterns.
- **`untrackable-target` rule** — restored with updated message ("add a drft.toml" instead of "create a lockfile").

## 0.4.0 (2026-03-31)

Graph model redesign — explicit graph declaration, pure rules, parser metadata, and enriched impact analysis.

### Breaking changes

- **Graph declaration**: `include`/`exclude` replaces implicit parser-glob union. `ignore` renamed to `exclude`.
- **Node types**: `Source` → `File`, `Resource` removed. Three types: File, External, Graph.
- **Rule renames**: `broken-link` → `dangling-edge`, `cycle` → `directed-cycle`, `containment` → `boundary-violation`, `encapsulation` → `encapsulation-violation`, `orphan` → `orphan-node`, `indirect-link` → `symlink-edge`, `directory-link` → `directory-edge`.
- **Parser contract**: `glob` replaced by `files` (array of globs), `types` replaced by `options` (arbitrary structured data). Parsers return raw link strings — graph builder owns normalization.
- **Edge model simplified**: `RawLink`/`EdgeType` removed. Edge = `{ source, target, link?, parser }`.
- **RuleContext**: reduced to `{ graph: &EnrichedGraph, options }`. No filesystem access, no config.
- **Lockfile**: regenerated with new node types.

### New features

- **`drft parse`** command: raw parser output for debugging script parsers and the options envelope protocol.
- **Frontmatter parser**: standalone built-in parser for YAML frontmatter (links + structured metadata).
- **Schema-violation rule**: validates node metadata against glob-scoped schemas with required/allowed fields. First consumer of parser metadata and rule options.
- **Impact-radius analysis**: per-node blast zone (transitive dependents, depth, direct count).
- **Enriched `drft impact`**: output annotated with depth, impact_radius, and betweenness; sorted by review priority.
- **Rule options**: `[rules.<name>.options]` for arbitrary structured data passed through to rules.
- **Parser options**: `[parsers.<name>.options]` passed to script parsers as JSON envelope on stdin.
- **Script rule enrichment**: script rules receive `{ graph, options }` with all 12 analyses.

### Fixes

- Replace deprecated `serde_yaml` with maintained `serde_yml` fork.
- Deterministic metadata merge across parser namespaces (sorted by key).
- Warning on invalid glob patterns in schema-violation options.
- `drft graph --dot` replaces `--format dot` (DOT output is graph-only).

## 0.3.0 (2026-03-30)

Major architecture overhaul: drft is now a structural integrity checker for any linked file system, not just markdown.

### Breaking changes

- Config format: unified `[parsers]` and `[rules]` sections replace prior layout
- Lockfile v2: nodes + hashes only, no edges
- `scope` terminology renamed to `graph` throughout

### New features

- **Configurable parsers**: built-in markdown parser + script-based parsers via `command` field
- **Batch script parsers**: one process per parser instead of one per file (PR #19)
- **Rust doc comment parser**: links source files to docs via `parse-rust.sh`
- **`drft report`** (unstable): unified command for 11 graph analyses and 15 scalar health metrics — run all with `drft report`, filter by name with `drft report depth orphan_ratio`
- **Custom analyses and custom metrics** via external scripts
- **Criterion benchmarks** for the full pipeline

### Rules

- New: `fragmentation`, `layer-violation`, `redundant-edge`
- `stale` rule now defaults to error severity
- Rules refactored to consume analysis results

### Fixes

- Wikilink/frontmatter parsers skip inline code spans and code blocks
- Ignore patterns now apply to child graph detection
- Dropped lockfile version migration check

## 0.2.1 (2026-03-29)

- Fix #9: boundary-violation rule now catches `../` edges escaping graph boundary
- Fix #11: custom rule commands resolve relative to config file, not CWD
- Fix #8: required-frontmatter example adds file exemptions (SKIP_NAMES)
- `lockfile-outdated` rule: `drft check` detects when lockfile doesn't match current graph
- Config inheritance: child graphs without `drft.toml` inherit from nearest ancestor
- Interface persisted in `drft.toml`: `[interface]` section is source of truth
- Failed custom rules now surface as diagnostics in JSON output

## 0.2.0 (2026-03-29)

- `--rule` filtering now works for custom rules
- `npx drft` documented for npm-based projects
- New custom rule examples: required-frontmatter, max-depth

## 0.1.3 (2026-03-29)

- Fix npm package downloading binaries from wrong release version

## 0.1.2 (2026-03-29)

- Add `lockfile-outdated` rule: `drft check` detects when lockfile doesn't match current graph
- Config inheritance: child graphs without `drft.toml` inherit from nearest ancestor
- Persist interface in `drft.toml`: `[interface]` section is the source of truth
- Add `drft impact` command for transitive dependency analysis
- JSON summary envelope for `drft check --format json`
- Structured JSON errors on stderr when `--format json` is set
- JSON Graph Format (JGF) output for `drft graph`
- Custom script rules via `[custom-rules]` in config
- Per-rule path ignores via `[ignore-rules]` in config
- `--max-depth` flag for recursive operations
- `--watch` mode for `drft check`
- Colored terminal output
- Diagnostics include `fix` field with actionable instructions
- Direct + transitive staleness differentiation
- `.gitignore` respect via `ignore` crate
- Lockfile version checking
- Fixed: email links no longer flagged as broken links
- Fixed: frontmatter parser rejects YAML objects/arrays/quoted strings
- Fixed: cycle detection panic on DFS root nodes
- Fixed: untrackable-target rule (was directory-edge) skips Graph nodes
- Fixed: ignored files detected as "excluded by ignore pattern" in dangling-edge

## 0.1.1 (2026-03-28)

- Fixed npm postinstall binary download
- Added CI and automated publish workflows

## 0.1.0 (2026-03-28)

Initial release.

### Commands

- `drft init` -- create default config
- `drft lock` -- snapshot file hashes and dependency graph
- `drft lock --check` -- verify lockfile is current (CI)
- `drft check` -- validate graph against rules
- `drft graph` -- export dependency graph (JSON Graph Format, DOT)
- `drft impact` -- show transitive dependents of given files
- `--recursive` flag for lock, check, and graph
- `--max-depth` flag to limit recursive depth
- `--watch` flag for check

### Rules

- `dangling-edge` -- missing edge targets, including files excluded by ignore patterns
- `boundary-violation` -- edges escaping graph boundary
- `directed-cycle` -- circular dependencies
- `untrackable-target` -- directory targets with no lockfile
- `encapsulation-violation` -- edges into child graph's non-interface files
- `symlink-edge` -- symlink targets
- `orphan-node` -- nodes with no inbound edges
- `stale` -- dependencies changed since last lock (direct + transitive)

### Features

- 6 link source types: inline, reference, autolink, image, frontmatter, wikilink
- 4 node types: Source, Resource, External, Graph
- BLAKE3 content hashing (`b3:` prefix)
- Hierarchical graphs with child-graph projection
- Interface support for child graphs
- `.gitignore` respect
- Per-rule path ignores (`[ignore-rules]`)
- Custom rules via external scripts (`[custom-rules]`)
- Colored terminal output (`--color`)
- JSON diagnostics with `fix` field and summary envelope for LLM workflows
- JSON Graph Format output for graph export
- Lockfile version checking (forward-compatible)

### Distribution

- Cargo: `cargo install drft-cli`
- npm: `npm install drft-cli`
- GitHub Releases: prebuilt binaries for macOS, Linux, and Windows
