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

| Layer             | Responsibility                                           | Does NOT do                                       |
| ----------------- | -------------------------------------------------------- | ------------------------------------------------- |
| **Parsers**       | Emit raw link strings as they appear in source           | No normalization, no classification, no filtering |
| **Graph builder** | Normalize targets, classify edges, resolve paths, enrich | No judgment — that's rules                        |
| **Rules**         | Judge the enriched graph, emit diagnostics               | No filesystem access, no re-computation           |

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

URI targets skip path resolution (they're not relative file paths) and are classified as `External(Remote)` on the edge.

### 3. Resolve paths

The graph builder resolves non-URI targets relative to the source file:

```
source: guides/intro.md
link:   ../setup.md#heading
target: setup.md             (path resolved, fragment stripped)
edge.link: setup.md#heading  (resolved path with fragment)
```

Uses standard path joining with `..` / `.` normalization.

### 4. Classify edge targets

Every edge target is classified into a `TargetKind`. This is a pure operation — string comparisons and hashmap lookups, no filesystem probing:

| Condition                                         | `TargetKind`        | Meaning                                        |
| ------------------------------------------------- | ------------------- | ---------------------------------------------- |
| Target is a URI (`is_uri()`)                      | `External(Remote)`  | Off-graph, remote resource                     |
| Target doesn't match any `include` pattern        | `External(Local)`   | Filesystem-shaped but outside drft's authority |
| Target matches `include` and is in `graph.nodes`  | `Internal(Found)`   | Resolved — both endpoints are in the graph     |
| Target matches `include` but not in `graph.nodes` | `Internal(Missing)` | Expected file is absent — broken link          |

`include` is drft's sole authority for what gets read from disk. Targets outside `include` — whether they exist on disk or not — are `External(Local)`. drft does not probe the filesystem to classify them.

### 5. Symlink policy

The walker follows symlinks so symlinked directories are traversable. The security boundary is enforced at hashing, not at walking.

For each entry matching `include`:

1. If not a symlink: read, hash, create node.
2. If a symlink: canonicalize the path. If the canonical form is under the graph root and matches `include`, read and hash. Otherwise, create the node with `hash = None` — content is intentionally not read.

This prevents content access through symlinks pointing outside the graph. `include` patterns don't traverse above the root (the walker is rooted at the `drft.toml` directory).

### 6. Probe filesystem properties

For non-URI edge targets, the graph builder probes the filesystem and stores results per-target in `graph.target_properties`:

| Property         | Type             | Description                  |
| ---------------- | ---------------- | ---------------------------- |
| `is_symlink`     | bool             | Target path is a symlink     |
| `is_directory`   | bool             | Target path is a directory   |
| `symlink_target` | Option\<String\> | Resolved symlink destination |

Rules access these via `graph.target_props(&edge.target)`. Properties are stored once per target, not duplicated across edges.

### 6. Enrich

After building, `enrich()` computes all [structural analyses](analyses/README.md) unconditionally. Rules receive the enriched graph — all properties pre-computed.

## Edge structure

Edges carry the relationship, classification, and provenance:

| Field         | Type             | Description                                                                          |
| ------------- | ---------------- | ------------------------------------------------------------------------------------ |
| `source`      | String           | Source file path                                                                     |
| `target`      | String           | Target path or URI                                                                   |
| `target_kind` | TargetKind       | Classification of the target (see [classify edge targets](#4-classify-edge-targets)) |
| `link`        | Option\<String\> | Original link when it differs from target (e.g., `bar.md#heading`)                   |
| `parser`      | String           | Which parser discovered this edge (provenance)                                       |

`target` is always the node ID for internal edges. `link` is present only when the original reference included a fragment. No transformation needed for consumers.

An edge is **internal** when `target_kind` is `Internal(Found)`. Use `graph.is_internal_edge(&edge)` to check.

## JSON output

The JSON graph output follows the [JGF v2.0](https://jsongraphformat.info/) schema. Parser provenance lives in edge `metadata`:

```json
{
  "source": "index.md",
  "target": "bar.md",
  "metadata": { "parser": "markdown" }
}
```

Node metadata includes `hash` (when present) and any parser-extracted metadata keyed by parser name:

```json
{
  "metadata": {
    "hash": "b3:...",
    "frontmatter": { "title": "Setup", "sources": ["../shared/glossary.md"] }
  }
}
```

Graph-level metadata includes `target_properties` (filesystem properties of edge targets):

```json
{
  "graph": {
    "directed": true,
    "metadata": {
      "target_properties": {
        "setup.md": { "is_symlink": false, "is_directory": false }
      }
    },
    "nodes": {},
    "edges": []
  }
}
```

## Utilities

| Function                        | Purpose                                                       |
| ------------------------------- | ------------------------------------------------------------- |
| `is_uri(target)`                | Check if target is a URI (WHATWG URL parsing + scheme filter) |
| `graph.target_props(target)`    | Get filesystem properties for a target                        |
| `graph.is_internal_edge(&edge)` | Check if the edge's `target_kind` is `Internal(Found)`        |

## Lockfile

`drft.lock` is a deterministic TOML snapshot of the graph's node set and content hashes. Nodes are hashed via BLAKE3 (raw bytes). It enables:

- **Staleness detection** — compare current hashes to locked hashes.
- **Change propagation** — BFS from changed nodes through reverse edges to find transitively stale dependents.
- **Structural drift detection** — node additions and removals since last lock.

The lockfile omits edges. If a file's links change, its content hash changes. Nodes with `hash = None` (symlinks whose canonical target is outside `include`) are stored but skipped during staleness comparison.

### Staleness propagation

Staleness is conservative. When A → B → C and C changes, drft flags both B and A as stale ("stale via C" and "stale via B" respectively). A might not actually need updating — it depends on B, and B's content could still be accurate. drft can't know this; it flags the whole reverse-reachable set.

"Stale via X" means "X changed and you depend on it — review whether your content still holds." It's a review prompt, not an error report.
