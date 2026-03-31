# Parsers

Parsers extract edges and metadata from files. Each parser receives File nodes (optionally filtered by `files` globs), parses their content, and emits links that become edges in the graph. Parsers can also emit structured metadata that is attached to nodes.

drft ships two built-in parsers. Additional parsers can be added via scripts.

| Parser | Link types | Built-in? |
|--------|------------|-----------|
| [frontmatter](frontmatter.md) | link | Yes |
| [markdown](markdown.md) | inline, reference, autolink, image | Yes |
| [script](script.md) | (defined by script) | No |
