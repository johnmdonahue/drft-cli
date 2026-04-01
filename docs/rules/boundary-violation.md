---
sources:
  - ../../src/rules/boundary_violation.rs
---

# boundary-violation

Flags edges that escape the graph boundary (reach outside the directory tracked by `drft.lock`).

## Example

```
project/
  drft.lock
  index.md       # contains [notes](../notes.md)
```

The graph has a lockfile, so linking outside it is a violation:

```
warn[boundary-violation]: index.md -> ../notes.md (links outside graph boundary)
```

Without a `drft.lock`, this rule has nothing to enforce.

## Configuration

```toml
[rules]
boundary-violation = "warn" # default
```

```toml
[rules.boundary-violation]
severity = "warn"
ignore = ["README.md"]
```

## Analysis

Uses the [graph-boundaries](../analyses/graph-boundaries.md) analysis, which identifies edges that cross graph boundaries.
