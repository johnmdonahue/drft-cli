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

| Rule              | When                                                           |
| ----------------- | -------------------------------------------------------------- |
| `stale-node`      | A node's current hash differs from its locked hash             |
| `stale-edge`      | An edge's locked target hash differs from the target's         |
| `new-edge`        | A current edge has no locked target hash                       |
| `removed-edge`    | The lockfile has an edge absent from the graph                 |
| `removed-node`    | The lockfile has a node absent from the graph                  |
| `unresolved-edge` | An edge target has no defining node (URIs excepted)            |
| `detached-node`   | A node has no inbound or outbound edges (directories excepted) |

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
withheld for paths written `./`, `../`, or `/`, which are relative by intent. It
renders as an indented line under the finding in text output and as a `cause`
field in JSON.

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
