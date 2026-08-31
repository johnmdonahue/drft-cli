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

/// Every `@frontmatter` edge target recorded for `source`.
///
/// Reads the targets rather than probing a guessed list of them, so a value drft
/// records under a spelling the test did not anticipate still counts as linked.
fn frontmatter_edge_targets(dir: &Path, source: &str) -> Vec<String> {
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
        .filter(|e| e["source"] == source)
        .filter(|e| e["metadata"]["@frontmatter"].is_object())
        .filter_map(|e| e["target"].as_str().map(str::to_string))
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
        "[graphs.frontmatter]\nparser = \"frontmatter\"\nfiles = [\"**/*.md\"]\nedge_keys = [\"sources\"]\n",
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
        "[graphs.frontmatter]\nparser = \"frontmatter\"\nfiles = [\"**/*.md\"]\nedge_keys = [\"sources\"]\n",
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
        "[graphs.frontmatter]\nparser = \"frontmatter\"\nfiles = [\"**/*.md\"]\nedge_keys = [\"sources\"]\n",
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

/// A value can sit *after* a multi-line span closes, sharing a masked line with
/// the span's opening. Its own line is where the span closed, not where it opened,
/// and the mask did not touch it — so a correction that maps a whole masked line
/// to where it began reports the wrong line for a value that is perfectly intact.
#[test]
fn a_value_after_a_span_closes_reports_the_closing_line() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "[graphs.frontmatter]\nparser = \"frontmatter\"\nfiles = [\"**/*.md\"]\nedge_keys = [\"note\"]\n",
    )
    .unwrap();
    fs::write(dir.path().join("target.md"), "# Target\n").unwrap();
    // The span opens on line 2 and closes on line 4, where the value also sits.
    fs::write(
        dir.path().join("doc.md"),
        "---\nnote: `x\ny\nz` ./target.md\n---\nbody\n",
    )
    .unwrap();

    assert_eq!(
        frontmatter_edge_lines(dir.path(), "doc.md", "target.md"),
        vec![4],
        "the value shares a masked line with the span's opening, but sits after \
         its close"
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
        "[graphs.frontmatter]\nparser = \"frontmatter\"\nfiles = [\"**/*.md\"]\nedge_keys = [\"sources\"]\n",
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

/// The code mask decides whether a claimed block yields metadata. Masking with
/// newlines kept there drops such a block out of frontmatter entirely — the block
/// is still claimed by its fences, so the markdown parser withholds its text
/// either way, but the file loses its declared keys and raises
/// `unreadable-frontmatter` instead of contributing them.
#[test]
fn a_block_parsing_only_through_the_fused_mask_stays_frontmatter() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "[graphs.markdown]\nparser = \"markdown\"\nfiles = [\"**/*.md\"]\n\n[graphs.frontmatter]\nparser = \"frontmatter\"\nfiles = [\"**/*.md\"]\nedge_keys = [\"sources\"]\n",
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

/// `edge_keys` scoping drops path-shaped values under other keys while keeping the
/// finding that reports a typo'd source. That combination is the point: the
/// rule-level `ignore` workaround silences both, so it cannot express this.
#[test]
fn frontmatter_edge_keys_scope_edges_without_hiding_broken_sources() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "[graphs.frontmatter]\nparser = \"frontmatter\"\nfiles = [\"**/*.md\"]\nedge_keys = [\"sources\"]\n",
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
        "`route` is outside `edge_keys` and must not yield an edge, got: {stdout}"
    );
    assert!(
        stdout.contains("unresolved-edge") && stdout.contains("missing.md"),
        "a broken `sources` path must still be reported, got: {stdout}"
    );
}

/// A leading byte-order mark used to cost a file its frontmatter entirely: the
/// block opens with `---` at offset 0 and the BOM sits ahead of it, so no parser
/// recognized the block. The file lost its metadata and its declared edges, the
/// markdown parser was handed the block as body text, and nothing reported any of
/// it — `detached-node` at exit 0, indistinguishable from a file that declared
/// nothing.
#[test]
fn a_byte_order_mark_does_not_cost_a_file_its_frontmatter() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "[graphs.frontmatter]\nparser = \"frontmatter\"\nfiles = [\"**/*.md\"]\nedge_keys = [\"sources\"]\n",
    )
    .unwrap();
    fs::write(dir.path().join("target.md"), "# Target\n").unwrap();
    fs::write(
        dir.path().join("bom.md"),
        "\u{feff}---\nsources:\n  - target.md\n---\n# Doc\n",
    )
    .unwrap();

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "--format",
            "json",
            "edges",
            "bom.md",
        ])
        .output()
        .unwrap();
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON");
    let edges = json["edges"].as_array().expect("edges array");
    assert_eq!(edges.len(), 1, "expected one edge, got: {json}");
    assert_eq!(edges[0]["target"], "target.md");
    assert_eq!(
        edges[0]["metadata"]["@frontmatter"]["occurrences"][0]["line"],
        serde_json::json!(3),
        "the declared source must yield an edge, at its own line"
    );
}

