# drft

A structural integrity checker for linked file systems.

Files link to other files. When a target changes, the files that depend on it may no longer be accurate. drft tracks these dependencies and flags what needs review.

It treats your directory as a graph — files are nodes, links are edges — and checks the structure for drift. `drft check` catches broken links and staleness. `drft lock` snapshots hashes so it can detect when a dependency changes and flag everything downstream. `drft impact <file>` tells you what to review when you touch a file.

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

drft builds a **set of independent graphs** and merges them by path:

- **`fs`** — walks the tree under the root (minus `ignore` and `.gitignore`), typing each file, symlink, and directory as a node and hashing the ones with content. This is the identity space.
- **`markdown`** — link edges from `[text](path)` body links.
- **`frontmatter`** — edges from frontmatter link-target values, plus the parsed frontmatter block as node metadata.

Composition merges the set into one graph, and `drft check` reads it to emit drift findings:

- **Broken links** — edges to a target with no defining node (`unresolved-edge`)
- **Staleness** — a node's content, or a dependency's, changed since the last lock (`stale-node`, `stale-edge`)
- **Structure** — edges added or removed since lock, or a node with no connections (`new-edge`, `removed-edge`, `removed-node`, `detached-node`)

All rules default to `warn`. Override to `error` for CI enforcement or `off` to suppress. Dependency cycles are permitted — staleness is computed locally, and `drft impact` is cycle-safe.

## Commands

| Command       | What it does                                                |
| ------------- | ----------------------------------------------------------- |
| `drft init`   | Create a default `drft.toml`                                |
| `drft graph`  | Export the composed graph as JGF (`--raw` for the set)      |
| `drft impact` | Show what depends on given files, sorted by review priority |
| `drft check`  | Compare the graph against the lockfile for drift            |
| `drft lock`   | Snapshot hashes to `drft.lock` for staleness tracking       |

All commands support `--format json`. Run `drft --help` for the full flag reference.

`drft lock` takes paths to lock a subset: `drft lock a.md b.md` snapshots only those nodes and merges them into the existing lockfile, leaving every other entry untouched. Locking a path whose file has been deleted drops its entry — how you clear a `removed-node` finding once you have reviewed the deletion. Paths that resolve are written even when another path in the same call does not; the unresolved ones are reported and set a non-zero exit.

## Configuration

`drft.toml` in the directory root:

```toml
ignore = ["target/**"] # remove from the walk (also respects .gitignore)

[graphs.markdown] # fs is implicit; declare the graphs you want
parser = "markdown"
files = ["**/*.md"]

[rules]
stale-node = "error" # escalate for CI
stale-edge = "error"

[rules.detached-node] # table form for ignore globs
severity = "off"
```

See the [configuration reference](docs/config.md) for all options.

## LLM integration

All commands support `--format json` with actionable `fix` fields in every diagnostic. `drft impact` is designed for agent workflows — it shows which files to review after an edit, sorted by priority.

This repo dogfoods the pattern with Claude Code hooks. See [CLAUDE.md](CLAUDE.md) for the agent instructions and `.claude/settings.json` for the hook configuration.

## Docs

The [full documentation](docs/README.md) covers the graph substrate, parsers, rules, and configuration.

## License

MIT
