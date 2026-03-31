# Parsers

Parsers extract edges and metadata from files. Each parser receives File nodes (optionally filtered by `files` globs), parses their content, and emits links that become edges in the graph. Parsers can also emit structured metadata that is attached to nodes.

drft ships one built-in parser. Additional parsers can be added via scripts.

| Parser | Link types | Built-in? |
|--------|------------|-----------|
| [markdown](markdown.md) | inline, reference, autolink, image, frontmatter, wikilink | Yes |
| [script](script.md) | (defined by script) | No |
