# Analyses

drft treats a directory of files as a dependency graph — files are nodes, links are edges. **Analyses** compute structural properties of this graph. They describe shape, not correctness.

Rules consume analyses to produce diagnostics (pass/fail judgments). The `drft report` command exposes analyses directly, without judgment, so you can understand your graph's structure before deciding what to enforce.

## Available analyses

| Analysis                                        | Command                            | Description                                 |
| ----------------------------------------------- | ---------------------------------- | ------------------------------------------- |
| [Betweenness centrality](betweenness.md)        | `drft report betweenness`          | Bridge document identification              |
| [Bridges / articulation points](bridges.md)     | `drft report bridges`              | Structural single points of failure         |
| [Change propagation](change-propagation.md)     | `drft report change-propagation`   | Direct changes and transitive staleness     |
| [Connected components](connected-components.md) | `drft report connected-components` | Finds disconnected clusters                 |
| [Degree distribution](degree.md)                | `drft report degree`               | In-degree and out-degree per node           |
| [Topological depth](depth.md)                   | `drft report depth`                | Layer assignment from roots                 |
| [Graph stats](graph-stats.md)                   | `drft report graph-stats`          | Density, diameter, average path length      |
| [Impact radius](impact-radius.md)               | `drft report impact-radius`        | Blast zone size per node                    |
| [PageRank](pagerank.md)                         | `drft report pagerank`             | Structural importance ranking               |
| [Graph boundaries](graph-boundaries.md)         | `drft report graph-boundaries`     | Graph escapes and encapsulation violations  |
| [Strongly connected components](scc.md)         | `drft report scc`                  | Finds groups of mutually reachable nodes    |
| [Transitive reduction](transitive-reduction.md) | `drft report transitive-reduction` | Finds edges that are structurally redundant |

## Graph theory and knowledge systems

When you organize files and link them together, you are building a graph — whether or not you think of it that way. Graph theory provides a vocabulary for reasoning about the structural properties that emerge. drft uses this vocabulary directly (e.g., "transitive reduction" rather than "redundant link detection") because the concepts are precise, well-studied, and transferable. Each analysis doc explains the underlying graph theory and how it applies to file-based knowledge systems.

## Analyses vs. rules

An **analysis** computes a property: "this edge is transitively redundant." A **rule** applies a threshold: "transitively redundant edges are a warning." This separation means:

- You can explore your graph's structure without being told what's wrong
- Rules stay thin — a judgment layer over shared data
- Multiple rules can consume the same analysis
- New rules can compose existing analyses (e.g., "warn on stale files that are also dominators")
