mod common;
use common::drft_bin;
use std::fs;
use tempfile::TempDir;

/// Frontmatter link-target values become edges that participate in staleness:
/// editing the linked file makes the linker's edge stale.
#[test]
fn frontmatter_sources_create_edges() {
    let dir = TempDir::new().unwrap();
    // Declare the markdown and frontmatter graphs.
    fs::write(dir.path().join("drft.toml"), common::DEFAULT_CONFIG).unwrap();
    fs::write(
        dir.path().join("analysis.md"),
        "---\nsources:\n  - ./data/notes.md\n---\n\n# Analysis\n",
    )
    .unwrap();
    let data = dir.path().join("data");
    fs::create_dir(&data).unwrap();
    fs::write(data.join("notes.md"), "# Notes").unwrap();

    drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock", "--all"])
        .output()
        .unwrap();

    let lockfile = fs::read_to_string(dir.path().join("drft.lock")).unwrap();
    assert!(lockfile.contains("analysis.md"));
    assert!(lockfile.contains("data/notes.md"));

    // Edit the linked source; the frontmatter edge should go stale.
    fs::write(data.join("notes.md"), "# Notes (edited)").unwrap();
    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("stale"),
        "frontmatter dependency should trigger staleness, got: {stdout}"
    );
}

/// `keys` scoping drops path-shaped values under other keys while keeping the
/// finding that reports a typo'd source. That combination is the point: the
/// rule-level `ignore` workaround silences both, so it cannot express this.
#[test]
fn frontmatter_keys_scope_edges_without_hiding_broken_sources() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "[graphs.frontmatter]\nparser = \"frontmatter\"\nfiles = [\"**/*.md\"]\nkeys = [\"sources\"]\n",
    )
    .unwrap();
    fs::write(dir.path().join("real.md"), "# Real").unwrap();
    // `route` is an API route, not a file — a false edge under shape detection.
    // `sources` points at a file that does not exist — a genuine broken source.
    fs::write(
        dir.path().join("doc.md"),
        "---\nroute: /customers\nsources:\n  - ./missing.md\n---\n\n# Doc\n",
    )
    .unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        !stdout.contains("/customers"),
        "`route` is outside `keys` and must not yield an edge, got: {stdout}"
    );
    assert!(
        stdout.contains("unresolved-edge") && stdout.contains("missing.md"),
        "a broken `sources` path must still be reported, got: {stdout}"
    );
}
