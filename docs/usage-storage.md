# Usage storage infrastructure (inactive)

The [storage module](../src/usage/store.rs) opens existing cache infrastructure
and acquires an exclusive lock on macOS and Linux. Commands do not call it.
Initialization, record validation, quotas, publication, pruning, export, and
configuration integration remain unimplemented. A successful guard establishes
infrastructure identity and synchronization only.

`Partition::open_existing` requires an absolute cache path outside the canonical
graph root. Existing cache ancestors resolve once; the cache entry itself must
be a directory. The partition name is the BLAKE3 digest produced by
[`partition_id`](../src/usage/identity.rs) from exact canonical graph-root OS
identity. The partition and its empty `.lock` file must already exist. Opening
never creates infrastructure, truncates a file, or removes an entry.

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
handle. No operation repairs a missing lock by creating a new inode.

The [native tests](../src/usage/store/native/tests.rs) cover persistent path
substitutions, symlinks, hardlinks, special files, permission changes, same-process
and child-process contention, and process-death release. Invalid-byte native path
fixtures run on Linux; macOS filesystems can reject their creation. Raw identity
codec fixtures cover those bytes independently. Passing these tests does not
qualify no-replace publication, retention, command parity, or collection overhead.
Other platforms return `Unsupported` before path access. Native platform coverage
must be recorded separately from source availability.