/// The mark is stripped from the decoded text, not from the bytes drft hashes.
///
/// Stripping in the `fs` source instead would move every mark-carrying file's
/// hash — reporting `stale-node` on files nobody edited, and stopping drft's
/// `b3:` from being the file's blake3. Two files identical but for the mark
/// therefore still hash differently, which is what rules the `fs` source out.
/// Ruling out the *frontmatter parser* is a different test, below: that location
/// cannot reach a file which has no frontmatter at all.
#[test]
fn a_byte_order_mark_is_still_hashed() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "[graphs.frontmatter]\nparser = \"frontmatter\"\nfiles = [\"**/*.md\"]\n",
    )
    .unwrap();
    fs::write(dir.path().join("plain.md"), "---\ntitle: t\n---\nbody\n").unwrap();
    fs::write(
        dir.path().join("bom.md"),
        "\u{feff}---\ntitle: t\n---\nbody\n",
    )
    .unwrap();

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "--format",
            "json",
            "nodes",
        ])
        .output()
        .unwrap();
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON");
    let hash = |id: &str| -> String {
        json["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["id"] == id)
            .unwrap()["metadata"]["@fs"]["hash"]
            .as_str()
            .unwrap()
            .to_string()
    };
    assert_ne!(
        hash("bom.md"),
        hash("plain.md"),
        "the mark is part of the file, so it is part of the hash"
    );

    // And both files parse to the same frontmatter, which is the point of
    // stripping it from the text.
    let frontmatter = |id: &str| -> Value {
        json["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["id"] == id)
            .unwrap()["metadata"]["@frontmatter"]
            .clone()
    };
    assert_eq!(frontmatter("bom.md"), frontmatter("plain.md"));
}

/// A block drft fails to recognize is handed to the markdown parser as body
/// text, and a single-paragraph block followed by its closing `---` is a setext
/// heading — so the file publishes an address it does not answer to, which a link
/// written to it then passes `unresolved-fragment` against.
///
/// Recognizing the block claims it, and the fabricated anchor goes with it. The
/// general case, where any block-recognition failure does this, is its own defect.
#[test]
fn a_byte_order_mark_does_not_fabricate_a_setext_anchor() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "[graphs.markdown]\nparser = \"markdown\"\nfiles = [\"**/*.md\"]\n\n[graphs.frontmatter]\nparser = \"frontmatter\"\nfiles = [\"**/*.md\"]\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("bom.md"),
        "\u{feff}---\npurpose: a title\n---\nbody\n",
    )
    .unwrap();

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "--format",
            "json",
            "nodes",
            "bom.md",
        ])
        .output()
        .unwrap();
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON");
    assert_eq!(
        json["nodes"][0]["metadata"]["@markdown"]["anchors"],
        serde_json::json!([]),
        "the frontmatter block is claimed, so it defines no heading"
    );
}

/// A mark costs a file its headings, so the strip cannot live in the frontmatter
/// parser: whatever that parser does to its own copy of the text, the markdown
/// parser still receives the text with the mark on the front and still loses the
/// first heading to it.
///
/// This is the plain reason that location is wrong and the one that shows up on
/// ordinary documents. The offset argument — that skipping the mark's bytes
/// without adjusting the block's end offset leaves the closing fence partly
/// unmasked — is also true, and takes a constructed file to exhibit.
#[test]
fn a_mark_on_a_file_with_no_frontmatter_still_costs_it_its_headings() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "[graphs.markdown]\nparser = \"markdown\"\nfiles = [\"**/*.md\"]\n",
    )
    .unwrap();
    fs::write(dir.path().join("doc.md"), "\u{feff}# Only a heading\n").unwrap();

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
    assert_eq!(
        json["nodes"][0]["metadata"]["@markdown"]["anchors"],
        serde_json::json!(["only-a-heading"]),
        "the heading is the file's first line once the mark is gone"
    );
}

