---
sources:
  - ../src/config.rs
  - ../src/cli.rs
---

# Configuration

`drft.toml` in the graph root controls file discovery, parsers, rules, and encapsulation.

## Graph declaration

```toml
include = ["*.md", "*.rs"] # which paths become File nodes (default: ["*.md"])
exclude = ["drafts/*"] # remove from the graph (also respects .gitignore)
```

`include` determines which files become nodes. `exclude` removes paths before discovery. Both use glob patterns. drft also respects `.gitignore` automatically.

## Interface

```toml
[interface]
files = ["overview.md", "api/*.md"]
ignore = ["internal/*"]
```

Declares which files are accessible from parent graphs. If `[interface]` is present, edges from a parent graph into non-interface files trigger `encapsulation-violation`. If `[interface]` is absent, the graph is open — anything can link into it.

## Parsers

Parsers extract links from File nodes. Built-in parsers need no `command` field; custom parsers require one.

```toml
[parsers.markdown] # built-in, all defaults

[parsers.frontmatter] # built-in, restricted to .md files
files = ["*.md"]

[parsers.tsx] # custom parser (has command)
files = ["*.tsx"]
command = "./scripts/parse-tsx.sh"
```

| Field     | Required | Default        | Description                                         |
| --------- | -------- | -------------- | --------------------------------------------------- |
| `files`   | no       | all File nodes | Glob patterns — which files the parser receives     |
| `command` | no       | —              | Shell command (present = custom, absent = built-in) |
| `timeout` | no       | 5000           | Timeout in milliseconds (custom parsers only)       |

Parser-specific options go under `[parsers.<name>.options]` and are passed to the command as a JSON envelope on stdin. See [custom parsers](parsers/custom.md) for the full protocol.

Disable a built-in parser with `markdown = false`.

## Rules

Rules validate the graph and emit diagnostics. Every rule has a severity: `"error"`, `"warn"`, or `"off"`.

```toml
[rules]
stale = "error" # shorthand: severity only
orphan-node = "off"

[rules.orphan-node] # table form: severity + options
severity = "warn"
files = ["docs/**"] # only evaluate against docs
ignore = ["README.md"] # exclude from diagnostics

[rules.directed-cycle] # scoped to frontmatter edges only
parsers = ["frontmatter"]

[rules.max-fan-out] # custom rule (has command)
command = "./scripts/max-fan-out.sh"
severity = "warn"

[rules.max-fan-out.options] # rule-specific options (passed through)
threshold = 5
```

| Field      | Required | Default | Description                                                                |
| ---------- | -------- | ------- | -------------------------------------------------------------------------- |
| `severity` | no       | warn    | `"error"`, `"warn"`, or `"off"`                                            |
| `files`    | no       | all     | Glob patterns — which nodes the rule evaluates                             |
| `ignore`   | no       | none    | Glob patterns — exclude nodes from diagnostics                             |
| `parsers`  | no       | all     | Parser names — which edges the rule evaluates (filters by edge provenance) |
| `command`  | no       | —       | Shell command (present = custom, absent = built-in)                        |

Rule-specific options go under `[rules.<name>.options]` and are passed to the rule. See [custom rules](rules/custom.md) for the protocol.

All built-in rules default to `warn`. Override to `error` for CI enforcement or `off` to suppress. `--rule <name>` on the command line overrides `off` to `warn` for on-demand checks without config changes. See [rules](rules/README.md) for the full list.

## Config patterns

Parsers and rules share the same config pattern:

- **Shorthand** — `markdown = true`, `stale = "error"` for the common case
- **Table form** — `[parsers.tsx]`, `[rules.orphan-node]` for options, custom commands, or file scoping
- **Options sub-table** — `[parsers.<name>.options]`, `[rules.<name>.options]` for arbitrary structured data passed through to the parser or rule

The `command` field is the discriminant: present means custom, absent means built-in.
