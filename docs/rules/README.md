---
purpose: the drift and structural findings drft check emits
sources:
  - ../../src/rules/staleness.rs
  - ../../src/rules/structural.rs
  - ../../src/config.rs
---

# Rules

A rule is a function over the composed graph: graph in, findings out. `drft check`
runs every rule, joins the lockfile for the staleness rules, and emits findings as
warnings or errors.

Configure rules in `drft.toml` under `[rules]`. Every rule has a severity:
`"warn"`, `"error"`, or `"off"`. All rules default to `warn`; override to
`error` for CI enforcement or `off` to suppress. A finding's `subject` is the
implicated path (the source node for edge-level findings). Edge-level findings
also report the source `lines` where the link appears, annotating the subject in
text output as `subject:line → target`.

```toml
[rules]
stale-node = "error"
stale-edge = "error"
```

## Built-in rules

The rule set is deliberately drift-focused. [`staleness.rs`](../../src/rules/staleness.rs)
derives the drift findings by joining the graph to the lockfile;
[`structural.rs`](../../src/rules/structural.rs) derives the rest from graph shape.

| Rule                  | When                                                           |
| --------------------- | -------------------------------------------------------------- |
| `stale-node`          | A node's current hash differs from its locked hash             |
| `stale-edge`          | An edge's locked target hash differs from the target's         |
| `new-edge`            | A current edge has no locked target hash                       |
| `removed-edge`        | The lockfile has an edge absent from the graph                 |
| `removed-node`        | The lockfile has a node absent from the graph                  |
| `unresolved-edge`     | An edge target has no defining node (URIs excepted)            |
| `unresolved-fragment` | A link's `#fragment` names no anchor its target defines        |
| `detached-node`       | A node has no inbound or outbound edges (directories excepted) |

**A link to a directory tracks the directory, not its contents.** Directories are
nodes, so the edge resolves and `unresolved-edge` stays quiet — but directories
carry no hash, so nothing inside one is tracked and no descendant's change makes
the linking file stale. A doc citing `` `src/` `` reads as an inventory of that
tree and is not one. Link the file that carries what the prose claims —
`src/lib.rs` over `src/` — when you want the promise tracked.

`unresolved-edge` carries a `cause` when the link text would resolve from the
graph root but not from the declaring file. Links resolve relative to the file
that declares them, so a root-relative path fails against a target nobody wrote
and reads as a typo; the `cause` names the base and suggests the rewrite. It is
withheld for paths written `./`, `../`, or `/`, which are relative by intent. The
check runs per link occurrence, so a target cited from several places carries the
cause when any one of those links is bare — the finding names every line, and the
cause describes the bare one. It renders as an indented line under the finding in
text output and as a `cause` field in JSON.

`unresolved-fragment` checks the other half of a link. A markdown parser
publishes the `#fragment` addresses each file it reads answers to — the GitHub
slug of every heading, in document order, with GitHub's `-1` disambiguator on a
repeat — and a link carrying a fragment its target does not define is a broken
reference that the file existing does not save. The finding names the citing line
rather than the edge, so a source citing two anchors of one target implicates
only the wrong one.

Matching is exact, and deliberately so: drft slugs the **heading** and compares
the fragment byte-for-byte, never slugging the citing side. Normalizing the
fragment would accept `#OBS 92` for an `obs-92` anchor, which a browser sends as
`#OBS%2092` and does not find — and certifying a link that 404s for a reader is
the one thing sub-file addressing must not do. A fragment that matches an anchor
once case is ignored carries a `cause` naming the anchor it meant; browsers do
fall back to a case-insensitive match, so such a link works today and is fragile
rather than broken.

A fragment is only checked against a target some parser read. A link into a `.rs`
file, or into a markdown file outside the graph's `files` scope, has **unknown**
fragments rather than broken ones, and drft says nothing. A file that was read
and defines no headings is the opposite case: every fragment into it is broken.
An unresolvable target reports `unresolved-edge` alone — the fragment is the
lesser half of the same mistake.

A finding is about an item in the graph. A statement about the _run_ that
produced it — an unknown rule name, a selector that matched nothing — is a
[hint](../reading.md#hints) instead, carried on the result document rather than
in `diagnostics`.

Staleness is computed locally — per node and per edge, with no recursive
propagation — so dependency cycles can't loop or produce ambiguous staleness. A
stale node subsumes its outbound `stale-edge` findings; a removed node subsumes
its `removed-edge` findings.

## Per-rule configuration

```toml
[rules.detached-node]
severity = "off"

[rules.unresolved-edge]
ignore = ["CHANGELOG.md", "LICENSE"]
```

- `severity`: `"error"`, `"warn"`, or `"off"`
- `ignore`: globs matched against the finding's subject path

An `ignore` set directly under `[rules]` (rather than `[rules.<name>]`) applies to
every rule, unioned with each rule's own `ignore`:

```toml
[rules]
ignore = ["vendor/**"] # don't validate these files under any rule
```

This suppresses findings _about_ the matched files (their staleness, broken
internal links, detachment) but not findings about your files that depend on them
— a `stale-edge` whose subject is your file survives, since its subject isn't
matched. The files stay in the graph, so your links to them still resolve. This
is distinct from the top-level `ignore`, which removes paths from the graph
entirely.