/// A tool re-marking an already-marked file writes two marks, and stripping one
/// would leave that file failing exactly as it did before — same silent loss, same
/// absence of any finding. Accommodating one mark and dropping a file with two is
/// the incoherent position, so every leading mark goes.
#[test]
fn more_than_one_leading_mark_is_stripped() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "[graphs.frontmatter]\nparser = \"frontmatter\"\nfiles = [\"**/*.md\"]\nedge_keys = [\"sources\"]\n",
    )
    .unwrap();
    fs::write(dir.path().join("target.md"), "# Target\n").unwrap();
    fs::write(
        dir.path().join("doc.md"),
        "\u{feff}\u{feff}---\nsources:\n  - target.md\n---\n# Doc\n",
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
    let edges = json["edges"].as_array().expect("edges array");
    assert_eq!(edges.len(), 1, "expected one edge, got: {json}");
    assert_eq!(edges[0]["target"], "target.md");
}

/// The strip removes marks and nothing else.
///
/// Every other test here asserts that something *is* removed, which leaves the
/// boundary pinned in one direction only — a refactor generalizing the trim to
/// "leading whitespace and zero-width characters" would land green while turning
/// documents that are not frontmatter into frontmatter. A space before the
/// opening fence is the cheap case: `--- ` is not a frontmatter opener, and a
/// mark ahead of the space does not make it one.
#[test]
fn only_marks_are_stripped() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "[graphs.frontmatter]\nparser = \"frontmatter\"\nfiles = [\"**/*.md\"]\nedge_keys = [\"sources\"]\n",
    )
    .unwrap();
    fs::write(dir.path().join("target.md"), "# Target\n").unwrap();
    fs::write(
        dir.path().join("doc.md"),
        "\u{feff} ---\nsources:\n  - target.md\n---\nbody\n",
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
    assert!(
        json["nodes"][0]["metadata"].get("@frontmatter").is_none(),
        "a space before the opening fence is not frontmatter, mark or no mark"
    );
}

/// The mark strip and the line correction compose.
///
/// They landed as separate changes reviewed separately against the same base, so
/// their combination was never read by either review. A mark is removed from the
/// front of the text before any parser sees it, and removing bytes at offset 0
/// removes no newline — so a value below a multi-line code span reports the same
/// line whether or not the file carries a mark.
#[test]
fn a_mark_does_not_shift_the_line_a_span_corrects() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "[graphs.frontmatter]\nparser = \"frontmatter\"\nfiles = [\"**/*.md\"]\nedge_keys = [\"sources\"]\n",
    )
    .unwrap();
    fs::write(dir.path().join("target.md"), "# Target\n").unwrap();
    let block =
        "---\nnote: \"a span `one\n  two` wrapping\"\nsources:\n  - ./target.md\n---\nbody\n";
    fs::write(dir.path().join("plain.md"), block).unwrap();
    fs::write(dir.path().join("marked.md"), format!("\u{feff}{block}")).unwrap();

    // `./target.md` is on line 5 of both files.
    assert_eq!(
        frontmatter_edge_lines(dir.path(), "marked.md", "target.md"),
        vec![5]
    );
    assert_eq!(
        frontmatter_edge_lines(dir.path(), "marked.md", "target.md"),
        frontmatter_edge_lines(dir.path(), "plain.md", "target.md"),
        "a mark changes nothing about which line a value is reported on"
    );
}

/// A config scoping edges to `sources`, so the fixtures below exercise the
/// declared-key path a corpus actually uses rather than shape detection.
const SCOPED_CONFIG: &str = "\
[graphs.frontmatter]
parser = \"frontmatter\"
files = [\"**/*.md\"]
edge_keys = [\"sources\"]
";

/// Write a scoped-config fixture whose frontmatter is `block`, alongside the
/// `target.md` its `sources` entry names.
fn scoped_fixture(block: &str) -> TempDir {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), SCOPED_CONFIG).unwrap();
    fs::write(dir.path().join("doc.md"), block).unwrap();
    fs::write(dir.path().join("target.md"), "# Target\n").unwrap();
    // A second resolvable file, so a fixture can put a hostile value beside a
    // working one — the shape where losing the hostile value leaves the node
    // connected and raises no `detached-node` to notice it by.
    fs::write(dir.path().join("other.md"), "# Other\n").unwrap();
    dir
}

