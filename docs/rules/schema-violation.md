# schema-violation

Validates that File nodes have the required metadata fields and that field values are within allowed sets. This rule only produces diagnostics when configured with options — without options, it does nothing.

**Default severity:** `warn`

## Source

[`src/rules/schema_violation.rs`](../../src/rules/schema_violation.rs)

## How it works

The rule reads node metadata (populated by parsers with `extract_metadata = true`) and checks it against the schema defined in rule options. Two levels of schema:

1. **Global** — `required` fields checked on every File node
2. **Per-glob** — `schemas.<glob>` with `required` and `allowed` checked on matching paths

## Configuration

```toml
[parsers.markdown.options]
extract_metadata = true       # enable frontmatter metadata extraction

[rules.schema-violation]
severity = "warn"

[rules.schema-violation.options]
required = ["title"]          # every File node must have "title"

[rules.schema-violation.options.schemas."observations/*.md"]
required = ["title", "date", "status"]
allowed.status = ["draft", "review", "final"]
```

### Options

| Key | Type | Description |
|-----|------|-------------|
| `required` | string[] | Fields required on all File nodes |
| `schemas.<glob>.required` | string[] | Fields required on nodes matching the glob |
| `schemas.<glob>.allowed.<field>` | string[] | Allowed values for a field on matching nodes |

## Diagnostics

| Message | Meaning |
|---------|---------|
| `missing required field "title"` | Node metadata lacks a required field |
| `field "status" has value "invalid", allowed: [draft, review, final]` | Field value is not in the allowed set |

## Requires

- Parser metadata: at least one parser must set `extract_metadata = true` in its options for nodes to have metadata to validate.
