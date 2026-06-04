---
sources:
  - ../src/config.rs
  - ../src/cli.rs
---

# Configuration

`drft.toml` in the graph root configures the walk, the graphs, and the rules.
The directory containing `drft.toml` is the graph root; nested `drft.toml` files
found while walking are ordinary files on disk, not graph boundaries.

## ignore

```toml
ignore = ["target/**", "drafts/**"]
```

The `fs` graph walks every file under the graph root. `ignore` removes paths
from that walk by glob; drft also respects `.gitignore` automatically. There is
no `include` — the graph is everything under the root minus what `ignore` and
`.gitignore` remove. drft excludes its own `drft.lock` from the graph.

## graphs

A graph wires a source, a file filter, and a builder. The `fs` graph is implicit
and always built — it owns the identity space (paths) and contributes each file's
`type` and `hash`. The text graphs are declared under `[graphs.<name>]`:

```toml
[graphs.markdown]
source = "fs"
filter = ["**/*.md"]
builder = "markdown"

[graphs.frontmatter]
source = "fs"
filter = ["**/*.md"]
builder = "frontmatter"
```

| Field     | Required | Default       | Description                                 |
| --------- | -------- | ------------- | ------------------------------------------- |
| `source`  | no       | `"fs"`        | Where bytes come from                       |
| `filter`  | no       | `["**/*.md"]` | Globs scoping which files the builder reads |
| `builder` | yes      | —             | `markdown` or `frontmatter`                 |

The graph's name is its compose-time namespace: its facts nest under `@<name>` in
the composed graph. When you declare any `[graphs.*]`, your declarations replace
the defaults; with no `[graphs.*]`, drft enables `markdown` and `frontmatter`
over `**/*.md`.

## rules

Every built-in rule is on at `warn`. Configure severity and ignore globs under
`[rules.<name>]`:

```toml
[rules]
stale-node = "error" # shorthand: severity only
stale-edge = "error"

[rules.detached-node] # table form: severity + ignore
severity = "off"

[rules.unresolved-edge]
ignore = ["CHANGELOG.md"] # globs matched against the finding's subject
```

| Field      | Required | Default | Description                                     |
| ---------- | -------- | ------- | ----------------------------------------------- |
| `severity` | no       | `warn`  | `"error"`, `"warn"`, or `"off"`                 |
| `ignore`   | no       | none    | Globs — suppress findings whose subject matches |

See [rules](rules/README.md) for the full set.
