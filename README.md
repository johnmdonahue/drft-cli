# drft

A drift checker for linked files, built for LLMs and humans working in the same repo.

When one file derives from another — a doc from code, a summary from its source, a plan from the research behind it — a change to the source can leave the dependent out of date. drft records these derivations as links in a graph, snapshots a reviewed baseline with `drft lock`, and reports which files a change has left stale or otherwise drifted. Staleness is one kind of drift; the graph's shape drifts too — a broken link, a removed file, a new edge — and drft reports those as well.

Whether a flagged derivation still holds is a reading, not something a linter can settle, so drft surfaces it for review rather than asserting it's broken. `drft impact <file>` lists what to review before an edit. `drft nodes <path>` reads a file's declared metadata so an agent can orient without opening it. `drft check` reports drift; `drft lock` records that a change was reviewed.

Each `drft.toml` declares one graph; run drft from anywhere inside the tree and it walks up to the nearest config.

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
- **`frontmatter`** — edges from frontmatter link-target values, plus the parsed frontmatter block as node metadata. Paths resolve relative to the declaring file, as its markdown links do. Set `keys = ["sources"]` to scope edges to named keys instead of every path-shaped value.

Composition merges the set into one graph, and `drft check` reads it to emit drift findings:

- **Broken links** — edges to a target with no defining node (`unresolved-edge`)
- **Staleness** — a node's content, or a dependency's, changed since the last lock (`stale-node`, `stale-edge`)
- **Structure** — edges added or removed since lock, or a node with no connections (`new-edge`, `removed-edge`, `removed-node`, `detached-node`)

All rules default to `warn`. Override to `error` for CI enforcement or `off` to suppress. Dependency cycles are permitted — staleness is computed locally, and `drft impact` is cycle-safe.

## Commands

| Command       | What it does                                                   |
| ------------- | -------------------------------------------------------------- |
| `drft init`   | Create a default `drft.toml`                                   |
| `drft graph`  | Render the composed graph as text or JGF (`--raw` for the set) |
| `drft nodes`  | Project node metadata by path, subtree, or glob                |
| `drft edges`  | Project edges (matched on source) by path, subtree, or glob    |
| `drft impact` | Show what depends on given files, sorted by review priority    |
| `drft check`  | Compare the graph against the lockfile for drift               |
| `drft lock`   | Snapshot hashes to `drft.lock` for staleness tracking          |

`drft lock` with no argument snapshots the whole graph. Given paths — `drft lock
src/lib.rs docs/guide.md` — it locks only those nodes and their outbound edges,
merging into the existing lockfile. A lock asserts the locked state was reviewed,
so scope it to what you actually read: a bulk lock also clears staleness you
never looked at, including someone else's unfinished work. Paths all resolve
before anything is written, so a typo fails the command rather than leaving a
partial lock behind.

All commands support `--format json`. Run `drft --help` for the full flag reference.

`drft impact` reports the files that name the seed **directly** — one hop. That is
the question an edit asks: each hit is a promise someone wrote down, so it lands
a specific thing to check. Every result also carries a `radius`, the count of
nodes reachable behind it, so a wider set is reported without being enumerated.
Widen with `--depth <n>` when a hit turns out to restate the change, or
`--depth all` for the full reachable set — what a rename sweep wants.

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

drft grounds an agent in a codebase's structure without making it open every file. The read verbs project what the graph already knows:

- `drft nodes <path>` returns a file's metadata — its frontmatter fields (say, `purpose` or `sources`) plus its `fs` type and hash — so an agent learns what a file is _for_ without reading it.
- `drft edges <path>` returns what a file links to; `drft impact <path>` returns what depends on it, sorted by review priority.
- `drft graph` renders the whole composed graph as compact text, so a model reads the structure in one call without parsing JSON.

Every command also emits `--format json` with actionable `fix` fields in each diagnostic. See [Reading the graph](docs/reading.md) for the selector grammar and the `--namespace` / `--field` filters.

This repo dogfoods the pattern with Claude Code hooks. See [CLAUDE.md](CLAUDE.md) for the agent instructions and `.claude/settings.json` for the hook configuration.

## Docs

The [full documentation](docs/README.md) covers the graph substrate, parsers, rules, and configuration.

## License

MIT
