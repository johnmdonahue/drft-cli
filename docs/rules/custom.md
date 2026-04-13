---
sources:
  - ../../src/rules/custom.rs
---

# Custom rules

Custom rules let you define custom checks as external commands. The command receives the enriched graph and rule options as JSON on stdin and emits diagnostics as newline-delimited JSON on stdout.

## Defining a custom rule

```toml
[rules.my-custom-rule]
severity = "warn"
command = "./scripts/my-rule.sh"

[rules.my-custom-rule.options]
threshold = 5
```

The command path is resolved relative to the directory containing `drft.toml`. Arguments can be included in the command string (split on whitespace). Options under `[rules.<name>.options]` are passed through to the command.

## Input format

The command receives a JSON object on stdin with two top-level keys: `graph` (the enriched graph including all analyses) and `options` (from `[rules.<name>.options]`, or `{}` if none):

```json
{
  "graph": {
    "directed": true,
    "nodes": {
      "index.md": { "metadata": { "type": "file", "included": true, "hash": "b3:abc..." } },
      "setup.md": { "metadata": { "type": "file", "included": true, "hash": "b3:def..." } }
    },
    "edges": [
      { "source": "index.md", "target": "setup.md", "parser": "markdown" }
    ],
    "analyses": {
      "degree": { "nodes": [...] },
      "scc": { "sccs": [...], ... },
      "bridges": { "cut_vertices": [...], "bridge_edges": [...] },
      "impact_radius": { "nodes": [...] },
      ...
    }
  },
  "options": {
    "threshold": 5
  }
}
```

## Output format

Emit one JSON object per line on stdout. Each object must have a `message` field; all other fields are optional:

```json
{"message": "custom issue", "node": "index.md", "fix": "do something about it"}
{"message": "bad link", "source": "a.md", "target": "b.md"}
```

Fields:

- `message` (required) -- the diagnostic message
- `source` -- the source file of a problematic edge
- `target` -- the target file of a problematic edge
- `node` -- a single file the diagnostic applies to
- `fix` -- a suggested fix

The `rule` name and `severity` are set automatically from the config -- the command does not need to provide them.

## Error handling

If the command exits with a non-zero status, `drft` emits a warning to stderr and surfaces a diagnostic so JSON consumers see the failure. Unparseable output lines are also warned about on stderr.

## Example

```bash
#!/bin/sh
# Flag any node whose path contains "draft"
# Reads { graph, options } from stdin
cat | jq -c '.graph.nodes | to_entries[] | select(.key | test("draft")) | {message: "file looks like a draft", node: .key}'
```
