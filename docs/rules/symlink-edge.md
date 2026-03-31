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
symlink-edge = "warn"    # default
```

```toml
[rules.symlink-edge]
severity = "warn"
ignore = []
```

## Analysis

This rule inspects graph edges and target properties directly — it does not consume a separate analysis. For each edge with a local target, it checks `graph.target_properties` for the `is_symlink` flag set during graph building.

## Source

[`src/rules/symlink_edge.rs`](../../src/rules/symlink_edge.rs)
