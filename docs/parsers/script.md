# Script parser

## The concept

A script parser lets you extend drft with custom link extraction logic. You provide an external command that receives a file path on stdin and emits discovered links as NDJSON on stdout. drft handles glob matching, type filtering, and timeout enforcement -- the script just needs to find links and print them.

## Configuration

Script parsers are defined under `[parsers]` in `drft.toml`. The parser name is whatever you choose. The `command` field is what distinguishes a script parser from a built-in one.

```toml
[parsers.yaml-refs]
glob = "*.yaml"
command = "./scripts/parse-yaml-refs.sh"
```

### Fields

| Field | Required | Default | Description |
|-------|----------|---------|-------------|
| `glob` | yes | -- | File pattern to match (matched against filename only) |
| `command` | yes | -- | Shell command to run |
| `types` | no | all | List of link types to keep |
| `timeout` | no | 5000 | Timeout in milliseconds |

If the command path is relative, drft resolves it against the directory containing `drft.toml`.

## Protocol

For each matched file, drft runs the command via `sh -c` and sends the file path on stdin. The script reads the path, extracts links, and prints one JSON object per line on stdout.

### Input (stdin)

The file path, as a plain string with no trailing newline guarantee. Example:

```
docs/guide.yaml
```

### Output (stdout)

One JSON object per line (NDJSON). Each object must have two fields:

| Field | Type | Description |
|-------|------|-------------|
| `target` | string | The link target (relative file path) |
| `type` | string | Link type label (your choice, e.g. `"import"`, `"ref"`) |

Example output:

```json
{"target": "shared/types.yaml", "type": "import"}
{"target": "../schemas/base.yaml", "type": "ref"}
```

Empty lines are silently skipped. The `type` value becomes part of the edge type in the graph as `<parser-name>:<type>` -- so the above would produce edges of type `yaml-refs:import` and `yaml-refs:ref`.

### Error handling

| Condition | Behavior |
|-----------|----------|
| Non-zero exit code | Warning printed to stderr, file produces no links |
| Malformed JSON line | Warning printed to stderr, that line is skipped, other lines still processed |
| Timeout exceeded | Process killed, warning printed, file produces no links |
| Command not found | Warning printed, file produces no links |

Errors are non-fatal. A failing script parser does not cause `drft check` to exit with an error -- it just means that file contributes no edges from that parser.

## Type filtering

You can restrict which link types are kept, just like the markdown parser:

```toml
[parsers.yaml-refs]
glob = "*.yaml"
command = "./scripts/parse-yaml-refs.sh"
types = ["import"]
```

This runs the script and keeps only links where `type` matches one of the listed values.

## Timeout

The default timeout is 5000ms (5 seconds) per file. Override it if your script needs more time:

```toml
[parsers.yaml-refs]
glob = "*.yaml"
command = "./scripts/parse-yaml-refs.sh"
timeout = 10000
```

If the script exceeds the timeout, drft kills the process and logs a warning.

## Example: a YAML reference parser

A shell script that extracts `$ref` values from YAML files:

```bash
#!/usr/bin/env bash
# scripts/parse-yaml-refs.sh
# Reads a file path on stdin, extracts $ref values, emits NDJSON.

read -r filepath

grep -oP '\$ref:\s*\K\S+' "$filepath" | while read -r ref; do
  # Skip URLs
  case "$ref" in
    http://*|https://*) continue ;;
  esac
  printf '{"target": "%s", "type": "ref"}\n' "$ref"
done
```

Configure it:

```toml
[parsers.yaml-refs]
glob = "*.{yaml,yml}"
command = "./scripts/parse-yaml-refs.sh"
```

Now `drft check` and `drft report` will include edges from YAML `$ref` values alongside the standard markdown links.
