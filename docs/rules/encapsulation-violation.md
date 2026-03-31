# encapsulation-violation

Flags edges from outside a child graph that bypass its declared interface.

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
warn[encapsulation-violation]: index.md -> research/internal.md (not in research interface)
```

## Configuration

```toml
[rules]
encapsulation-violation = "warn" # default
```

```toml
[rules.encapsulation-violation]
severity = "warn"
ignore = []
```

## Analysis

Uses the [graph-boundaries](../analyses/graph-boundaries.md) analysis, which reads child graph configurations to determine interface boundaries.

## Source

[`src/rules/encapsulation_violation.rs`](../../src/rules/encapsulation_violation.rs)