/// The `@frontmatter` metadata a projection reports for `source`.
fn frontmatter_metadata(dir: &Path, source: &str) -> Value {
    let output = drft_bin()
        .args([
            "-C",
            dir.to_str().unwrap(),
            "--format",
            "json",
            "nodes",
            source,
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "drft nodes failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).expect("valid JSON");
    json["nodes"][0]["metadata"]["@frontmatter"].clone()
}

/// An unclosed fence inside a block scalar latches the mask's fenced pass, which
/// blanks every line below it — `sources:` included. The raw block is well-formed
/// YAML throughout, so metadata reported the derivation while the edge scan found
/// none and `detached-node` was the only thing said about the file.
///
/// Nothing fails to parse here, so no parse-failure diagnostic reaches this: the
/// edge scan has to read the block the metadata read.
#[test]
fn a_fence_inside_a_block_scalar_keeps_the_blocks_edges() {
    let dir = scoped_fixture("---\nnote: |\n  ```\n  code\nsources:\n  - ./target.md\n---\nbody\n");

    assert_eq!(
        frontmatter_edge_lines(dir.path(), "doc.md", "target.md"),
        vec![6],
        "the `sources` entry on line 6 should yield an edge despite the fence above it"
    );
}

/// The mask's inline pass pairs backticks across lines, so one written as a
/// literal character in two separate values blanks everything between them. The
/// raw block parses — a backtick is only a reserved indicator at the *start* of a
/// scalar — so the same contradiction appears by a second route.
#[test]
fn stray_backticks_in_two_values_keep_the_blocks_edges() {
    let dir = scoped_fixture(
        "---\na: one ` two\nsources:\n  - ./target.md\nb: three ` four\n---\nbody\n",
    );

    assert_eq!(
        frontmatter_edge_lines(dir.path(), "doc.md", "target.md"),
        vec![4],
        "the `sources` entry on line 4 should yield an edge despite the paired backticks"
    );
}

/// The masked copy is still read, and is still the only thing that recovers a
/// block the raw parse rejects. A value *beginning* with a backtick is invalid
/// YAML — the character is a reserved indicator there — so the raw parse fails and
/// the mask is what keeps the sibling `sources:` entry structured.
///
/// The line is corrected against the mask's table, so it is the file's line rather
/// than the shortened masked block's.
#[test]
fn a_block_only_the_mask_can_parse_still_yields_edges() {
    let dir = scoped_fixture("---\nnote: `unquoted span`\nsources:\n  - ./target.md\n---\nbody\n");

    assert_eq!(
        frontmatter_metadata(dir.path(), "doc.md")["note"],
        Value::Null,
        "the masked fallback blanks the span, which is what makes this the fallback path"
    );
    assert_eq!(
        frontmatter_edge_lines(dir.path(), "doc.md", "target.md"),
        vec![4],
        "the masked fallback should still yield the edge, at the file's line"
    );
}

/// The edge target is the value the metadata reports, backticks and all.
///
/// Reading the mask unconditionally made these two disagree in the quiet
/// direction as well as the loud one: a trailing code span blanks to spaces, YAML
/// strips them from a plain scalar, and the edge resolved to a path the file does
/// not declare while `@frontmatter` reported the value the author wrote. An
/// `unresolved-edge` naming the declared value is the honest reading, and it is
/// the same contradiction this module exists to remove.
#[test]
fn a_trailing_code_span_does_not_silently_clean_up_an_edge_target() {
    let dir = scoped_fixture("---\nsources: ./target.md `x`\n---\nbody\n");

    assert_eq!(
        frontmatter_metadata(dir.path(), "doc.md")["sources"],
        Value::String("./target.md `x`".into()),
        "the metadata reports the value as written"
    );
    assert_eq!(
        frontmatter_edge_lines(dir.path(), "doc.md", "target.md"),
        Vec::<u64>::new(),
        "the span is not silently dropped to manufacture a resolving target"
    );
    assert_eq!(
        frontmatter_edge_lines(dir.path(), "doc.md", "target.md `x`"),
        vec![2],
        "the edge records the declared value, which `unresolved-edge` then reports"
    );
}

/// A declared value keeps its edge when a code span sits inside it.
///
/// Reading the raw block is what stops a span blanking a sibling key, but the
/// candidacy test still has to see the value without its spans: `target.md` plus a
/// trailing span puts the backticks inside the extension test, which rejects them,
/// and the value is then neither an edge nor a finding. That is the same silent
/// drop the raw-first order exists to remove, arriving from the other side.
#[test]
fn a_span_inside_a_declared_value_does_not_cost_it_the_edge() {
    let dir = scoped_fixture("---\nsources: target.md`x`\n---\nbody\n");

    assert_eq!(
        frontmatter_metadata(dir.path(), "doc.md")["sources"],
        Value::String("target.md`x`".into()),
        "the metadata reports the value as written"
    );
    assert_eq!(
        frontmatter_edge_lines(dir.path(), "doc.md", "target.md`x`"),
        vec![2],
        "the value is a candidate, and the edge records it as declared"
    );
}

/// The same value inside a list, beside one that resolves — the shape where the
/// loss leaves no `detached-node` behind either, because the node keeps its other
/// edge and `check` output is byte-identical to a run that lost nothing.
#[test]
fn a_span_inside_one_list_value_does_not_cost_it_the_edge() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), SCOPED_CONFIG).unwrap();
    fs::write(
        dir.path().join("doc.md"),
        "---\nsources:\n  - ./other.md\n  - target.md`x`\n---\nbody\n",
    )
    .unwrap();
    fs::write(dir.path().join("other.md"), "# Other\n").unwrap();
    fs::write(dir.path().join("target.md"), "# Target\n").unwrap();

    assert_eq!(
        frontmatter_edge_lines(dir.path(), "doc.md", "other.md"),
        vec![3],
        "the plain entry keeps its edge"
    );
    assert_eq!(
        frontmatter_edge_lines(dir.path(), "doc.md", "target.md`x`"),
        vec![4],
        "the entry carrying a span keeps one too, rather than vanishing beside it"
    );
}

