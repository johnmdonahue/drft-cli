# Usage storage infrastructure (inactive)

The [storage module](../src/usage/store.rs) opens or initializes cache infrastructure
and acquires an exclusive lock on macOS and Linux. Its bounded inventory accounts
for safe regular files and reads individual entries. A record inventory classifies
envelopes and calculates grouped retention and start-write reservations. Commands do not call it.
Native start/finish publication applies retention under that lock. Configuration
and command lifecycle integration remain unimplemented. A successful guard alone
establishes infrastructure identity and synchronization, not record validity.

`Partition::open_existing` requires an absolute cache path outside the canonical
graph root. Existing cache ancestors resolve once; the cache entry itself must
be a directory. The partition name is the BLAKE3 digest produced by
[`partition_id`](../src/usage/identity.rs) from exact canonical graph-root OS
identity. The partition and its empty `.lock` file must already exist. Opening
never creates infrastructure, truncates a file, or removes an entry.

`Partition::open_or_create` can create missing infrastructure through the
[initializer](../src/usage/store/native/bootstrap.rs). It resolves the longest
existing cache ancestor once and checks graph placement before creating any
missing suffix. Parent traversal (`..`) is refused. New directories use mode
`0700`, subject to the process umask; the initializer never changes permissions
on existing entries. Race-created directories must pass the same no-follow,
ownership, and identity checks. Created directories remain after later failure.

Only the process that exclusively creates the digest partition may create its
initial `.lock`, with exclusive creation and mode `0600`, subject to umask.
Other initializers open the existing lock. A contender that arrives before lock
creation returns an error and can succeed on a later invocation. If the creator
stops before creating the lock, the partition remains unavailable. Recovery
requires deliberate external maintenance while collection is inactive; automatic
repair could replace an established lock still held by another process.
The creator retains its new lock descriptor and identity through the returned
handle, so final validation rejects a replacement after creation.
Bootstrap creates no record or staging file. Its empty lock contributes one file
and zero bytes to partition accounting.

The [native backend](../src/usage/store/native.rs) retains directory handles from
the filesystem root through the resolved graph and cache ancestry. It opens each
component without following symlinks, compares device/inode identities, and
revalidates retained relationships at lock acquisition and on `guard.validate()`.
Cache, partition, and lock entries must belong to the effective user and must
not be writable by group or others. The lock must be a singly linked, empty
regular file. System-owned and sticky ancestors are allowed; ACLs and arbitrary
concurrent directory relocation are outside these permission checks. Deployment
requires stable, owner-controlled local ancestry.

Every acquisition opens an independent lock descriptor and attempts nonblocking
exclusive `flock` once. Contention returns `Busy` without retry. The guard
exclusively borrows the partition handle; dropping it explicitly unlocks and
closes the descriptor. Explicit unlock also covers rejected acquisitions and
prevents an inherited descriptor from extending the guard's lock lifetime.
If unlock fails, descriptor closure remains the fallback. Detectable partition or lock replacement invalidates the old
handle. No operation repairs a missing established lock by creating a new inode.

The [native tests](../src/usage/store/native/tests.rs) cover persistent path
substitutions, symlinks, hardlinks, special files, permission changes, same-process
and child-process contention, and process-death release. Invalid-byte native path
fixtures run on Linux; macOS filesystems can reject their creation. Raw identity
codec fixtures cover those bytes independently. Passing these tests does not
qualify no-replace publication, retention, command parity, or collection overhead.
The [initialization tests](../src/usage/store/native/bootstrap/tests.rs) cover
creator/contender interleaving, interrupted initialization, lock removal while a
guard survives, creation collisions, and directory substitution before writes.
Other platforms return `Unsupported` before path access. Native platform coverage
must be recorded separately from source availability.

## Bounded physical inventory

`PartitionGuard::inventory` uses the [native scanner](../src/usage/store/native/scan.rs)
to enumerate the locked partition without writing or removing entries. It counts
every regular file, including the empty lock, malformed content, temporary files,
and arbitrary names. Symlinks, directories, special files, multiply linked files,
and entries failing the ownership/permission checks invalidate the scan.
Successful inventory does not establish recognized filenames, supported envelopes,
time coverage, or authority to prune. A transaction must validate those separately.

