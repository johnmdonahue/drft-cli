# Releasing drft

## The process

1. **Bump version** in `Cargo.toml`
2. **Update CHANGELOG.md** with the new version and date
3. **Run `drft lock Cargo.toml CHANGELOG.md`** — see [the release commit is drift](#the-release-commit-is-drift) below
4. **Open a PR** from a release branch (e.g., `release/v0.x.x`) — main is protected
5. **Merge** after CI passes
6. **Tag on main**: `git checkout main && git pull && git tag v0.x.x`
7. **Push tag**: `git push origin v0.x.x`

That's it. Everything else is automated.

### The release commit is drift

`check` is a required status check on `main`, and it runs `cargo run -- check`.
Both `Cargo.toml` and `CHANGELOG.md` are nodes in drft's own graph, so steps 1
and 2 make them stale and CI fails on the release commit itself. Run `drft lock
Cargo.toml CHANGELOG.md` before committing — those two are the only nodes the
release touches, so the lock stays scoped to what the release actually changed.

`main` also requires branches to be up to date before merging, so a release
branch that falls behind needs `git merge origin/main` before it can land. If
that merge pulls in tracked changes, run `drft check` and lock the files it
reports.

## What happens automatically

When you push a version tag (`v*`):

1. **CI** (`ci.yml`) — runs fmt, clippy, tests
2. **Release** (`release.yml`) — cargo-dist builds binaries for macOS (arm64, x64), Linux (arm64, x64), and Windows (x64), then uploads to GitHub Releases
3. **Publish** (`publish.yml`) — after Release succeeds, publishes to crates.io and npm with the version from `Cargo.toml`

## Secrets required

Set these in GitHub repo settings (Settings > Secrets and variables > Actions):

- **`CARGO_REGISTRY_TOKEN`** — from https://crates.io/settings/tokens (needs publish-update scope)

npm uses **trusted publishing** (OIDC) — no token needed. Configure at https://www.npmjs.com/package/drft-cli/settings → add GitHub Actions as a trusted publisher (repo: `johnmdonahue/drft-cli`, workflow: `publish.yml`).

## Version numbers

- `Cargo.toml` `version` is the source of truth
- npm `package.json` version is synced automatically during publish
- Git tag must match: `v` + Cargo.toml version (e.g., `v0.2.0`)

## Quick example

```bash
# Edit Cargo.toml: version = "0.3.0"
# Edit CHANGELOG.md: add ## 0.3.0 section
git checkout -b release/v0.3.0
drft lock Cargo.toml CHANGELOG.md # both are tracked nodes; CI runs drft check
git commit -am "Release v0.3.0"
git push -u origin release/v0.3.0
gh pr create --title "Release v0.3.0"
# Merge PR, then:
git checkout main && git pull
git tag v0.3.0
git push origin v0.3.0
# Done. Go get coffee.
```
