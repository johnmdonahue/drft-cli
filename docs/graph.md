---
sources:
  - ../src/discovery.rs
  - ../src/graph.rs
  - ../src/diagnostic.rs
  - ../src/lockfile.rs
---

# Graph builder

The graph builder sits between parsers and rules. It takes raw parser output and produces the enriched graph that everything else consumes.

## Responsibility boundaries

| Layer             | Responsibility                                         | Does NOT do                                       |
| ----------------- | ------------------------------------------------------ | ------------------------------------------------- |
| **Parsers**       | Emit raw link strings as they appear in source         | No normalization, no classification, no filtering |
| **Graph builder** | Normalize targets, resolve paths, create nodes, enrich | No judgment — that's rules                        |
| **Rules**         | Judge the enriched graph, emit diagnostics             | No filesystem access, no re-computation           |

Parsers should emit what they find. The graph builder decides what it means.

## What parsers emit

A parser returns a list of link strings. Each is a raw string exactly as it appears in the source file:

```
setup.md                     → file path
setup.md#installation        → file path with fragment
https://example.com          → URI
https://example.com#section  → URI with fragment
mailto:user@example.com      → URI (mailto scheme)
#heading                     → anchor-only (no file target)
```

Parsers decide what constitutes a link in their format and extract a path or URI. They don't strip fragments, detect URI schemes, or classify targets — that's the graph builder's job.

## What the graph builder does

### 1. Normalize targets

Every raw link passes through `normalize_link_target()`. Fragments are stripped for node identity and stored in `edge.link` when present:

| Raw link                      | `edge.target` (node ID)   | `edge.link`                           | Action                                  |
| ----------------------------- | ------------------------- | ------------------------------------- | --------------------------------------- |
| `setup.md#heading`            | `setup.md`                | `Some("setup.md#heading")`            | Fragment stripped, original preserved   |
| `https://example.com#section` | `https://example.com`     | `Some("https://example.com#section")` | Same for URIs                           |
| `mailto:user@example.com`     | `mailto:user@example.com` | `None`                                | No fragment — target is complete        |
| `setup.md`                    | `setup.md`                | `None`                                | No fragment — target is complete        |
| `#heading`                    | —                         | —                                     | **Dropped** — no file target to resolve |
| _(empty)_                     | —                         | —                                     | **Dropped**                             |

Only two things are dropped: empty targets and anchor-only targets (no file to resolve). Everything else enters the graph.

`edge.target` is always the node ID — you can join on it directly without any transformation.

### 2. Detect URIs

