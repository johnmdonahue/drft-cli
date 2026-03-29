# Graph stats

## The concept

**Graph stats** provides aggregate structural metrics for the entire graph: how many nodes and edges exist, how densely connected they are, and how far apart the most distant nodes are.

- **Density** — the ratio of actual edges to the maximum possible edges. A density of 1.0 means every node links to every other node; near 0 means the graph is sparse.
- **Diameter** — the longest shortest path between any two nodes. A small diameter means information is reachable in few hops.
- **Average path length** — the mean shortest-path distance across all pairs of nodes. Indicates how "close" documents are to each other on average.

## Why it matters for knowledge systems

These metrics give you a high-level health check:

- **High density** often indicates a "link everything to everything" pattern that dilutes the meaning of links.
- **Low density** suggests a sparse, tree-like structure — which may be intentional (layered hierarchy) or a sign of poor cross-referencing.
- **Large diameter** means some documents are many hops apart, which can make navigation difficult.
- **Disconnected graph** (diameter unavailable) means some documents can't reach others at all — see the `connected-components` analysis for details.

## What drft surfaces

### As an analysis (`drft report`)

```bash
drft report --analysis graph-stats
```

```
=== graph-stats ===
nodes: 12
edges: 23
density: 0.17
diameter: 4
avg path length: 2.3
```

If the directed graph is not strongly connected (some nodes can't reach others), diameter and average path length are reported as unavailable:

```
diameter: - (disconnected)
avg path length: - (disconnected)
```

JSON output:

```json
{
  "analyses": {
    "graph-stats": {
      "node_count": 12,
      "edge_count": 23,
      "density": 0.17,
      "diameter": 4,
      "average_path_length": 2.3
    }
  }
}
```

When the graph is disconnected, `diameter` and `average_path_length` are `null` in JSON.

## Algorithm

Density is computed as `|E| / (|V| * (|V| - 1))` for a directed graph. Diameter and average path length are computed via BFS from each real node (all-pairs shortest paths). This is O(V * (V + E)), which is fast for the size of markdown repos.
