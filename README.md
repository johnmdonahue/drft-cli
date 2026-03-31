# drft

A structural integrity checker for linked file systems. drft treats a directory of files as a dependency graph -- files are nodes, links are edges -- and validates the graph against configurable rules.

## Install

```bash
cargo install drft-cli    # via Cargo
npm install -g drft-cli   # via npm
```

Or download a prebuilt binary from [GitHub Releases](https://github.com/johnmdonahue/drft-cli/releases).

The binary is called `drft`.

## Quick start

```bash
# Check a directory for issues (no setup required)
drft check

# Initialize a config file
drft init

# Snapshot the current state
drft lock

# Check for staleness (files changed since last lock)
drft check

# Verify lockfile is current (for CI)
drft lock --check
```

## What it does

drft discovers files, runs configurable parsers to extract links between them, and builds a dependency graph. It then validates the graph against rules:

| Rule                      | Description                                           |
| ------------------------- | ----------------------------------------------------- |
| `dangling-edge`           | Edge target does not exist                            |
| `directed-cycle`          | Circular dependency detected                          |
| `untrackable-target`      | Directory target with no lockfile                     |
| `stale`                   | Dependency changed since last lock                    |
| `boundary-violation`      | Edge escapes the graph boundary                       |
| `encapsulation-violation` | Edge reaches into a child graph's non-interface files |
| `orphan-node`             | Node has no inbound edges                             |
| `symlink-edge`            | Edge target is a symlink                              |
| `fragility`               | Structural single point of failure                    |
| `fragmentation`           | Disconnected graph component                          |
| `layer-violation`         | Edge violates depth hierarchy                         |
| `redundant-edge`          | Edge is transitively redundant                        |
| `schema-violation`        | Node metadata violates schema (requires options)      |

All rules default to `warn`. Override to `error` for CI enforcement or `off` to suppress.

See the [full documentation](docs/README.md) for details on parsers, analyses, and rules.

## Commands

### `drft check`

Validate the graph against all enabled rules.

```bash
drft check                    # check current directory
drft check -C path/to/docs   # check a different directory
drft check --recursive        # include child graphs
drft check --rule orphan-node  # run only specific rules
drft check --watch            # re-check on file changes
drft check --format json      # machine-readable output
```

### `drft lock`

Snapshot file hashes to `drft.lock`. This enables staleness detection -- when a file changes, drft flags its dependents.

```bash
drft lock                     # create/update drft.lock
drft lock --check             # verify lockfile is current (CI)
drft lock --recursive         # lock all graphs bottom-up
```

### `drft parse`

Show raw parser output — what edges each parser found, before graph construction.

```bash
drft parse                    # all parsers, text output
drft parse --parser markdown  # only the markdown parser
drft parse --format json      # machine-readable output
```

### `drft graph`

Export the dependency graph.

```bash
drft graph                    # JSON Graph Format output
drft graph --dot              # GraphViz DOT output
drft graph --recursive        # include child graphs
```

### `drft impact`

Show what depends on the given files (transitively), sorted by review priority. Each dependent is annotated with its depth from the changed file, impact radius (its own transitive dependents), and betweenness centrality.

```bash
drft impact setup.md              # text output
drft impact --format json setup.md  # JSON with structural context
drft impact config.md faq.md      # multiple files
```

### `drft report`

Query structural analyses of the graph — degree, betweenness, SCC, depth, and more.

```bash
drft report                       # all analyses, text output
drft report degree                # single analysis
drft report betweenness pagerank  # multiple analyses
drft report --format json degree  # machine-readable output
```

See [analyses documentation](docs/analyses/README.md) for the full list.

### `drft config`

Show the resolved configuration (defaults filled in).

```bash
drft config show              # resolved config as TOML
drft config show --format json # machine-readable
drft config show --recursive  # config for each graph in the tree
```

### `drft init`

Create a default `drft.toml` config file.

```bash
drft init                     # write default drft.toml
```

## Configuration

`drft.toml` in the graph root:

```toml
# Which paths become File nodes (default: ["*.md"])
include = ["*.md", "*.yaml"]

# Remove from the graph (also respects .gitignore)
exclude = ["drafts/*", "archive/*"]

# Public interface — files accessible from parent graphs.
# Presence of this section enables encapsulation.
[interface]
files = ["overview.md", "api/*.md"]

# Parsers — edge extraction from File nodes
[parsers.markdown]
files = ["*.md"] # restrict to .md files (default: all)

[parsers.tsx] # custom (has command)
files = ["*.tsx"]
command = "./scripts/parse-tsx-links.sh"

# Rule severities: "error", "warn", or "off"
# Table form for per-rule options or custom rules
[rules]
dangling-edge = "error"
directed-cycle = "error"
stale = "error"

[rules.orphan-node]
severity = "warn"
ignore = ["README.md", "CLAUDE.md"]

[rules.max-fan-out]
command = "./scripts/max-fan-out.sh"
severity = "warn"

[rules.max-fan-out.options] # rule-specific options (passed through)
threshold = 5
```

drft automatically respects `.gitignore`.

## Graph nesting

A directory with a `drft.toml` or `drft.lock` becomes a **graph**. Child directories with their own config are **child graphs** -- they appear as `Directory` nodes in the parent graph and are checked independently.

```
project/
  drft.lock              # root graph
  drft.toml
  index.md
  docs/
    overview.md
  research/
    drft.toml            # child graph (Directory node in parent)
    drft.lock
    overview.md
    internal.md
```

Use `--recursive` to check or lock all graphs in one command. Use `[interface]` in `drft.toml` to declare a graph's public interface and control which files are visible to the parent.

## Output

Text format (default):

```
error[dangling-edge]: index.md -> gone.md (file not found)
error[stale]: index.md (stale via setup.md)
warn[directed-cycle]: cycle detected: a.md -> b.md -> c.md -> a.md
```

JSON format (`--format json`):

```json
{"rule":"dangling-edge","severity":"error","source":"index.md","target":"gone.md","message":"file not found","fix":"gone.md does not exist -- either create it or update the link in index.md"}
```

Every JSON diagnostic includes a `fix` field with actionable instructions.

DOT format (`drft graph --dot`) for graph export:

```dot
digraph {
  "index.md"
  "setup.md"
  "index.md" -> "setup.md"
}
```

Graph JSON follows the [JSON Graph Format](https://github.com/jsongraph/json-graph-specification) specification.

## Exit codes

| Code | Meaning                                                                           |
| ---- | --------------------------------------------------------------------------------- |
| 0    | Clean (warnings may be present)                                                   |
| 1    | Rule violations at `error` severity, or `lock --check` found lockfile out of date |
| 2    | Usage or configuration error                                                      |

## Custom rules

Custom rules are commands that receive the dependency graph as JSON on stdin and emit diagnostics as newline-delimited JSON on stdout:

```toml
[rules.max-fan-out]
command = "./scripts/max-fan-out.sh"
severity = "warn"
```

```sh
#!/bin/sh
# Flag nodes with more than 5 outbound links
python3 -c "
import json, sys
data = json.load(sys.stdin)
for node, count in ...:
    print(json.dumps({'message': '...', 'node': node, 'fix': '...'}))
"
```

See [examples/custom-rules](examples/custom-rules/drft.toml) for complete examples.

**Security note:** Custom rules and custom parsers execute arbitrary shell commands defined in `drft.toml`. Review the `[rules]` and `[parsers]` sections before running `drft` in untrusted repositories, the same as you would review npm scripts or Makefiles.

## LLM integration

drft works with LLMs as both a validation tool during editing sessions and a CI gate.

### JSON output

All commands support `--format json`. The `check` command returns a summary envelope:

```json
{
  "status": "warn",
  "total": 2,
  "errors": 0,
  "warnings": 2,
  "diagnostics": [
    {
      "rule": "stale",
      "severity": "warn",
      "message": "stale via",
      "node": "design.md",
      "via": "observations.md",
      "fix": "observations.md has changed — review design.md to ensure it still accurately reflects observations.md, then run drft lock"
    }
  ]
}
```

Every diagnostic includes a `fix` field with actionable instructions an LLM can follow directly.

### Impact analysis

After editing a file, check what depends on it:

```bash
drft impact src/main.rs --format json
```

```json
{
  "files": ["src/main.rs"],
  "total": 2,
  "impacted": [
    {
      "node": "docs/analyses/degree.md",
      "via": "src/main.rs",
      "depth": 1,
      "impact_radius": 4,
      "betweenness": 0.01,
      "fix": "..."
    },
    {
      "node": "README.md",
      "via": "docs/analyses/degree.md",
      "depth": 2,
      "impact_radius": 0,
      "betweenness": 0.005,
      "fix": "..."
    }
  ]
}
```

Results are sorted by review priority — high-radius nodes at shallow depth first. `impact_radius` tells you how many files cascade if you miss this one. `depth` tells you how far from the original change. `betweenness` signals structural centrality. The `fix` field provides actionable instructions.

### Claude Code hooks

A PreToolUse hook can run `drft impact` before each file edit, so the agent sees the blast radius before it acts. See [`.claude/settings.json`](.claude/settings.json) for this repo's hook configuration. Adapt the glob patterns (`*.md`, `*.rs`) to match the file types in your project.

### Agent instructions

The hook gives the agent data. What the agent _does_ with it depends on how you instruct it. Tell the agent how to use drft in your `CLAUDE.md` (or equivalent) — when to run impact, how to review dependents, when locking is appropriate. This repo dogfoods the pattern: see [`CLAUDE.md`](CLAUDE.md) for agent instructions and [`.claude/settings.json`](.claude/settings.json) for the hook configuration.

**A note on expectations:** LLM agents don't follow hooks and instructions deterministically. The agent may blow past impact output when it's deep in a multi-file change, or skip the upfront impact scan when it's eager to start editing. This is normal. The hooks and instructions improve the baseline — the agent catches more drift than it would without them — but they don't eliminate it. Treat this as a collaboration pattern to experiment with, not a guarantee. Adjust the wording in your agent instructions, try different hook triggers, and see what sticks for your workflow.

### CI

```bash
npx drft check --recursive          # catch violations and staleness
npx drft lock --check --recursive   # verify lockfiles are committed
```

Run `check` first — it catches broken links, cycles, and stale files. Then `lock --check` verifies the lockfile is committed (structural consistency). Both exit with code 1 on failure.

The workflow: impact upfront → plan includes dependents → edit → hook fires → review impacts inline → `drft lock` (only after review) → commit.

## License

MIT
