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
| **Graph builder** | Normalize targets, classify nodes, resolve paths, enrich | No judgment — that's rules                        |
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

`is_uri()` checks if a target has a URI scheme per [RFC 3986](https://www.rfc-editor.org/rfc/rfc3986): `[a-zA-Z][a-zA-Z0-9+.-]*:`. Any valid scheme is recognized without maintaining an explicit list.

URI targets skip path resolution (they're not relative file paths) and become External nodes.

### 3. Resolve paths

The graph builder resolves non-URI targets relative to the source file:

```
source: guides/intro.md
link:   ../setup.md#heading
target: setup.md             (path resolved, fragment stripped)
edge.link: setup.md#heading  (resolved path with fragment)
```

Uses standard path joining with `..` / `.` normalization.

### 4. Classify nodes

Edge targets that aren't already in the graph get classified:

| Condition                                   | Node type                                        | `included` | Tracked?                                          |
| ------------------------------------------- | ------------------------------------------------ | ---------- | ------------------------------------------------- |
| Target matches `include` patterns           | **File**                                         | `true`     | Yes — hashed, parsed                              |
| Target is a URI                             | **External**                                     | `false`    | No                                                |
| Target exists on disk but outside `include` | **File**                                         | `false`    | Yes — hashed                                      |
| Target is inside a child graph              | **File**                                         | `false`    | Yes — hashed                                      |
| Target escapes to parent (`../`)            | **File**                                         | `false`    | No — node created, not hashed (outside root)      |
| Target is a directory                       | **Directory** (`is_graph` if `drft.toml` exists) | `false`    | When `drft.toml` exists — hashed from `drft.toml` |
| Target doesn't exist on disk                | No node created                                  | —          | dangling-edge candidate                           |

### 5. Directory traversal prevention

The graph builder only reads and hashes files that canonicalize to within the graph root. Every filesystem access — during discovery and edge resolution — passes through `is_within_root()`, which resolves symlinks via `canonicalize()` and verifies the result starts with the canonical root path.

This prevents content access via:

- **`../` chains** — `../../etc/passwd` resolves outside the root
- **Symlinks** — a symlink inside the root pointing to `/etc/passwd` canonicalizes outside
- **Absolute paths** — `/etc/hosts` from a markdown link resolves outside the root

Nodes are still created for these targets so rules can flag the references (boundary-violation, dangling-edge). Only content reading and hashing is gated.

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

Edges carry the relationship and provenance:

| Field    | Type             | Description                                                        |
| -------- | ---------------- | ------------------------------------------------------------------ |
| `source` | String           | Source file path                                                   |
| `target` | String           | Node ID — always matches a node key (or is a dangling target)      |
| `link`   | Option\<String\> | Original link when it differs from target (e.g., `bar.md#heading`) |
| `parser` | String           | Which parser discovered this edge (provenance)                     |

`target` is always the node ID. `link` is present only when the original reference included a fragment. No transformation needed for consumers.

Whether an edge is **internal** (both endpoints are `included` nodes) is derived from node state, not stored on the edge. Use `graph.is_internal_edge(&edge)` to check.

## JSON output

The JSON graph output follows the [JGF v2.0](https://jsongraphformat.info/) schema. Parser provenance and computed properties live in edge `metadata`:

```json
{
  "source": "index.md",
  "target": "bar.md",
  "metadata": { "parser": "markdown", "internal": true }
}
```

Node metadata includes `type`, `hash`, and `included`:

```json
{
  "metadata": { "type": "file", "hash": "b3:...", "included": true }
}
```

## Utilities

| Function                        | Purpose                                                   |
| ------------------------------- | --------------------------------------------------------- |
| `is_uri(target)`                | Check if target is a URI (RFC 3986 scheme detection)      |
| `graph.target_props(target)`    | Get filesystem properties for a target                    |
| `graph.is_file_node(path)`      | Check if a path is a File node (capability check)         |
| `graph.is_included_node(path)`  | Check if a node was matched by `include` (scope check)    |
| `graph.is_internal_edge(&edge)` | Check if both endpoints are included (derived from nodes) |

## Lockfile

`drft.lock` is a deterministic TOML snapshot of the graph's node set and content hashes. All File nodes are hashed via BLAKE3 (raw bytes). It enables:

- **Staleness detection** — compare current hashes to locked hashes.
- **Change propagation** — BFS from changed nodes through reverse edges to find transitively stale dependents.
- **Structural drift detection** — node additions and removals since last lock.

The lockfile omits edges. If a file's links change, its content hash changes. Directory nodes with `drft.toml` are hashed from the child's `drft.toml` content — a config change in a child graph triggers staleness in the parent. The parent does not depend on the child's lockfile.

### Staleness propagation

Staleness is conservative. When A → B → C and C changes, drft flags both B and A as stale ("stale via C" and "stale via B" respectively). A might not actually need updating — it depends on B, and B's content could still be accurate. drft can't know this; it flags the whole reverse-reachable set.

"Stale via X" means "X changed and you depend on it — review whether your content still holds." It's a review prompt, not an error report.
