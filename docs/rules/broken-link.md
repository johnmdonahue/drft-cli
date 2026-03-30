# broken-link

Flags links whose target file does not exist.

## Example

```
docs/
  index.md       # contains [setup](setup.md)
```

`setup.md` doesn't exist, so `drft check` reports:

```
warn[broken-link]: index.md -> setup.md (file not found)
```

If the target file exists but is excluded by an ignore pattern, the diagnostic says "file excluded by ignore pattern" instead.

## Configuration

```toml
[rules]
broken-link = "warn"    # default
```

```toml
[rules.broken-link]
severity = "warn"
ignore = ["drafts/"]
```

## Analysis

This rule inspects the graph edges directly -- it does not consume a separate analysis. For each edge with a local target, it checks whether the target exists in the graph or on the filesystem.
