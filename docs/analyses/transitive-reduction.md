# Transitive reduction

## The concept

The **transitive reduction** of a directed graph is the smallest set of edges that preserves all reachability. If you can get from A to C through B, then a direct edge A → C is redundant — it adds no reachability that the path A → B → C doesn't already provide.

The difference between your actual graph and its transitive reduction is the set of **transitively redundant edges**: shortcuts that can be removed without changing which nodes can reach which other nodes.

```
Before                          Transitive reduction

A ──→ B ──→ C                   A ──→ B ──→ C
│           ▲
└───────────┘  ← redundant
```

The direct edge A → C is redundant because A already reaches C through B. Removing it doesn't change what A can reach — it only removes a shortcut.

## Why it matters for knowledge systems

In a file-based knowledge system with intentional layers — say, research → observations → design → acceptance criteria — each layer is meant to cite only the layer below it. A direct link that skips a layer is usually a structural mistake:

```
synthesis.md → observations.md → evidence/EVD-01.md
synthesis.md → evidence/EVD-01.md   ← redundant
```

Synthesis already reaches EVD-01 through observations. The direct link:

- **Obscures the actual dependency structure.** The layered chain tells you _how_ synthesis depends on EVD-01 (through observations). The shortcut hides that.
- **Creates unnecessary staleness propagation.** When EVD-01 changes, drft flags both observations _and_ synthesis as stale. But only observations should be the direct dependent — synthesis should hear about it through observations.
- **Makes impact analysis noisier.** `drft impact evidence/EVD-01.md` reports more dependents than necessary because the shortcut creates an extra propagation path.

Not every redundant edge is a mistake. In some structures, direct links are intentional convenience — a table of contents linking to every page, for example. That's why the `redundant-edge` rule defaults to `warn` rather than `error`. But in systems with intentional layering, redundant edges almost always indicate structural drift.

## What drft surfaces

### As an analysis (`drft report`)

```bash
drft report transitive-reduction
```

```
=== transitive-reduction ===
synthesis.md → evidence/EVD-01.md (via observations.md)
```

JSON output:

```json
{
  "transitive-reduction": {
    "redundant_edges": [
      {
        "source": "synthesis.md",
        "target": "evidence/EVD-01.md",
        "via": "observations.md"
      }
    ]
  }
}
```

The `via` field shows one intermediate node proving the alternate path exists. When multiple alternate paths exist, only one is reported — the goal is to show _that_ the edge is redundant, not to enumerate every alternate route.

### As a rule (`drft check`)

The `redundant-edge` rule wraps this analysis in a diagnostic:

```
warn[redundant-edge]: synthesis.md → evidence/EVD-01.md (transitively redundant via observations.md)
```

Enable it in `drft.toml`:

```toml
[rules]
redundant-edge = "warn" # or "error"
```

Or check on demand without changing config:

```bash
drft check --rule redundant-edge
```

## Algorithm

For each edge (A, C) in the graph, drft runs a breadth-first search from A, following forward edges but skipping the direct edge to C. If C is still reachable, the edge is redundant and the first intermediate node encountered on the alternate path is recorded as the `via`.

Edges to nodes outside the graph (broken links, external URLs) and self-loops are excluded from the analysis.

## Further reading

- [Transitive reduction](https://en.wikipedia.org/wiki/Transitive_reduction) on Wikipedia
- Aho, Garey, and Ullman (1972), "The transitive reduction of a directed graph" — the original paper establishing that every finite directed graph has a unique transitive reduction

## Source

[`src/analyses/transitive_reduction.rs`](../../src/analyses/transitive_reduction.rs)
