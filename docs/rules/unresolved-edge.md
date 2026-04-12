---
sources:
  - ../../src/rules/unresolved_edge.rs
---

# unresolved-edge

Flags edges whose target matches an `include` pattern but does not exist as a node in the graph. This means the file is expected to be part of the graph but is missing — a broken internal link.

Edges to targets outside `include` are classified as `External(Local)` and are not flagged by this rule. Edges to URIs are classified as `External(Remote)` and are also not flagged.

## Example

```
docs/
  index.md       # contains [setup](setup.md)
```

`setup.md` doesn't exist, so `drft check` reports:

```
warn[unresolved-edge]: index.md -> setup.md (file not found)
```

## Configuration

```toml
[rules]
unresolved-edge = "warn" # default
```

```toml
[rules.unresolved-edge]
severity = "warn"
ignore = ["drafts/"]
```

## Analysis

This rule matches edges with `TargetKind::Internal(Resolution::Missing)`. The classification is computed during graph building — no filesystem probing at rule time.
