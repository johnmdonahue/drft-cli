# orphan

Flags files with no inbound links (in-degree = 0).

## Example

```
docs/
  index.md       # links to setup.md
  setup.md
  stray.md       # nothing links here
```

```
warn[orphan]: stray.md (no inbound links)
warn[orphan]: index.md (no inbound links)
```

Note that root entry points (like `index.md`) will also be flagged since they naturally have zero in-degree. Use the ignore list to suppress expected roots.

## Configuration

```toml
[rules]
orphan = "off"    # default
```

```toml
[rules.orphan]
severity = "warn"
ignore = ["index.md", "README.md"]
```

## Analysis

Powered by the [degree](../analyses/degree.md) analysis, which computes in-degree and out-degree for every node.

## Source

[`src/rules/orphan.rs`](../../src/rules/orphan.rs)
