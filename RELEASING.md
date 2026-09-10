# Releasing drft

## The process

1. **Review the release range** from the latest version tag through remote `main`. Confirm that the changelog covers every user-visible change and that the release version does not already exist in GitHub Releases, crates.io, or npm.
2. **Verify publishing prerequisites**. Confirm that the repository has a `CARGO_REGISTRY_TOKEN` Actions secret and that npm trusted publishing names this repository and `publish.yml`. Do not expose the secret value. Stop before tagging if either prerequisite cannot be established.
3. **Bump the version** in `Cargo.toml` and update the `drft-cli` entry in `Cargo.lock`. `Cargo.toml` is the version source for both registries; do not bump the checked-in npm manifest.
4. **Update `CHANGELOG.md`** with the new version and date.
5. **Open a PR** from a release branch such as `release/v0.x.x`. Main is protected.
6. **Verify the final PR commit**. Require every CI job and the cargo-dist Release plan to pass. Confirm that a locked Cargo read leaves the tree unchanged, cargo-dist's targets match the archive names in `npm/install.js`, and the publish workflow's npm version sync produces the release version in both `version` and `binaryVersion`.
7. **Merge the reviewed PR** and record its merge commit. Update local `main`, then confirm that remote `main` and the intended release commit still match.
8. **Tag that exact commit** and verify the tag before pushing it:

   ```bash
   git tag v0.x.x <release-commit>
   test "$(git rev-parse v0.x.x)" = "<release-commit>"
   git push origin v0.x.x
   ```

9. **Monitor publication**. Require the tag-triggered Release workflow and the downstream Publish workflow to complete successfully. The Publish workflow must contain successful `publish-crates` and `publish-npm` jobs for the release commit.
10. **Verify every public surface**. Confirm that the Git tag, GitHub Release, crates.io, and npm serve the same version from the release commit. Check the GitHub Release's platform archives, per-archive checksums, aggregate checksum, source archive, and dist manifest against `dist-workspace.toml` and `npm/install.js`. Download the assets and verify their checksums.
11. **Smoke-test an installed artifact**. Install the published npm package in a temporary directory, run `drft --version`, and run one command against a repository. This exercises npm, its postinstall downloader, the GitHub archive, and the binary together.
12. **Finish cleanly**. Confirm that the public checkout and any private release workspace contain no uncommitted release changes.

`main` also requires branches to be up to date before merging, so a release branch that falls behind needs `git merge origin/main` before it can land.

### drft does not gate the release

Neither the config nor the lockfile is tracked, so CI has no graph to check and a release commit cannot fail on drift. Bumping `Cargo.toml` and `CHANGELOG.md` still makes them stale in a maintainer's local graph — `drft lock Cargo.toml CHANGELOG.md` clears that, and it is bookkeeping on one machine rather than a step the release depends on.

## What happens automatically

CI (`ci.yml`) runs on pushes to `main` and pull requests targeting `main`. It checks Rust and document formatting, clippy, tests, and benchmark compilation. Require release-branch CI to pass before merging; version tags do not trigger CI.

When you push a version tag (`v*`):

1. **Release** (`release.yml`) — cargo-dist builds binaries for macOS (arm64, x64), Linux (arm64, x64), and Windows (x64), then uploads to GitHub Releases
2. **Publish** (`publish.yml`) — after a tag-push Release succeeds, publishes to crates.io and npm with the version from `Cargo.toml`

The two Publish jobs are independent and publish immutable versions. If one registry succeeds and the other fails, rerun only the failed job after correcting its cause. Do not rerun the successful publish.

Release also runs on pull requests without publishing.

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
# Update the drft-cli version in Cargo.lock to match
# Edit CHANGELOG.md: add ## 0.3.0 section
git checkout -b release/v0.3.0
drft lock Cargo.toml CHANGELOG.md # local bookkeeping only — see above
git commit -am "Release v0.3.0"
git push -u origin release/v0.3.0
gh pr create --title "Release v0.3.0"
# Merge PR, then:
git checkout main && git pull
RELEASE_COMMIT=$(git rev-parse HEAD)
git tag v0.3.0 "$RELEASE_COMMIT"
test "$(git rev-parse v0.3.0)" = "$RELEASE_COMMIT"
git push origin v0.3.0
gh run list --workflow release.yml --branch v0.3.0
# Wait for Release and Publish, then verify the release assets and registries.
```
