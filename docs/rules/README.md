---
sources:
  - ../../src/rules/mod.rs
  - ../../src/config.rs
---

# Rules

Rules are predicates over the dependency graph that emit diagnostics. Each rule inspects the graph (or the output of an analysis) and reports violations as warnings or errors.

Configure rules in `drft.toml` under `[rules]`. Every rule has a severity: `"warn"`, `"error"`, or `"off"`. drft skips rules set to `"off"`. All rules default to `warn` for immediate discoverability — override to `error` for CI enforcement or `off` to suppress. [`src/config.rs`](../../src/config.rs) defines the defaults.

```toml
[rules]
stale = "error" # escalate for CI
orphan-node = "off" # suppress if expected
```

## Built-in rules

| Rule                                        | What it checks                                           | Analysis                                                    |
| ------------------------------------------- | -------------------------------------------------------- | ----------------------------------------------------------- |
| [boundary-violation](boundary-violation.md) | Edges whose target resolves above the graph root         | graph (direct)                                              |
| [dangling-edge](dangling-edge.md)           | Edges to nodes that don't exist                          | graph (direct)                                              |
| [directed-cycle](directed-cycle.md)         | Circular dependencies between files                      | [scc](../analyses/scc.md)                                   |
| [fragmentation](fragmentation.md)           | Disconnected components in the graph                     | [connected-components](../analyses/connected-components.md) |
| [orphan-node](orphan-node.md)               | Nodes with no connections (no inbound or outbound edges) | [degree](../analyses/degree.md)                             |
| [schema-violation](schema-violation.md)     | Node metadata violates required fields or allowed values | graph (metadata)                                            |
| [stale](stale.md)                           | Files whose content has changed since the last lock      | [change-propagation](../analyses/change-propagation.md)     |
| [symlink-edge](symlink-edge.md)             | Edges whose target is a symlink                          | graph (direct)                                              |

## Custom rules

You can define custom rules that run an external command. See [custom](custom.md).

## Per-rule configuration

Every rule supports `files`, `ignore`, and `parsers`, applied at the runner level:

```toml
[rules.orphan-node]
severity = "warn"
ignore = ["CHANGELOG.md"]

[rules.schema-violation]
severity = "warn"
files = ["docs/**"] # only evaluate this rule against docs

[rules.directed-cycle]
parsers = ["frontmatter"] # only detect cycles through frontmatter edges
```

- `files`: scope which nodes the rule evaluates (default: all)
- `ignore`: exclude nodes from diagnostics (default: none)
- `parsers`: scope which parser edges the rule evaluates (default: all). When set, the rule runs against a filtered graph containing only edges from the named parsers. This lets you distinguish structural dependencies from navigation links.
