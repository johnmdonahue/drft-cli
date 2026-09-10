---
purpose: configure the walk, graphs, and rules through .drft/config.toml
sources:
  - ../src/config.rs
  - ../src/cli.rs
  - ../src/layout.rs
  - ../src/lock.rs
  - ../src/sources/fs.rs
---

# Configuration

`.drft/config.toml` configures the walk, the graphs, and the rules. Its parent project directory is the graph root. drft walks up to the nearest project containing either the current config or a legacy config marker. Every `.drft` directory and its descendants are reserved project state and stay outside filesystem graphs.

## Committing the config and lockfile

drft has no setup mode and no flag for this. Whether `.drft/config.toml` and `.drft/lock.toml` are tracked is your decision, expressed through `.gitignore` and whether CI runs `drft check`. Common shapes include:

**Tracked.** The graph and its reviewed baseline are shared, `drft check` in CI gates on drift, and staleness is a claim the repository makes to everyone who clones it. This is the right default for a team that has adopted drft together.

**Tracked config, untracked baseline.** The graph definition is shared, while each working tree keeps its own reviewed state. Ignore only `.drft/lock.toml`.

**Untracked.** A clone gets no graph, nothing gates, and staleness is a fact about one working tree. Use it to run drft on a repository whose owners have not adopted it, or when drft is an authoring aid you reach for while writing rather than a check the project enforces. This repository is the second case; its `.gitignore` names both files with a comment saying so.

Tracking never changes graph membership. drft prunes `.drft` structurally, so neither configuration, baselines, temporary lock writes, nor future project state appear in `drft nodes` or a generated lock.

## Legacy root-level files

drft does not read root-level `drft.toml` or `drft.lock`. If either file exists at the selected graph root, repository-dependent commands exit 2 before parsing configuration, building the graph, recording usage, or writing output. The error names each manual move to `.drft/config.toml` or `.drft/lock.toml`.

Create `.drft` and move each legacy file without changing its contents. If a destination already exists, reconcile the two files manually; drft selects neither and never overwrites one. `drft init` applies the same checks to its requested directory and performs no migration.

## ignore

```toml
ignore = ["target/**", "drafts/**"]
```

The `fs` graph walks every file under the graph root, including dot-directories like `.github/`. It prunes version-control stores (`.git`, `.hg`, `.svn`, `.jj`) and every `.drft` state directory. `ignore` removes paths from that walk by glob. There is no `include`: the graph is everything under the root minus reserved state, configured globs, and active repository ignore sources.

In a Git repository, discovery applies the same three pattern sources as Git, in the same precedence order: repository `.gitignore` files, the per-clone `.git/info/exclude`, and the effective `core.excludesFile`. drft reads `.gitignore` files from the graph root through the repository root, plus nested files under the graph root. It asks Git to resolve machine-local configuration, so repository-local overrides and included configuration select the same global excludes file that Git uses.

Machine-local sources can make graph membership differ between clones. A path ignored through either source stays out of the local graph and out of a newly written lockfile, just as it stays out of Git's untracked working-tree surface. Use repository `.gitignore` or configured `ignore` globs when every clone needs the same exclusion.

In a native Jujutsu repository without a Git working tree, repository `.gitignore` files still apply and the two Git-only sources do not. Outside a Git or Jujutsu repository, drft does not consult repository ignore files. `.ignore` files never affect discovery.

Run `drft config --show-ignores` to list the repository `.gitignore` files drft consults and confirm which source classes are enabled. Add `--format json` for structured output. The command only reads configuration and ignore policy; it does not build the graph or update `.drft/lock.toml`.

This top-level `ignore` is a **discovery** filter: matching paths never become nodes, so nothing links to them and nothing is validated against them. To keep files in the graph (so your links to them resolve and stay drift-tracked) but skip _validating_ them, use the rule-level `ignore` instead — see [rules](rules/README.md).

## graphs

A graph pairs a file scope (`files`) with a parser that interprets the matched files. The `fs` graph is implicit and always built — it owns the identity space (paths) and contributes each file's `type` and `hash`. There are no default graphs: declare each one you want under `[graphs.<name>]`, and that set is the whole set.

