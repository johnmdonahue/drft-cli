# Change propagation

## The concept

**Change propagation** compares the current state of files against the lockfile snapshot and determines which nodes have changed (directly) and which are stale as a consequence (transitively). It also detects scope boundary changes (new or removed child scopes).

## Why it matters for knowledge systems

When a file changes, its dependents may need review:

- **Direct changes** — the file's content hash differs from the lockfile. This is the root cause of staleness.
- **Transitive staleness** — a file that links to a changed file is stale by propagation. It may reference information that is now outdated.
- **Boundary changes** — child scopes appearing or disappearing changes the structure of the graph.

## What drft surfaces

### As an analysis (`drft report`)

```bash
drft report --analysis change-propagation
```

```
=== change-propagation ===
setup.md: content changed
index.md: stale via setup.md
```

JSON output:

```json
{
  "analyses": {
    "change-propagation": {
      "has_lockfile": true,
      "directly_changed": [
        { "node": "setup.md", "reason": "content changed" }
      ],
      "transitively_stale": [
        { "node": "index.md", "via": "setup.md" }
      ],
      "boundary_changes": []
    }
  }
}
```

### As a rule (`drft check`)

The `stale` rule consumes this analysis:

```
warn[stale]: setup.md (content changed)
warn[stale]: index.md (stale via setup.md)
```

## Algorithm

1. Read the lockfile; if absent, report empty results.
2. For each locked node, compute the current BLAKE3 hash and compare to the stored hash. Mismatches and deletions are direct changes.
3. Check child scope boundaries: compare current `find_child_scopes()` against locked Frontier nodes.
4. BFS propagation: from each directly changed node, follow reverse edges in the lockfile to mark dependents as transitively stale.
