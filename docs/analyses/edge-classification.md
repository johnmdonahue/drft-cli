# Edge classification

## The concept

**Edge classification** examines every link in the graph and determines the status of its target: valid file, broken link, excluded by ignore rules, directory, symlink, or external URL. This gives a complete picture of link health.

## Why it matters for knowledge systems

Understanding the health of every link in your documentation:

- **Broken links** (target doesn't exist) indicate missing files or stale references.
- **Excluded links** (target exists but is ignored) suggest a config mismatch — either the link or the ignore rule should be updated.
- **Directory targets** link to folders rather than specific files, which is usually unintentional.
- **Symlink targets** create indirect dependencies that can break when the symlink target moves.
- **External URLs** are tracked but not validated for reachability.

## What drft surfaces

### As an analysis (`drft report`)

```bash
drft report --analysis edge-classification
```

```
=== edge-classification ===
15 edges
  valid: 12
  broken: 1
  external: 2
```

JSON output includes per-edge classification:

```json
{
  "analyses": {
    "edge-classification": {
      "edges": [
        { "source": "index.md", "target": "setup.md", "edge_type": "inline", "status": "valid" },
        { "source": "index.md", "target": "gone.md", "edge_type": "inline", "status": "broken" }
      ]
    }
  }
}
```

### As rules (`drft check`)

Three rules consume this analysis:
- **`broken-link`** — flags `broken` and `excluded` edges
- **`directory-link`** — flags `directory_target` edges
- **`indirect-link`** — flags `symlink_target` edges

## Algorithm

Single pass over all edges. For each edge, checks: URL prefix (external), graph membership (known node), then filesystem properties (exists, is_dir, is_symlink). Edges to Frontier nodes (scope boundaries) are classified as valid.
