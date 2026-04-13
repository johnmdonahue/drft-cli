---
sources:
  - ../../src/rules/stale.rs
---

# stale

Flags files whose content has changed since the last `drft lock`, and files that are transitively stale because a dependency changed.

## Example

```
docs/
  drft.lock
  index.md       # links to setup.md
  setup.md       # edited since last lock
```

```
warn[stale]: setup.md (content changed)
warn[stale]: index.md (stale via setup.md)
```

Without a `drft.lock`, this rule has nothing to check.

## Configuration

```toml
[rules]
stale = "warn" # default
```

```toml
[rules.stale]
severity = "warn"
ignore = ["CHANGELOG.md"]
```

## Analysis

Uses the [change-propagation](../analyses/change-propagation.md) analysis, which compares current content hashes against the lockfile and propagates staleness along dependency edges. Nodes with no hash (symlinks whose canonical target resolves outside `include`) are skipped — drft cannot reason about content it did not read.
