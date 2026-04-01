---
sources:
  - ../../src/analyses/pagerank.rs
---

# PageRank

## The concept

**PageRank** assigns each node an importance score based on the link structure. A node is important if important nodes link to it. Scores are between 0 and 1 and sum to 1 across all nodes.

The intuition: imagine randomly clicking links in your documentation. PageRank is the probability of landing on each page after clicking for a long time, with a small chance of jumping to a random page on each step.

## Why it matters for knowledge systems

PageRank reveals which documents are structurally most important:

- **High PageRank** documents are referenced (directly or indirectly) by many other important documents. These are the foundational pages of your knowledge system.
- **Low PageRank** documents are peripheral — either leaf pages or pages referenced only by other unimportant pages.
- **Surprising results** (e.g., a page you consider important having low PageRank) may indicate missing inbound links.

## What drft surfaces

### As an analysis (`drft report`)

```bash
drft report pagerank
```

```
=== pagerank ===
converged in 23 iterations
index.md    0.1800
hub.md      0.1200
setup.md    0.0900
leaf.md     0.0400
```

Nodes are sorted by score (highest first). Scores sum to approximately 1.0.

JSON output:

```json
{
  "pagerank": {
    "iterations": 23,
    "converged": true,
    "nodes": [
      { "node": "index.md", "score": 0.18 },
      { "node": "hub.md", "score": 0.12 }
    ]
  }
}
```

## Algorithm

Uses the power iteration method with damping factor d = 0.85, maximum 100 iterations, and convergence threshold ε = 1e-6. Dangling nodes (out-degree 0) redistribute their rank evenly to all nodes. Convergence is checked via the L1 norm of the rank difference vector. Complexity is O(iterations * (V + E)).