/// A lone carriage return is a line break to the YAML parser and not to the file,
/// so a raw block is not already numbered the way the file is. Reporting saphyr's
/// line unaltered moves every value below a stray `\r` — one line per return.
#[test]
fn a_lone_carriage_return_does_not_shift_the_reported_line() {
    let dir = scoped_fixture("---\nnote: \"a\r  b\"\nsources:\n  - ./target.md\n---\nbody\n");

    assert_eq!(
        frontmatter_edge_lines(dir.path(), "doc.md", "target.md"),
        vec![4],
        "the entry is on the file's line 4, which is what `grep -n` reports"
    );
}

/// Two returns, so an off-by-one and an off-by-N are told apart. A fix that
/// advanced the correction once per block rather than once per return passes the
/// single-return case above and fails this one.
#[test]
fn every_lone_carriage_return_shifts_the_line_it_would_report() {
    let dir = scoped_fixture(
        "---\nnote: \"one\r  two\r  three\"\nsources:\n  - ./target.md\n---\nbody\n",
    );

    assert_eq!(
        frontmatter_edge_lines(dir.path(), "doc.md", "target.md"),
        vec![4],
        "two returns above the entry still leave it on the file's line 4"
    );
}

/// The pass property, over every hostile block anyone has constructed: **a
/// path-shaped value under a declared key is an edge or a finding, never
/// silence.**
///
/// Asserted against `drft check` rather than against a list of expected edges,
/// because the failure this guards is the quiet one — an edge disappearing while
/// the output stays byte-identical to a run that lost nothing.
#[test]
fn no_hostile_block_drops_a_declared_value_in_silence() {
    // Each block declares `./target.md` or a value naming it. Whatever drft does
    // with the value, it may not do it silently.
    let blocks = [
        (
            "fence in a block scalar",
            "note: |\n  ```\n  code\nsources:\n  - ./target.md",
        ),
        (
            "backticks in two values",
            "a: one ` two\nsources:\n  - ./target.md\nb: three ` four",
        ),
        (
            "span before the key",
            "note: `unquoted span`\nsources:\n  - ./target.md",
        ),
        ("trailing span in the value", "sources: ./target.md `x`"),
        ("interior span in the value", "sources: target.md`x`"),
        ("leading span in the value", "sources: `x` ./target.md"),
        (
            "colon hidden in a span",
            "note: x `has: a colon`\nsources:\n  - ./target.md",
        ),
        (
            "lone carriage return above",
            "note: \"a\r  b\"\nsources:\n  - ./target.md",
        ),
        (
            "crlf throughout",
            "note: plain\r\nsources:\r\n  - ./target.md",
        ),
        (
            "span crossing two lines",
            "note: `one\ntwo`\nsources:\n  - ./target.md",
        ),
        (
            "tilde fence in a block scalar",
            "note: |\n  ~~~\n  code\nsources:\n  - ./target.md",
        ),
        (
            "value opening with a fence marker",
            "sources:\n  - \"```target.md\"",
        ),
        (
            "value opening with a tilde fence",
            "sources:\n  - \"~~~target.md\"",
        ),
        (
            "fence-marker value beside one that resolves",
            "sources:\n  - \"```target.md\"\n  - ./other.md",
        ),
    ];

    let mut examined = 0;
    for (name, body) in blocks {
        let dir = scoped_fixture(&format!("---\n{body}\n---\nbody\n"));
        let meta = frontmatter_metadata(dir.path(), "doc.md");
        // Only blocks whose metadata still carries the declaration are in scope:
        // where the block itself is unreadable, the diagnostic rule owns it.
        let declared = meta["sources"].to_string().contains("target.md");
        if !declared {
            continue;
        }
        examined += 1;

        let output = drft_bin()
            .args(["-C", dir.path().to_str().unwrap(), "check"])
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let linked = frontmatter_edge_targets(dir.path(), "doc.md")
            .iter()
            .any(|t| t.contains("target.md"));
        // `detached-node` is deliberately not evidence. A file reports it *because*
        // the edge was lost, so accepting it lets the defect satisfy the assertion:
        // with this clause reading `|| stdout.contains("detached-node")`, this test
        // passed with the entire fix reverted while five others failed.
        let reported = stdout.contains("unresolved-edge");

        assert!(
            linked || reported,
            "`{name}`: `@frontmatter` carries {}, and drft produced neither an edge \
             naming it nor an `unresolved-edge` about it. check said:\n{stdout}",
            meta["sources"]
        );
    }

    // A skipped block asserts nothing, so the count is part of the property: an
    // edit that stops drft recognising these blocks would otherwise empty this
    // test while leaving it green.
    assert_eq!(
        examined,
        blocks.len(),
        "every block here should keep its declaration in `@frontmatter`; {} of {} did not, and asserted nothing",
        blocks.len() - examined,
        blocks.len()
    );
}

