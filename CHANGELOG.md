# Changelog

All notable changes to drft are documented here.

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
- **`drft analysis`** (unstable): 11 graph analyses — degree, SCC, connected components, depth, graph stats, bridges, betweenness centrality, PageRank, transitive reduction, graph boundaries, change propagation
- **`drft metrics`** (unstable): scalar health metrics extracted from analyses
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

- Fix #9: containment rule now catches `../` links escaping graph boundary
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
- Fixed: directory-link rule skips Graph nodes
- Fixed: ignored files detected as "excluded by ignore pattern" in broken-link

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
- `broken-link` -- missing link targets, including files excluded by ignore patterns
- `containment` -- links escaping graph boundary
- `cycle` -- circular dependencies
- `directory-link` -- links to directories instead of files
- `encapsulation` -- links into child graph's non-interface files
- `indirect-link` -- symlink targets
- `orphan` -- files with no inbound links
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
