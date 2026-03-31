# Documentation

drft's documentation is organized by the pipeline: parsers extract, the graph builder normalizes, and rules judge.

## Pipeline

- [Parsers](parsers/README.md) — extract raw links and metadata from files
- [Graph builder](graph.md) — normalize targets, classify nodes, resolve paths, enrich
- [Analyses](analyses/README.md) — structural properties computed from the graph
- [Rules](rules/README.md) — diagnostics emitted when the graph violates constraints
