# drft

A structural integrity checker for linked file systems.

Files link to other files. When a target changes, the files that depend on it may no longer be accurate. drft tracks these dependencies and flags what needs review.

It treats your directory as a graph — files are nodes, links are edges — and validates the structure. `drft check` catches broken links, cycles, and staleness. `drft lock` snapshots hashes so it can detect when a dependency changes and flag everything downstream. `drft impact <file>` tells you what to review when you touch a file.

Each `drft.toml` declares exactly one graph. Run drft from anywhere inside the directory tree and it walks up to the nearest config. That's the model — everything else is configuration.

## Install

```bash
cargo install drft-cli    # via Cargo
npm install -g drft-cli   # via npm
```

Or download a prebuilt binary from [GitHub Releases](https://github.com/johnmdonahue/drft-cli/releases).

The binary is called `drft`.

## Quick start

```bash
drft init                 # create a drft.toml
drft check                # validate the graph
drft lock                 # snapshot file hashes
drft check                # now detects staleness too
```

## How it works

drft discovers files matching your `include` patterns, runs parsers to extract links, and builds a dependency graph. It then validates the graph against rules:

- **Broken links** — edges to files that don't exist (`unresolved-edge`)
- **Cycles** — circular dependencies (`directed-cycle`)
- **Staleness** — files changed since last lock, including transitive dependents (`stale`)
- **Isolation** — nodes with no connections or disconnected components (`orphan-node`, `fragmentation`)

drft ships with [built-in rules](docs/rules/README.md) covering structural integrity, plus support for [custom rules](docs/rules/custom.md) via external commands. All rules default to `warn`. Override to `error` for CI enforcement or `off` to suppress.

Underneath the rules, drft computes [structural analyses](docs/analyses/README.md) — degree, centrality, connected components, depth, and more. Rules consume these analyses; you can also query them directly with `drft report`.

## Commands

| Command            | What it does                                                |
| ------------------ | ----------------------------------------------------------- |
| `drft check`       | Validate the graph against enabled rules                    |
| `drft lock`        | Snapshot file hashes to `drft.lock` for staleness tracking  |
| `drft impact`      | Show what depends on given files, sorted by review priority |
| `drft graph`       | Export the dependency graph (JSON Graph Format or DOT)      |
| `drft report`      | Query structural analyses directly                          |
| `drft parse`       | Show raw parser output before graph construction            |
| `drft config show` | Display resolved configuration                              |
| `drft init`        | Create a default `drft.toml`                                |

Most commands support `--format json` and `drft check` supports `--watch`. Run `drft --help` for the full flag reference.

## Configuration

`drft.toml` in the directory root:

```toml
include = ["**/*.md"] # which files become nodes (default)
exclude = ["drafts/*"] # also respects .gitignore

[parsers.markdown] # built-in parser, no config needed

[rules]
stale = "error" # escalate for CI
orphan-node = "off" # suppress if expected

[rules.orphan-node] # or use table form for options
severity = "warn"
ignore = ["README.md"]
```

Parsers extract links from files. drft includes markdown and YAML frontmatter parsers; you can add [custom parsers](docs/parsers/custom.md) for any format. See the [configuration reference](docs/config.md) for all options.

## LLM integration

All commands support `--format json` with actionable `fix` fields in every diagnostic. `drft impact` is designed for agent workflows — it shows which files to review after an edit, sorted by priority.

This repo dogfoods the pattern with Claude Code hooks. See [CLAUDE.md](CLAUDE.md) for the agent instructions and [`.claude/settings.json`](.claude/settings.json) for the hook configuration.

## Docs

The [full documentation](docs/README.md) covers parsers, graph construction, structural analyses, rules, and configuration.

## License

MIT
