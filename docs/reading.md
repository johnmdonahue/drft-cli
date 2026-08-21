---
purpose: read the graph with nodes, edges, and graph to ground an agent
sources:
  - ../src/nodes.rs
  - ../src/edges.rs
  - ../src/impact.rs
  - ../src/projection.rs
  - ../src/main.rs
  - ../src/cli.rs
  - ../src/config.rs
---

# Reading the graph

The read verbs project what the graph already knows about your files, so a reader
— often an LLM agent — can ground itself on a file's role and connections without
opening every file. `drft nodes` answers "what is this file?", `drft edges`
answers "what does it link to?", and `drft graph` hands back the whole composed
graph at once.

| Verb                | Projects                                                                          |
| ------------------- | --------------------------------------------------------------------------------- |
| `drft nodes <sel…>` | Node metadata: the `@fs` type and hash plus any parser blocks (`@frontmatter`, …) |
| `drft edges <sel…>` | Edges leaving the selected nodes, matched on source                               |
| `drft graph`        | The whole composed graph — every node, then every edge                            |

These are projections, not traversals. `drft impact` walks the graph transitively
to answer "what depends on this?"; `drft check` gates the whole graph for drift.
The read verbs just show a scoped slice of what is already there.

## Selectors

Every read verb takes the same positional selector, matched against node keys —
the same vocabulary as `drft.toml`'s `files` and `ignore`. A selector is one of:

- An **exact path** — `docs/config.md` — resolving to that node.
- A **bare directory** — `docs/` — standing for its recursive subtree.
- A **globset pattern** — `'**/*.md'` — matched against node keys.

`docs`, `docs/`, and `docs/**` all name the same set, so there is no wrong
spelling. Quote globs so the shell passes them through rather than expanding them
itself. `drft edges` matches the selector against edge **sources**, so a directory
selector projects every edge leaving that subtree.

Selector expansion is a reader affordance only. Writers like `drft lock` stay
exact-path-only, so a subtree or glob never fans out into a write.

The empty selector divides on the same line. A reader with no selector returns
everything, because the cost is a large read. `drft lock` with no paths errors
instead, and names the whole graph as `--all`, because the cost there is a
durable claim that every node was reviewed.

An exact path that matches nothing is a likely typo: the command errors (exit 2)
with a suggestion. A glob that matches nothing is a legitimate empty result
(exit 0) — it answers which files match, and none do.

## Narrowing with `--namespace` and `--field`

Two repeatable flags narrow what comes back, shared by `nodes` and `edges`:

- `--namespace <name>` restricts to one graph's lens — `fs`, `markdown`,
  `frontmatter`, or any `[graphs.*]` you declare. It accepts the bare name or its
  `@`-prefixed key (`frontmatter` or `@frontmatter`) and filters the **set** as
  well as the metadata: a node with no `@frontmatter` block drops out rather than
  appearing empty. An unknown namespace is a typo — it errors, listing the
  declared graphs.
- `--field <name>` restricts the returned metadata to named keys and lists only
  the nodes that declare them. Frontmatter is open-ended and drft does not own its
  schema, so a wholly unmatched field is a legitimate empty result, not an error —
  it answers which files declare that field.

```
$ drft nodes docs/guide.md --namespace frontmatter --field purpose
docs/guide.md
  @frontmatter
    purpose: how to configure the walk
```

## Output: text or JSON

`--format text` (the default) is a compact block per node or edge, built for
reading without parsing JSON. A node block is its id, then each namespace, then
each field:

```
$ drft nodes docs/guide.md
docs/guide.md
  @frontmatter
    purpose: how to configure the walk
    sources: ["config.rs"]
  @fs
    hash: b3:…
    type: file
```

An edge block is `source → target`, then the same indented metadata:

```
$ drft edges docs/guide.md
docs/guide.md → docs/config.md
  @markdown
    lines: [12]
    raw: config.md
```

