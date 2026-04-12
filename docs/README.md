# Documentation

drft builds a dependency graph from your files and validates it. The documentation is organized by the pipeline: parsers extract links, the graph builder normalizes them into a graph, analyses compute structural properties, and rules emit diagnostics.

## Reference

- [Configuration](config.md) — `drft.toml` options: include/exclude, parsers, rules
- [File discovery](discovery.md) — include/exclude patterns, glob semantics, gitignore interaction
- [Parsers](parsers/README.md) — how links and metadata are extracted from files
- [Graph builder](graph.md) — normalization, edge classification, path resolution, lockfile
- [Analyses](analyses/README.md) — structural properties computed from the graph
- [Rules](rules/README.md) — diagnostics emitted when the graph violates constraints
