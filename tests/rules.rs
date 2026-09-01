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

/// The addresses a file answers to come from more than its headings, and a
/// fragment is compared the way a browser compares it: a raw `<a id>`/`<a name>`
/// is an address, and a percent-encoded fragment is decoded first.
#[test]
fn anchors_and_fragments_match_what_a_browser_resolves() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), DEFAULT_CONFIG).unwrap();
    fs::write(
        dir.path().join("target.md"),
        "---\npurpose: a single-key block\n---\n\n\
         # Target\n\n\
         <a id=\"faq\"></a>\n\n\
         ## Café\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("cite.md"),
        "# Cite\n\n\
         - [html](./target.md#faq)\n\
         - [encoded](./target.md#caf%C3%A9)\n\
         - [frontmatter](./target.md#purpose-a-single-key-block)\n",
    )
    .unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        !stdout.contains("#faq"),
        "a raw <a id> is an address GitHub resolves, got: {stdout}"
    );
    assert!(
        !stdout.contains("caf%C3%A9"),
        "a percent-encoded fragment decodes before matching, got: {stdout}"
    );
    assert!(
        stdout.contains("cite.md:5 → target.md#purpose-a-single-key-block"),
        "the block is claimed, so its closing --- is not a setext heading, got: {stdout}"
    );
}

/// A frontmatter block the fences claim but the YAML cannot supply reaches
/// `drft check` as a finding, and config governs it like any other rule.
///
/// This covers the whole delivery path, which unit tests cannot: the parser
/// raises a `Diagnostic`, the builder turns it into a `Finding`, `build_set`
/// carries it out, and `check` merges it before severity is applied. A mutation
/// sweep found every one of those four links deletable with the suite green,
/// because nothing asserted the string ever reached a user.
#[test]
fn an_unreadable_frontmatter_block_reaches_check() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), DEFAULT_CONFIG).unwrap();
    // A bare scalar between fences: claimed as a block, carries no keys.
    fs::write(
        dir.path().join("bad.md"),
        "---\nJust A Title\n---\n\nBody\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("good.md"),
        "---\nsources:\n  - ./bad.md\n---\n\n# Good\n",
    )
    .unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("warn[unreadable-frontmatter]: bad.md:1"),
        "the claimed block should be reported at its opening fence, got: {stdout}"
    );
    assert!(
        !stdout.contains("unreadable-frontmatter]: good.md"),
        "a block that reads cleanly is not reported, got: {stdout}"
    );
}

/// `unreadable-frontmatter` promotes to `error` and fails the run, and `off`
/// silences it — the property that makes it a rule rather than a hint.
#[test]
fn unreadable_frontmatter_severity_is_configurable() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("bad.md"),
        "---\nJust A Title\n---\n\nBody\n",
    )
    .unwrap();

    let promoted =
        format!("{DEFAULT_CONFIG}\n[rules.unreadable-frontmatter]\nseverity = \"error\"\n");
    fs::write(dir.path().join("drft.toml"), &promoted).unwrap();
    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("error[unreadable-frontmatter]"),
        "promotion should raise the severity, got: {stdout}"
    );
    assert_eq!(
        output.status.code(),
        Some(1),
        "an error-severity finding fails the run, got: {stdout}"
    );
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains("unknown-rule"),
        "the rule must be in BUILTIN_RULES or config silently configures nothing"
    );

    let silenced =
        format!("{DEFAULT_CONFIG}\n[rules.unreadable-frontmatter]\nseverity = \"off\"\n");
    fs::write(dir.path().join("drft.toml"), &silenced).unwrap();
    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("unreadable-frontmatter"),
        "`off` should silence it entirely"
    );
}

/// A file selected by text graphs cannot disappear at the UTF-8 decode seam.
/// The finding is one per file even when graph scopes overlap, and it carries
/// every namespace that could not read the file.
#[test]
fn invalid_utf8_is_reported_once_by_matching_text_graphs() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), DEFAULT_CONFIG).unwrap();
    let bytes = b"title: \xff\n";
    fs::write(dir.path().join("bad.md"), bytes).unwrap();

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "--format",
            "json",
            "check",
        ])
        .output()
        .unwrap();
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("valid JSON check output");
    let findings: Vec<_> = json["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|finding| finding["name"] == "unreadable-text")
        .collect();

    assert_eq!(
        findings.len(),
        1,
        "one file should yield one finding: {json}"
    );
    assert_eq!(findings[0]["subject"], "bad.md");
    assert_eq!(
        findings[0]["_graphs"],
        serde_json::json!(["@frontmatter", "@markdown"])
    );
    assert!(
        findings[0]["message"]
            .as_str()
            .unwrap()
            .contains("not valid UTF-8")
    );

    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "--format",
            "json",
            "nodes",
            "bad.md",
        ])
        .output()
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let metadata = &json["nodes"][0]["metadata"];
    assert_eq!(
        metadata["@fs"]["hash"],
        format!("b3:{}", blake3::hash(bytes))
    );
    assert!(
        metadata.get("@markdown").is_none() && metadata.get("@frontmatter").is_none(),
        "invalid text must not be decoded lossily into a text graph: {metadata}"
    );
}

/// Binary bytes outside every configured text graph are ordinary graph content,
/// not an unreadable text document.
#[test]
fn invalid_utf8_outside_text_graph_scope_is_not_reported() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("drft.toml"), DEFAULT_CONFIG).unwrap();
    fs::write(dir.path().join("image.bin"), b"\xff\x00\xfe").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("unreadable-text"),
        "an unmatched binary should stay quiet"
    );
}

/// `unreadable-text` is a registered rule: it can fail the run, be silenced,
/// and does not raise an unknown-rule hint.
#[test]
fn unreadable_text_severity_is_configurable() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("bad.md"), b"\xff").unwrap();

    let promoted = format!("{DEFAULT_CONFIG}\n[rules.unreadable-text]\nseverity = \"error\"\n");
    fs::write(dir.path().join("drft.toml"), promoted).unwrap();
    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stdout).contains("error[unreadable-text]"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("unknown-rule"));

    let silenced = format!("{DEFAULT_CONFIG}\n[rules.unreadable-text]\nseverity = \"off\"\n");
    fs::write(dir.path().join("drft.toml"), silenced).unwrap();
    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();
    assert!(!String::from_utf8_lossy(&output.stdout).contains("unreadable-text"));

    let rule_ignored =
        format!("{DEFAULT_CONFIG}\n[rules.unreadable-text]\nignore = [\"bad.md\"]\n");
    fs::write(dir.path().join("drft.toml"), rule_ignored).unwrap();
    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("unreadable-text"),
        "the rule-level ignore must suppress the matching subject"
    );

    let globally_ignored = format!("{DEFAULT_CONFIG}\n[rules]\nignore = [\"bad.md\"]\n");
    fs::write(dir.path().join("drft.toml"), globally_ignored).unwrap();
    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("unreadable-text"),
        "the global rule ignore must suppress the matching subject"
    );
}
