# fragmentation

Flags disconnected components in the graph.

## Example

```
docs/
  index.md     # links to setup.md
  setup.md
  orphan.md    # no links to or from anything else
```

`orphan.md` forms its own disconnected component:

```
warn[fragmentation]: orphan.md (disconnected component, 1 node)
```

Only non-largest components are flagged. A fully connected graph produces no diagnostics.

## Configuration

```toml
[rules]
fragmentation = "warn" # default
```

```toml
[rules.fragmentation]
severity = "warn"
ignore = ["CHANGELOG.md"]
```

## Analysis

Uses the [connected-components](../analyses/connected-components.md) analysis, which finds weakly connected components in the undirected projection of the graph.

## Source

[`src/rules/fragmentation.rs`](../../src/rules/fragmentation.rs)
