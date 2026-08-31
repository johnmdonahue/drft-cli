---
purpose: the drift and structural findings drft check emits
sources:
  - ../../src/rules/staleness.rs
  - ../../src/rules/structural.rs
  - ../../src/rules/check.rs
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

| Rule                     | When                                                            |
| ------------------------ | --------------------------------------------------------------- |
| `stale-node`             | A node's current hash differs from its locked hash              |
| `stale-edge`             | An edge's locked target hash differs from the target's          |
| `new-edge`               | A current edge has no locked target hash                        |
| `removed-edge`           | The lockfile has an edge absent from the graph                  |
| `removed-node`           | The lockfile has a node absent from the graph                   |
| `unresolved-edge`        | An edge target has no defining node (URIs excepted)             |
| `unresolved-fragment`    | A link's `#fragment` names no anchor its target defines         |
| `detached-node`          | A node has no inbound or outbound edges (directories excepted)  |
| `unlocked-node`          | A lockable node has no lock entry, so it has no baseline        |
| `no-baseline`            | The lockfile is absent or has no entries, so nothing is checked |
| `unreadable-frontmatter` | A frontmatter block was recognized and is not a YAML mapping    |

**A block nobody can read is reported, not dropped.** A leading fenced block is
frontmatter by its fences alone, so its text is never rendered as content. When
the YAML inside it is not a mapping, the file contributes no metadata and no
declared edges, and without a finding it looks identical to a file that declares
nothing. `unreadable-frontmatter` names the file and its opening line.

It is a rule rather than a hint because a repository whose derivations are
declared in frontmatter needs a dropped block to be able to fail a run. It
defaults to `warn` like every other rule, and promotes to `error` the same way.

**A baseline that does not exist is reported, not assumed.** `drft check` derives
staleness by comparing the graph against the lockfile, so with no lockfile — or one
with no entries — there is nothing to compare against and every staleness rule
becomes a no-op. That used to be indistinguishable from a clean run: no findings,
exit 0, either way. `no-baseline` states it once, at the run rather than per node.

It fires on three states, which are one fact — no usable baseline: the lockfile is
absent, it carries no entries, or it could not be parsed. An unparseable lockfile
also raises the `unparseable-lock` hint, which names the cause; the finding reports
the consequence. `no-baseline` stays quiet when nothing in the graph could have
been locked in the first place, since an absent baseline over a tree of directories
alone covers everything there is to cover.

**An empty lockfile still runs the staleness rules. An absent or unparseable one
does not.** Absent is the ordinary state of a repo that has never locked, and one
finding per file would bury it. Unparseable means the baseline may be intact and
merely unreadable, so reporting every node as unlocked would assert something drft
cannot know. Empty means a baseline was established and then emptied — every
lockable node really is unlocked, and `unlocked-node` says so per node, which is
what keeps a promoted rule gating in the state where it matters most. The last also raises the
`unparseable-lock` hint, which is what names the cause; the finding reports the
consequence.

It is a rule and not a hint because hints never change an exit code. At its default
`warn` a first `check` in a new repo stays quiet, and a repo that wants a missing
baseline to fail its run promotes it:

```toml
[rules]
no-baseline = "error"
```

**`unlocked-node` covers the partial case**, where a lockfile exists but a given
node has no entry in it. Which nodes can be locked is asked of the lockfile writer
rather than derived independently, so the rule and the writer cannot disagree: a
directory and an unreferenced escaping symlink carry no hash and no outbound edge,
are absent from a correct lockfile by design, and are never reported. An unlocked
node subsumes its outbound `new-edge` findings, the way a stale node subsumes its
`stale-edge` findings — the node having no baseline is the single fact behind every
one of them. The subsumption is applied after severity and ignore globs, so
silencing `unlocked-node` restores the `new-edge` findings it was standing in for
rather than dropping both.

**In a repo that locks only what it reads**, every file never named to `drft lock`
reports this, which in a source tree means most of it. That is accurate rather than
noisy — those files genuinely have no baseline — but it is not always what a repo
wants to see on every run. Narrow it the way any rule is narrowed, with a severity
of `off` or an `ignore` glob:

```toml
[rules.unlocked-node]
ignore = ["src/**", "tests/**"]
```

Narrowing it this way gives up what the rule reports — an ignored path with no lock
entry no longer says so — while leaving the `new-edge` findings that predate it in
place. That is a reasonable trade for a tree you do not track, and a bad one for a
tree you do, so scope the glob to the former.

A node with no entry of its own can still be the target of a locked edge, whose
recorded target hash catches an edit to it. `unlocked-node` reports the absent
baseline, not an absence of checking.

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
the fragment to the result, never slugging the citing side. Normalizing the
fragment would accept `#OBS 92` for an `obs-92` anchor, which a browser sends as
`#OBS%2092` and does not find — and certifying a link that 404s for a reader is
the one thing sub-file addressing must not do.

The one transformation drft does apply is percent-decoding, because a browser
applies it too: `#caf%C3%A9` resolves to a `café` anchor, and a permalink copied
from the address bar is encoded for any non-ASCII anchor. Decoding accepts
exactly what the platform accepts and loosens nothing — `#OBS%2092` still decodes
to `OBS 92` and still misses.

Id matching is case-sensitive, so a fragment differing from an anchor only in
case is broken rather than untidy. It carries a `cause` naming the anchor it
meant, which makes the fix obvious without excusing it.

A fragment is only checked against a target some parser read. A link into a `.rs`
file, into a symlink, or into a markdown file outside the graph's `files` scope
has **unknown** fragments rather than broken ones, and drft says nothing. Only a
graph whose parser publishes anchors is authoritative about them, so a file
writing `anchors:` in its own frontmatter claims nothing. A file that was read
and defines no headings is the opposite case: every fragment into it is broken.
An unresolvable target reports `unresolved-edge` alone — the fragment is the
lesser half of the same mistake.

**Anchor-only links are not checked.** `[see](#section)` names no file, so it
produces no edge and the rule never sees it — which is the most common broken
fragment in a long document, and the gap worth knowing about. Write the path
(`[see](./this-file.md#section)`) to have it checked.

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
