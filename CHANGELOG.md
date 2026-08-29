# Changelog

All notable changes to drft are documented here.

## Unreleased

Makes a lock's coverage observable. Every way of ending up with a baseline that does not cover what you think it covers was silent — a directory lock that wrote nothing, a lockfile that had gone missing, a node with no entry, a bare name that matched a different file — and each returned exit 0. The run now says what it locked, and `check` says when there is nothing to check against.

### Breaking changes

- **`drft lock` prints a result document** (#120, #125). Text reports `locked 2 nodes`, `locked 1 node, dropped 1 entry`, or `locked 0 nodes`, followed by the node names; JSON emits `{locked, dropped}`. It previously printed nothing at all, which is what made a lock covering nothing indistinguishable from one covering the files you meant. **This breaks any consumer asserting `drft lock` is silent**, and moves `lock`'s hints from the stderr `{"hints": […]}` envelope into the result document, where every other command already carries them. No alias, per no-backwards-compat.

- **An unlocked node subsumes its outbound `new-edge` findings** (#120). Adding a new file that links two locked files previously reported one `new-edge` per link; it now reports one `unlocked-node` naming the file. The node having no baseline is the single fact behind every one of those edges, and this is the subsumption `stale-node` and `removed-node` already apply to their own edge findings. The trade is real: `new-edge` carried line numbers and the subsuming finding does not.

- **A scoped lock over an unparseable lockfile fails instead of proceeding** (#120). `drft.lock` that cannot be parsed reads as absent, so `drft lock <path>` used to replace the entire baseline with just the paths named — every other entry gone, and the nodes behind them left as unlocked leaves whose loss no rule reported. A hint said the file was unreadable while the destruction happened anyway. The command now refuses and names both remedies. `drft lock --all` is unaffected, since it rebuilds the baseline by design.

### New

- **`no-baseline` reports a lockfile that is absent or empty** (#120). Staleness is derived by comparing the graph against the lockfile, so with no lockfile — or one with no entries — there is nothing to compare against and every staleness rule becomes a no-op. That was indistinguishable from a clean run: no findings, exit 0, either way. It is a rule rather than a hint because hints never change an exit code, and a hint-only answer would leave an automated caller exactly as blind as it was. At its default `warn` a first `check` in a new repo stays quiet; `[rules] no-baseline = "error"` makes a missing baseline fail the run.

- **`unlocked-node` reports a node with no lock entry** (#120). A node absent from the lockfile was compared against nothing and reported nothing, so it was silently exempt from every staleness rule. Which nodes can be locked is asked of the lockfile writer rather than derived independently, so the rule and the writer cannot disagree — a directory and an unreferenced escaping symlink carry no hash and no outbound edge, are absent from a correct lockfile by design, and are never reported.

- **`directory-lock` hint** (#125). A directory named to `drft lock` resolves to the directory node, which carries no hash and no edges and so has nothing to snapshot. The run reports `locked 0 nodes` and names how many nodes beneath it were not locked.

### Fixed

- **A scoped lock no longer snapshots a file the caller did not name** (#127). A bare argument had `.md` appended and that invented candidate was tried *ahead* of the path as spelled, so with both `docs/` and `docs.md` present, `drft lock docs` snapshotted `docs.md` — clearing its `stale-node` finding and writing a durable "this was reviewed" claim against a file nobody opened. The exact path now resolves first. The related case where a bare name falls back to a same-named node at the graph root is now visible in the result document, which names what was locked.

- **A directory lock no longer manufactures an empty baseline** (#125). In a repo that had never been locked, `drft lock <dir>` wrote a valid, parseable, zero-entry lockfile — a baseline covering nothing, produced by a command that reported success, which made every staleness rule a no-op while the file's presence made the baseline look established. A lock that writes nothing now leaves no lockfile behind.

## 0.16.0 (2026-08-20)

Gives every command a run-level layer. A `hints` channel says what happened to _this invocation_ — a selector that matched nothing, a projection large enough to crowd the context it was meant to ground — where findings only ever describe the graph. And `drft lock` stops reading zero paths as every path, so a command substitution that matched nothing can't write a whole-graph review claim.

### Breaking changes

- **The whole-graph lock is `drft lock --all`** (#104). `drft lock` with no paths is a usage error (exit 2) naming both remedies, where it previously snapshotted every node. Zero paths is what a shell hands over when a command substitution matches nothing, so `drft lock $(cmd)` on an empty `cmd` turned a scoped invocation into a whole-graph one — silently, at exit 0, writing a claim that every node had been reviewed into a file that outlives the session. **This breaks any script, hook, or habit that relied on the bare form**: pass `--all` to snapshot the whole graph, which is the right call when regenerating a baseline and the wrong one otherwise. `--all` combined with paths is also an error, since the two state different scopes. There is no short form — spelling out the call that asserts whole-graph review is the point, and a long flag is greppable, so a hook or CI check can forbid `--all` while leaving scoped locks alone. `drft nodes` and `drft edges` keep their zero-selector default: for a reader the cost is a large read, not a durable claim.

- **`Finding.hint` is now `Finding.cause`**. `drft check --format json` emits `cause` where it emitted `hint`, and text output renders `cause:` where it rendered `hint:`. **This breaks any consumer reading `.diagnostics[].hint`.** The field names the likely reason a finding reads as the wrong problem — an `unresolved-edge` whose path would resolve from the graph root — which is what `cause` already called it in the rules reference and in its own test. The word is freed for the run-level `hints` channel added below, where a `hint` on a finding and a `hints` list on the document would otherwise mean two different things one letter apart. No alias, per no-backwards-compat.

### New

- **A `hints` advisory channel on every command**. A hint is a statement about the _run_ rather than about any item in the result: a selector that matched nothing, a projection large enough to crowd the context it was meant to ground, a misspelled rule name that configures nothing. Findings describe the graph; hints describe the run that read it. Each is `{name, locus?, message, next?}` — structured, so a reader can act on one by `name` or ignore it, which prose on stderr cannot offer. `locus` is not necessarily a path: a selector, a config key like `rules.stale-nodes`, or absent. The launch set is four: `unknown-rule` and `unparseable-lock` (both previously stderr prose, unchanged in stream and now structured in shape), plus `zero-match-selector` and `large-projection`. The last fires past 64KB of rendered output, measured after rendering rather than estimated, and reports the node count alongside the size since a byte count alone is opaque. In JSON the channel is a key on the result document; where a command prints no document (`init`, `lock`) or prints one that is a format rather than drft's own envelope (`drft graph --format json`, whose JGF root is exactly `graph`), the hints take a `{"hints": […]}` envelope on stderr instead, and join the error envelope on the error path. In text they go to stderr after the result, so a pipe carries only the projection. **Hints never change an exit code and never replace a guard** — `drft lock` with no arguments still refuses, and an unresolved path still errors, because an advisory an agent can skip does not stop it.

## 0.15.0 (2026-08-19)

Finishes the read verbs of the #90 query surface. `drft edges` projects the graph's edges, and `drft graph` gains a text rendering of the whole composed graph — so an agent can read nodes, edges, or the entire graph without parsing JSON.

### Breaking changes

- **`drft graph` defaults to text output** (#90). `drft graph` now honors the global `--format`, whose default is `text`, and renders the composed graph as `# nodes` / `# edges` sections. It previously always emitted the composed JGF as pretty JSON. **This breaks pipelines**: `drft graph | jq …` now feeds text to `jq`, which errors — pass `--format json` to restore the JGF document. `--raw` is unaffected — it stays JSON-only and ignores `--format`, so `drft graph --raw` is unchanged.

### New

- **`drft edges` projects the graph's edges** (#90). `drft edges <selector…> [--namespace <g>…] [--field <k>…] [--format text|json]` reads the composed graph's edge half, matched on source: the selector resolves to source nodes — the same vocabulary as `drft nodes` (exact path, bare directory standing for its recursive subtree, or a globset over node keys) — and the projection is every edge leaving them, the outbound one-hop view. With no selector, every edge. It stays distinct from `drft impact` (a transitive traversal from a seed set) and `drft check` (a whole-graph gate). `--namespace` and `--field` narrow the edge set and its metadata as they do for `nodes`; a mistyped source errors while an empty glob is exit 0. Text is one block per edge — `source → target` then its metadata; JSON carries `{ total, edges: [{ source, target, metadata }] }`. An edge with no per-graph metadata still projects. The metadata narrowing and text rendering shared with `nodes` moved into a new `projection` module.
- **`drft graph` renders the composed graph as text** (#90). Under the default `--format text`, `drft graph` prints every node's metadata under a `# nodes` header, then every edge under `# edges`, reusing the `nodes` and `edges` renderings — so a model reads the whole graph in one call without parsing JSON. `--format json` still yields the composed JGF (see the breaking-changes note on the default). Scoped selectors on `graph` are out of scope — `drft nodes` and `drft edges` cover scoped projection.

### Fixed

- **`@frontmatter` metadata keeps code spans** (#95). The frontmatter parser masks backtick code spans before scanning for link targets, so a `path.md` written in prose can't be mistaken for an edge. That masked buffer was also captured as node metadata, so an `@frontmatter` value like `purpose` came back with its code spans blanked — surfaced once `drft nodes` and `drft graph --format json` read frontmatter metadata as prose. Metadata is now captured from the raw frontmatter while edge extraction still runs on the masked copy, so code spans survive as authored and edges are byte-identical to before. A value that is invalid YAML on its own — one beginning with a backtick, or hiding a `:` in an unquoted span — still falls back to the masked parse; quoting it or using a `|` block scalar captures it verbatim.

## 0.14.0 (2026-08-18)

Adds the first read verb of the #90 query surface. `drft nodes` reads what the graph knows about a set of paths, so an agent can ground itself on a file's metadata without exporting and parsing the whole composed graph.

### New

- **`drft nodes` projects node metadata** (#90). `drft nodes <selector…> [--namespace <g>…] [--field <k>…] [--format text|json]` reads the composed graph's node half, scoped by a selector and narrowed by namespace and field. A selector is an exact path (cwd-aware, with the `.md` fallback `impact` and `lock` use), a bare directory standing for its recursive subtree (`docs/` ⇒ `docs/**`), or a globset pattern over node keys — the same vocabulary as `drft.toml`'s `files`/`ignore` — and `docs`, `docs/`, and `docs/**` all name one set. `--namespace` accepts the bare graph name or its `@`-prefixed key, filters the node set as well as the metadata, and errors on an unknown namespace with the declared graphs listed; `--field` narrows to named keys and lists only the nodes that declare them, so a wholly unmatched field is a legitimate empty result rather than an error. Text output is a first-class projection — one compact block per node — for reading without parsing JSON; JSON carries `{ total, nodes: [{ id, metadata }] }`. As a reader it expands selectors freely, so a mistyped path errors while an empty glob is exit 0. First increment of #90; `edges` and `graph --format text` follow.

### Fixed

- **`drft lock` clears a removed node when you name its deleted path** (#89). A file deleted from the graph left a `removed-node` finding that nothing could clear, because the path no longer resolved to a node to re-snapshot. Naming the vanished path now drops its lockfile entry, so a reviewed deletion clears the finding — the case the live graph alone could not resolve. Relatedly, the `.md` fallback is only tried when the named path has no extension, so locking `guide.md` no longer also invents a `guide.md.md` candidate.

## 0.13.0 (2026-07-30)

Makes the honest form of `drft lock` the convenient one, so agent guidance can scope the lock rather than ban it.

### New

- **`drft lock` accepts several paths.** `drft lock a.md b.md` locks exactly those nodes and their outbound edges, merging into the existing lockfile; with no arguments it still snapshots the whole graph. A lock asserts the locked state was reviewed, and the scoped form is what keeps that assertion narrow enough to be true — a bulk lock also clears staleness nobody looked at, including work another person has in flight. Accepting one path at a time pushed callers toward the bulk form for any multi-file change, which is the case where scoping matters most. Every path resolves before anything is written, so an unresolvable path fails the command (exit 2) rather than leaving a partial lock behind.

## 0.12.0 (2026-07-30)

Makes `drft impact` answer the question an edit actually asks. The output is a review list now, not a reachability dump.

### Breaking changes

- **`drft impact` reports one hop by default** (#75). It previously traversed unbounded, which on a repo where docs cross-reference normally returns most of the graph — 21 of 29 nodes for one seed in the reported case. That is not a review list, and an oversized one trains its consumer to skip the output entirely, taking the real finding with it. The default is now `--depth 1`: the files that name the seed directly, each a promise someone wrote down. Every result still carries `radius`, so a wider set is reported without being enumerated. **This is a quiet change** — scripts calling `drft impact` keep working and silently return less. Pass `--depth all` to restore the previous behavior.
- **`--depth all` spells the full reachable set.** `--depth <n>` still bounds traversal to n hops. `--depth 0` is now a usage error pointing at `all`, rather than being given the maximal meaning — "0" reads as "traverse nothing", and a flag that silently means the opposite of what it says is worse than one that errors.

## 0.11.0 (2026-07-30)

Closes the ways drft could report success while tracking nothing: a config that parsed but did nothing, a frontmatter graph that turned any path-shaped value into an edge, and a broken link that named a path nobody wrote.

### Breaking changes

- **Unknown keys in `drft.toml` are now config errors** (#71). Extra keys in a `[graphs.*]` table, at the top level, or in a `[rules.*]` table were previously parsed and discarded, so a speculative option like `include_keys = ["sources"]` exited 0 having done nothing. All three now fail with exit 2, naming the key and the accepted set. A config carrying a typo'd or aspirational key was already not doing what it said; it now says so. Unknown rule _names_ still only warn — that path is unchanged.

### New

- **`keys` scopes the frontmatter graph to named keys** (#73). The parser classified by value shape, so any path-shaped frontmatter value became an edge whatever key it sat under — an API route (`route: /customers`) or a glob naming the files a rule governs both read as derivations. `keys = ["sources"]` on a `[graphs.*]` table using the `frontmatter` parser restricts edges to values reachable through those keys; a matched key contributes its whole subtree, and the key matches at any depth. Omitting `keys` keeps shape detection, so existing configs are unaffected. This replaces the two workarounds that were available: a `files` allowlist only works when the collision is tree-separable, and a rule-level `ignore` on `unresolved-edge` suppresses genuinely broken `sources:` paths along with the false ones. `keys` scopes edges only — metadata still captures the whole block.
- **`unresolved-edge` names a wrong resolution base** (#72). Link paths resolve relative to the declaring file, so a path written against the graph root resolves somewhere that does not exist and is reported as a target the author never wrote — which reads as a typo rather than a wrong base. The finding now carries a `hint` when the literal link text would resolve from the graph root, naming the cause and suggesting the rewrite. It is withheld for paths written `./`, `../`, or `/`, which are relative by intent. Findings render the hint as an indented line in text output and as a `hint` field in JSON. Doc-relative resolution is now stated in the README, the frontmatter parser reference, and the `drft init` template, none of which said it before.

## 0.10.0 (2026-06-24)

### Breaking changes

- **Dot-directories are now part of the graph.** The `fs` walk no longer skips hidden entries, so directories like `.github/` and `.claude/` and files like `.gitignore` become nodes. Only version-control stores (`.git`, `.hg`, `.svn`, `.jj`) are pruned, by name — `.git` matches whether it is a directory or a file (submodules and linked worktrees). Lockfiles regenerate with `drft lock` to capture the newly-visible nodes (#69).
- **Only committed ignore sources prune the walk.** drft previously inherited the `ignore` crate's defaults, applying your global gitignore, the per-clone `.git/info/exclude`, and `.gitignore` files in directories above the graph root. These are now disabled: only the in-root `.gitignore` plus the configured `ignore` globs affect traversal, so the graph depends solely on what is committed at the root and is reproducible across clones (#69).

## 0.9.1 (2026-06-05)

### Fixed

- **`drft impact` / `drft lock` path arguments resolve relative to the current directory** (#66). A project-relative path given from inside a subdirectory is now resolved against the current directory and converted to the graph-root-relative node key, instead of being matched verbatim — so the same file resolves whether given project-relative from a subdir or root-relative from the top, matching `git log <path>` behavior. On a miss, the error suggests the node whose key matches the given suffix (or lists candidates when the suffix is ambiguous).

## 0.9.0 (2026-06-05)

Directories and symlinks get a proper place in the graph, link edges learn where they live, and the `impact`/`check`/rules surface gets sharper.

### Breaking changes

- **`drft impact` requires a path.** Running `impact` with no argument is now a usage error (exit 2). It previously defaulted to the impact of all currently-stale sources, which conflated a structural query with a lock-derived one. The drift blast-radius use case is deferred, not abandoned.
- **Symlinks are untrackable indirection.** The `fs` walk no longer follows symlinks: a symlink is a leaf node with an edge to its target, and is never hashed — staleness propagates through the edge to the target. Lockfiles that recorded symlink hashes regenerate with `drft lock`. This also closes a leak where a symlink to a directory outside the graph root pulled outside files into the graph as nodes.

### New

- **Directory nodes.** The `fs` walk emits a node per directory (typed `directory`, hash-less). A link to an existing directory now resolves instead of flagging `unresolved-edge`; a link to a missing one still flags. Directories are never hashed, locked, or flagged `detached-node`.
- **Link line numbers.** Markdown and frontmatter link edges record the 1-based source line(s) where the link appears as `lines` metadata — surfaced in `drft graph`, in `drft impact` fix instructions (`review guide.md:3,5 …`), and on `drft check` edge findings (`README.md:50 → src/lib.rs`, including `unresolved-edge`). Line numbers are graph-only and never locked, so a link moving lines is not drift.
- **Global `[rules].ignore`.** An `ignore` set directly under `[rules]` applies to every rule, unioned with each rule's own. Unlike the top-level `ignore`, matched paths stay in the graph — links to them resolve and they keep drift hashes — so you can stop validating a group you depend on but don't own while keeping the transitive-staleness signal on your own files.

### Changed

- **Frontmatter YAML engine** switched from `serde_yml` (a continuation of the unmaintained `serde_yaml`) to `saphyr` (maintained, pure-safe Rust, with source spans — the source of frontmatter line numbers). Metadata output is unchanged. Malformed frontmatter is now skipped silently instead of printing a YAML warning — drft checks link drift, not YAML validity.

## 0.8.0 (2026-06-04)

Rebuilt on a set-of-graphs substrate with an explicit composition step. drft now models a directory as a set of independent JGF graphs of bare-path nodes — `fs` (a node per file, typed and hashed), `markdown` (link edges), and `frontmatter` (edges plus metadata) — that a `compose` step merges by path. The `impact → edit → check → lock` loop is unchanged.

### Breaking changes

- **Config schema reshaped.** `drft.toml` is now `ignore` + `[graphs.*]` (each with a `parser` and `files` globs) + `[rules.*]` (severity, ignore). `include`, `exclude`, and `[parsers.*]` are removed. The graph root is the directory containing `drft.toml`.
- **Lockfile format changed.** `drft.lock` is path-keyed with a node hash and nested per-edge target hashes, no version field. Old lockfiles report as unparseable and point at `drft lock` to regenerate.
- **Command surface rebuilt.** The surface is `init`, `graph` (composed by default, `--raw` for the raw graph set), `impact` (with `--depth` and `--direction`), `check`, and `lock` (bulk and scoped). The `parse`, `report`, and `config` commands are removed.
- **Rule set is drift-focused.** Rules are `stale-node`, `stale-edge`, `new-edge`, `removed-edge`, `removed-node`, `unresolved-edge`, and `detached-node`. The structural-hygiene rules (`directed-cycle`, `fragmentation`, `schema-violation`, `symlink-edge`) are removed; cycles are now permitted.
- **No migration path.** Pre-v1, this is a clean break — old `drft.toml`/`drft.lock` formats are not migrated.

### New

- **Set-of-graphs substrate** — `fs` is the base graph that walks every file, types it (`file`/`symlink`), and is the only graph drft auto-hashes. `markdown` and `frontmatter` add edges and metadata over the same paths.
- **Composition by path** — `compose` merges the graph set: each graph's facts nest under `@<graph>` with a `_graphs` provenance list. Resolution is namespace presence — a path with no `@fs` block is unresolved.
- **`drft graph --raw`** — emit the uncomposed graph set as JGF `{"graphs": [...]}` alongside the composed default.
- **`drft init`** — scaffold a `drft.toml` for a new graph root.

### Removed

- The analyses and metrics subsystem, custom subprocess parsers and rules, the criterion benchmark harness, and the v0.7 docs and examples for those features.

## 0.7.0 (2026-04-12)

Included vs referenced nodes — every edge target is a node. `include` controls what drft reads and hashes, not what exists in the graph.

### Breaking changes

- **Node model redesigned.** Every edge target is a node. Included nodes (`included: true`) match `include` patterns — drft reads, hashes, and manages them. Referenced nodes (`included: false`) are edge targets drft knows about but doesn't manage.
- **`NodeType` replaced.** Nodes carry `type` from stat: `file`, `directory`, `symlink`, `uri`, or `null` (broken link). Type is intrinsic — a file outside `include` is still `type: "file"`.
- **`boundary-edge` rule removed.** The containment concept it encoded is handled by the included/referenced model.
- **`dangling-edge` rule renamed to `unresolved-edge`.** Configs using the old name will see an "unknown rule" warning. Rename to `unresolved-edge` in `drft.toml`.
- **`drft graph --format json`**: nodes carry `type` and `included` in metadata. Edges are simple (`source`, `target`, `parser`). No `target_kind` on edges, no `target_properties` at graph level. Lockfile entries no longer carry `type`. Existing lockfiles need regeneration with `drft lock`.
- **Nested-graph machinery removed.** `[interface]` section, `is_graph` flag, `child_graphs` tracking, `--recursive` / `--max-depth` flags on `lock`/`check`/`graph`/`config show`.
- **Glob patterns use shell semantics.** `*` matches a single path component (not `/`), `**` crosses directories. The default include changed from `["*.md"]` to `["**/*.md"]`. Parser `files` patterns like `["*.md"]` should become `["**/*.md"]`.

### New

- **Included vs referenced nodes** — `include` controls what drft reads and hashes, not what exists in the graph. Every edge target gets a node with `type` from stat and `included` from scope.
- **Symlinks are filesystem edges** — symlinks in `include` get an edge to their resolved target with `parser: "filesystem"`. The symlink node has `type: "symlink"`.
- **Symlink hash policy** — symlinks matching `include` become nodes, but drft only hashes content when the canonical target is also in `include`. Otherwise `hash = null`.
- **Literal include path fallback** — `include` paths with no glob characters (e.g., `.claude/settings.json`) are checked directly on disk when the walker misses them due to gitignore directory exclusion.
- **`docs/discovery.md`** — documents include/exclude patterns, glob semantics, and gitignore interaction.
- **Examples tracked in root graph** — example READMEs are nodes with `sources:` frontmatter linking to the docs they illustrate.

### Fixed

- `drft impact <path>` works on any file under a graph root, even when an unrelated `drft.toml` lives in a subdirectory.
- `monorepo` example removed (demonstrated nested graphs, which no longer exist).

## 0.6.1 (2026-04-08)

Edge detection hardening — proper URI validation and frontmatter parsing, full JGF export.

### Fixes

- **False positive URI detection** — `is_uri` replaced hand-rolled RFC 3986 scheme check with the `url` crate (WHATWG URL Standard). YAML values like `name: foo bar` no longer match as URIs.
- **Frontmatter link extraction** — replaced line-by-line string splitting with `serde_yml` tree walking. YAML mapping keys within lists (e.g., `- name: foo bar`) are correctly ignored — only values are examined.
- **Extension heuristic cap** — bumped from 4 to 6 characters, covering `.swift`, `.proto`, `.patch`, `.class`, and similar.

### New features

- **Full JGF export** — `drft graph --format json` now includes all internal graph data: node `graph` membership, `is_graph` flag, parser metadata (e.g., frontmatter YAML payload), and graph-level `interface` and `target_properties` in `graph.metadata`.

## 0.6.0 (2026-04-08)

Node/edge model refinement — scope as a first-class concept, JGF compliance, directory traversal prevention.

### Breaking changes

- **`External` node type narrowed** — only URLs are `External`. Files discovered via edges (outside `include`, in child graphs, `../` targets) are `File` with `included: false`.
- **JSON graph output follows JGF v2.0** — `parser` moved from top-level edge field to `edge.metadata.parser`. `internal` computed in `edge.metadata.internal`. Node metadata includes `included`.
- **Degree counts all edges** — `orphan-node` no longer produces false positives for files linking only to URLs or directories.
- **Lockfile node types changed** — nodes previously typed `"external"` for on-disk files are now `"file"`. Requires `drft lock` after upgrade.

### New features

- **`included: bool` on nodes** — marks whether a node was matched by `include` during discovery. Available in JSON output and to custom rules.
- **`graph.is_internal_edge()`** — derived from node `included` state. Structural analyses scope to included nodes and internal edges.
- **`drft --version` / `drft -V`** — version output support.
- **Frontmatter parser emits URIs** — URLs in YAML frontmatter produce External edges instead of being silently dropped.
- **Improved frontmatter link detection** — `has_file_extension` replaced with `is_link_candidate`: rejects prose with spaces, abbreviations (`e.g.`), version numbers (`v2.0`).

### Security

- **Directory traversal prevention** — all filesystem access gated by canonical path verification. Targets outside the graph root (via `../`, symlinks, absolute paths) get nodes but no content is read or hashed. Included files that resolve outside root via symlinks produce a warning.

### Fixes

- **`drft init` template** — `[interface]` section uses `files` (not `nodes`).
- **Metrics scope** — cyclomatic complexity and redundant edge ratio use internal edges consistently.

## 0.5.2 (2026-04-06)

Parser-scoped rules and frontmatter improvements.

### Features

- **Per-rule parser scoping** — rules can scope to specific parsers via `parsers = ["frontmatter"]` in config. The rule evaluates against a filtered graph containing only edges from the named parsers, distinguishing structural dependencies from navigation links.
- **`--parser` flag on `graph` and `impact`** — filter edges by parser for ad-hoc exploration. `drft graph --parser frontmatter` shows only frontmatter edges.
- **`Graph::filter_by_parsers()`** — new graph primitive that produces a parser-filtered view while preserving all nodes.

### Fixes

- **Frontmatter parser detects same-directory references** — `sources: [setup.md]` is now extracted as a link. Previously required a path separator (`./setup.md` or `dir/setup.md`).
- **Unknown parser names in rule config produce warnings** — a typo in `parsers = ["fronmatter"]` now warns instead of silently running against an empty graph.
- **Sorted parser names in error messages** — `--parser nonexistent` error output is deterministic.

## 0.5.1 (2026-04-01)

Post-v0.5 audit — simplify graph, enforce frontmatter, remove noisy rules.

### Breaking changes

- **Removed rules**: `fragility`, `layer-violation`, and `redundant-edge` flagged properties inherent to tree-shaped filesystems. The underlying analyses remain in `drft report`.
- **`orphan-node` semantics changed** — flags isolated nodes (in-degree 0 AND out-degree 0), not roots. Files with outbound links but no inbound links are entry points, not orphans.

### Changes

- **Frontmatter as dependency layer** — doc files use `sources:` YAML frontmatter instead of prose `## Source` sections. `schema-violation` enforces `required = ["sources"]` on `docs/**`.
- **Removed artificial READMEs** — directory index files in `src/`, `tests/`, `benches/`, and `docs/` subdirectories that existed only to satisfy drft rules. Graph went from 103 nodes / 181 edges to 80 / 118.
- **Simplified `drft.toml`** — removed all ignore rules (now zero), deduplicated config.
- **Rewrote README** — leads with the mental model (links create obligations, lockfile is a checkpoint, graphs nest like directories).
- **Added `docs/config.md`** — configuration reference with examples.
- **CI: `cargo bench --no-run`** — catches benchmark compilation failures.

## 0.5.0 (2026-03-31)

drft.toml as the sole graph marker — simpler mental model, no ordering constraints.

### Breaking changes

- **Require `drft.toml` to run** — `drft check`, `drft lock`, etc. exit 2 without a config file instead of silently applying defaults.
- **Child graph discovery by `drft.toml` only** — bare `drft.lock` no longer marks a graph boundary.
- **Directory nodes hashed from `drft.toml`** — parent tracks child config for staleness; no dependency on child lockfiles.
- **`Graph` node type replaced by `Directory`** — `NodeType::Graph` is now `NodeType::Directory` with an `is_graph` boolean. JSON output, lockfiles, and custom rule input use `"directory"` instead of `"graph"`.
- **Node `graph` field semantics expanded** — indicates graph membership: `"."` (local), `".."` (escape), child graph name, or `null` (not on filesystem). Replaces the previous "set only for child graph nodes" convention.
- **`[interface] nodes` renamed to `[interface] files`** — config and lockfile both changed. New `ignore` field for excluding interface paths.
- **`directory-edge` rule replaced by `untrackable-target`** — the config key changed; `directory-edge` is no longer recognized.
- **Child graph paths normalized** — no trailing slash (`"research"` not `"research/"`).
- **"Script" terminology renamed to "custom"** — "script parsers" and "script rules" are now "custom parsers" and "custom rules" in docs and source.

### New features

- **`drft config show`** — display the resolved configuration (defaults filled in). Supports `--format json` and `--recursive`.
- **Per-rule `files` scoping** — `[rules.<name>] files = ["docs/**"]` restricts which nodes a rule evaluates, complementing the existing `ignore` field.

### Fixes

- **Directory staleness detection** — `compute_current_hash` was hashing `drft.lock` while `build_graph` hashed `drft.toml`, causing child graphs to always appear stale.
- **Interface file promotion** — now honors child's `exclude` and interface `ignore` patterns.
- **`untrackable-target` rule** — restored with updated message ("add a drft.toml" instead of "create a lockfile").

## 0.4.0 (2026-03-31)

Graph model redesign — explicit graph declaration, pure rules, parser metadata, and enriched impact analysis.

### Breaking changes

- **Graph declaration**: `include`/`exclude` replaces implicit parser-glob union. `ignore` renamed to `exclude`.
- **Node types**: `Source` → `File`, `Resource` removed. Three types: File, External, Graph.
- **Rule renames**: `broken-link` → `dangling-edge`, `cycle` → `directed-cycle`, `containment` → `boundary-violation`, `encapsulation` → `encapsulation-violation`, `orphan` → `orphan-node`, `indirect-link` → `symlink-edge`, `directory-link` → `directory-edge`.
- **Parser contract**: `glob` replaced by `files` (array of globs), `types` replaced by `options` (arbitrary structured data). Parsers return raw link strings — graph builder owns normalization.
- **Edge model simplified**: `RawLink`/`EdgeType` removed. Edge = `{ source, target, link?, parser }`.
- **RuleContext**: reduced to `{ graph: &EnrichedGraph, options }`. No filesystem access, no config.
- **Lockfile**: regenerated with new node types.

### New features

- **`drft parse`** command: raw parser output for debugging script parsers and the options envelope protocol.
- **Frontmatter parser**: standalone built-in parser for YAML frontmatter (links + structured metadata).
- **Schema-violation rule**: validates node metadata against glob-scoped schemas with required/allowed fields. First consumer of parser metadata and rule options.
- **Impact-radius analysis**: per-node blast zone (transitive dependents, depth, direct count).
- **Enriched `drft impact`**: output annotated with depth, impact_radius, and betweenness; sorted by review priority.
- **Rule options**: `[rules.<name>.options]` for arbitrary structured data passed through to rules.
- **Parser options**: `[parsers.<name>.options]` passed to script parsers as JSON envelope on stdin.
- **Script rule enrichment**: script rules receive `{ graph, options }` with all 12 analyses.

### Fixes

- Replace deprecated `serde_yaml` with maintained `serde_yml` fork.
- Deterministic metadata merge across parser namespaces (sorted by key).
- Warning on invalid glob patterns in schema-violation options.
- `drft graph --dot` replaces `--format dot` (DOT output is graph-only).

## 0.3.0 (2026-03-30)

Major architecture overhaul: drft is now a structural integrity checker for any linked file system, not just markdown.

### Breaking changes

- Config format: unified `[parsers]` and `[rules]` sections replace prior layout
- Lockfile v2: nodes + hashes only, no edges
- `scope` terminology renamed to `graph` throughout

### New features

- **Configurable parsers**: built-in markdown parser + script-based parsers via `command` field
- **Batch script parsers**: one process per parser instead of one per file (PR #19)
- **Rust doc comment parser**: links source files to docs via `parse-rust.sh`
- **`drft report`** (unstable): unified command for 11 graph analyses and 15 scalar health metrics — run all with `drft report`, filter by name with `drft report depth orphan_ratio`
- **Custom analyses and custom metrics** via external scripts
- **Criterion benchmarks** for the full pipeline

### Rules

- New: `fragmentation`, `layer-violation`, `redundant-edge`
- `stale` rule now defaults to error severity
- Rules refactored to consume analysis results

### Fixes

- Wikilink/frontmatter parsers skip inline code spans and code blocks
- Ignore patterns now apply to child graph detection
- Dropped lockfile version migration check

## 0.2.1 (2026-03-29)

- Fix #9: boundary-violation rule now catches `../` edges escaping graph boundary
- Fix #11: custom rule commands resolve relative to config file, not CWD
- Fix #8: required-frontmatter example adds file exemptions (SKIP_NAMES)
- `lockfile-outdated` rule: `drft check` detects when lockfile doesn't match current graph
- Config inheritance: child graphs without `drft.toml` inherit from nearest ancestor
- Interface persisted in `drft.toml`: `[interface]` section is source of truth
- Failed custom rules now surface as diagnostics in JSON output

## 0.2.0 (2026-03-29)

- `--rule` filtering now works for custom rules
- `npx drft` documented for npm-based projects
- New custom rule examples: required-frontmatter, max-depth

## 0.1.3 (2026-03-29)

- Fix npm package downloading binaries from wrong release version

## 0.1.2 (2026-03-29)

- Add `lockfile-outdated` rule: `drft check` detects when lockfile doesn't match current graph
- Config inheritance: child graphs without `drft.toml` inherit from nearest ancestor
- Persist interface in `drft.toml`: `[interface]` section is the source of truth
- Add `drft impact` command for transitive dependency analysis
- JSON summary envelope for `drft check --format json`
- Structured JSON errors on stderr when `--format json` is set
- JSON Graph Format (JGF) output for `drft graph`
- Custom script rules via `[custom-rules]` in config
- Per-rule path ignores via `[ignore-rules]` in config
- `--max-depth` flag for recursive operations
- `--watch` mode for `drft check`
- Colored terminal output
- Diagnostics include `fix` field with actionable instructions
- Direct + transitive staleness differentiation
- `.gitignore` respect via `ignore` crate
- Lockfile version checking
- Fixed: email links no longer flagged as broken links
- Fixed: frontmatter parser rejects YAML objects/arrays/quoted strings
- Fixed: cycle detection panic on DFS root nodes
- Fixed: untrackable-target rule (was directory-edge) skips Graph nodes
- Fixed: ignored files detected as "excluded by ignore pattern" in dangling-edge

## 0.1.1 (2026-03-28)

- Fixed npm postinstall binary download
- Added CI and automated publish workflows

## 0.1.0 (2026-03-28)

Initial release.

### Commands

- `drft init` -- create default config
- `drft lock` -- snapshot file hashes and dependency graph
- `drft lock --check` -- verify lockfile is current (CI)
- `drft check` -- validate graph against rules
- `drft graph` -- export dependency graph (JSON Graph Format, DOT)
- `drft impact` -- show transitive dependents of given files
- `--recursive` flag for lock, check, and graph
- `--max-depth` flag to limit recursive depth
- `--watch` flag for check

### Rules

- `dangling-edge` -- missing edge targets, including files excluded by ignore patterns
- `boundary-violation` -- edges escaping graph boundary
- `directed-cycle` -- circular dependencies
- `untrackable-target` -- directory targets with no lockfile
- `encapsulation-violation` -- edges into child graph's non-interface files
- `symlink-edge` -- symlink targets
- `orphan-node` -- nodes with no inbound edges
- `stale` -- dependencies changed since last lock (direct + transitive)

### Features

- 6 link source types: inline, reference, autolink, image, frontmatter, wikilink
- 4 node types: Source, Resource, External, Graph
- BLAKE3 content hashing (`b3:` prefix)
- Hierarchical graphs with child-graph projection
- Interface support for child graphs
- `.gitignore` respect
- Per-rule path ignores (`[ignore-rules]`)
- Custom rules via external scripts (`[custom-rules]`)
- Colored terminal output (`--color`)
- JSON diagnostics with `fix` field and summary envelope for LLM workflows
- JSON Graph Format output for graph export
- Lockfile version checking (forward-compatible)

### Distribution

- Cargo: `cargo install drft-cli`
- npm: `npm install drft-cli`
- GitHub Releases: prebuilt binaries for macOS, Linux, and Windows
