# Examples

Sample projects for manual testing and learning drft. Each is its own graph with a `drft.toml`.

| Example                              | What it demonstrates                   |
| ------------------------------------ | -------------------------------------- |
| [simple](simple/README.md)           | Clean project with no findings         |
| [broken](broken/README.md)           | Broken links and detached files        |
| [cyclic](cyclic/README.md)           | A dependency cycle (permitted)         |
| [with-assets](with-assets/README.md) | Non-markdown resource references       |
| [with-config](with-config/README.md) | Ignore patterns and rule configuration |

Run any example with `drft check -C examples/<name>`.
