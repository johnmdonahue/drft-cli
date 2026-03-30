# dangling-edge

Flags edges whose target node does not exist.

## Example

```
docs/
  index.md       # contains [setup](setup.md)
```

`setup.md` doesn't exist, so `drft check` reports:

```
warn[dangling-edge]: index.md -> setup.md (file not found)
```

If the target file exists but is excluded by an ignore pattern, the diagnostic says "file excluded by ignore pattern" instead.

## Configuration

```toml
[rules]
dangling-edge = "warn"    # default
```

```toml
[rules.dangling-edge]
severity = "warn"
ignore = ["drafts/"]
```

## Analysis

This rule inspects the graph edges directly -- it does not consume a separate analysis. For each edge with a local target, it checks whether the target exists in the graph or on the filesystem.

## Source

[`src/rules/dangling_edge.rs`](../../src/rules/dangling_edge.rs)