`drft graph --format text` renders the whole graph as a `# nodes` section
followed by a `# edges` section — the node and edge blocks above, under headers.

`--format json` returns a structured document instead: `{ total, nodes: [...],
hints: [...] }` for `nodes`, `{ total, edges: [...], hints: [...] }` for `edges`,
and the composed JGF for `graph`. `drft graph` honors `--format` like the others
(text by default); pass `--format json` for the JGF, or `--raw` for the unmerged
set of per-graph fragments, which is JSON only.

For the full flag list, run `drft nodes --help`, `drft edges --help`, or
`drft graph --help`.

## Hints

Every command carries a `hints` channel: advisories about the **run** rather than
about anything in the result. A finding says a file drifted; a hint says the
selector you passed matched nothing, or the projection you asked for is large
enough to crowd out the task it was meant to ground.

Each hint is `{name, locus?, message, next?}` — structured rather than prose, so
a reader can act on one by `name` or ignore it. `locus` is what the hint points
at when there is something to point at, and it is not always a path: a selector,
a config key like `rules.stale-nodes`, sometimes nothing.

```json
{
  "total": 0,
  "nodes": [],
  "hints": [
    {
      "name": "zero-match-selector",
      "locus": "docs/*.rs",
      "message": "matched no nodes",
      "next": "check the pattern against node keys — `*` stops at a path separator, `**` crosses it"
    }
  ]
}
```

In text, hints go to stderr after the result, so a pipe carries only the
projection.

In JSON they are a key on the result document, always present so `.hints[]` reads
without a guard — for `nodes`, `edges`, `impact`, and `check`. Three cases have no
such document to carry them: `init` and `lock` print no result at all, and
`drft graph --format json` prints a JGF document whose root is exactly `graph`, a
format rather than drft's own envelope, where a sibling key would cost the
translatability the format was chosen for. Those take a `{"hints": [...]}`
envelope on stderr instead, the same shape the error envelope uses, so a consumer
parsing stderr as JSON keeps working. Hints raised before a failure join that
error envelope rather than vanishing.

**Hints never change an exit code, and never replace a guard.** A hint annotates
output and lets it stand — so anything that has to stop a caller is an error
instead. `drft nodes docs/typo.md` fails rather than hinting, and `drft lock`
with no arguments refuses rather than locking everything, because a collapsed
`$(...)` that reads as success is the failure those guards exist for.

| Hint                  | Says                                                              |
| --------------------- | ----------------------------------------------------------------- |
| `zero-match-selector` | A selector resolved to nothing — an empty answer, not a clean one |
| `large-projection`    | The rendered output is big enough to crowd a reader's context     |
| `unknown-rule`        | A `drft.toml` rule name is not built in, so it configures nothing |
| `unparseable-lock`    | `drft.lock` could not be read, so every node reads as unlocked    |

## Grounding an agent

The read verbs let an agent learn a file's role and connections from the graph
rather than from its bytes:

- **What is this file?** `drft nodes path/to/file.md` returns its metadata — the
  authored frontmatter (its purpose, its declared sources) and its `fs` type and
  hash — so the agent knows what the file is for without reading it.
- **What does it depend on, and what depends on it?** `drft edges path/to/file.md`
  is the outbound one-hop view; `drft impact path/to/file.md` is the inbound one,
  sorted by review priority.
- **What is the whole structure?** `drft graph` renders every node and edge as
  text in a single call, so a model reads the shape of the project at once.

Because a selector expands to many nodes, one call can ground a whole subtree:
`drft nodes docs/ --namespace frontmatter --field purpose` returns every doc's
stated purpose, and `drft nodes $(drft impact observations.md --format json | jq
-r '.impacted[].node')` reads the metadata of exactly the files an edit puts in
review. (The key is `.node`; `impact` reports no `.path`, and an empty expansion
would collapse to a bare `drft nodes` that projects every node.)
