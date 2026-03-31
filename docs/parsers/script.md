# Script parser

## The concept

A script parser lets you extend drft with custom link extraction logic. You provide an external command that receives file paths on stdin and emits discovered links as NDJSON on stdout. drft handles file routing, type filtering, and timeout enforcement -- the script just needs to find links and print them.

## Configuration

Script parsers are defined under `[parsers]` in `drft.toml`. The parser name is whatever you choose. The `command` field is what distinguishes a script parser from a built-in one.

```toml
[parsers.yaml-refs]
files = ["*.yaml"]
command = "./scripts/parse-yaml-refs.sh"
```

### Fields

| Field | Required | Default | Description |
|-------|----------|---------|-------------|
| `files` | no | all File nodes | Glob patterns for which File nodes to send |
| `command` | yes | -- | Shell command to run |
| `timeout` | no | 5000 | Timeout in milliseconds |

Parser-specific options go under `[parsers.<name>.options]` and are passed through to the script (see Protocol below).

If the command path is relative, drft resolves it against the directory containing `drft.toml`.

## Protocol

drft runs the command once via `sh -c`. Stdin carries a JSON options envelope on line 1, followed by file paths (one per line). The script processes each file and prints NDJSON links on stdout, each tagged with the source file.

### Input (stdin)

Line 1 is the JSON options envelope from `[parsers.<name>.options]` (always `{}` if no options). Remaining lines are file paths:

```
{"types":["inline"],"extract_metadata":true}
src/analyses/mod.rs
src/rules/mod.rs
src/parsers/mod.rs
```

### Output (stdout)

One JSON object per line (NDJSON). Each object must have three fields:

| Field | Type | Description |
|-------|------|-------------|
| `file` | string | The source file path (as received on stdin) |
| `target` | string | The link target (relative file path) |
| `type` | string | Link type label (your choice, e.g. `"import"`, `"ref"`) |

Example output:

```json
{"file": "src/api.yaml", "target": "shared/types.yaml", "type": "import"}
{"file": "src/api.yaml", "target": "../schemas/base.yaml", "type": "ref"}
{"file": "src/models.yaml", "target": "shared/types.yaml", "type": "import"}
```

Empty lines are silently skipped. The `type` value becomes part of the edge type in the graph as `<parser-name>:<type>` -- so the above would produce edges of type `yaml-refs:import` and `yaml-refs:ref`.

The batch approach (one process for all files) is much faster than per-file spawning.

### Error handling

| Condition | Behavior |
|-----------|----------|
| Non-zero exit code | Warning printed to stderr, file produces no links |
| Malformed JSON line | Warning printed to stderr, that line is skipped, other lines still processed |
| Timeout exceeded | Process killed, warning printed, file produces no links |
| Command not found | Warning printed, file produces no links |

Errors are non-fatal. A failing script parser does not cause `drft check` to exit with an error -- it just means that file contributes no edges from that parser.

## Type filtering

You can restrict which link types are kept via parser options:

```toml
[parsers.yaml-refs]
files = ["*.yaml"]
command = "./scripts/parse-yaml-refs.sh"

[parsers.yaml-refs.options]
types = ["import"]
```

This runs the script and keeps only links where `type` matches one of the listed values. The `types` option is the one parser option that drft interprets itself (for filtering); all other options are passed through to the script.

## Timeout

The default timeout is 5000ms (5 seconds). Override it if your script needs more time:

```toml
[parsers.yaml-refs]
files = ["*.yaml"]
command = "./scripts/parse-yaml-refs.sh"
timeout = 10000
```

If the script exceeds the timeout, drft kills the process and logs a warning.

## Example: a YAML reference parser

A shell script that extracts `$ref` values from YAML files:

```bash
#!/usr/bin/env bash
# scripts/parse-yaml-refs.sh
# Line 1 is the JSON options envelope — read and skip it.
# Remaining lines are file paths.

read -r _options
while IFS= read -r filepath; do
  [ -z "$filepath" ] && continue
  grep -oP '\$ref:\s*\K\S+' "$filepath" | while read -r ref; do
    # Skip URLs
    case "$ref" in
      http://*|https://*) continue ;;
    esac
    printf '{"file": "%s", "target": "%s", "type": "ref"}\n' "$filepath" "$ref"
  done
done
```

Configure it:

```toml
[parsers.yaml-refs]
files = ["*.yaml", "*.yml"]
command = "./scripts/parse-yaml-refs.sh"
```

Now `drft check` and `drft report` will include edges from YAML `$ref` values alongside the standard markdown links.

## Source

[`src/parsers/script.rs`](../../src/parsers/script.rs)
