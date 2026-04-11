---
sources:
  - ../../src/rules/boundary_edge.rs
---

# boundary-edge

Flags edges whose target resolves above the graph root. The graph root is the directory containing `drft.toml`, and drft treats that directory as a [subgraph](https://en.wikipedia.org/wiki/Glossary_of_graph_theory#subgraph) of the filesystem. A **boundary edge** has one endpoint inside the subgraph and one outside — here, a link from a file in the graph to something that resolves above the root.

## Example

```
project/
  drft.toml
  index.md       # links to ../outside.md
../outside.md    # lives above the graph root
```

```
warn[boundary-edge]: index.md → ../outside.md (link crosses the graph boundary)
```

The target is still created as a node so other rules and traversals see it, but no content is read from it and the hash is omitted.

## Why flag boundary edges

The rule covers two related concerns. Whether you care about one, the other, or both depends on what the graph is for.

### 1. Portability

Keeping a graph self-contained makes it movable as a unit. If every edge stays inside the root, you can copy or rename the directory — or extract it into its own repo — without chasing down broken links. A boundary edge breaks that guarantee: the target lives somewhere only the surrounding filesystem can resolve, and the graph stops being a closed world.

### 2. Path traversal

`../` chains, absolute paths, and symlinks that resolve above the root are the classic shape of a directory traversal. drft's graph builder already refuses to read or hash anything that canonicalizes outside the root — the safety decision is made below the rule layer so parsers can't accidentally pull `/etc/hosts` into the graph. This rule is the user-facing signal for the same invariant: it tells you _which_ link tried to cross the boundary so you can fix the source, not just know the content wasn't read.

## What counts as a boundary edge

- Relative paths that climb above the root: `../foo.md`, `../../a/b.md`
- Absolute paths: `/etc/hosts`, `/tmp/notes.md`

The check is lexical — it looks at the target path the parser emitted, not the canonicalized result. A symlink inside the root whose real target lives above the root is caught by the graph builder's safety check (the node exists with no hash) but isn't flagged by this rule today.

## Configuration

```toml
[rules]
boundary-edge = "warn" # default
```

```toml
[rules.boundary-edge]
severity = "error" # escalate for CI
ignore = ["legacy/**"] # suppress for files you know leak
```

## Fix

Either pull the target inside the graph (move the file under the `drft.toml` root and add it to `include`) or drop the link. If you deliberately want a graph to reference its surroundings, set the rule to `off`.
