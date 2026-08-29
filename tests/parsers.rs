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

/// The shift is one line per newline the span swallowed, not one line. A fix that
/// restores only the span's first newline reports the right line for a two-line
/// span and the wrong one for anything taller, and passes a suite that only ever
/// asks about two.
#[test]
fn frontmatter_line_survives_a_code_span_taller_than_two_lines() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "[graphs.frontmatter]\nparser = \"frontmatter\"\nfiles = [\"**/*.md\"]\nkeys = [\"sources\"]\n",
    )
    .unwrap();
    fs::write(dir.path().join("target.md"), "# Target\n").unwrap();
    // The span opens on line 2 and closes on line 5, so `./target.md` is line 7.
    fs::write(
        dir.path().join("doc.md"),
        "---\nnote: \"a span like `one\n  two\n  three\n  four` wrapping four lines\"\nsources:\n  - ./target.md\n---\nbody\n",
    )
    .unwrap();

    assert_eq!(
        frontmatter_edge_lines(dir.path(), "doc.md", "target.md"),
        vec![7],
        "a four-line span shifts by three lines, not by one"
    );
}

/// Blanking a span's newlines is also what lets some blocks parse at all: fusing
/// the lines hides a construct that would otherwise break the mapping. The edge
/// scan reads that same mask, so a block reaching frontmatter only through it
/// still yields its declared edges — and still reports their real lines, because
/// the correction is a table rather than a second mask.
///
/// Masking this block any other way produces no edge, no `stale-edge`, and
/// `drft impact target.md` reporting no dependents, while the file still plainly
/// declares it. Under a config gating on `stale-edge` that turns a failing check
/// into a passing one.
#[test]
fn frontmatter_edges_survive_a_block_that_parses_only_when_spans_fuse() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "[graphs.frontmatter]\nparser = \"frontmatter\"\nfiles = [\"**/*.md\"]\nkeys = [\"sources\"]\n",
    )
    .unwrap();
    fs::write(dir.path().join("target.md"), "# Target\n").unwrap();
    fs::write(
        dir.path().join("doc.md"),
        "---\nstatus `ok\nfine` note: value\nsources:\n  - ./target.md\n---\nbody\n",
    )
    .unwrap();

    // The span fuses the block's first two lines, so `./target.md` is line 5 and
    // an uncorrected mask reports 4.
    assert_eq!(
        frontmatter_edge_lines(dir.path(), "doc.md", "target.md"),
        vec![5],
        "a declared `sources` entry must yield an edge, at its own line, when the \
         block reaches frontmatter only through the line-collapsing mask"
    );
}

/// The mask blanks a span to spaces, and a span *inside* a link value is part of
/// that value's text — `collect_links` reads the scalar out of the masked copy.
/// So the mask decides the edge target, not only where the target was found.
///
/// Masking with the span's newlines kept would fold them to a single space
/// instead, changing the target string, the node it resolves to, the lockfile
/// entry, and — where the two spellings name different files — the exit code. The
/// blanked width is what pins that: five characters of span become five spaces.
#[test]
fn a_code_span_inside_a_link_value_blanks_to_its_own_width() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "[graphs.frontmatter]\nparser = \"frontmatter\"\nfiles = [\"**/*.md\"]\nkeys = [\"sources\"]\n",
    )
    .unwrap();
    // The span is "`a\nb`" — five characters, one of them a newline.
    fs::write(
        dir.path().join("doc.md"),
        "---\nsources: ./`a\nb`target.md\n---\nbody\n",
    )
    .unwrap();

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "--format",
            "json",
            "edges",
            "doc.md",
        ])
        .output()
        .unwrap();
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON");
    let raw = json["edges"][0]["metadata"]["@frontmatter"]["occurrences"][0]["raw"]
        .as_str()
        .expect("occurrence raw");
    assert_eq!(
        raw, "./     target.md",
        "the span's five characters blank to five spaces; folding them to one \
         would move the edge to a different target"
    );
}

/// The same mask decides whether a block *is* frontmatter, for this parser's
/// metadata and for the markdown parser's mask. Masking with newlines kept there
/// drops such a block out of frontmatter entirely, and the markdown parser then
/// reads it as body: a setext heading slugged from the frontmatter text, and any
/// link inside it lifted into the graph.
#[test]
fn a_block_parsing_only_through_the_fused_mask_stays_frontmatter() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "[graphs.markdown]\nparser = \"markdown\"\nfiles = [\"**/*.md\"]\n\n[graphs.frontmatter]\nparser = \"frontmatter\"\nfiles = [\"**/*.md\"]\nkeys = [\"sources\"]\n",
    )
    .unwrap();
    fs::write(dir.path().join("target.md"), "# Target\n").unwrap();
    fs::write(dir.path().join("decoy.md"), "# Decoy\n").unwrap();
    fs::write(
        dir.path().join("doc.md"),
        "---\nstatus `ok\nfine` note: \"see [d](./decoy.md)\"\nsources: \"./target.md\"\n---\n\n# Heading\n",
    )
    .unwrap();

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "--format",
            "json",
            "nodes",
            "doc.md",
        ])
        .output()
        .unwrap();
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON");
    let metadata = &json["nodes"][0]["metadata"];

    assert!(
        metadata.get("@frontmatter").is_some(),
        "the block should still be frontmatter, got: {metadata}"
    );
    assert_eq!(
        metadata["@markdown"]["anchors"],
        serde_json::json!(["heading"]),
        "only the body heading defines an anchor; slugging the frontmatter text \
         publishes an address the file does not answer to"
    );
    assert!(
        frontmatter_edge_lines(dir.path(), "doc.md", "decoy.md").is_empty(),
        "a link inside frontmatter is not a body link"
    );
    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "--format",
            "json",
            "edges",
            "doc.md",
        ])
        .output()
        .unwrap();
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON");
    let targets: Vec<&str> = json["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["target"].as_str().unwrap())
        .collect();
    assert_eq!(
        targets,
        vec!["target.md"],
        "the declared source is the only edge; `decoy.md` sits inside frontmatter"
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