The scan refuses more than 10,000 files or more than 100 MiB of logical file
lengths, including sparse files. Externally oversized partitions return
`ScanLimit` without cleanup. It retains at most 10,000 names of at most 255 native
bytes each, plus stat metadata and fixed per-entry bookkeeping. It holds no
per-entry open descriptors or payload buffers. Results sort by native filename
bytes; names remain exact OS strings.

An inventory borrows its guard, keeping the advisory lock alive. `read` accepts
an entry index and a byte limit up to 256 KiB. If the measured file exceeds that
limit, it returns `Oversized` without reading a prefix. Otherwise it opens the
entry relative to the retained partition descriptor with no-follow and nonblocking
flags. It allocates at most the measured size and probes for growth with a stack
buffer. A short read or observed identity, size, ownership, mode, link-count, or
modification/change-time difference rejects the result. `validate` rechecks all
retained entries; scans and reads also recheck infrastructure and directory
mutation metadata. Each scan uses an independent directory stream.

These are observations under an advisory lock. A process ignoring that lock can
rewrite bytes in place; metadata checks do not promise detection of every transient
rewrite or timestamp collision. The inventory does not freeze an export snapshot.
The [scan fixtures](../src/usage/store/native/scan/tests.rs) exercise quota boundaries,
safe and unsafe entry types, bounded reads, and substitutions or changes at
deterministic scan/open/read checkpoints. They qualify neither event analysis
nor mutation transactions.

## Record classification and retention plans

`Inventory::records` uses the [record inventory](../src/usage/store/records.rs)
and independent [wire reader](../src/usage/record.rs) to classify the complete
physical inventory. It recognizes `.lock`, `<id>.start.json`, `<id>.finish.json`,
and `.tmp.<id>`, with exactly 32 lowercase hexadecimal characters in each ID.
Other names invalidate classification, including unrecognized control names.
Health records have no reserved name or schema; the collector does not write them. Recognized temporary contents
are abandoned staging bytes and are not parsed as envelopes.

Finals are grouped by filename ID. Supported revision-1 envelopes must match the
filename ID and event kind and satisfy the complete wire shape, required fields,
encoding rules, and producer-established count/capture invariants. Duplicate JSON
keys, unknown revision-1 fields, invalid timestamps, and malformed encodings are
rejected. Caller-supplied observations are not verified against command execution.
Matching headers with later positive integer revisions are unsupported for analysis.
Malformed, unsupported, and oversized finals stay accounted for in their groups;
their original files can be copied manually while retained.

Classification reads one capped payload at a time and retains only bounded group
metadata, inventory indices, sizes, and validated wall times. JSON parsing retains
the recursion limit and rejects duplicate keys at every depth. Its allocations are
bounded by the event byte limit, with JSON representation overhead; the event limit
is not a peak-memory measurement. Record paths never become filesystem authority.
The inventory and plans borrow the physical snapshot and its lock. Metadata is
revalidated after classification, before planning, and on `plan.validate()`.

Pairing and time coverage are separate. A filename pair has known coverage only
when both envelopes validate, all entry/collection times are available, entry times
agree, and entry ≤ start collection ≤ finish collection. Everything else has unknown
coverage, including incomplete starts, orphan finishes, and clock rollback. This
classification neither compares findings nor proves shared provenance.

The planner expires a known group when its latest endpoint is at least seven days
older than the supplied current time, including the exact nanosecond boundary.
An unavailable current time disables age expiration. Unknown-coverage groups
survive age expiration but remain eligible for quota eviction. Quota eviction takes
known groups first by latest endpoint, then unknown groups by ID; equal known times
also use ID order. ID order expresses an eviction policy, never inferred chronology.
Each selected group loses all its retained members together in the plan.

Plans remove recognized abandoned temporaries first. Start reservations reject a
collision with either existing group member before proposing any cleanup, including
an expired or rejected record. They reserve one staging file and the complete event
length within the partition's byte/file limits; its later rename needs no second
slot. `PruneAll` plans record and temporary removal while retaining the stable
empty lock. Externally excessive accounting fails without a cleanup plan.

Plans return indices into their retained physical inventory and predicted retained
and peak occupancy. They perform no writes or removals. A native transaction must
recheck identities and track each successful mutation because its own deletions
invalidate the original snapshot. Finish reservations protect their matching start from eviction and reserve the
finish staging file. A calculation alone grants no publication authority; native
finish publication requires the successful-start receipt described below.

