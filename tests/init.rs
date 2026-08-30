mod common;
use common::drft_bin;
use std::fs;
use tempfile::TempDir;

#[test]
fn init_creates_config() {
    let dir = TempDir::new().unwrap();
    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "init"])
        .output()
        .unwrap();

    assert!(output.status.success(), "expected exit code 0");
    let config = fs::read_to_string(dir.path().join("drft.toml")).unwrap();
    assert!(config.contains("[rules]"));
    assert!(config.contains("stale"));
    // Template scaffolds graphs explicitly with the current field vocabulary.
    assert!(config.contains("[graphs.markdown]"));
    assert!(config.contains("parser ="));
    assert!(config.contains("files ="));
    // v0.7: no interface section in the template
    assert!(
        !config.contains("[interface]"),
        "init template should not emit [interface]"
    );
}

#[test]
fn init_fails_if_exists() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), "# existing").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "init"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2), "expected exit code 2");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("already exists"),
        "expected 'already exists' in stderr: {stderr}"
    );
}

/// The template is the first config most repos ever run, so it has to produce a
/// working graph rather than merely a parseable one. Asserting substrings of the
/// file cannot catch a template that declares a frontmatter graph tracking
/// nothing — only running the tree it produced can.
#[test]
fn the_init_template_produces_a_graph_that_emits_frontmatter_edges() {
    let dir = TempDir::new().unwrap();
    let init = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "init"])
        .output()
        .unwrap();
    assert!(init.status.success());

    fs::write(dir.path().join("target.md"), "# Target\n").unwrap();
    fs::write(
        dir.path().join("doc.md"),
        "---\nsources:\n  - ./target.md\n---\n\n# Doc\n",
    )
    .unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "edges"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("doc.md → target.md"),
        "the template's frontmatter graph emitted no edge: {stdout}"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("edge-keys-matched-nothing"),
        "the template's declared key must match the frontmatter it scaffolds: {stderr}"
    );
}
