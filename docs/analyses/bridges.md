# Articulation points and bridges

## The concept

An **articulation point** (or cut vertex) is a node whose removal disconnects the graph. A **bridge** is an edge whose removal disconnects the graph. Both indicate structural single points of failure.

```
a ── b ── c ── d
     ▲         ▲
     cut vertex bridge (c—d)
```

If you remove node `b`, nodes `a` and `c,d` can no longer reach each other. The edge `c—d` is the only connection to `d`.

## Why it matters for knowledge systems

Cut vertices and bridges reveal fragility in your documentation structure:

- **Cut vertex documents** are single points of failure. If a cut vertex is deleted, moved, or becomes stale, entire sections of the graph become unreachable. Consider adding alternative paths.
- **Bridge edges** are the only connection between two parts of the graph. They are the minimum set of links you cannot remove without fragmenting the structure.

## What drft surfaces

### As an analysis (`drft report`)

```bash
drft report bridges
```

```
=== bridges ===
2 cut vertices, 1 bridge
cut vertex: hub.md
cut vertex: index.md
bridge: setup.md ↔ advanced.md
```

### As a rule (`drft check`)

The `fragility` rule warns about cut vertices and bridge edges:

```
warn[fragility]: hub.md (cut vertex)
warn[fragility]: setup.md → advanced.md (bridge edge)
```

Enable it in `drft.toml`:

```toml
[rules]
fragility = "warn"
```

## Algorithm

Treats the directed graph as undirected. Uses [Tarjan's bridge-finding algorithm](https://en.wikipedia.org/wiki/Bridge_(graph_theory)#Tarjan's_bridge-finding_algorithm): a single DFS pass maintaining discovery times and low-link values. A node is a cut vertex if it is the root with 2+ children, or has a child where `low[child] >= disc[node]`. An edge is a bridge if `low[child] > disc[node]`. Complexity is O(V + E).

## Source

[`src/analyses/bridges.rs`](../../src/analyses/bridges.rs)
