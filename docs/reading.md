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

## Impact and missing connections

`drft impact <path>...` traverses the current graph from exact seed paths. Its
default inbound direction finds dependents; `--direction outbound` finds
dependencies, and `--direction both` follows either direction. `--depth` bounds
the hops, with `--depth all` traversing the full reachable set. Seeds must exist
in the current graph, even when the lockfile remembers a removed file.

Impact carries diagnostics that qualify the answer:

- Construction findings cover all configured graphs, including disconnected
  files and metadata-only graphs. An unreadable declaration could name any
  target, so current reachability cannot establish its relevance. These findings
  identify read failures; they do not claim that a failed file depended on a seed.
- `unresolved-edge` and `unresolved-fragment` cover every current edge inspected
  in the requested direction. Nodes at the depth limit are reached but not
  expanded. Inspected edges include cycles, alternate paths, and edges between
  seeds.
- `removed-edge` and applicable `removed-node` findings use historical pairs
  from the optional `drft.lock` beside that same current expansion frontier.
  Historical pairs never extend traversal or contribute to ranking or `total`.
  A chain of removed declarations therefore does not reconstruct a historical
  dependency graph.

Configured severities and subject ignores apply as they do in `check`. Impact
omits `stale-node`, `stale-edge`, `new-edge`, `detached-node`, `unlocked-node`,
and `no-baseline`.
A missing or empty baseline is quiet; an unparseable baseline carries the
`unparseable-lock` hint. Impact reads the lockfile without updating it.

JSON returns `{seeds, total, impacted, diagnostics, hints}`, with `diagnostics`
always present and `total` counting impacted nodes only. Text appends ordinary
findings after the traversal. When diagnostics accompany an empty traversal, its
message says `in the current graph`. Displayed construction findings also add
`graph read has diagnostics; dependency coverage may be incomplete`.

A completed impact read exits 0 even when a diagnostic has severity `error`.
Read the diagnostics before acting on the impacted set. Use `check` when errors
must gate a caller; it exits 1 for findings configured as errors.

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
exact-path-only, so a subtree or glob never fans out into a write. A directory
named to `drft lock` therefore resolves to the directory node itself, which
carries no content to snapshot — the command fails rather than claiming a lock
that changed nothing. Name the files you reviewed. Any recursive locking syntax
must use an explicit selector rather than reinterpret a directory path as its subtree.

`drft nodes` and `drft edges` require at least one selector or an explicit
`--all`. A missing selector errors in every environment, so an empty shell
expansion cannot turn a scoped read into the whole graph. You may combine
`--all` with `--namespace` and `--field`, but not with a selector. `drft lock`
uses the same explicit spelling because the whole-graph form makes a durable
claim that every node was reviewed.

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
  the entries that declare them. A field names what it is wherever it is
  declared: the filter also descends into a list of objects, narrowing each entry
  to the named keys, so `--field role` reaches an authored `authors:` list and
  `--field line` reaches an edge's link occurrences without naming the array they
  live in. An entry that keeps nothing drops out, so a node declaring the field
  only inside a list still survives the filter. Frontmatter is open-ended and drft
  does not own its schema, so a wholly unmatched field is a legitimate empty
  result, not an error — it answers which files declare that field.

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
  @markdown
    anchors: ["the-walk","scoping"]
```

`anchors` are the `#fragment` addresses that file answers to — one per heading,
in the flavor a reader's platform resolves. They are what makes a link's fragment
checkable, and `drft nodes <path> --field anchors` is how to see what a file can
be cited by.

An edge block is `source → target`, then the same indented metadata:

```
$ drft edges docs/guide.md
docs/guide.md → docs/config.md
  @markdown
    occurrences
      - line: 12
        raw: config.md
      - line: 40
        link: docs/config.md#ignore
        raw: config.md
```

Each entry in `occurrences` is one link the author wrote, so a target cited from
several places keeps each line paired with the spelling on that line. `--field
line` narrows to one of those facts.

Any list of objects renders this way, in `nodes` and `graph` as well as `edges`:
one `-`-marked sub-block per entry, so an authored `authors:` list reads as
entries rather than as one line of JSON. An entry with no fields renders as
`- {}`.

`drft graph --format text` renders the whole graph as a `# nodes` section
followed by a `# edges` section — the node and edge blocks above, under headers.

