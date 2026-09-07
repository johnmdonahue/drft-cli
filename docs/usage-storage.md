# Usage storage infrastructure (inactive)

The [storage module](../src/usage/store.rs) opens or initializes cache infrastructure
and acquires an exclusive lock on macOS and Linux. Its bounded inventory accounts
for safe regular files and reads individual entries. A record inventory classifies
envelopes and calculates grouped retention and start-write reservations. Commands do not call it.
Native publication, pruning, export, and configuration integration remain
unimplemented. A successful guard establishes
infrastructure identity and synchronization only.

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
Health records have no reserved name or schema yet. Recognized temporary contents
are abandoned staging bytes and are not parsed as envelopes.

Finals are grouped by filename ID. Supported revision-1 envelopes must match the
filename ID and event kind and satisfy the complete wire shape, required fields,
encoding rules, and producer-established count/capture invariants. Duplicate JSON
keys, unknown revision-1 fields, invalid timestamps, and malformed encodings are
rejected. Caller-supplied observations are not verified against command execution.
Matching headers with later positive integer revisions are unsupported for analysis.
Malformed, unsupported, and oversized finals stay accounted for in their groups;
their original files remain available for a future raw export implementation.

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
invalidate the original snapshot. Finish reservations require a future opaque
successful-start receipt bound to the partition and published start. Native
publication, partial-deletion reporting, health replacement, and frozen export
snapshots remain separate work.

The [reader fixtures](../src/usage/record/tests.rs) and
[planner fixtures](../src/usage/store/records/tests.rs) cover hostile wire data,
recognized rejected finals, complete-scan refusal, pairing, uncertain coverage,
expiry boundaries, collision refusal, quota ordering, staging occupancy, and
snapshot changes. These checks qualify calculation and classification, not native
mutation or usable collection.
