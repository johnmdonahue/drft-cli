# encapsulation

Flags links from outside a child graph that bypass its declared interface.

## Example

```
project/
  index.md                # contains [internal](research/internal.md)
  research/
    drft.toml             # interface: [overview.md]
    overview.md
    internal.md
```

`research/` is a child graph with `overview.md` as its interface. Linking directly to `internal.md` from outside violates encapsulation:

```
warn[encapsulation]: index.md -> research/internal.md (not in research/ interface)
```

## Configuration

```toml
[rules]
encapsulation = "warn"    # default
```

```toml
[rules.encapsulation]
severity = "warn"
ignore = []
```

## Analysis

Powered by the [graph-boundaries](../analyses/graph-boundaries.md) analysis, which reads child graph configurations to determine interface boundaries.

## Source

[`src/rules/encapsulation.rs`](../../src/rules/encapsulation.rs)
