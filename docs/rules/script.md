# Script rules

Script rules let you define custom checks as external scripts. The script receives the graph as JSON on stdin and emits diagnostics as newline-delimited JSON on stdout.

## Defining a script rule

```toml
[rules.my-custom-rule]
severity = "warn"
command = "./scripts/my-rule.sh"
```

The command path is resolved relative to the directory containing `drft.toml`. Arguments can be included in the command string (split on whitespace).

## Input format

The script receives a JGF (JSON Graph Format) object on stdin:

```json
{
  "graph": {
    "directed": true,
    "nodes": {
      "index.md": { "metadata": { "type": "source", "hash": "b3:abc..." } },
      "setup.md": { "metadata": { "type": "source", "hash": "b3:def..." } }
    },
    "edges": [
      { "source": "index.md", "target": "setup.md", "relation": "markdown:inline" }
    ]
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

The `rule` name and `severity` are set automatically from the config -- the script does not need to provide them.

## Error handling

If the script exits with a non-zero status, `drft` emits a warning to stderr and surfaces a diagnostic so JSON consumers see the failure. Unparseable output lines are also warned about on stderr.

## Example script

```bash
#!/bin/sh
# Flag any node whose path contains "draft"
cat | jq -c '.graph.nodes | to_entries[] | select(.key | test("draft")) | {message: "file looks like a draft", node: .key}'
```
