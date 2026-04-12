# Examples

Sample projects for manual testing and learning drft. Each is its own graph with a `drft.toml`.

| Example                                       | What it demonstrates                   |
| --------------------------------------------- | -------------------------------------- |
| [simple](simple/README.md)                    | Clean project with no violations       |
| [broken](broken/README.md)                    | Broken links and orphan files          |
| [cyclic](cyclic/README.md)                    | Circular dependencies                  |
| [with-assets](with-assets/README.md)          | Non-markdown resource references       |
| [with-config](with-config/README.md)          | Ignore patterns and rule configuration |
| [custom-rules](custom-rules/README.md)        | Custom rules via external commands     |
| [custom-parsers](custom-parsers/wikilinks.sh) | Custom parser for `[[wikilinks]]`      |

Run any example with `drft check -C examples/<name>`.
