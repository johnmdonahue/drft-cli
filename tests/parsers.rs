mod common;
use common::drft_bin;
use serde_json::Value;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

/// The `@frontmatter` occurrence lines a projection reports for `source`, in
/// order. Reads the edge itself rather than searching output for a word, so a
/// pass that stops extracting frontmatter edges fails here instead of passing on
/// a finding the `fs` graph raises anyway.
fn frontmatter_edge_lines(dir: &Path, source: &str, target: &str) -> Vec<u64> {
    let output = drft_bin()
        .args([
            "-C",
            dir.to_str().unwrap(),
            "--format",
            "json",
            "edges",
            source,
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "drft edges failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON");
    json["edges"]
        .as_array()
        .expect("edges array")
        .iter()
        .filter(|e| e["source"] == source && e["target"] == target)
        .flat_map(|e| {
            e["metadata"]["@frontmatter"]["occurrences"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .map(|o| o["line"].as_u64().expect("occurrence line"))
        .collect()
}

/// Frontmatter link-target values become edges that participate in staleness:
/// editing the linked file makes the *declaring* file's edge stale.
///
/// Both halves are asserted against the edge by name. Asserting that `check`
/// output contains "stale" passes on `stale-node` for the edited target, which
/// the `fs` graph raises with no frontmatter edge in the graph at all.
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

    assert_eq!(
        frontmatter_edge_lines(dir.path(), "analysis.md", "data/notes.md"),
        vec![3],
        "the `sources` entry on line 3 should yield a frontmatter edge"
    );

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
        stdout.contains("stale-edge]: analysis.md:3 \u{2192} data/notes.md"),
        "editing the target should make the declaring file's edge stale, got: {stdout}"
    );
}

/// A code span crossing a line boundary blanks to spaces *and newlines*, so every
/// entry below it keeps its real line. Blanking the newline too shortened the
/// masked block by a line and reported everything under the span one line high —
/// in `drft edges`, in `drft impact`, and in every finding's location.
#[test]
fn frontmatter_line_survives_a_multiline_code_span() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "[graphs.frontmatter]\nparser = \"frontmatter\"\nfiles = [\"**/*.md\"]\nkeys = [\"sources\"]\n",
    )
    .unwrap();
    fs::write(dir.path().join("target.md"), "# Target\n").unwrap();
    // `./target.md` is on line 5. The span opens on line 2 and closes on line 3.
    fs::write(
        dir.path().join("doc.md"),
        "---\nnote: \"a span like `one\n  two` wrapping two lines\"\nsources:\n  - ./target.md\n---\nbody\n",
    )
    .unwrap();

    assert_eq!(
        frontmatter_edge_lines(dir.path(), "doc.md", "target.md"),
        vec![5],
        "the entry under a two-line code span should report its own line"
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
