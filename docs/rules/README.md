# Rules

Rules are predicates over the dependency graph that emit diagnostics. Each rule inspects the graph (or the output of an analysis) and reports violations as warnings or errors.

Configure rules in `drft.toml` under `[rules]`. Every rule has a severity: `"warn"`, `"error"`, or `"off"`. drft skips rules set to `"off"`. All rules default to `warn` for immediate discoverability — override to `error` for CI enforcement or `off` to suppress. [`src/config.rs`](../../src/config.rs) defines the defaults.

```toml
[rules]
stale = "error" # escalate for CI
fragility = "off" # suppress if tree-shaped graphs are expected
```

## Built-in rules

| Rule                                                  | What it checks                                                      | Analysis                                                    |
| ----------------------------------------------------- | ------------------------------------------------------------------- | ----------------------------------------------------------- |
| [boundary-violation](boundary-violation.md)           | Edges that escape the graph boundary                                | [graph-boundaries](../analyses/graph-boundaries.md)         |
| [dangling-edge](dangling-edge.md)                     | Edges to nodes that don't exist                                     | graph (direct)                                              |
| [directed-cycle](directed-cycle.md)                   | Circular dependencies between files                                 | [scc](../analyses/scc.md)                                   |
| [encapsulation-violation](encapsulation-violation.md) | Edges into a child graph that bypass its interface                  | [graph-boundaries](../analyses/graph-boundaries.md)         |
| [fragility](fragility.md)                             | Cut vertices and bridge edges (structural single points of failure) | [bridges](../analyses/bridges.md)                           |
| [fragmentation](fragmentation.md)                     | Disconnected components in the graph                                | [connected-components](../analyses/connected-components.md) |
| [layer-violation](layer-violation.md)                 | Upward or skip-layer links in the depth hierarchy                   | [depth](../analyses/depth.md)                               |
| [orphan-node](orphan-node.md)                         | Nodes with no inbound edges                                         | [degree](../analyses/degree.md)                             |
| [redundant-edge](redundant-edge.md)                   | Direct links that are transitively redundant                        | [transitive-reduction](../analyses/transitive-reduction.md) |
| [schema-violation](schema-violation.md)               | Node metadata violates required fields or allowed values            | graph (metadata)                                            |
| [stale](stale.md)                                     | Files whose content has changed since the last lock                 | [change-propagation](../analyses/change-propagation.md)     |
| [symlink-edge](symlink-edge.md)                       | Edges whose target is a symlink                                     | graph (direct)                                              |
| [untrackable-target](untrackable-target.md)           | Directory target has no `drft.toml`                                 | graph (direct)                                              |

## Custom rules

You can define custom rules that run an external command. See [custom](custom.md).

## Per-rule configuration

Every rule supports `files` and `ignore` (glob patterns), applied at the runner level:

```toml
[rules.orphan-node]
severity = "warn"
ignore = ["README.md", "CHANGELOG.md"]

[rules.layer-violation]
severity = "warn"
files = ["docs/**"] # only evaluate this rule against docs
```

- `files`: scope which nodes the rule evaluates (default: all)
- `ignore`: exclude nodes from diagnostics (default: none)
