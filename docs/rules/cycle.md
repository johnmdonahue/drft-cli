# cycle

Flags circular dependencies between files.

## Example

```
docs/
  a.md     # links to b.md
  b.md     # links to c.md
  c.md     # links to a.md
```

```
warn[cycle]: cycle detected (a.md -> b.md -> c.md -> a.md)
```

Each strongly connected component with more than one member produces one diagnostic showing the cycle path.

## Configuration

```toml
[rules]
cycle = "warn"    # default
```

```toml
[rules.cycle]
severity = "warn"
ignore = ["glossary.md"]
```

## Analysis

Powered by the [scc](../analyses/scc.md) (strongly connected components) analysis, which uses Tarjan's algorithm to find all cycles.
