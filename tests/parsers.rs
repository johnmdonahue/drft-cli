mod common;
use common::drft_bin;
use std::fs;
use tempfile::TempDir;

#[test]
fn frontmatter_sources_create_edges() {
    let dir = TempDir::new().unwrap();
    // Enable the frontmatter parser (frontmatter links moved out of markdown parser)
    fs::write(
        dir.path().join("drft.toml"),
        "[parsers.markdown]\n[parsers.frontmatter]\n",
    )
    .unwrap();
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
#[cfg(unix)]
fn custom_parser_batch_protocol() {
    let dir = TempDir::new().unwrap();

    // Write a batch parser command that reads the options envelope on line 1,
    // then file paths, and emits NDJSON
    let parser_cmd = dir.path().join("parse-txt.sh");
    fs::write(
        &parser_cmd,
        r#"#!/bin/sh
# Line 1 is the JSON options envelope — read and skip it
IFS= read -r _options
while IFS= read -r filepath; do
    [ -z "$filepath" ] && continue
    grep -oE '\[[^]]+\]\([^)]+\)' "$filepath" 2>/dev/null | while IFS= read -r match; do
        target=$(echo "$match" | sed 's/.*](//;s/)$//')
        printf '{"file":"%s","target":"%s","type":"ref"}\n' "$filepath" "$target"
    done
done
"#,
    )
    .unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&parser_cmd, fs::Permissions::from_mode(0o755)).unwrap();
    }

    // Config with a custom parser for .txt files
    fs::write(
        dir.path().join("drft.toml"),
        format!(
            r#"include = ["*.txt", "*.md"]

[parsers.custom]
files = ["*.txt"]
command = "{}"
"#,
            parser_cmd.to_string_lossy().replace('"', "\\\"")
        ),
    )
    .unwrap();

    // Two .txt files with markdown-style links
    fs::write(
        dir.path().join("a.txt"),
        "Link to [target](shared.md) here.",
    )
    .unwrap();
    fs::write(dir.path().join("b.txt"), "Another [link](shared.md) here.").unwrap();
    fs::write(dir.path().join("shared.md"), "# Shared").unwrap();

    // Run drft parse --format json and verify links from both files. (The
    // parser pipeline is exercised through `parse`; `graph` emits only the fs
    // graph until the markdown/frontmatter builders land.)
    let output = drft_bin()
        .args([
            "-C",
            dir.path().to_str().unwrap(),
            "parse",
            "--format",
            "json",
        ])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "drft parse failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value =
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("invalid JSON: {e}\n{stdout}"));

    let edges = json["edges"].as_array().expect("edges should be an array");

    let a_edges: Vec<_> = edges
        .iter()
        .filter(|e| e["file"].as_str() == Some("a.txt"))
        .collect();
    let b_edges: Vec<_> = edges
        .iter()
        .filter(|e| e["file"].as_str() == Some("b.txt"))
        .collect();

    assert!(
        !a_edges.is_empty(),
        "expected links from a.txt, got none in: {edges:?}"
    );
    assert!(
        !b_edges.is_empty(),
        "expected links from b.txt, got none in: {edges:?}"
    );
    assert_eq!(a_edges[0]["link"].as_str(), Some("shared.md"));
    assert_eq!(b_edges[0]["link"].as_str(), Some("shared.md"));
}
