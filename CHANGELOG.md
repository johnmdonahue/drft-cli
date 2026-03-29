# Changelog

All notable changes to drft are documented here.

## 0.1.2 (2026-03-29)

- Add `lockfile-outdated` rule: `drft check` detects when lockfile doesn't match current graph
- Config inheritance: child scopes without `drft.toml` inherit from nearest ancestor
- Persist manifest in `drft.toml`: `manifest = "README.md"` is the source of truth
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
- Fixed: directory-link rule skips frontier nodes
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
- `containment` -- links escaping scope boundary
- `cycle` -- circular dependencies
- `directory-link` -- links to directories instead of files
- `encapsulation` -- links into sealed scope's non-manifest files
- `indirect-link` -- symlink targets
- `orphan` -- files with no inbound links
- `stale` -- dependencies changed since last lock (direct + transitive)

### Features
- 6 link source types: inline, reference, autolink, image, frontmatter, wikilink
- 5 node types: document, asset, external, frontier, virtual
- BLAKE3 content hashing (`b3:` prefix)
- Hierarchical scopes with frontier/virtual nodes
- Manifest support for sealed scopes
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
