# Topological depth

## The concept

**Topological depth** assigns each node its distance from the nearest root (a node with in-degree 0). Roots are at depth 0. Nodes reachable only through one hop are at depth 1, and so on. When multiple paths lead to a node, the longest path determines its depth (giving the "deepest" layer assignment).

```
depth 0:  index.md
depth 1:  setup.md, overview.md
depth 2:  details/install.md
```

For graphs with cycles, drft uses SCC contraction: all nodes in a cycle are assigned the same depth (the depth of their contracted super-node in the condensation DAG) and marked `in_cycle: true`.

## Why it matters for knowledge systems

In systems with intentional layering — like research → observations → design → acceptance criteria — depth reveals whether documents follow the intended hierarchy:

- **Correct layering.** Depth 0 documents are entry points. Depth 1 documents are one step removed. Each layer cites only the layer below it.
- **Layer violations.** A document at depth 0 linking directly to something at depth 3 "skips" intermediate layers, suggesting a structural shortcut.
- **Upward links.** A document at depth 2 linking to something at depth 0 reverses the flow, pointing "up" the hierarchy.
- **Cycle entanglement.** Nodes flagged `in_cycle` don't have a clean depth — they form a mutual dependency cluster.

## What drft surfaces

### As an analysis (`drft report`)

```bash
drft report depth
```

```
=== depth ===
depth 0: index.md
depth 1: setup.md, overview.md
depth 2: details/install.md
depth 1 (cyclic): a.md, b.md
```

JSON output:

```json
{
  "depth": {
    "max_depth": 2,
    "nodes": [
      { "node": "index.md", "depth": 0, "in_cycle": false },
      { "node": "overview.md", "depth": 1, "in_cycle": false },
      { "node": "setup.md", "depth": 1, "in_cycle": false },
      { "node": "a.md", "depth": 1, "in_cycle": true },
      { "node": "b.md", "depth": 1, "in_cycle": true },
      { "node": "details/install.md", "depth": 2, "in_cycle": false }
    ]
  }
}
```

### As a rule (`drft check`)

The `layer-violation` rule flags two kinds of issues:

- **Skip-layer link:** An edge from depth D to depth D+K where K > 1.
- **Upward link:** An edge from depth D to depth D' where D' < D.

Edges involving cyclic nodes are skipped (their depth is approximate).

```
warn[layer-violation]: a.md → c.md (skip-layer link (depth 0 → depth 2))
```

Enable it in `drft.toml`:

```toml
[rules]
layer-violation = "warn"
```

## Algorithm

1. Run the SCC analysis to identify cycles.
2. Contract each non-trivial SCC into a single super-node, producing a DAG.
3. BFS from all roots (super-nodes with in-degree 0), assigning each the maximum depth across all incoming paths.
4. Expand back: all members of a super-node get its depth and `in_cycle: true`.

Complexity is O(V + E) for the SCC analysis plus O(V + E) for the BFS.

## Source

[`src/analyses/depth.rs`](../../src/analyses/depth.rs)
