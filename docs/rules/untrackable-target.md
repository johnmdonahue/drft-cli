# untrackable-target

Flags edges that point to a directory with no lockfile — there is nothing to hash, so drft cannot track it for staleness.

Edges to directories that have a `drft.lock` produce no diagnostic from this rule. The lockfile is hashed and the `stale` rule handles the rest.

## Example

```
docs/
  index.md       # contains [guides](guides)
  guides/
    README.md
    setup.md
```

```
warn[untrackable-target]: index.md -> guides (directory has no lockfile — cannot track for staleness)
fix: lock it (drft init -C guides && drft lock -C guides) or link to a specific file
```

## Configuration

```toml
[rules]
untrackable-target = "warn" # default
```

```toml
[rules.untrackable-target]
severity = "warn"
ignore = []
```

## Analysis

This rule inspects graph nodes directly. For each edge whose target is a `Directory` node with no hash, it emits a diagnostic. Directory nodes get a hash when they have a `drft.lock` — that lockfile content is hashed.

## Source

[`src/rules/untrackable_target.rs`](../../src/rules/untrackable_target.rs)
