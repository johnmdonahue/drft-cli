# Custom parser

## The concept

A custom parser lets you extend drft with custom link extraction logic. You provide an external command that receives file paths on stdin and emits discovered links as NDJSON on stdout. drft handles file routing and timeout enforcement -- the command just needs to find links and print them.

## Configuration

Custom parsers are defined under `[parsers]` in `drft.toml`. The parser name is whatever you choose. The `command` field is what distinguishes a custom parser from a built-in one.

```toml
[parsers.yaml-refs]
files = ["*.yaml"]
command = "./scripts/parse-yaml-refs.sh"
```

### Fields

| Field     | Required | Default        | Description                                |
| --------- | -------- | -------------- | ------------------------------------------ |
| `files`   | no       | all File nodes | Glob patterns for which File nodes to send |
| `command` | yes      | --             | Shell command to run                       |
| `timeout` | no       | 5000           | Timeout in milliseconds                    |

Parser-specific options go under `[parsers.<name>.options]` and are passed through to the command (see Protocol below).

If the command path is relative, drft resolves it against the directory containing `drft.toml`.

## Protocol

drft runs the command once via `sh -c`. Stdin carries a JSON options envelope on line 1, followed by file paths (one per line). The command processes each file and prints NDJSON links on stdout, each tagged with the source file.

### Input (stdin)

Line 1 is the JSON options envelope from `[parsers.<name>.options]` (always `{}` if no options). Remaining lines are file paths:

```
{"ref_style":"jsonpath"}
src/analyses/mod.rs
src/rules/mod.rs
src/parsers/mod.rs
```

### Output (stdout)

One JSON object per line (NDJSON). Two kinds of lines:

**Edge lines** — discovered links:

| Field    | Type   | Description                                 |
| -------- | ------ | ------------------------------------------- |
| `file`   | string | The source file path (as received on stdin) |
| `target` | string | The link target (relative file path)        |

**Metadata lines** — structured data on a node:

| Field      | Type   | Description                                |
| ---------- | ------ | ------------------------------------------ |
| `file`     | string | The file path this metadata belongs to     |
| `metadata` | object | Arbitrary JSON object attached to the node |

drft distinguishes the two by checking for a `target` or `metadata` field.

Example output:

```json
{"file": "src/api.yaml", "target": "shared/types.yaml"}
{"file": "src/api.yaml", "target": "../schemas/base.yaml"}
{"file": "src/api.yaml", "metadata": {"version": "3.0", "title": "API Spec"}}
{"file": "src/models.yaml", "target": "shared/types.yaml"}
```

Empty lines are silently skipped. Each edge carries the parser name as provenance — so edges from this parser would have `parser: "yaml-refs"`. Metadata is namespaced by parser name on the node as `node.metadata["yaml-refs"]`.

The batch approach (one process for all files) is much faster than per-file spawning.

### Error handling

| Condition           | Behavior                                                                     |
| ------------------- | ---------------------------------------------------------------------------- |
| Non-zero exit code  | Warning printed to stderr, file produces no links                            |
| Malformed JSON line | Warning printed to stderr, that line is skipped, other lines still processed |
| Timeout exceeded    | Process killed, warning printed, file produces no links                      |
| Command not found   | Warning printed, file produces no links                                      |

Errors are non-fatal. A failing custom parser does not cause `drft check` to exit with an error -- it just means that file contributes no edges from that parser.

## Timeout

The default timeout is 5000ms (5 seconds). Override it if the command needs more time:

```toml
[parsers.yaml-refs]
files = ["*.yaml"]
command = "./scripts/parse-yaml-refs.sh"
timeout = 10000
```

If the command exceeds the timeout, drft kills the process and logs a warning.

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
    printf '{"file": "%s", "target": "%s"}\n' "$filepath" "$ref"
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

[`src/parsers/custom.rs`](../../src/parsers/custom.rs)