The [reader fixtures](../src/usage/record/tests.rs) and
[planner fixtures](../src/usage/store/records/tests.rs) cover hostile wire data,
recognized rejected finals, complete-scan refusal, pairing, uncertain coverage,
expiry boundaries, collision refusal, quota ordering, staging occupancy, and
snapshot changes. These checks cover calculation and classification; the native
transaction fixtures below cover their filesystem effects.

## Native publication transactions

`PartitionGuard::publish_start` and `publish_finish` call the
[native transaction module](../src/usage/store/native/transaction.rs). Incoming
bytes must pass the independent supported-envelope reader for the requested ID
and event kind before any mutation. Complete partition classification and plan
validation also precede cleanup. Unknown names, unsafe entries, excessive external
occupancy, and invocation collisions refuse publication without earning cleanup.

Start returns an opaque, process-local `StartReceipt` only after no-replace
publication and verification succeed. It binds partition and lock identities,
invocation ID, start metadata including length, and a BLAKE3 digest of the exact
bytes. It retains neither payload nor a file descriptor across command execution.
A receipt therefore does not retain deleted payload allocation after eviction.
Its native identity observations inherit the transient-change limitations above.

Finish requires a receipt and a surviving start whose metadata and bounded byte
read match it. Missing starts return `MissingStart`; absence does not establish
why the file disappeared. Changed starts, foreign receipts, and existing finishes
refuse before cleanup. The finish plan protects its matching start from eviction
and skips publication if the remaining quota cannot hold the finish. Records still
can become incomplete or orphaned through interrupted cleanup or external changes;
readers must preserve that uncertainty.

The transaction copies bounded names and stat metadata into a mutable ledger.
After each confirmed unlink it removes only that entry from the expected set.
Fresh scans compare exact membership and surviving metadata with the ledger around
mutations, so its own directory changes do not authorize unrelated changes.
These full scans can make large cleanup expensive; their overhead is unmeasured.
A process ignoring the advisory lock can still race the interval between a final
identity check and its filesystem operation. Stable owner-controlled ancestry and
cooperating publishers remain required.

Retention stops at the first failure and preserves confirmed deletions. Removing
an invocation pair is not physically atomic: failure between its unlinks can leave
one member. `PublishOutcome` reports retention removal counts and observed lengths,
whether a group was partially removed, staging disposition, publication status,
and the primary error. Occupancy is available only when a final scan agrees with
the ledger; unexplained changes leave it unknown. Staging cleanup is reported
separately from retention removals. No rollback or complete loss history is implied.

Publication reserves the complete event length and one staging slot. It creates
an internally generated `.tmp.<id>` exclusively with mode `0600`, subject to
umask, through no-follow/nonblocking descriptor-relative access. It handles short
and interrupted writes, rejects failed or zero writes, and verifies completed
size, attributes, and bytes. Native `NOREPLACE` rename moves staging to the final
name without a second quota slot; a destination that appears immediately before
rename survives unchanged.

Successful rename is the publication commit point. A later verification failure
returns `Published` with an error and no usable start receipt. Finals are never
removed as rollback. Before publication, failed work removes staging only when its
retained descriptor and named entry agree; failed or unsafe cleanup leaves
`MayRemain` for a later transaction to inspect. `Published` reports the observed
rename, not power-loss durability or continuing visibility.

The [transaction fixtures](../src/usage/store/native/transaction/tests.rs) cover
late invalid entries, collision races, partial deletion, external membership
changes, short/failed writes, pre/post-rename failures, receipt rejection,
quota boundaries, and abrupt subprocess exits. Native lock fixtures isolate a
transient independent-reopen substitution. The lock open's `NONBLOCK` flag is
checked separately from FIFO rejection: a read/write FIFO need not block on Linux.
Native platform results must be recorded separately; these fixtures do not
establish command integration or measured collection overhead.

## Manual copying

Copy retained raw start/finish files during a quiet period. An ordinary filesystem
copy does not acquire the collector's lock and is not a consistent snapshot;
concurrent publication or cleanup may leave missing or unmatched records in the
copy. Keep those gaps explicit during analysis. No inspect, prune, or export
command is required. Never remove or replace partition or lock infrastructure
while a collector may be using it. Conventional per-user paths and integrated
configuration remain outside these inactive primitives.
