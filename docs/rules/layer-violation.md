---
sources:
  - ../../src/rules/layer_violation.rs
---

# layer-violation

Flags links that violate the depth hierarchy: upward links (deeper file linking to a shallower one) and skip-layer links (jumping more than one level down).

## Example

```
docs/
  index.md     # depth 0, links to guide.md
  guide.md     # depth 1, links to detail.md
  detail.md    # depth 2, links to index.md  (upward!)
```

```
warn[layer-violation]: detail.md -> index.md (upward link, depth 2 -> depth 0)
```

A skip-layer link:

```
warn[layer-violation]: index.md -> detail.md (skip-layer link, depth 0 -> depth 2)
```

Nodes involved in cycles are excluded from this rule, since their depth is ambiguous.

## Configuration

```toml
[rules]
layer-violation = "warn" # default
```

```toml
[rules.layer-violation]
severity = "warn"
ignore = ["index.md"]
```

## Analysis

Uses the [depth](../analyses/depth.md) analysis, which computes the longest-path depth of each node from the graph roots.
