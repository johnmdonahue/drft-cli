---
purpose: use the installed binary's operational guide
sources:
  - ../src/guide.rs
  - ../src/cli.rs
  - ../src/policy.rs
  - ../src/main.rs
---

# Installed-binary guidance

Run `drft guide` to learn the edit workflow and command contracts of the binary
you are using. The guide reads compiled metadata only: it works without a
repository, ignores `-C`, and does not read configuration or lockfiles.

The workflow starts with `impact` before editing and `check` afterward. Read
every finding, review the affected content and outbound promises, then lock the
reviewed paths. An unchanged dependent can belong in that scope because its
promise was reviewed. A warning-only check can exit 0; the exit code does not
replace reading its findings.

## Structured consumers

Use `drft guide --format json` for the same contract as structured data. Text
and JSON contain the same properties, including empty lists and null values.
JSON output is never colorized.

`schema_version` identifies the guide document's schema. Stop on an unknown
schema version. `drft_version` identifies the emitting binary; it is informational
and does not replace the schema check.

Each command carries its role, read and write effects, operand semantics,
output shape, exit codes, and mutation boundaries. Its `syntax` comes from clap,
including argument IDs, flag spellings, defaults, allowed values, and scope
constraints. Argument IDs such as `paths` differ from displayed metavars such as
`PATHS`. Global controls appear once and are inherited by every command.

An empty `possible_values` list means clap does not enumerate a finite set.
Consult the control's help and operational semantics for custom parsers such as
the traversal depth. Examples are checked against the CLI parser.

The guide's [implementation](../src/guide.rs) joins clap metadata with typed
operational records. [Shared policies and result definitions](../src/policy.rs)
supply exit meanings and serialized field names to both dispatch and the guide.

## Scope and failures

Reader selectors can expand a directory or glob; lock operands name exact nodes.
An empty scope fails. Use a whole-graph lock only after whole-graph review or for
a deliberate whole-baseline operation. The guide describes these boundaries
alongside each command.

Clap usage failures write text to stderr, including when `--format json` was
requested. Dispatch failures use the requested error format. Read commands can
refuse an oversized result before writing stdout; see
[output budgets](reading.md#refusing-oversized-output). The guide itself has no
output-budget control and emits its complete contract.
