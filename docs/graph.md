# Graph builder

The graph builder sits between parsers and rules. It takes raw parser output and produces the enriched graph that everything else consumes.

## Responsibility boundaries

| Layer | Responsibility | Does NOT do |
|-------|---------------|-------------|
| **Parsers** | Emit raw link strings as they appear in source | No normalization, no classification, no filtering |
| **Graph builder** | Normalize targets, classify nodes, resolve paths, enrich | No judgment — that's rules |
| **Rules** | Judge the enriched graph, emit diagnostics | No filesystem access, no re-computation |

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

| Raw link | `edge.target` (node ID) | `edge.link` | Action |
|----------|------------------------|-------------|--------|
| `setup.md#heading` | `setup.md` | `Some("setup.md#heading")` | Fragment stripped, original preserved |
| `https://example.com#section` | `https://example.com` | `Some("https://example.com#section")` | Same for URIs |
| `mailto:user@example.com` | `mailto:user@example.com` | `None` | No fragment — target is complete |
| `setup.md` | `setup.md` | `None` | No fragment — target is complete |
| `#heading` | — | — | **Dropped** — no file target to resolve |
| *(empty)* | — | — | **Dropped** |

Only two things are dropped: empty targets and anchor-only targets (no file to resolve). Everything else enters the graph.

`edge.target` is always the node ID — you can join on it directly without any transformation.

### 2. Detect URIs

`is_uri()` checks if a target has a URI scheme per [RFC 3986](https://www.rfc-editor.org/rfc/rfc3986): `[a-zA-Z][a-zA-Z0-9+.-]*:`. This covers all schemes — http, https, mailto, ftp, tel, ssh, custom schemes — without maintaining an explicit list.

URI targets skip path resolution (they're not relative file paths) and become External nodes.

### 3. Resolve paths

Non-URI targets are resolved relative to the source file:

```
source: guides/intro.md
link:   ../setup.md#heading
target: setup.md             (path resolved, fragment stripped)
edge.link: setup.md#heading  (resolved path with fragment)
```

Uses standard path joining with `..` / `.` normalization.

### 4. Classify nodes

Edge targets that aren't already in the graph get classified:

| Condition | Node type | Tracked? |
|-----------|-----------|----------|
| Target matches `include` patterns | **File** | Yes — hashed, parsed |
| Target is a URI | **External** | No |
| Target exists on disk but outside `include` | **External** | No |
| Target is inside a child graph | **External** (with `graph` field) | No |
| Target doesn't exist on disk | No node created | dangling-edge candidate |
| Target is a directory | No node created | directory-edge candidate |

### 5. Probe filesystem properties

For non-URI edge targets, the graph builder probes the filesystem and stores results per-target in `graph.target_properties`:

| Property | Type | Description |
|----------|------|-------------|
| `is_symlink` | bool | Target path is a symlink |
| `is_directory` | bool | Target path is a directory |
| `symlink_target` | Option\<String\> | Resolved symlink destination |

Rules access these via `graph.target_props(&edge.target)`. Properties are stored once per target, not duplicated across edges.

### 6. Enrich

After building, `enrich()` computes structural analyses unconditionally: degree, SCC, connected components, depth, bridges, transitive reduction, betweenness, pagerank, graph boundaries, change propagation. Rules receive the enriched graph — all properties pre-computed.

## Edge structure

Edges are minimal — just the relationship and provenance:

| Field | Type | Description |
|-------|------|-------------|
| `source` | String | Source file path |
| `target` | String | Node ID — always matches a node key (or is a dangling target) |
| `link` | Option\<String\> | Original link when it differs from target (e.g., `bar.md#heading`) |
| `parser` | String | Which parser discovered this edge (provenance) |

`target` is always the node ID. `link` is present only when the original reference included a fragment. No transformation needed for consumers.

## JSON output

```json
{"source": "index.md", "target": "bar.md", "parser": "markdown"}
{"source": "index.md", "target": "bar.md", "link": "bar.md#heading", "parser": "markdown"}
```

`link` is omitted when it would be identical to `target`.

## Utilities

| Function | Purpose |
|----------|---------|
| `is_uri(target)` | Check if target is a URI (RFC 3986 scheme detection) |
| `graph.target_props(target)` | Get filesystem properties for a target |
| `graph.is_file_node(path)` | Check if a path is a File node |

## Source

[`src/graph.rs`](../src/graph.rs)