```toml
[graphs.markdown]
parser = "markdown"
files = ["**/*.md"]

[graphs.frontmatter]
parser = "frontmatter"
files = ["**/*.md"]
```

| Field       | Required | Default       | Description                                                        |
| ----------- | -------- | ------------- | ------------------------------------------------------------------ |
| `parser`    | yes      | —             | How to interpret the files (see [parsers](parsers/README.md))      |
| `files`     | no       | `["**/*.md"]` | Globs scoping which files the parser reads                         |
| `edge_keys` | no       | none          | `frontmatter` only — the frontmatter keys whose values yield edges |

`edge_keys` names what the graph tracks: every string value reachable through one of those keys is an edge, and every other field is node metadata. drft never decides whether a value looks like a path, so a value naming nothing that resolves raises `unresolved-edge` rather than disappearing.

Omitting it is a supported shape — a frontmatter graph may exist purely to seed node metadata — so the graph loads, emits no edges, and says nothing about it. `edge_keys = []` is that same state written out: an empty set names nowhere to look, so it behaves identically.

Declaring keys states an expectation the corpus can fail to meet, and that is the state worth reporting: a graph declaring `edge_keys` that ends up with no edges raises an `edge-keys-matched-nothing` hint. A misspelled key otherwise produces a graph tracking nothing while the config says otherwise, at exit 0.

It applies to the `frontmatter` parser only — `markdown` has no keyed structure — and declaring it elsewhere is a config error. See [the frontmatter parser](parsers/frontmatter.md#naming-the-keys-that-yield-edges).

These are the only accepted keys. Any other key is a config error naming the key and the accepted set. A graph table that parses is read as a graph that works, so an option drft does not support fails loudly rather than being silently discarded.

The graph's name is its compose-time namespace: its facts nest under `@<name>` in the composed graph. A name must not contain `@`, start with `_`, or be `fs` — all reserved. `fs` is always built without being declared, and is a provider rather than a parser, so `parser = "fs"` is rejected too. With no `[graphs.*]`, only the `fs` graph is built.

## rules

Every built-in rule is on at `warn`. Configure severity and ignore globs under `[rules.<name>]`:

```toml
[rules]
ignore = ["vendor/**"] # global: suppress every rule for these subjects
stale-node = "error" # shorthand: severity only
stale-edge = "error"

[rules.detached-node] # table form: severity + ignore
severity = "off"

[rules.unresolved-edge]
ignore = ["CHANGELOG.md"] # globs matched against the finding's subject
```

The `ignore` key directly under `[rules]` is a **diagnostic** filter applied to _every_ rule, unioned with each rule's own `ignore`. Unlike the top-level `ignore`, matching paths stay in the graph — they still resolve links and carry drift hashes — so a file of yours that links a suppressed one is still flagged when that target changes (the finding's subject is your file, not the suppressed one). Use it for whole groups you depend on but don't own: "validate my files, not theirs." (`ignore` is a reserved key here; no rule may be named `ignore`.)

| Field      | Required | Default | Description                                     |
| ---------- | -------- | ------- | ----------------------------------------------- |
| `severity` | no       | `warn`  | `"error"`, `"warn"`, or `"off"`                 |
| `ignore`   | no       | none    | Globs — suppress findings whose subject matches |

Any other key in a `[rules.<name>]` table is a config error. An unknown rule _name_ only warns, since both fields default and a misspelled rule would otherwise configure nothing in silence. The warning is an `unknown-rule` [hint](reading.md#hints), carrying the config key as its locus.

See [rules](rules/README.md) for the full set.

## experimental.usage

```toml
[experimental.usage]
enabled = true
```

Opt in to bounded local command records on macOS and Linux. The default is `false`; missing or empty tables leave collection disabled. Unknown experimental keys and incorrect types are configuration errors, including when disabled. See [local usage records](usage.md) for coverage, cache paths, retention, and manual copying. A shared config opt-in applies to every user of that config; records stay in each user's local cache.
