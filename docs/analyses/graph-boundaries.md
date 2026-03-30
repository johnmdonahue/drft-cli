# Scope boundaries

## The concept

drft scopes create **partitions** in the graph. A scope is a directory with its own `drft.lock` — a boundary that separates its internal structure from the outside. **Scope boundary analysis** identifies edges that cross these partitions:

- **Escapes** — edges from inside a sealed scope to targets outside it (via `../` paths). These violate containment.
- **Encapsulation violations** — edges from outside a child scope to non-manifest files inside it. These bypass the scope's declared interface.

## Why it matters for knowledge systems

Scopes let you decompose a large documentation system into independently manageable units. Boundary crossings undermine this:

- **Scope escapes** create hidden dependencies on parent/sibling content. If the scope is later moved or extracted, these links break.
- **Encapsulation violations** reach into a scope's internals, bypassing the manifest that defines its public API. Changes to internal files can break consumers without warning.

## What drft surfaces

### As an analysis (`drft report`)

```bash
drft report --analysis scope-boundaries
```

```
=== scope-boundaries ===
sealed: yes
escape: index.md → ../README.md
encapsulation: parent.md → research/internal.md (bypasses research/manifest)
```

JSON output:

```json
{
  "analyses": {
    "scope-boundaries": {
      "sealed": true,
      "escapes": [
        { "source": "index.md", "target": "../README.md" }
      ],
      "encapsulation_violations": [
        {
          "source": "parent.md",
          "target": "research/internal.md",
          "scope": "research/",
          "manifest_file": "overview.md"
        }
      ]
    }
  }
}
```

### As rules (`drft check`)

Two rules consume this analysis:
- **`containment`** — flags scope escapes (only when sealed)
- **`encapsulation`** — flags manifest bypasses

## Algorithm

For escapes: checks `drft.lock` existence (sealed scope), then scans edges for `../` target prefixes. For encapsulation: iterates Frontier nodes, reads each child scope's lockfile manifest, and identifies edges targeting non-manifest files inside the scope. Skips edges from Virtual nodes (implicit scope-internal edges).

## Source

[`src/analyses/graph_boundaries.rs`](../../src/analyses/graph_boundaries.rs)
