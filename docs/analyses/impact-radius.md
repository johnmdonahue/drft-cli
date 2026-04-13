---
sources:
  - ../../src/analyses/impact_radius.rs
---

# Impact radius

## The concept

Impact radius measures the blast zone when a file changes. For each File node, it counts how many other File nodes are transitively affected — walking reverse edges (dependents) outward from the node. A file with high impact radius is load-bearing: when it changes, many downstream files may need review.

This is the reverse complement of depth. Depth asks "how far is this node from a root?" Impact radius asks "how far does damage travel when this node changes?"

## Relationship to other analyses

- **Degree** tells you local connectivity (direct in/out). Impact radius tells you transitive reach in one direction (reverse).
- **Betweenness** tells you how often a node bridges paths. Impact radius tells you how large the blast zone is when it changes.
- **PageRank** tells you structural importance via link weight. Impact radius tells you maintenance cost.

## Output

```json
{
  "nodes": [
    { "node": "shared/glossary.md", "radius": 14, "direct_dependents": 3, "max_depth": 5 },
    { "node": "guides/setup.md", "radius": 2, "direct_dependents": 2, "max_depth": 1 },
    { "node": "notes/scratch.md", "radius": 0, "direct_dependents": 0, "max_depth": 0 }
  ]
}
```

| Field               | Description                                                        |
| ------------------- | ------------------------------------------------------------------ |
| `radius`            | Count of transitive dependents (nodes reachable via reverse edges) |
| `direct_dependents` | Count of direct dependents (immediate reverse neighbors)           |
| `max_depth`         | Longest reverse path from this node to a dependent                 |

Nodes are sorted alphabetically by path. Only included nodes appear in the analysis output and are valid seeds for `drft impact`.

## Used by `drft impact`

The `drft impact` command annotates each impacted node with its impact radius and betweenness centrality, and sorts results by review priority. A high-radius node at shallow depth is the most important thing to review — missing it cascades the furthest.

## Algorithm

BFS on reverse edges per seed node. For each node, walk all transitive dependents, counting total reach (radius), immediate neighbors (direct_dependents), and maximum path length (max_depth). Non-included dependents are skipped during traversal. O(V*(V+E)) worst case, negligible at drft scale.
