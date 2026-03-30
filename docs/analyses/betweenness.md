# Betweenness centrality

## The concept

**Betweenness centrality** measures how often a node appears on the shortest paths between other nodes. A node with high betweenness acts as a bridge — many shortest paths pass through it.

Scores are normalized between 0 and 1. A score of 0 means the node lies on no shortest paths between other nodes. A score of 1 would mean every shortest path passes through it (rare in practice).

## Why it matters for knowledge systems

Betweenness identifies **bridge documents** — files that connect different parts of the knowledge graph:

- **High betweenness** documents are structurally important connectors. They link otherwise distant parts of the graph. Changes to these documents can disrupt navigation paths.
- **Low betweenness** documents are either leaf nodes (endpoints) or deeply embedded within a cluster. They are less structurally critical.
- **Unexpectedly high betweenness** on a document that shouldn't be a hub may indicate missing cross-links elsewhere.

## What drft surfaces

### As an analysis (`drft report`)

```bash
drft report betweenness
```

```
=== betweenness ===
hub.md      0.7200
index.md    0.4500
setup.md    0.1200
leaf.md     0.0000
```

Nodes are sorted by score (highest first).

JSON output:

```json
{
  "betweenness": {
    "nodes": [
      { "node": "hub.md", "score": 0.72 },
      { "node": "index.md", "score": 0.45 }
    ]
  }
}
```

## Algorithm

Uses [Brandes' algorithm](https://en.wikipedia.org/wiki/Betweenness_centrality#Brandes'_algorithm) on the directed graph. For each source node, BFS computes shortest paths and predecessor lists, then back-propagation accumulates pair-dependencies. Scores are normalized by `(n-1)*(n-2)` for directed graphs. Complexity is O(V * E).

## Source

[`src/analyses/betweenness.rs`](../../src/analyses/betweenness.rs)
