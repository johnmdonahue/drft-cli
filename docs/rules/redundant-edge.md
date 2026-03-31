# redundant-edge

Flags direct links that are transitively redundant -- the source already reaches the target through an intermediate path.

## Example

```
docs/
  a.md     # links to b.md and c.md
  b.md     # links to c.md
```

`a.md` links directly to `c.md`, but already reaches it via `b.md`:

```
warn[redundant-edge]: a.md -> c.md (transitively redundant, via b.md)
```

## Configuration

```toml
[rules]
redundant-edge = "warn" # default
```

```toml
[rules.redundant-edge]
severity = "warn"
ignore = ["index.md"]
```

## Analysis

Uses the [transitive-reduction](../analyses/transitive-reduction.md) analysis, which computes the minimal edge set that preserves all reachability.

## Source

[`src/rules/redundant_edge.rs`](../../src/rules/redundant_edge.rs)
