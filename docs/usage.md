---
purpose: enable local usage records and find, copy, and interpret them
sources:
  - ../src/config.rs
  - ../src/usage/lifecycle.rs
  - usage-events.md
  - usage-storage.md
---

# Local usage records

Experimental usage collection saves bounded command records locally for later
analysis. Enable it in a project's `drft.toml`:

```toml
[experimental.usage]
enabled = true
```

Omitting the setting or using `false` disables collection. Unknown experimental
settings and incorrect TOML types are configuration errors. A committed opt-in
also applies to collaborators and CI; each user's records stay in their local
cache. drft does not upload records.

Collection covers `check`, `graph`, `nodes`, `edges`, `impact`, and `lock` after
configuration loads successfully, including subsequent command failures. Help,
version, argument errors, `guide`, `init`, config inspection, invalid `-C`, and
missing or invalid configuration are outside this coverage.

## Find and copy records

The collector uses these directories:

| Platform                           | Directory                         |
| ---------------------------------- | --------------------------------- |
| macOS                              | `$HOME/Library/Caches/drft/usage` |
| Linux with `XDG_CACHE_HOME` set    | `$XDG_CACHE_HOME/drft/usage`      |
| Linux with `XDG_CACHE_HOME` absent | `$HOME/.cache/drft/usage`         |

The selected environment value must be an absolute path. An empty or relative
value disables collection; a present invalid `XDG_CACHE_HOME` does not fall back
to `HOME`. Other platforms skip collection while retaining ordinary command
behavior. Storage inside the active graph root is refused.

Each project has a subdirectory named by a BLAKE3 digest of its canonical root
and OS encoding. Open a `.start.json` file and read `canonical_graph_root` to
identify the project. Its `encoding` distinguishes UTF-8 from base64 native path
units. Moving a checkout can leave records in its former partition.

An invocation writes `<id>.start.json` and, when completion is observed,
`<id>.finish.json`. Copy the raw JSON files into a directory of your choice during
a quiet period when no drft commands are running for that project. Ordinary file
copying is sufficient. A concurrent copy is not a locked snapshot: collection and
cleanup can leave missing or unmatched records in the copy.

## What records establish

Records retain exact arguments and project identity, binary version, config
fingerprint, timing, intended exit status, available counts, findings, hints,
errors, and bounded stdout/stderr write-input prefixes. Arguments and output can
contain project content. Caller transcripts and shell history are not collected.

Unavailable observations differ from empty results. Truncation and omitted-record
counts describe retained prefixes. A successful write does not establish that
another program consumed its output. A missing finish can follow a panic, signal,
kill, contention, or storage failure; it does not prove no lock mutation occurred.
Storage failures skip evidence without replacing command errors.

Compare findings only across complete, nonoverlapping observations with compatible
project, config, binary, schema, and coverage. Changing the opt-in changes the exact
config fingerprint; if `drft.toml` is graph-visible, its content hash also changes.
Observed deltas alone establish neither causality nor semantic correctness.
The [event reference](usage-events.md) defines the fields and limits.

## Retention and cleanup

Each project is capped at 100 MiB and 10,000 files, including synchronization and
temporary files. Logging lazily expires complete pairs at least seven days old when
their timestamps are consistent. Incomplete, rejected, and uncertain records
remain eligible for quota eviction. An inactive store can remain beyond seven
days; total storage grows with the number of projects. Collection scans retained
records and rechecks membership around each mutation. Large partitions and quota
cleanup can add seconds to a command; the space limits are not latency limits.

Disable collection and stop all commands for a project before manually removing
its partition. Do not remove an active partition or its `.lock`. A missing lock
in an existing partition is not repaired automatically. The next enabled command
can initialize a new partition after the old, inactive one has been removed.

Native storage requires stable, owner-controlled local ancestry. ACLs, arbitrary
concurrent directory relocation, network filesystems, and synchronous power-loss
durability are outside its guarantees. The [storage reference](usage-storage.md)
describes publication, quota accounting, and failure behavior.
