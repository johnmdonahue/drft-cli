---
sources:
  - ../../src/rules/unresolved_edge.rs
---

# unresolved-edge

Fires when an edge target is an included node (`included: true`) with `type: null` — meaning the file matches `include` patterns and should exist but doesn't.

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

This rule checks whether the target node is included and has `type: null`. The classification is computed during graph building — no filesystem probing at rule time.
