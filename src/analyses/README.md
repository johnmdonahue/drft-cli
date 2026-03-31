# analyses

Graph analyses compute structural properties. Each implements the `Analysis` trait.

- [betweenness.rs](betweenness.rs) — betweenness centrality
- [bridges.rs](bridges.rs) — bridges and articulation points
- [change_propagation.rs](change_propagation.rs) — direct changes and transitive staleness
- [connected_components.rs](connected_components.rs) — disconnected clusters
- [degree.rs](degree.rs) — in-degree and out-degree per node
- [depth.rs](depth.rs) — topological depth from roots
- [graph_boundaries.rs](graph_boundaries.rs) — graph escapes and encapsulation violations
- [graph_stats.rs](graph_stats.rs) — density, diameter, average path length
- [impact_radius.rs](impact_radius.rs) — blast zone size per node
- [mod.rs](mod.rs) — `Analysis` trait, `AnalysisContext`, name registry
- [pagerank.rs](pagerank.rs) — structural importance ranking
- [scc.rs](scc.rs) — strongly connected components
- [transitive_reduction.rs](transitive_reduction.rs) — redundant edge detection
