# Parsers

Parsers extract links from files. Each parser matches files by glob, parses their content, and emits links that become edges in the graph.

drft ships one built-in parser. Additional parsers can be added via scripts.

| Parser | Default glob | Link types | Built-in? |
|--------|-------------|------------|-----------|
| [markdown](markdown.md) | `*.md` | inline, reference, autolink, image, frontmatter, wikilink | Yes |
| [script](script.md) | (configured) | (defined by script) | No |
