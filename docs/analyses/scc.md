# Strongly connected components

## The concept

A **strongly connected component** (SCC) is a maximal set of nodes where every node can reach every other node following edge directions. In other words, there is a directed cycle connecting all members.

```
SCC {a, b, c}                  Not an SCC

a ──→ b                        a ──→ b ──→ c
▲     │                        (c cannot reach a)
└──── c ◄─┘
```

A trivial SCC contains a single node with no self-loop. Non-trivial SCCs (size > 1, or a self-loop) indicate cyclic dependencies.

## Why it matters for knowledge systems

The existing `cycle` rule tells you a cycle exists. SCCs tell you **exactly which documents** are entangled in each cycle:

- **Refactoring guidance.** Knowing the exact set of files in a cycle tells you where to break the dependency. A 2-node cycle is easy to resolve; a 12-node cycle may indicate a deeper structural issue.
- **Impact understanding.** Changes to any file in an SCC can propagate to every other file in the same SCC. Larger SCCs have larger blast radii.
- **Structural health.** Many small SCCs may be acceptable; one large SCC spanning most of the graph usually signals that the document hierarchy has collapsed.

## What drft surfaces

### As an analysis (`drft report`)

```bash
drft report --analysis scc
```

```
=== scc ===
1 non-trivial SCC
scc 1 (3 nodes): a.md, b.md, c.md
```

For acyclic graphs:

```
=== scc ===
no non-trivial SCCs (graph is acyclic)
```

JSON output:

```json
{
  "analyses": {
    "scc": {
      "scc_count": 4,
      "nontrivial_count": 1,
      "sccs": [
        { "id": 1, "members": ["a.md", "b.md", "c.md"] }
      ],
      "node_scc": {
        "a.md": 1,
        "b.md": 1,
        "c.md": 1,
        "d.md": 2
      }
    }
  }
}
```

`scc_count` includes trivial SCCs (single nodes). `nontrivial_count` and `sccs` only reflect SCCs with size > 1 or self-loops. `node_scc` maps every real node to its SCC ID.

## Algorithm

Uses [Tarjan's algorithm](https://en.wikipedia.org/wiki/Tarjan%27s_strongly_connected_components_algorithm): a single DFS pass with a stack, maintaining discovery index and low-link values. When a node's low-link equals its index, the stack is popped to form an SCC. Only edges between real nodes are followed. Complexity is O(V + E).
