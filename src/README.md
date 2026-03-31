# src

Core modules for the drft CLI.

## Entry points

- [main.rs](main.rs) — command dispatch, `run_parse`, `run_graph`, `run_check`, `run_report`, `run_lock`, etc.
- [cli.rs](cli.rs) — clap-derived CLI definition and argument parsing
- [lib.rs](lib.rs) — library crate root, re-exports public modules

## Graph construction

- [discovery.rs](discovery.rs) — .gitignore-aware file discovery and child graph detection
- [graph.rs](graph.rs) — `Graph`, `Node`, `Edge`, `EdgeType` types; `build_graph` construction
- [config.rs](config.rs) — config loading, defaults, parser/rule configuration
- [lockfile.rs](lockfile.rs) — lockfile read/write, hash comparison

## Output

- [diagnostic.rs](diagnostic.rs) — `Diagnostic` struct, text/JSON formatting
- [metrics.rs](metrics.rs) — `Metric` type, `MetricKind`, `compute_metrics()`

## Subdirectories

- [parsers/](parsers/README.md) — parser trait, built-in markdown parser, script-based parser runner
- [analyses/](analyses/README.md) — analysis trait, 11 graph analyses
- [rules/](rules/README.md) — rule trait, 14 built-in rules, script-based rule runner