/// A CRLF document reports the file's line, which requires counting `\r\n` as one
/// break rather than two.
///
/// The raw path's line table is a copy of the mask's walk, and the two must agree
/// about what opens a line. Simplifying the test to `c == '\n' || c == '\r'` reads
/// as equivalent, drops the lookahead, opens two rows per CRLF line, and reports
/// this entry two lines high — with the whole suite green, until this fixture
/// asserted the line.
#[test]
fn a_crlf_document_reports_the_files_line() {
    let dir = scoped_fixture(
        "---\r\ntitle: t\r\nnote: plain\r\nsources:\r\n  - ./target.md\r\n---\r\nbody\r\n",
    );

    assert_eq!(
        frontmatter_edge_lines(dir.path(), "doc.md", "target.md"),
        vec![5],
        "`\\r\\n` is one line break, so the entry is on the file's line 5"
    );
}

/// Every string reachable through a declared key is an edge. A value naming
/// nothing that resolves becomes an edge that resolves to nothing, and
/// `unresolved-edge` reports it exactly as it reports a typo'd path.
///
/// Each value below was silently discarded before: no edge, no finding, no
/// record that the file had declared anything at all. The remedy is the reader's
/// — fix the value, fix the config, or move the field — and none of those is
/// available while the drop is invisible.
#[test]
fn prose_under_a_declared_key_is_reported_rather_than_dropped() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "[graphs.frontmatter]\nparser = \"frontmatter\"\nfiles = [\"**/*.md\"]\nedge_keys = [\"sources\"]\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("doc.md"),
        "---\nsources:\n  - TBD\n  - needs review\n---\n\n# Doc\n",
    )
    .unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    for value in ["TBD", "needs review"] {
        assert!(
            stdout.contains("unresolved-edge") && stdout.contains(value),
            "{value:?} must be reported, not dropped, got: {stdout}"
        );
    }
}

/// A value naming a directory resolves. drft has directory nodes, so a
/// derivation naming one is legitimate; the extension test discarded it for
/// having no dot, which is a defect the heuristic was hiding.
#[test]
fn a_declared_value_naming_a_directory_resolves() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "[graphs.frontmatter]\nparser = \"frontmatter\"\nfiles = [\"**/*.md\"]\nedge_keys = [\"sources\"]\n",
    )
    .unwrap();
    fs::create_dir(dir.path().join("notes")).unwrap();
    fs::write(dir.path().join("notes").join("a.md"), "# A\n").unwrap();
    fs::write(
        dir.path().join("doc.md"),
        "---\nsources: notes\n---\n\n# Doc\n",
    )
    .unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "edges"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("doc.md → notes"),
        "a directory value must resolve to its node, got: {stdout}"
    );

    let check = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();
    assert!(
        !String::from_utf8_lossy(&check.stdout).contains("unresolved-edge"),
        "the directory node exists, so nothing is unresolved"
    );
}

/// A value written as markdown link syntax is diagnosed, never unwrapped. The
/// finding names the literal text, which is the whole remedy signal: unwrapping
/// it would be inference about what the author meant.
#[test]
fn a_markdown_link_value_is_diagnosed_rather_than_unwrapped() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "[graphs.frontmatter]\nparser = \"frontmatter\"\nfiles = [\"**/*.md\"]\nedge_keys = [\"sources\"]\n",
    )
    .unwrap();
    fs::write(dir.path().join("real.md"), "# Real\n").unwrap();
    fs::write(
        dir.path().join("doc.md"),
        "---\nsources: \"[Design notes](real.md)\"\n---\n\n# Doc\n",
    )
    .unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("unresolved-edge") && stdout.contains("[Design notes](real.md)"),
        "the finding must name the literal value, got: {stdout}"
    );
}

