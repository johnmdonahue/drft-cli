---
sources:
  - ../../src/rules/boundary_violation.rs
---

# boundary-violation

Flags edges whose target resolves above the graph root. The graph root is the directory containing `drft.toml` — any link that escapes it crosses the boundary of what the graph can validate.

## Example

```
project/
  drft.toml
  index.md       # links to ../outside.md
../outside.md    # lives above the graph root
```

```
warn[boundary-violation]: index.md → ../outside.md (link escapes the graph root)
```

The target node is still created so downstream rules can reason about it, but no content is read from it and the hash is omitted.

## What counts as an escape

- Relative paths that climb above the root: `../foo.md`, `../../a/b.md`
- Absolute paths: `/etc/hosts`, `/tmp/notes.md`
- Symlinks whose resolved target lands outside the root (reported via the same node, though the path stored is whatever the parser emitted)

## Configuration

```toml
[rules]
boundary-violation = "warn" # default
```

```toml
[rules.boundary-violation]
severity = "error" # escalate for CI
ignore = ["legacy/**"] # suppress for files you know leak
```

## Fix

Either pull the target inside the graph (move the file under the `drft.toml` root and add it to `include`) or drop the link. If you deliberately want links to point outside the graph, set the rule to `off`.
