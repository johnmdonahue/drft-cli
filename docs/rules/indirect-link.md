# indirect-link

Flags links whose target is a symlink rather than a regular file.

## Example

```
docs/
  index.md       # contains [setup](setup.md)
  setup.md       # symlink -> ../shared/setup.md
```

```
warn[indirect-link]: index.md -> setup.md (target is a symlink to ../shared/setup.md)
```

The fix suggests linking to the actual file directly instead of going through the symlink.

## Configuration

```toml
[rules]
indirect-link = "off"    # default
```

```toml
[rules.indirect-link]
severity = "warn"
ignore = []
```

## Analysis

This rule inspects the graph edges directly -- it does not consume a separate analysis. For each edge with a local target, it checks whether the target path is a symlink on the filesystem.

## Source

[`src/rules/indirect_link.rs`](../../src/rules/indirect_link.rs)
