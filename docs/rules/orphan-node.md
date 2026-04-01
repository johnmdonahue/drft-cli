---
sources:
  - ../../src/rules/orphan_node.rs
---

# orphan-node

Flags nodes with no connections — no inbound edges and no outbound edges. These are files that exist in the graph but don't participate in any link relationships.

Files with outbound links but no inbound links (like `index.md` or `README.md`) are roots, not orphans. They are entry points into the graph and are not flagged.

## Example

```
docs/
  index.md       # links to setup.md
  setup.md
  stray.md       # nothing links here, links to nothing
```

```
warn[orphan-node]: stray.md (no connections)
```

`index.md` is not flagged — it has outbound links, making it a root node.

## Configuration

```toml
[rules]
orphan-node = "warn" # default
```

```toml
[rules.orphan-node]
severity = "warn"
ignore = ["CHANGELOG.md"]
```

## Analysis

Uses the [degree](../analyses/degree.md) analysis, which computes in-degree and out-degree for every node.
