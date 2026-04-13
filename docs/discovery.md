---
sources:
  - ../src/discovery.rs
  - ../src/config.rs
---

# File discovery

Discovery determines which files become nodes in the graph. It runs before parsing and graph building — only discovered files are hashed, parsed, and tracked.

## Include patterns

`include` in `drft.toml` declares which paths become nodes. Patterns use glob syntax matched against paths relative to the graph root:

```toml
include = [
  "docs/**/*.md",
  "src/**/*.rs",
  "README.md",
]
```

`include` is drft's sole authority for what gets read from disk. Targets that parsers emit but that don't match `include` become nodes with `included: false` — they exist in the graph for edge resolution but drft does not read or hash them.

## Exclude patterns

`exclude` removes paths after discovery. A file must match at least one `include` pattern and not match any `exclude` pattern:

```toml
exclude = ["src/lib.rs"]
```

drft also respects `.gitignore` automatically via the [`ignore`](https://docs.rs/ignore) crate.

## Glob semantics

Patterns use the [`globset`](https://docs.rs/globset) crate with `literal_separator` enabled:

- `*` matches any characters except `/` (a single path component)
- `**` matches any characters including `/` (crosses directory boundaries)
- `?` matches a single non-`/` character

This matches standard shell glob behavior. Use `**/*.md` to match all `.md` files recursively, and `*.md` to match only in the current directory.

| Pattern                | Matches                                | Does not match                     |
| ---------------------- | -------------------------------------- | ---------------------------------- |
| `*.md`                 | `README.md`                            | `docs/graph.md`                    |
| `**/*.md`              | `README.md`, `docs/graph.md`           |                                    |
| `docs/*.md`            | `docs/graph.md`                        | `docs/rules/stale.md`              |
| `docs/**/*.md`         | `docs/graph.md`, `docs/rules/stale.md` |                                    |
| `examples/*/README.md` | `examples/simple/README.md`            | `examples/broken/guides/README.md` |

## Gitignore interaction

The walker respects `.gitignore` and won't enter ignored directories. This can conflict with `include` when a literal path lives inside a gitignored directory.

For example, if `.gitignore` has `.claude/*` (with a `!.claude/settings.json` negation), the walker may still skip the directory entirely. To handle this, discovery checks literal `include` paths (patterns with no glob characters) directly on disk as a fallback when the walker misses them.

## Walker behavior

Discovery uses the `ignore` crate's walker with `follow_links(true)` so symlinked directories are traversable. The security boundary for symlinks is enforced at hashing time, not at discovery — see [graph builder](graph.md#5-symlink-policy) for details.

The walker is rooted at the directory containing `drft.toml`. Patterns cannot reach above this root.
