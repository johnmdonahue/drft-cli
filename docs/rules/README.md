# Rules

Rules are predicates over the dependency graph that emit diagnostics. Each rule inspects the graph (or the output of an analysis) and reports violations as warnings or errors.

Rules are configured in `drft.toml` under `[rules]`. Every rule has a severity: `"warn"`, `"error"`, or `"off"`. Rules set to `"off"` are skipped entirely.

```toml
[rules]
broken-link = "warn"
orphan = "error"
fragility = "off"
```

## Built-in rules

| Rule | Default | What it checks | Analysis |
|------|---------|---------------|----------|
| [broken-link](broken-link.md) | warn | Links to files that don't exist | graph (direct) |
| [containment](containment.md) | warn | Links that escape the graph boundary | [graph-boundaries](../analyses/graph-boundaries.md) |
| [cycle](cycle.md) | warn | Circular dependencies between files | [scc](../analyses/scc.md) |
| [directory-link](directory-link.md) | warn | Links that point to a directory instead of a file | graph (direct) |
| [encapsulation](encapsulation.md) | warn | Links into a child graph that bypass its interface | [graph-boundaries](../analyses/graph-boundaries.md) |
| [fragility](fragility.md) | off | Cut vertices and bridge edges (structural single points of failure) | [bridges](../analyses/bridges.md) |
| [fragmentation](fragmentation.md) | off | Disconnected components in the graph | [connected-components](../analyses/connected-components.md) |
| [indirect-link](indirect-link.md) | off | Links whose target is a symlink | graph (direct) |
| [layer-violation](layer-violation.md) | off | Upward or skip-layer links in the depth hierarchy | [depth](../analyses/depth.md) |
| [orphan](orphan.md) | off | Files with no inbound links | [degree](../analyses/degree.md) |
| [redundant-edge](redundant-edge.md) | off | Direct links that are transitively redundant | [transitive-reduction](../analyses/transitive-reduction.md) |
| [stale](stale.md) | warn | Files whose content has changed since the last lock | [change-propagation](../analyses/change-propagation.md) |

## Script rules

You can define custom rules that run an external script. See [script](script.md).

## Per-rule configuration

Any rule can be configured with an ignore list to suppress diagnostics for specific files:

```toml
[rules.orphan]
severity = "warn"
ignore = ["README.md", "CHANGELOG.md"]
```
