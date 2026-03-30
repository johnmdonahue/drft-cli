# rules

Rules evaluate the graph and emit diagnostics. Each implements the `Rule` trait.

- [broken_link.rs](broken_link.rs) — detects links to missing files
- [containment.rs](containment.rs) — detects links escaping the scope root
- [cycle.rs](cycle.rs) — detects dependency cycles
- [directory_link.rs](directory_link.rs) — detects links to directories instead of files
- [encapsulation.rs](encapsulation.rs) — enforces graph boundary interfaces
- [fragility.rs](fragility.rs) — detects structural single points of failure
- [fragmentation.rs](fragmentation.rs) — detects disconnected components
- [indirect_link.rs](indirect_link.rs) — detects links through symlinks
- [layer_violation.rs](layer_violation.rs) — enforces layer ordering
- [mod.rs](mod.rs) — `Rule` trait, `RuleContext`, `all_rules()` registry
- [orphan.rs](orphan.rs) — detects files with no inbound links
- [redundant_edge.rs](redundant_edge.rs) — detects transitively redundant edges
- [script.rs](script.rs) — script-based rule runner
- [stale.rs](stale.rs) — detects content changes since last lock
