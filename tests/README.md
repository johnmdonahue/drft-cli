# tests

Integration tests for drft CLI commands. Each file exercises one command or feature area.

## Commands

- [check.rs](check.rs) — `drft check`: broken links, cycles, untrackable targets, orphans, rule filtering
- [config.rs](config.rs) — `drft config show`: defaults, custom config, JSON, recursive
- [graph.rs](graph.rs) — `drft graph`: JGF output, recursive multi-graph
- [impact.rs](impact.rs) — `drft impact`: transitive dependents
- [init.rs](init.rs) — `drft init`: config scaffolding
- [lock.rs](lock.rs) — `drft lock`: first lock, staleness, `--check` mode
- [report.rs](report.rs) — `drft report`: analysis/metric output, filtering
- [output.rs](output.rs) — output formatting (text, JSON)

## Features

- [graphs.rs](graphs.rs) — child graphs, encapsulation, recursive check/lock
- [parsers.rs](parsers.rs) — frontmatter, wikilinks (custom parser), custom parser batch protocol
- [rules.rs](rules.rs) — individual rule behavior, ignore patterns, JSON output
- [custom_rules.rs](custom_rules.rs) — custom rules via external commands

## Shared

- [common/mod.rs](common/mod.rs) — test harness helpers (`drft_bin()`)
