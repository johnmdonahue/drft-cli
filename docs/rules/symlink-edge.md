---
sources:
  - ../../src/rules/symlink_edge.rs
---

# symlink-edge

Flags edges whose target is a symlink rather than a regular file.

## Example

```
docs/
  index.md       # contains [setup](setup.md)
  setup.md       # symlink -> ../shared/setup.md
```

```
warn[symlink-edge]: index.md -> setup.md (target is a symlink to ../shared/setup.md)
```

The fix suggests linking to the actual file directly instead of going through the symlink.

## Configuration

```toml
[rules]
symlink-edge = "warn" # default
```

```toml
[rules.symlink-edge]
severity = "warn"
ignore = []
```

## Analysis

This rule checks whether an edge target is a node with `type: "symlink"` and reports the filesystem edge to the resolved target.