/// `drft impact` renders a record per line, and both halves of it come from the
/// graph. Escaping one and not the other split a record across two lines, the
/// first carrying no `(via …, depth …, radius …)` suffix — the failure the
/// escaping exists to prevent, reached through a declared frontmatter value.
#[test]
fn an_impact_record_carrying_a_newline_stays_on_one_line() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "[graphs.frontmatter]\nparser = \"frontmatter\"\nfiles = [\"**/*.md\"]\nedge_keys = [\"sources\"]\n[rules.detached-node]\nseverity = \"off\"\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("doc.md"),
        "---\nsources: |\n  first line\n  second line\n---\n",
    )
    .unwrap();

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "impact",
            "doc.md",
            "--direction",
            "outbound",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
        assert!(
            line.contains("(via "),
            "every record carries its suffix, so none was split: {line:?}"
        );
    }
    assert!(
        stdout.contains("first line\\nsecond line"),
        "the newline is escaped rather than emitted: {stdout}"
    );
}

/// Both halves of an impact record come from the graph, and the second half is
/// only reachable at depth two: a chain whose middle hop is a file whose *name*
/// carries a newline. A markdown link cannot spell that, so the seed declares it
/// as a frontmatter value — which this change made possible. Escaping the
/// location and not the via left this record split.
#[test]
fn an_impact_record_escapes_the_via_as_well_as_the_location() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "[graphs.markdown]\nparser = \"markdown\"\nfiles = [\"**/*.md\"]\n\n[graphs.fm]\nparser = \"frontmatter\"\nfiles = [\"**/*.md\"]\nedge_keys = [\"sources\"]\n[rules.detached-node]\nseverity = \"off\"\n",
    )
    .unwrap();
    fs::write(dir.path().join("target.md"), "# T\n").unwrap();
    fs::write(dir.path().join("we\nird.md"), "[t](./target.md)\n").unwrap();
    fs::write(
        dir.path().join("doc.md"),
        "---\nsources: \"we\\nird.md\"\n---\n",
    )
    .unwrap();

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "impact",
            "doc.md",
            "--direction",
            "outbound",
            "--depth",
            "2",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
        assert!(
            line.contains("(via ") && line.contains("depth "),
            "every record carries its whole suffix: {line:?}"
        );
    }
    assert!(
        stdout.contains("via we\\nird.md"),
        "the via is escaped, not emitted raw: {stdout}"
    );
}

/// The two builders pass different link policies, and nothing pinned *which*.
/// The unit tests exercise `link_edges` with a policy handed to them, so swapping
/// the policy each builder passes left the whole suite green — twice, across two
/// review rounds. These two run through the binary, so the wiring is what is
/// under test rather than the function.
///
/// A body link naming only a fragment is an intra-file anchor. It names a
/// position in the file it sits in, so there is nothing to draw an edge to.
#[test]
fn the_markdown_builder_draws_no_edge_for_an_anchor_only_link() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "[graphs.markdown]\nparser = \"markdown\"\nfiles = [\"**/*.md\"]\n[rules.detached-node]\nseverity = \"off\"\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("doc.md"),
        "# Doc\n\n## Section\n\n[see](#section)\n",
    )
    .unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "edges"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.trim().is_empty(),
        "an anchor-only body link is not an edge: {stdout}"
    );
}

/// A frontmatter value cites another document — a provenance claim has no "this
/// file" form — so a value beginning with `#` names no document at all. It cannot
/// resolve, and it must not vanish either.
#[test]
fn the_frontmatter_builder_draws_an_edge_for_a_fragment_only_value() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        "[graphs.fm]\nparser = \"frontmatter\"\nfiles = [\"**/*.md\"]\nedge_keys = [\"sources\"]\n[rules.detached-node]\nseverity = \"off\"\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("doc.md"),
        "---\nsources: \"#overview\"\n---\n\n# Doc\n",
    )
    .unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "edges"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("doc.md → #overview"),
        "a declared fragment-only value is an edge that resolves to nothing: {stdout}"
    );

    let check = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&check.stdout).contains("unresolved-edge"),
        "and it is reported rather than dropped"
    );
}