`is_uri()` uses the [`url`](https://docs.rs/url) crate (WHATWG URL Standard) to parse the target, then accepts it as a URI if it has authority (`://`) or uses a known opaque scheme (`mailto`, `tel`, `data`, `urn`, `javascript`).

URI targets skip path resolution (they're not relative file paths) and become referenced nodes with `type: "uri"`.

### 3. Resolve paths

The graph builder resolves non-URI targets relative to the source file:

```
source: guides/intro.md
link:   ../setup.md#heading
target: setup.md             (path resolved, fragment stripped)
edge.link: setup.md#heading  (resolved path with fragment)
```

Uses standard path joining with `..` / `.` normalization.

### 4. Create referenced nodes

After parsing, every unique edge target that isn't already a node gets added to the graph as a **referenced node** with `included: false`. This ensures every `edge.target` resolves to a node — you can always join on it.

URI targets (detected by `is_uri()`) get `type: "uri"`. Filesystem targets get statted to determine their type:

| `type`        | Source         | Meaning                       |
| ------------- | -------------- | ----------------------------- |
| `"file"`      | stat           | Regular file on disk          |
| `"directory"` | stat           | Directory on disk             |
| `"symlink"`   | stat           | Symbolic link on disk         |
| `"uri"`       | string parsing | Off-filesystem (http, mailto) |
| `null`        | stat failed    | Nothing on disk — broken link |

`include` controls what drft reads and hashes. drft stats any non-URI target within the graph root to determine its type.

### 5. Symlink policy

The walker follows symlinks so symlinked directories are traversable. The security boundary is enforced at hashing, not at walking.

For each entry matching `include`:

1. If not a symlink: read, hash, create node.
2. If a symlink: canonicalize the path. If the canonical form is under the graph root and matches `include`, read and hash. Otherwise, create the node with `hash = None` — content is intentionally not read.

This prevents content access through symlinks pointing outside the graph. `include` patterns don't traverse above the root (the walker is rooted at the `drft.toml` directory).

Symlinks in `include` also get a filesystem edge. The graph builder reads the symlink target, resolves it relative to the source, and adds an edge with `parser: "filesystem"`. If the resolved target isn't already a node, it gets statted and added as a referenced node.

### 6. Enrich

After building, `enrich()` computes all [structural analyses](analyses/README.md) unconditionally. Rules receive the enriched graph — all properties pre-computed.

## Edge structure

Edges carry the relationship and provenance:

| Field    | Type             | Description                                                        |
| -------- | ---------------- | ------------------------------------------------------------------ |
| `source` | String           | Source file path                                                   |
| `target` | String           | Target path or URI (always matches a key in `graph.nodes`)         |
| `link`   | Option\<String\> | Original link when it differs from target (e.g., `bar.md#heading`) |
| `parser` | String           | Which parser discovered this edge (provenance)                     |

`target` is always a node ID — you can join on it directly. `link` is present only when the original reference included a fragment. No transformation needed for consumers.

An edge is **internal** when its target node is `included`. Use `graph.is_internal_edge(&edge)` to check.

## JSON output

The JSON graph output follows the [JGF v2.0](https://jsongraphformat.info/) schema. Parser provenance lives in edge `metadata`:

```json
{
  "source": "index.md",
  "target": "bar.md",
  "metadata": { "parser": "markdown" }
}
```

Edges include all targets — included, referenced, and missing.

Node metadata includes `type`, `included`, `hash` (when present), and any parser-extracted metadata keyed by parser name:

```json
{
  "id": "setup.md",
  "metadata": {
    "type": "file",
    "included": true,
    "hash": "b3:...",
    "frontmatter": { "title": "Setup", "sources": ["../shared/glossary.md"] }
  }
}
```

Referenced nodes (targets not in `include`) have `included: false` and no hash:

```json
{
  "id": "https://example.com",
  "metadata": {
    "type": "uri",
    "included": false
  }
}
```

## Utilities

| Function                        | Purpose                                                       |
| ------------------------------- | ------------------------------------------------------------- |
| `is_uri(target)`                | Check if target is a URI (WHATWG URL parsing + scheme filter) |
| `graph.included_nodes()`        | Iterate over nodes where `included` is true                   |
| `graph.is_internal_edge(&edge)` | Check if the edge's target node is `included`                 |

## Lockfile

`drft.lock` is a deterministic TOML snapshot of the graph's node set and content hashes. Nodes are hashed via BLAKE3 (raw bytes). It enables:

- **Staleness detection** — compare current hashes to locked hashes.
- **Change propagation** — BFS from changed nodes through reverse edges to find transitively stale dependents.
- **Structural drift detection** — node additions and removals since last lock.

The lockfile omits edges. If a file's links change, its content hash changes. Nodes with `hash = None` (symlinks whose canonical target is outside `include`) are stored but skipped during staleness comparison.

### Staleness propagation

Staleness is conservative. When A → B → C and C changes, drft flags both B and A as stale ("stale via C" and "stale via B" respectively). A might not actually need updating — it depends on B, and B's content could still be accurate. drft can't know this; it flags the whole reverse-reachable set.

"Stale via X" means "X changed and you depend on it — review whether your content still holds." It's a review prompt, not an error report.
