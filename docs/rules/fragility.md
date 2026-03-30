# fragility

Flags structural single points of failure: cut vertices and bridge edges.

## Example

```
docs/
  a.md     # links to b.md
  b.md     # links to c.md
  c.md     # links to d.md
```

`b.md` is a cut vertex (removing it disconnects the graph), and `b.md -- c.md` is a bridge edge:

```
warn[fragility]: b.md (cut vertex)
warn[fragility]: b.md -> c.md (bridge edge)
```

## Configuration

```toml
[rules]
fragility = "warn"    # default
```

```toml
[rules.fragility]
severity = "warn"
ignore = ["index.md"]
```

## Analysis

Powered by the [bridges](../analyses/bridges.md) analysis, which uses Tarjan's bridge-finding algorithm to identify cut vertices and bridge edges in O(V + E).

## Source

[`src/rules/fragility.rs`](../../src/rules/fragility.rs)
