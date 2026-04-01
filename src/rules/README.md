# rules

Rules evaluate the graph and emit diagnostics. Each implements the `Rule` trait.

- [boundary_violation.rs](boundary_violation.rs) — detects edges escaping the scope root
- [dangling_edge.rs](dangling_edge.rs) — detects edges to missing nodes
- [directed_cycle.rs](directed_cycle.rs) — detects dependency cycles
- [encapsulation_violation.rs](encapsulation_violation.rs) — enforces graph boundary interfaces
- [fragility.rs](fragility.rs) — detects structural single points of failure
- [fragmentation.rs](fragmentation.rs) — detects disconnected components
- [layer_violation.rs](layer_violation.rs) — enforces layer ordering
- [mod.rs](mod.rs) — `Rule` trait, `RuleContext`, `all_rules()` registry
- [orphan_node.rs](orphan_node.rs) — detects isolated nodes (no inbound or outbound edges)
- [redundant_edge.rs](redundant_edge.rs) — detects transitively redundant edges
- [schema_violation.rs](schema_violation.rs) — validates node metadata against schema options
- [custom.rs](custom.rs) — custom rule runner
- [stale.rs](stale.rs) — detects content changes since last lock
- [symlink_edge.rs](symlink_edge.rs) — detects edges through symlinks
- [untrackable_target.rs](untrackable_target.rs) — detects directory targets without `drft.toml`
