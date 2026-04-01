---
sources:
  - ../../src/rules/untrackable_target.rs
---

# untrackable-target

Flags edges to directory nodes that have no `drft.toml`. Without a config, drft cannot discover or hash the directory's contents, so it cannot track the target for staleness.

## Example

```
project/
  index.md       # contains [research](research/)
  research/      # no drft.toml
    notes.md
```

`research/` is a directory without `drft.toml`, so `drft check` reports:

```
warn[untrackable-target]: index.md -> research (directory has no drft.toml — cannot track for staleness)
```

## Fix

Add a `drft.toml` to the target directory to declare it as a graph:

```bash
drft init -C research
```

Once the directory has a config, drft computes a content hash from the child graph's files and tracks it for staleness.

## Configuration

```toml
[rules]
untrackable-target = "warn" # default
```

```toml
[rules.untrackable-target]
severity = "warn"
ignore = ["vendor/"]
```

## Analysis

This rule inspects graph edges directly — it does not consume a separate analysis. For each edge whose target is a `Directory` node without a hash, it emits a diagnostic.
