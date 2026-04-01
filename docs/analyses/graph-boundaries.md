---
sources:
  - ../../src/analyses/graph_boundaries.rs
---

# Graph boundaries

## The concept

drft graphs create **partitions** in the dependency graph. A graph is a directory with its own `drft.lock` — a boundary that separates its internal structure from the outside. **Graph boundary analysis** identifies edges that cross these partitions:

- **Escapes** — edges from inside a graph with interface to targets outside it (via `../` paths). These violate containment.
- **Encapsulation violations** — edges from outside a child graph to non-interface files inside it. These bypass the graph's declared interface.

## Why it matters for knowledge systems

Graphs let you decompose a large documentation system into independently manageable units. Boundary crossings undermine this:

- **Graph escapes** create hidden dependencies on parent/sibling content. If the graph is later moved or extracted, these links break.
- **Encapsulation violations** reach into a graph's internals, bypassing the interface that defines its public API. Changes to internal files can break consumers without warning.

## What drft surfaces

### As an analysis (`drft report`)

```bash
drft report graph-boundaries
```

```
=== graph-boundaries ===
has_interface: yes
escape: index.md → ../README.md
encapsulation: parent.md → research/internal.md (bypasses research/ interface)
```

JSON output:

```json
{
  "graph-boundaries": {
    "has_interface": true,
    "escapes": [
      { "source": "index.md", "target": "../README.md" }
    ],
    "encapsulation_violations": [
      {
        "source": "parent.md",
        "target": "research/internal.md",
        "graph": "research/"
      }
    ]
  }
}
```

### As rules (`drft check`)

Two rules consume this analysis:

- **`boundary-violation`** — flags graph escapes (only when interface is declared)
- **`encapsulation-violation`** — flags interface bypasses

## Algorithm

For escapes: checks for nodes with `graph: ".."` (targets that escaped the graph boundary). For encapsulation: iterates Directory nodes with `is_graph`, reads each child graph's interface configuration, and identifies edges from local nodes to child-graph nodes that are not in the interface.
