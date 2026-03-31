# Connected components

## The concept

A **connected component** is a maximal set of nodes where every node can reach every other node when edge direction is ignored. If your graph has more than one component, it means some files are completely disconnected from the rest — there is no path of links (in either direction) between them.

```
Component 1             Component 2

a.md ──→ b.md           c.md ──→ d.md
  │
  └──→ e.md
```

These two groups cannot reach each other through any chain of links.

## Why it matters for knowledge systems

A fragmented knowledge graph is usually a sign of structural drift:

- **Orphaned clusters.** A group of files that was once connected to the main graph lost its links during a refactor. The content is still there, but nothing leads to it.
- **Parallel structures.** Two teams or efforts independently built overlapping documentation without cross-referencing each other.
- **Incomplete onboarding.** New files were added but never linked into the existing structure.

A single connected component means every document is reachable from every other document (ignoring direction). This doesn't mean every file needs a direct link — transitive reachability through chains of links is sufficient.

## What drft surfaces

### As an analysis (`drft report`)

```bash
drft report connected-components
```

```
=== connected-components ===
2 components
component 1 (3 nodes): a.md, b.md, e.md
component 2 (2 nodes): c.md, d.md
```

JSON output:

```json
{
  "connected-components": {
    "component_count": 2,
    "components": [
      { "id": 1, "members": ["a.md", "b.md", "e.md"] },
      { "id": 2, "members": ["c.md", "d.md"] }
    ]
  }
}
```

Components are sorted by size (largest first). Members within each component are sorted alphabetically. Only File nodes are included — External and Graph nodes are excluded.

### As a rule (`drft check`)

The `fragmentation` rule warns when the graph has more than one connected component:

```
warn[fragmentation]: c.md, d.md (disconnected component (2 nodes))
```

Enable it in `drft.toml`:

```toml
[rules]
fragmentation = "warn"
```

## Algorithm

Treats the directed graph as undirected by considering both forward and reverse edges. Runs BFS from each unvisited real node to discover components. This is O(V + E).

## Source

[`src/analyses/connected_components.rs`](../../src/analyses/connected_components.rs)
