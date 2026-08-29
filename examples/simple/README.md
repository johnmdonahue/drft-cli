---
sources:
  - ../../docs/config.md
---

# Simple

A project whose links all resolve. Good for verifying drft works.

```bash
drft check -C examples/simple
```

The examples ship no lockfile, so the first run reports `no-baseline` — there is
nothing recorded to compare the tree against — alongside `detached-node` for the
files nothing links to. Establish a baseline and the drift rules have something
to work with:

```bash
drft lock --all -C examples/simple
drft check -C examples/simple
```
