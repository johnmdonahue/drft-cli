# Changelog

All notable changes to drft are documented here.

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
