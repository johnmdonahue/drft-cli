---
purpose: map of drft's reference documentation
---

# Documentation

drft is a drift checker for linked files, built for LLMs and humans working in
the same repo. It builds a dependency graph from your files and checks it for
drift. The substrate is a **set of independent graphs** — `fs` (a node per file,
typed and hashed), `markdown` (link edges), and `frontmatter` (edges plus
metadata). A composition step merges them by path into one graph, and rules read
the composed graph to emit findings.

## Reference

- [Configuration](config.md) — `drft.toml`: `ignore`, `[graphs.*]`, `[rules.*]`
- [Parsers](parsers/README.md) — how parsers extract links and metadata
- [Rules](rules/README.md) — the drift and structural findings `drft check` emits
- [Reading the graph](reading.md) — the `nodes`, `edges`, and `graph` read verbs, and grounding an agent on graph metadata
