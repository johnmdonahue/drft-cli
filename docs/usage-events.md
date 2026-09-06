# Usage event envelopes (inactive)

The [event producer](../src/usage/event.rs) can construct revision-1 `drft-usage` start and finish JSON records in
memory. Commands do not call this producer. Configuration, persistence, inspection,
export, and lifecycle integration remain unimplemented. This document describes the
producer contract; it does not establish collection availability, command parity,
filesystem safety, or measured overhead.

The inactive [storage handle layer](../src/usage/store.rs) opens or initializes
infrastructure and acquires its stable lock. Its scope is described in
[storage infrastructure](usage-storage.md).

The literal [start fixture](../tests/fixtures/usage/start-v1.json) and
[finish fixture](../tests/fixtures/usage/finish-v1.json) fix field names and example
values. JSON object order and whitespace are not significant. Unsupported revisions
must be preserved as original records and rejected for analysis.

## Identity and time

Both envelopes carry `schema`, integer `revision`, `event` (`start` or `finish`),
and a 32-character hexadecimal invocation `id` representing 128 OS-random bits.
Random failure has no fallback. Future storage must still refuse replacement when
an ID collides.

Start carries the binary version, original cwd, effective directory, canonical
graph root, command, requested format, exact argv, and a BLAKE3 fingerprint of the
bytes supplied from normal config parsing. The producer neither reads config nor
canonicalizes paths. The caller must supply the already-parsed bytes and resolved
identities. Command is restricted to `check`, `graph`, `nodes`, `edges`, `impact`,
or `lock`; requested format is `text` or `json`.

Every OS string carries `os_encoding` (`unix-bytes` or `windows-utf16le`),
`encoding` (`utf8` or `base64`), and `value`. Exact UTF-8 is preferred; otherwise
base64 preserves native units, including unpaired Windows surrogates. No path
normalization occurs. The optional `caller` contains only the supplied
`DRFT_USAGE_CALLER_ID` value. `authenticated: false` and
`unique_to_invocation: false` prohibit treating that label as trusted or unique.
An oversized caller label fails required-metadata admission just like argv.

Availability is explicit: `{"status":"available","value":...}` differs from
`{"status":"unavailable","value":"not_observed"}`. Unavailable values carry
a constrained reason: `not_evaluated`, `execution_stopped`, `not_observed`,
`requires_start`, `publication_not_yet_performed`, or `traversal_stopped`.
An available zero is evidence distinct from an unavailable count.

Each envelope has its own `collected_wall_time` and the invocation's
`entry_wall_time`. Wall timestamps use signed integer Unix seconds and nanoseconds
in `0..1000000000`. Finish `elapsed` uses unsigned seconds and nanoseconds from a
monotonic clock. The caller must measure it from process entry through finalization,
including start-publication overhead. Clock rollback may put finish wall time
before start wall time without changing elapsed time.

`collector_work_through_preparation` covers completed collector work through
finish preparation. `finish_publication_duration` is always unavailable because
an immutable finish cannot include its own subsequent write duration. These are
supplied observations; the producer does not read clocks. External process
measurements are needed for full latency.

## Result and output evidence

Finish carries intended exit (`clean` = 0, `violations` = 1, `usage_error` = 2),
completion (`returned`, `command_error`, `output_budget_refused`, or
`stdout_write_failed`), and actual output mode (`text`, `json`, `bare_jgf`,
`raw_graph_set`, or `no_document`). Bare composed JGF and the raw graph set are
different JSON representations. A broken stdout write may accompany intended
exit `clean` when that is existing command behavior. These fields report the
caller's observations; they do not themselves enforce the command lifecycle or
validate consistency between independently supplied observations.

`graph_sizes` contains available graph/node/edge counts. `result_sizes` separately
reports counts for the returned result. Neither causes additional traversal or
rule evaluation. Finding coverage distinguishes `full_policy_filtered_evaluation`,
`construction_diagnostics`, and `selected_impact_diagnostics`. Availability,
coverage, and payload omission are separate: complete storage of construction
findings does not establish a full rule evaluation.

Finding and hint records keep their existing typed fields and input order.
Prefixes expose known `total`, `included`, and `omitted` counts. Error records
are complete `Display` strings in source-chain order. Chain totals stay unavailable
when formatting, payload limits, or the 64-record traversal limit stop collection;
`traversal` explains the stop. A complete traversal reports its exact total.
Formatting stops on the first refused string write and latches overflow even if a
formatter ignores its error. A formatter returning an error produces
`formatter_failed`. Custom error implementations can still allocate, compute,
panic, or ignore repeated write failures internally; this producer bounds its own
storage and cooperative traversal, not arbitrary user-defined code execution.

`hint_observation` keeps embedding, selected route, suppression, and the attempt
at the hint's own write site separate. A hint embedded in a refused result has no
stdout write attempt. Aggregate stderr activity cannot establish a hint attempt.
Hint-specific writer acceptance, OS acceptance, and downstream consumption remain
`unknown` in this producer; aggregate stream outcomes are separate evidence.
`Hints::delivered()` supplies routing state, not delivery proof.

`budget_refusal` stores rendered bytes and the requested byte budget independently
of stdout captures. Rejected output is never invented as a stdout write input.

Stream snapshots can only enter the producer through `StreamCapture`. They report
base64 write-input prefixes, observed input bytes, retained bytes, truncation,
write outcome, and `writer_accepted_bytes`. A successful `write_all` establishes
only writer acceptance; a buffering writer may not have sent anything to the OS.
Failed or unfinished writes leave acceptance unknown. `os_accepted_bytes` and
`downstream_consumption` are always `unknown`. Trimming bytes preserves all these
observations. Byte-count overflow is explicit and sticky.

## Bounds and interpretation

Required start metadata is serialized into a capped 256 KiB buffer. OS values are
encoded one at a time; argv is borrowed and stops at the first serialization error.
No complete encoded argv vector or cloned result collection is constructed.
Failure returns no partial event and never calls truncated identity exact.

The shared structured limit is 128 KiB, including all findings, hints, error
records, wrappers, and omission metadata. Each whole event is capped at 256 KiB,
including metadata and base64 expansion. Optional admission order is findings,
hints, error-chain records, stdout prefix, stderr prefix. The first oversized
record stops its category; later categories may still fit. The producer reserves
1024 bytes for counter-width and state changes. This deterministic conservative
policy does not promise maximal packing. Borrowed records are individually
serialized into remaining capacity before bounded JSON storage is allocated.

Stream capture retains at most 64 KiB stdout and 16 KiB stderr raw input. Event
admission may reduce those prefixes further, preserving raw byte boundaries rather
than UTF-8 character boundaries. Finish has fixed, small metadata rather than
repeating variable start identity, so today's full structured and stream limits
normally fit together. Tests use a smaller internal event limit to exercise
aggregate stream trimming; that limit is not a public configuration option.

Finish sets `comparison_requires_start: true`. An orphan finish lacks full
project/config/binary provenance and cannot establish finding-comparison
compatibility. Even paired records support observed deltas only for complete,
nonoverlapping findings with compatible project, config, binary, schema, and
coverage. No record establishes that a fix happened or a review was valid.
Wall times give approximate order, not causality. A missing finish may follow a
lock mutation, panic, signal, or kill; it never proves that no mutation occurred.