`--format json` returns a structured document instead: `{ total, nodes: [...],
hints: [...] }` for `nodes`, `{ total, edges: [...], hints: [...] }` for `edges`,
and the composed JGF for `graph`. `drft graph` honors `--format` like the others
(text by default); pass `--format json` for the JGF, or `--raw` for the unmerged
set of per-graph fragments, which is JSON only.

For the full flag list, run `drft nodes --help`, `drft edges --help`, or
`drft graph --help`.

## Refusing oversized output

`nodes`, `edges`, `graph`, and `impact` accept `--max-bytes <N>`. The budget
counts the complete UTF-8 document written to stdout, including its final
newline and JSON hints. If the result exceeds the budget, drft writes no stdout
and exits 2. Text commands explain the refusal on stderr; JSON commands emit one
JSON error envelope there. `graph --raw` follows the JSON contract even when you
omit `--format json`.

Omitting `--max-bytes` remains unbounded. drft never truncates or summarizes a
result: either the complete text or JSON document fits, or the command refuses
before its first write. Narrow `nodes` and `edges` with selectors,
`--namespace`, or `--field`; narrow `impact` with its seeds, `--depth`, or
`--direction`; use a scoped `nodes` or `edges` read instead of `graph`.

Impact's budget includes diagnostics and their explanatory text. Construction
diagnostics cover all configured graphs, so narrowing seeds, depth, or direction
only shrinks traversal output. Increase `--max-bytes` or repair the read failures
when construction diagnostics exceed the budget.

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

Text output is line-oriented, so one record is always one line. A path or value
carrying a newline or a control character — anything a filename or a YAML scalar
can hold — is escaped when rendered as text, so a finding, an edge, a node id, or
a hint locus can never split across lines. The escaping belongs to the rendering:
every value reaches JSON exactly as written, which is the authoritative form for
anything reading drft rather than reading with drft.

In text, hints go to stderr after the result, so a pipe carries only the
projection.

In JSON they are a key on the result document, always present so `.hints[]` reads
without a guard — for `nodes`, `edges`, `impact`, `check`, and `lock`. Two cases
have no such document to carry them: `init` prints no result at all, and
`drft graph --format json` prints a JGF document whose root is exactly `graph`, a
format rather than drft's own envelope, where a sibling key would cost the
translatability the format was chosen for. Those take a `{"hints": [...]}`
envelope on stderr instead, the same shape the error envelope uses, so a consumer
parsing stderr as JSON keeps working. Hints raised before a failure join that
error envelope rather than vanishing. In text, a hint raised before a failure
appears on stderr before the `error:` record. In JSON, it appears in the single
error envelope; drft does not print a separate hints envelope first.

`drft lock` prints a result document too, `{locked, dropped, hints}`, naming the nodes it
wrote and the entries it removed. Reporting the effect is what makes a lock that
covered nothing distinguishable from one that covered the files you meant. Path
operands select only the exact cwd-relative node they name. A missing bare path may
suggest a unique `.md` correction, but the command exits 2 without selecting it.

**Hints never change an exit code, and never replace a guard.** A hint annotates
output and lets it stand — so anything that has to stop a caller is an error
instead. `drft nodes docs/typo.md` fails rather than hinting, and `drft lock`
with no arguments refuses rather than locking everything, because a collapsed
`$(...)` that reads as success is the failure those guards exist for.

| Hint                        | Says                                                                                                   |
| --------------------------- | ------------------------------------------------------------------------------------------------------ |
| `zero-match-selector`       | A selector resolved to nothing — an empty answer, not a clean one                                      |
| `large-projection`          | The rendered output is big enough to crowd a reader's context                                          |
| `unknown-rule`              | A `drft.toml` rule name is not built in, so it configures nothing                                      |
| `unparseable-lock`          | `drft.lock` could not be read, so staleness cannot be evaluated                                        |
| `directory-lock`            | A formerly lockable path is now a directory; its old entry was dropped, but no descendants were locked |
| `nothing-to-lock`           | A locked path carries no content to snapshot                                                           |
| `replaced-unreadable-lock`  | A rebuild replaced a lockfile it could not read, so its drops are unlisted                             |
| `edge-keys-matched-nothing` | A graph declares `edge_keys` and gets no edges — nothing yielded one, or no file was read              |

When files matched by a zero-edge frontmatter graph cannot be read, the hint
advises repairing those files first. If some matched files are readable, it also
names the key, glob, ignore, and string-value checks to run if no edges remain
after repair. This advice uses raw evidence for that graph even when a rule is
off or its subjects are ignored. File-read errors influence this advice but do
not have a construction finding of their own.

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
fails instead of widening the read.)