/// One record is one line, across every command and both renderers.
///
/// This exists because the per-call-site version of this rule failed four times
/// in a row, always the same way: a `format!` interpolating two file-derived
/// values with one escaped and one not. Escaping is a property of the rendering
/// layer, and asserting it once over real output is the only check that does not
/// have to be remembered at every new call site.
///
/// **Three earlier versions of this test passed while asserting nothing**, which
/// is the failure mode to guard against when extending it:
///
/// - The first classified lines as record heads or indented continuations. An
///   orphan line satisfies "head", so it asserted nothing at all.
/// - The second injected a newline into an unquoted YAML key, which is invalid,
///   so the block did not parse and no value reached the output. A *quoted* key
///   carries one fine — the fixture below uses that, and the comment claiming the
///   channel was unreachable was wrong.
/// - The third used tails `"ird "` and `"ird("`, neither of which matches an
///   orphan line whose whole content is `ird` — so the namespace header, the very
///   site the commit adding this test had fixed, was uncovered.
///
/// Extending it means adding a channel to the fixture **and** proving the new
/// tail fails when its escape is reverted. A tail that never appears is a tail
/// that asserts nothing.
///
/// **What it covers, measured rather than assumed.** A mutation sweep over every
/// `one_line` call site found this catches the node id, both halves of an edge,
/// a finding's target, the `@namespace` header, a metadata key, the lock report's
/// locked list, a hint's locus, and `impact`'s location. It does **not** reach a
/// finding's cause, a hint's `next` (every one is a literal today), the dropped
/// list, `resolved-elsewhere`, or the not-found error's own interpolations —
/// several of those are pinned by their own tests nearby, and the rest are
/// recorded in the queue. This is a net, not a proof.
#[test]
fn no_command_splits_a_record_across_lines() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("drft.toml"),
        // The graph name reaches text output as the `@namespace` header. The
        // second graph declares a key nothing uses, which is what raises a hint —
        // without one, every hint render site is unreachable and untested.
        "[graphs.markdown]\nparser = \"markdown\"\nfiles = [\"**/*.md\"]\n\n         [graphs.\"we\\nird\"]\nparser = \"frontmatter\"\nfiles = [\"**/*.md\"]\nedge_keys = [\"sources\"]\n\n         [graphs.\"un\\nused\"]\nparser = \"frontmatter\"\nfiles = [\"**/*.md\"]\nedge_keys = [\"no\\nthing\"]\n",
    )
    .unwrap();
    // A broken link as well as a good one: the broken one makes this file an
    // edge finding's *subject*, which is the only way that escape is reachable.
    fs::write(
        dir.path().join("we\nird.md"),
        "# W\n\n[t](./target.md)\n\n[gone](./missing.md)\n",
    )
    .unwrap();
    fs::write(dir.path().join("target.md"), "# T\n").unwrap();
    // Nothing links to this one, so it becomes a `detached-node` subject — the
    // only way a finding's *subject* escape is reachable at all.
    fs::write(dir.path().join("or\nphan.md"), "# O\n").unwrap();
    // A quoted key carries a newline; an unquoted one makes the block invalid.
    fs::write(
        dir.path().join("doc.md"),
        // `target.md` written bare from a file in a subdirectory would resolve from
        // the root — that is what raises a `cause` line, the second renderer whose
        // escapes nothing else reaches.
        "---\n\"ke\\ny\": value\nsources: |\n  first line\n  second line\n---\n\n# Doc\n",
    )
    .unwrap();
    fs::create_dir(dir.path().join("sub")).unwrap();
    fs::write(
        dir.path().join("sub").join("bare.md"),
        "# B\n\n[t](we\nird.md)\n",
    )
    .unwrap();

    // The text following each injected newline. If any render site emits its
    // value raw, one of these begins a line of its own. Bare `ird` rather than
    // `ird.md`: the namespace header's orphan line is just `ird`.
    let tails = ["ird", "phan", "second line", "y: value", "used", "thing"];
    let root = dir.path().to_str().unwrap();

    for args in [
        vec!["check"],
        // The colored renderer is a second set of interpolations, and every one of
        // its escapes was unpinned until this ran it.
        vec!["check", "--color", "always"],
        vec!["nodes"],
        vec!["edges"],
        vec![
            "impact",
            "doc.md",
            "--direction",
            "outbound",
            "--depth",
            "2",
        ],
        vec!["lock", "we\nird.md"],
        vec!["nodes", "no\nsuch.md"],
    ] {
        let mut full = vec!["-C", root];
        full.extend(args.iter().copied());
        let output = drft_bin().args(&full).output().unwrap();

        for (stream, bytes) in [("stdout", &output.stdout), ("stderr", &output.stderr)] {
            let text = String::from_utf8_lossy(bytes);
            for line in text.lines() {
                // Strip indentation and any color escape so a continuation line
                // and a colored head are compared on their content.
                let trimmed = line.trim_start().trim_start_matches(|c: char| {
                    c == '\u{1b}' || c == '[' || c == ';' || c.is_ascii_digit() || c == 'm'
                });
                for tail in tails {
                    assert!(
                        !trimmed.starts_with(tail),
                        "{args:?} {stream}: a value was rendered raw, so {tail:?} \
                         begins its own line — full output:\n{text}"
                    );
                }
            }
        }
    }
}
