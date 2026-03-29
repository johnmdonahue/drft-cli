# Releasing drft

## The process

1. **Bump version** in `Cargo.toml`
2. **Update CHANGELOG.md** with the new version and date
3. **Commit**: `git commit -am "Release v0.x.x"`
4. **Tag**: `git tag v0.x.x`
5. **Push**: `git push && git push origin v0.x.x`

That's it. Everything else is automated.

## What happens automatically

When you push a version tag (`v*`):

1. **CI** (`ci.yml`) — runs fmt, clippy, tests
2. **Release** (`release.yml`) — cargo-dist builds binaries for macOS (arm64, x64), Linux (arm64, x64), and Windows (x64), then uploads to GitHub Releases
3. **Publish** (`publish.yml`) — after Release succeeds, publishes to crates.io and npm with the version from `Cargo.toml`

## Secrets required

Set these in GitHub repo settings (Settings > Secrets > Actions):

- **`CARGO_REGISTRY_TOKEN`** — from https://crates.io/settings/tokens (needs publish-update scope)
- **`NPM_TOKEN`** — from https://www.npmjs.com/settings/tokens (create an **Automation** token — this bypasses 2FA for CI)

## Version numbers

- `Cargo.toml` `version` is the source of truth
- npm `package.json` version is synced automatically during publish
- Git tag must match: `v` + Cargo.toml version (e.g., `v0.2.0`)

## Quick example

```bash
# Edit Cargo.toml: version = "0.2.0"
# Edit CHANGELOG.md: add ## 0.2.0 section
git commit -am "Release v0.2.0"
git tag v0.2.0
git push && git push origin v0.2.0
# Done. Go get coffee.
```
