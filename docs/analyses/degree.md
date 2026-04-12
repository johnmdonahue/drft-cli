---
sources:
  - ../../src/analyses/degree.rs
---

# Degree distribution

## The concept

The **degree** of a node in a directed graph is the number of edges connected to it. In-degree counts inbound edges (how many files link to this one); out-degree counts outbound edges (how many files this one links to). Together they describe a node's role in the graph.

```
     a.md           in:0  out:2
    ↙    ↘
b.md      c.md      in:1  out:1  /  in:1  out:0
    ↘
     d.md           in:1  out:0
```

Nodes with high out-degree are hubs (index pages, tables of contents). Nodes with high in-degree are foundational (frequently referenced concepts). Nodes with zero in-degree are orphans — nothing links to them.

## Why it matters for knowledge systems

Degree distribution reveals the structural role of each document:

- **in-degree = 0 (orphan):** A file no one references. It may be abandoned, misplaced, or a root entry point that should be explicitly designated. The `orphan` rule flags these.
- **High out-degree:** Hub documents that link to many others. These are often index files or overviews. When they have too many outbound links, they can become hard to maintain.
- **High in-degree:** Foundational documents that many others depend on. Changes to these files have outsized impact — `drft impact` will show many dependents.
- **in-degree = 0, out-degree = 0:** Completely isolated files with no connections at all.

## What drft surfaces

### As an analysis (`drft report`)

```bash
drft report degree
```

```
=== degree ===
a.md  in:0  out:2
b.md  in:1  out:1
c.md  in:1  out:0
d.md  in:1  out:0
```

JSON output:

```json
{
  "degree": {
    "nodes": [
      { "node": "a.md", "in_degree": 0, "out_degree": 2 },
      { "node": "b.md", "in_degree": 1, "out_degree": 1 },
      { "node": "c.md", "in_degree": 1, "out_degree": 0 },
      { "node": "d.md", "in_degree": 1, "out_degree": 0 }
    ]
  }
}
```

Nodes are sorted alphabetically by path. Degree counts all edges, including those to external targets.

### As a rule (`drft check`)

The `orphan-node` rule consumes the degree analysis and flags nodes with in-degree = 0:

```
warn[orphan-node]: a.md (no inbound links)
```

Enable it in `drft.toml`:

```toml
[rules]
orphan-node = "warn"
```

## Algorithm

For each real node, count the entries in the forward adjacency list (out-degree) and reverse adjacency list (in-degree), filtering to only count edges where the other endpoint is also a real node. This is O(V + E).
