---
sources:
  - ../../docs/rules/custom.md
---

# Custom Rules

Demonstrates custom rules that receive the dependency graph as JSON and emit diagnostics. Includes checks for max fan-out, kebab-case naming, required frontmatter, and max depth.

```bash
drft check -C examples/custom-rules
```
