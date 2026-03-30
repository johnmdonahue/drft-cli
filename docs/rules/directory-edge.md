# directory-edge

Flags edges that point to a directory instead of a specific file.

## Example

```
docs/
  index.md       # contains [guides](guides)
  guides/
    README.md
    setup.md
```

```
warn[directory-edge]: index.md -> guides (links to directory, not file)
```

The fix suggestion recommends linking to a specific file, e.g. `guides/README.md`.

## Configuration

```toml
[rules]
directory-edge = "warn"    # default
```

```toml
[rules.directory-edge]
severity = "warn"
ignore = []
```

## Analysis

This rule inspects the graph edges directly -- it does not consume a separate analysis. For each edge whose target is not in the graph, it checks whether the path is a directory on the filesystem.

## Source

[`src/rules/directory_edge.rs`](../../src/rules/directory_edge.rs)
