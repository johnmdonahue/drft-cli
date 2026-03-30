mod common;
use common::drft_bin;
use std::fs;
use tempfile::TempDir;

#[test]
fn frontmatter_sources_create_edges() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("analysis.md"),
        "---\nsources:\n  - ./data/notes.md\n---\n\n# Analysis\n",
    )
    .unwrap();
    let data = dir.path().join("data");
    fs::create_dir(&data).unwrap();
    fs::write(data.join("notes.md"), "# Notes").unwrap();

    // Lock and verify the edge exists
    drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock"])
        .output()
        .unwrap();

    let lockfile = fs::read_to_string(dir.path().join("drft.lock")).unwrap();
    assert!(lockfile.contains("analysis.md"));
    assert!(lockfile.contains("data/notes.md"));
    // v2 lockfile has no edges — edge types verified at check time

    // Edit the source, check for staleness
    fs::write(data.join("notes.md"), "# Notes (edited)").unwrap();
    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("stale"),
        "frontmatter dep should trigger staleness, got: {stdout}"
    );
}

#[test]
fn wikilinks_create_edges() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("index.md"), "See [[setup]] for details.").unwrap();
    fs::write(dir.path().join("setup.md"), "# Setup").unwrap();

    // Lock and verify wikilink edge
    drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "lock"])
        .output()
        .unwrap();

    let lockfile = fs::read_to_string(dir.path().join("drft.lock")).unwrap();
    assert!(lockfile.contains("setup.md"));
    // v2 lockfile has no edges — edge types verified at check time

    // Broken wikilink should be caught
    fs::write(dir.path().join("index.md"), "See [[missing]] here.").unwrap();
    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("broken-link"),
        "broken wikilink should fire broken-link, got: {stdout}"
    );
}
