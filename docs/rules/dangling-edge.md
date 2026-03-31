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

If the target file exists on disk but outside `include`, it appears as an External node in the graph and is not flagged by this rule.

## Configuration

```toml
[rules]
dangling-edge = "warn" # default
```

```toml
[rules.dangling-edge]
severity = "warn"
ignore = ["drafts/"]
```

## Analysis

This rule inspects graph edges directly — it does not consume a separate analysis. For each edge with a local target, it checks whether the target exists as a node in the graph. Edges to symlinks are skipped (handled by `symlink-edge`). Directories are represented as `Directory` nodes in the graph, so they are not flagged by this rule — `untrackable-target` handles directories without lockfiles.

## Source

[`src/rules/dangling_edge.rs`](../../src/rules/dangling_edge.rs)
