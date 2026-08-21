mod common;
use common::{DEFAULT_CONFIG, drft_bin};
use std::fs;
use tempfile::TempDir;

/// A link to an existing directory resolves (directories are nodes); a link to a
/// missing directory is flagged `unresolved-edge`.
#[test]
fn directory_links_resolve() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), DEFAULT_CONFIG).unwrap();
    fs::create_dir(dir.path().join("guides")).unwrap();
    fs::write(dir.path().join("guides/intro.md"), "# Intro").unwrap();
    fs::write(
        dir.path().join("index.md"),
        "[the guides](guides/) and [gone](missing/)",
    )
    .unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("unresolved-edge") && stdout.contains("missing"),
        "link to a missing directory should be unresolved, got: {stdout}"
    );
    assert!(
        !stdout.contains("index.md → guides"),
        "link to an existing directory should resolve, got: {stdout}"
    );
}

/// A per-rule `ignore` glob suppresses diagnostics for matching subjects while
/// leaving others flagged.
#[test]
fn ignore_glob_suppresses_diagnostics() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "[rules.detached-node]\nignore = [\"README.md\"]\n",
    )
    .unwrap();
    fs::write(dir.path().join("README.md"), "# Readme").unwrap();
    fs::write(dir.path().join("other.md"), "# Other").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("README.md"),
        "README.md should be suppressed by the ignore glob, got: {stdout}"
    );
    assert!(
        stdout.contains("other.md"),
        "other.md should still be flagged, got: {stdout}"
    );
}

/// A link's `#fragment` is checked against the anchors its target defines: an
/// anchor that exists is quiet, one that does not fires `unresolved-fragment` on
/// the line that cites it, and a case-only mismatch names the anchor it meant.
#[test]
fn fragments_are_checked_against_the_targets_anchors() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), DEFAULT_CONFIG).unwrap();
    fs::write(
        dir.path().join("owners.md"),
        "# Owners\n\n## security-console\n\nAna.\n\n## ngwaf-edge\n\nBo.\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("work-items.md"),
        "# Work\n\n\
         - [W-01](./owners.md#security-console)\n\
         - [W-02](./owners.md#NGWAF-Edge)\n\
         - [W-03](./owners.md#no-such-team)\n\
         - [W-04](./owners.md)\n",
    )
    .unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        !stdout.contains("#security-console"),
        "an anchor the target defines is quiet, got: {stdout}"
    );
    assert!(
        stdout.contains("work-items.md:5 → owners.md#no-such-team"),
        "a missing anchor fires on its own line, got: {stdout}"
    );
    assert!(
        stdout.contains("work-items.md:4 → owners.md#NGWAF-Edge")
            && stdout.contains("differs only in case from `#ngwaf-edge`"),
        "a case-only mismatch names the anchor it meant, got: {stdout}"
    );
}

/// A fragment into a target no parser read as a document is unknown, not broken:
/// nothing published an anchor list for it, so drft cannot say the anchor is
/// missing and does not guess.
#[test]
fn a_fragment_into_an_unread_target_is_not_flagged() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), DEFAULT_CONFIG).unwrap();
    fs::write(dir.path().join("lib.rs"), "pub fn go() {}\n").unwrap();
    fs::write(
        dir.path().join("guide.md"),
        "# Guide\n\nSee [go](lib.rs#L1).\n",
    )
    .unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("unresolved-fragment"),
        "a non-markdown target's fragments are unknown, got: {stdout}"
    );
}
