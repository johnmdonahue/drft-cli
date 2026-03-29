# Analyses

drft treats a directory of markdown files as a dependency graph — files are nodes, links are edges. **Analyses** compute structural properties of this graph. They describe shape, not correctness.

Rules consume analyses to produce diagnostics (pass/fail judgments). The `drft report` command exposes analyses directly, without judgment, so you can understand your graph's structure before deciding what to enforce.

## Available analyses

| Analysis | Command | Description |
|----------|---------|-------------|
| [Transitive reduction](transitive-reduction.md) | `drft report --analysis transitive-reduction` | Finds edges that are structurally redundant |

## Graph theory and knowledge systems

When you organize files and link them together, you are building a graph — whether or not you think of it that way. Graph theory provides a vocabulary for reasoning about the structural properties that emerge. drft uses this vocabulary directly (e.g., "transitive reduction" rather than "redundant link detection") because the concepts are precise, well-studied, and transferable. Each analysis doc explains the underlying graph theory and how it applies to file-based knowledge systems.

## Analyses vs. rules

An **analysis** computes a property: "this edge is transitively redundant." A **rule** applies a threshold: "transitively redundant edges are a warning." This separation means:

- You can explore your graph's structure without being told what's wrong
- Rules stay thin — a judgment layer over shared data
- Multiple rules can consume the same analysis
- New rules can compose existing analyses (e.g., "warn on stale files that are also dominators")
