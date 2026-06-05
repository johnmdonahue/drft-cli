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
