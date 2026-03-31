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

The rule also detects graph boundary changes (new or removed child graphs). Without a `drft.lock`, this rule has nothing to check.

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

Uses the [change-propagation](../analyses/change-propagation.md) analysis, which compares current content hashes against the lockfile and propagates staleness along dependency edges.

## Source

[`src/rules/stale.rs`](../../src/rules/stale.rs)
