---
purpose: the markdown parser — link edges from body link syntax
sources:
  - ../../src/parsers/markdown.rs
---

# Markdown parser

## The concept

The markdown parser is drft's built-in parser for standard markdown link syntax. It scans markdown files and extracts the links that form edges in the dependency graph. For YAML frontmatter links and metadata, see the [frontmatter parser](frontmatter.md).

## Link types

Each link type becomes an edge with parser provenance `markdown` in the graph.
Every edge carries an `occurrences` array in its `markdown` metadata, one entry
per link the author wrote. Each entry records that link's own 1-based source
`line`, its fragment-qualified `link`, and its literal `raw` spelling, so a
target cited from several places keeps each line paired with what that line
actually said. `drft graph` exposes the array; `drft impact` reads the lines to
point a review at the exact reference.

### inline

Standard markdown links where the URL is inline with the text.

```markdown
[setup guide](setup.md)
[with fragment](setup.md#installation)
```

Fragments (`#heading`) are stripped from the target -- `setup.md#installation` produces an edge to `setup.md` -- and kept on the occurrence as `link`.

### reference

Reference-style links, including collapsed and shortcut forms.

```markdown
[setup guide][ref]

[ref]: setup.md
```

Collapsed (`[ref][]`) and shortcut (`[ref]`) forms are also detected as reference links.

### autolink

Autolinks using angle bracket syntax.

```markdown
<https://example.com>
```

Email autolinks (`<user@example.com>`) are skipped entirely.

### image

Image references.

```markdown
![architecture diagram](assets/arch.png)
```

Images follow the same resolution rules as inline links -- the graph builder strips fragments and skips anchor-only references.

## Anchors

The parser also records the `#fragment` addresses each file answers to, as
`anchors` in its `markdown` node metadata, in document order. Read them with
`drft nodes <path> --field anchors`.

Two things define an address, because a reader's platform resolves both:

- **A heading**, through the GitHub slug of its rendered text, with GitHub's `-1`
  suffix when a slug repeats. The suffix is re-checked rather than appended, so a
  document holding `a`, `a`, and a literal `a-1` yields `a`, `a-1`, `a-1-1`.
- **A raw `<a id="…">` or `<a name="…">`**, verbatim and case-sensitive. This is
  how a hand-rolled table of contents and a back-compat anchor kept after a
  heading rename stay addressable.

YAML frontmatter is masked before parsing, so a single-key block's closing `---`
is not read as a setext heading. The block has to parse as a YAML mapping to be
masked — the same block the [frontmatter parser](frontmatter.md) consumes — so a
document that merely opens with a `---` thematic break keeps its headings and its
links.

The slug downcases the heading's rendered text, drops every character that is not
a letter, digit, combining mark, underscore, hyphen, or space, then turns spaces
into hyphens. Punctuation is **removed rather than replaced**, so a character
between two spaces leaves both behind: `## Sizing — notes` answers to
`#sizing--notes`, with two hyphens. Rendered text excludes image alt text and
inline HTML, which are not part of an element's text content. A `{#custom}`
attribute is not honored, because GitHub ignores it too and an anchor resolving
only in drft would 404 for a reader.

A file the parser read defines an `anchors` list even when it has no headings, so
an empty list is a fact rather than a silence. Anchors are what
[`unresolved-fragment`](../rules/README.md) checks a link's fragment against.

## Configuration

Declare a graph that uses the markdown parser:

```toml
[graphs.markdown]
parser = "markdown"
files = ["**/*.md", "**/*.mdx"]
```

`files` scopes which files the parser reads (default `["**/*.md"]`). There are no
default graphs — the markdown parser runs only where you declare it. See
[configuration](../config.md) for the full graph schema.

## Directory targets

A link whose target is a directory (`[services](edge/)`) creates an edge to the directory node. Directories are nodes, so the link resolves — but they carry no hash, so nothing inside is tracked: editing `edge/src/main.rs` leaves the linking file clean, and `drft impact` on that file does not report it.

This matters most where it reads like coverage. A layout table citing `` `edge/` ``, `` `tenant/` ``, `` `console/api/` `` looks to a reader like an inventory of those trees and tracks none of them. Point each link at the file carrying what the prose claims — `edge/src/main.rs` — and the promise becomes one drft can check.

## External URLs

External links (`http://`, `https://`, `mailto:`, and other URI schemes) are emitted as raw edge targets. A URI target has no defining node, so the `unresolved-edge` rule treats it as an intentional external reference and does not flag it. Anchor-only links (`#heading`) are dropped, and fragments are stripped from targets (`file.md#section` → `file.md`) and kept on the occurrence — all handled when the edge is built, not by the parser.
