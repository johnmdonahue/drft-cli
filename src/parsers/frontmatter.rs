use saphyr::{LoadableYamlNode, MarkedYaml, Scalar, YamlData};

use super::{Diagnostic, Link, ParseResult, Parser};

/// Whether the character at `i` opens a new line **as the YAML parser counts
/// them**.
///
/// saphyr breaks on `\n` and on a lone `\r`, and counts `\r\n` as one break.
/// `str::lines` disagrees on the middle case — it splits on `\n` and strips a
/// `\r` only where one immediately precedes a `\n`. So a carriage return left
/// inside a line is a break to the parser and was not one to this table, which
/// desynchronized the two and sent the correction to the wrong line.
///
/// The lookahead is what makes `\r\n` one break rather than two, and it is
/// load-bearing on any CRLF block: testing characters one at a time opens a row
/// for each half, over-counting by one and reintroducing the same defect a line in
/// the other direction. Both directions fail a named test — `'\r' => true` fails
/// `a_crlf_document_reports_the_files_line`, and `'\r' => false` fails the two
/// lone-carriage-return tests.
///
/// This is not the same question as which line of the *file* a value sits on.
/// A file's lines are counted by `\n` alone, which is what an editor and
/// `grep -n` report, so a lone `\r` opens a new row of this table that maps to
/// the same source line as the row before it.
fn opens_line(chars: &[char], i: usize) -> bool {
    match chars[i] {
        '\n' => true,
        // A newline behind it will open the row; this one would double it.
        '\r' => chars.get(i + 1) != Some(&'\n'),
        _ => false,
    }
}

/// The source line a value reported at 1-based `line` came from.
fn source_line_in(lines: &[usize], line: usize) -> usize {
    // The fallback covers a line the table does not hold: saphyr reports line 0
    // for a scalar carrying a bare `!` tag, and there is no zeroth line.
    lines.get(line.wrapping_sub(1)).copied().unwrap_or(line)
}

/// One entry per line saphyr counts, carrying the file line that entry sits on.
///
/// **A block is not already numbered the way the file is.** saphyr breaks on `\n`
/// and on a lone `\r`; a file's lines are counted by `\n` alone, which is what an
/// editor and `grep -n` report. So a stray carriage return above a value opens a
/// row here without advancing the file line, and reporting saphyr's line unaltered
/// moves that value and every one below it.
///
/// [`opens_line`] is what makes this agree with the parser it corrects for, and
/// the `\r\n` lookahead is the load-bearing half: testing characters one at a time
/// opens a row for each half of a `\r\n` and shifts every value below it.
fn line_table(text: &str) -> Vec<usize> {
    let chars: Vec<char> = text.chars().collect();
    let mut lines = Vec::new();
    let mut source_line = 1usize;
    for i in 0..chars.len() {
        if opens_line(&chars, i) {
            lines.push(source_line);
            if chars[i] == '\n' {
                source_line += 1;
            }
        }
    }
    lines.push(source_line);
    lines
}

/// Built-in frontmatter parser. Extracts YAML frontmatter as links and metadata.
pub struct FrontmatterParser {
    /// File routing filter. None = receives all File nodes.
    pub file_filter: Option<globset::GlobSet>,
    /// Keys whose values yield edges. Empty means the graph emits none, which is
    /// a supported configuration: a frontmatter graph may exist purely to seed
    /// node metadata. Scopes edges only — metadata always captures the entire
    /// block, whatever this holds.
    pub edge_keys: Vec<String>,
}

impl Parser for FrontmatterParser {
    fn matches(&self, path: &str) -> bool {
        match &self.file_filter {
            Some(set) => set.is_match(path),
            None => true,
        }
    }

    fn parse(&self, _path: &str, content: &str) -> ParseResult {
        // One boundary decision, one YAML parse, three jobs reading the result.
        // Asking the markdown library where the block ends costs a pass over the
        // whole document, so this parser asks once rather than once per job. In a
        // config carrying both graphs the question is still asked twice per file,
        // because the markdown parser asks it too to mask the block.
        let Some(block) = frontmatter_block(content) else {
            return ParseResult {
                links: Vec::new(),
                anchors: Vec::new(),
                metadata: None,
                diagnostics: Vec::new(),
            };
        };
        // A recognized block either is a mapping — metadata and edges, read from
        // one parse of one rendering — or it is reported. There is no third
        // reading: a block whose YAML is invalid is invalid, and recovering one by
        // blanking the characters that broke it was inference about what the
        // author meant.
        let Some(root) = mapping(block) else {
            return ParseResult {
                links: Vec::new(),
                anchors: Vec::new(),
                metadata: None,
                diagnostics: vec![Diagnostic {
                    rule: UNREADABLE_FRONTMATTER,
                    // What the reader has to know is that the block was recognized
                    // and is being skipped wholesale — not which YAML construct
                    // failed, which drft is not a linter for.
                    message: "frontmatter block could not be read completely as a YAML \
                              mapping, so its keys, values and edges are all dropped"
                        .to_string(),
                    // The opening fence, which is the line a reader edits. Line 1
                    // whenever there is a block at all, since the fence has to
                    // open the file.
                    line: Some(1),
                }],
            };
        };
        ParseResult {
            links: self.edges(&root, &line_table(block)),
            // Frontmatter defines no addressable sub-file positions; anchors come
            // from the markdown body's headings.
            anchors: Vec::new(),
            metadata: Some(to_json(&root)),
            diagnostics: Vec::new(),
        }
    }
}

impl FrontmatterParser {
    /// The edges a parsed block declares: every string reachable through a
    /// declared key.
    ///
    /// A code span inside a value is part of that value, so an edge names what the
    /// file declared rather than what a mask left of it.
    ///
    /// `lines` maps a line of the block back to the file line it came from. It is
    /// not optional: saphyr breaks on a lone `\r` and a file's lines are counted by
    /// `\n` alone, so a stray carriage return above a value shifts every line
    /// below it.
    fn edges(&self, root: &MarkedYaml, lines: &[usize]) -> Vec<Link> {
        let mut candidates = Vec::new();
        let wanted: std::collections::HashSet<&str> =
            self.edge_keys.iter().map(String::as_str).collect();
        collect_scoped(root, &wanted, &mut candidates);
        candidates
            .into_iter()
            // Every string reachable through a declared key is an edge. The key is
            // the whole of the signal, so nothing here asks what a value looks
            // like — one that resolves to nothing becomes an edge that resolves to
            // nothing, and `unresolved-edge` says so.
            .map(|(target, line)| Link {
                target,
                line: Some(source_line_in(lines, line)),
            })
            .collect()
    }
}

/// The block's YAML root when it parses as a **mapping** — the shape that yields
/// metadata.
///
/// This does not decide what is frontmatter; the fences do, and they are the
/// markdown library's. It decides only whether a block drft already claims has
/// anything a node can carry. A comment-only block parses to no document at all
/// and a bare scalar carries no keys, so neither contributes metadata, and both
/// raise [`UNREADABLE_FRONTMATTER`] rather than falling through in silence.
///
/// The gate used to be the boundary too, which is what made the YAML/markdown
/// collision load-bearing: `# First` is a comment and an ATX heading, and
/// `---\nMy Title\n---` is a block and a setext heading, so a wrong answer here
/// deleted real content. Now the two questions are separate and only this one
/// rests on the YAML.
fn mapping(block: &str) -> Option<MarkedYaml<'_>> {
    // saphyr's scanner uses NUL as its end-of-input sentinel. A literal NUL in
    // the source therefore returns a valid prefix document instead of an error.
    // Reject it in the already-recognized block so the prefix cannot become
    // metadata or edges; body NULs remain body content.
    if block.contains('\0') {
        return None;
    }
    let root = MarkedYaml::load_from_str(block).ok()?.into_iter().next()?;
    matches!(root.data, YamlData::Mapping(_)).then_some(root)
}

/// The rule an unreadable block becomes. A rule rather than a hint because a
/// hint cannot change an exit code, and a repository whose derivations are
/// declared in frontmatter needs a dropped block to be able to fail its run.
pub const UNREADABLE_FRONTMATTER: &str = "unreadable-frontmatter";

/// The raw text of the leading YAML frontmatter block, or `None` when the file
/// does not open with one.
///
/// **One decider owns the boundary, and it is the markdown library.** The block's
/// extent is fence syntax the library already implements, so it is asked rather
/// than mirrored: a rule copied from the component it has to agree with drifts
/// from that component, and every place the two drift apart one parser claims a
/// span the other renders, publishing the same text as metadata and as an address.
///
/// A block below line one is not frontmatter. The library is willing to claim one
/// anywhere in a document, and no frontmatter convention accepts a fence that is
/// not on the first line, so taking its answer unchecked would read a thematic
/// break and a setext heading as metadata.
///
/// **Two conditions enforce that here and they overlap completely**: the fast path
/// below, and `leading_metadata_block`'s own `range.start == 0`. A block the
/// library reports at a non-zero offset needs blank lines above it, and a file
/// with blank lines above its fence does not start with `---` — so the fast path
/// rejects every input the offset check would have. Neither is independently
/// observable from this parser, and the offset check is load-bearing for the
/// markdown parser's mask, where `a_block_below_a_blank_first_line_is_prose`
/// fails on its removal.
///
/// The slice runs from the newline **ending the opening fence line** to the one
/// **preceding the closing fence**, which is the span the library treats as the
/// block's content. It is that span rather than the library's own `Text` event,
/// which normalises `\r\n` — this keeps the bytes the file carried, and saphyr
/// reads the trailing `\r` as a break. Keeping that leading newline is what makes a value's line
/// within the block its line within the file; because the block starts at byte 0,
/// the fence is on line 1 and there is no offset to carry. Slicing from just past
/// the `---` instead leaves the fence line's trailing whitespace inside the block,
/// which is a form feed saphyr rejects on a document every renderer accepts.
fn frontmatter_block(content: &str) -> Option<&str> {
    // Asking the library costs a pass over the whole document, and it answers
    // `None` for every file that does not open with the fence characters, so this
    // buys that pass back on any file without frontmatter.
    //
    // Deleting it leaves the suite green, and that is not evidence it is dead:
    // it enforces the same thing the offset check above enforces, so either alone
    // suffices and only removing both changes behaviour. Delete it and the cost
    // returns; delete both and a block below a blank first line becomes metadata.
    if !content.starts_with("---") {
        return None;
    }
    let end = super::markdown::leading_metadata_block(content)?;
    // `find` never exceeds `rfind`, so the slice cannot invert. A block the
    // library claims carries both fences on their own lines, so the two land on
    // different newlines.
    let open = content[..end].find('\n')?;
    let close = content[..end].rfind('\n')?;
    Some(&content[open..close])
}

/// Collect string leaf *values* (not keys) with their position in the block — the
/// frontmatter link candidates. The line is 1-based and the column is 0-based,
/// which is saphyr's convention for each whatever its docs say. The caller carries
/// a table mapping that position back to a file line, because the block is not
/// already numbered the way the file is: saphyr breaks on a lone `\r` and a file's
/// lines are counted by `\n` alone.
///
/// Mirrors the metadata walk but keeps only strings.
fn collect_links(node: &MarkedYaml, out: &mut Vec<(String, usize)>) {
    match &node.data {
        YamlData::Value(Scalar::String(s)) => out.push((s.to_string(), node.span.start.line())),
        YamlData::Sequence(items) => {
            for item in items {
                collect_links(item, out);
            }
        }
        YamlData::Mapping(map) => {
            for (_key, value) in map {
                collect_links(value, out);
            }
        }
        YamlData::Tagged(_, inner) => collect_links(inner, out),
        _ => {}
    }
}

/// Collect scalars reachable only through one of `keys`. A matched key hands its
/// whole subtree to `collect_links`, so a nested map or list under `sources:`
/// still yields every path beneath it. Unmatched keys are still descended into,
/// so a key nested under an unrelated one is found — the key is what scopes the
/// walk, not its depth. A scalar reached under no matched key yields nothing.
fn collect_scoped(
    node: &MarkedYaml,
    keys: &std::collections::HashSet<&str>,
    out: &mut Vec<(String, usize)>,
) {
    match &node.data {
        YamlData::Sequence(items) => {
            for item in items {
                collect_scoped(item, keys, out);
            }
        }
        YamlData::Mapping(map) => {
            for (key, value) in map {
                match &key.data {
                    YamlData::Value(Scalar::String(k)) if keys.contains(k.as_ref()) => {
                        collect_links(value, out)
                    }
                    _ => collect_scoped(value, keys, out),
                }
            }
        }
        YamlData::Tagged(_, inner) => collect_scoped(inner, keys, out),
        _ => {}
    }
}

/// Convert a marked YAML node to `serde_json::Value` for the `@frontmatter`
/// metadata namespace.
fn to_json(node: &MarkedYaml) -> serde_json::Value {
    use serde_json::Value as J;
    match &node.data {
        YamlData::Value(scalar) => scalar_to_json(scalar),
        YamlData::Representation(raw, _, _) => J::String(raw.to_string()),
        YamlData::Sequence(items) => J::Array(items.iter().map(to_json).collect()),
        YamlData::Mapping(map) => {
            let obj: serde_json::Map<String, J> = map
                .iter()
                .filter_map(|(k, v)| Some((json_key(k)?, to_json(v))))
                .collect();
            J::Object(obj)
        }
        YamlData::Tagged(_, inner) => to_json(inner),
        YamlData::Alias(_) | YamlData::BadValue => J::Null,
    }
}

/// Render a mapping key as a JSON object key: a string scalar verbatim, anything
/// else as its JSON serialization (matching the prior behavior).
fn json_key(node: &MarkedYaml) -> Option<String> {
    match &node.data {
        YamlData::Value(Scalar::String(s)) => Some(s.to_string()),
        _ => serde_json::to_string(&to_json(node)).ok(),
    }
}

fn scalar_to_json(scalar: &Scalar) -> serde_json::Value {
    use serde_json::Value as J;
    match scalar {
        Scalar::Null => J::Null,
        Scalar::Boolean(b) => J::Bool(*b),
        Scalar::Integer(i) => J::Number((*i).into()),
        Scalar::FloatingPoint(f) => serde_json::Number::from_f64(f.0)
            .map(J::Number)
            .unwrap_or(J::Null),
        Scalar::String(s) => J::String(s.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse with `sources` as the sole edge key — the shape every fixture below
    /// uses unless it names its own.
    fn parse(content: &str) -> ParseResult {
        parse_with(content, &["sources"])
    }

    fn parse_with(content: &str, edge_keys: &[&str]) -> ParseResult {
        let parser = FrontmatterParser {
            file_filter: None,
            edge_keys: edge_keys.iter().map(|k| k.to_string()).collect(),
        };
        parser.parse("test.md", content)
    }

    /// A line the table does not hold falls back to the reported line rather than
    /// panicking.
    ///
    /// One thing reaches it: saphyr reports line 0 for a scalar carrying a *local*
    /// tag — `!`, `!foo`, or a verbatim `!<…>` — and there is no zeroth line.
    /// A global tag such as `!!str` reports its real line. Line-terminator
    /// disagreements do not reach it — `opens_line` matches saphyr break for
    /// break, so the table holds a row for every line the parser can name.
    #[test]
    fn a_line_the_table_does_not_hold_falls_back() {
        let lines = line_table("a: b\n");
        assert_eq!(source_line_in(&lines, 0), 0);
        assert_eq!(source_line_in(&lines, 99), 99);
    }

    /// The lookup is off by one in a direction that hides itself: rows are counted
    /// from zero and saphyr's lines from one, so reading `lines[line]` rather than
    /// `lines[line - 1]` still returns a plausible answer for every row.
    ///
    /// A plain fixture would catch that one — the lookup returns the *next* row's
    /// answer, which is wrong wherever a next row exists. It hides only on the
    /// block's last row, where running off the end lands on the fallback and the
    /// fallback happens to be right.
    ///
    /// **The `\r` is here for the other defect**: dropping the correction
    /// altogether is invisible unless the parser's rows and the file's lines
    /// disagree, because where they agree the uncorrected answer already is the
    /// corrected one.
    #[test]
    fn a_row_resolves_to_the_source_line_it_opened_on() {
        let lines = line_table("a: b\rc: d\n");
        assert_eq!(lines, vec![1, 1, 2]);
        // The lone carriage return opens row 2, which is still file line 1.
        assert_eq!(source_line_in(&lines, 2), 1);
        assert_eq!(source_line_in(&lines, 3), 2);
    }

    /// A lone carriage return opens a new row of the table without advancing the
    /// source line.
    ///
    /// saphyr breaks on it and a file's lines are counted by `\n` alone — which is
    /// what an editor and `grep -n` report — so the table has to break on it too
    /// or its rows stop lining up with the parser's line numbers, while both rows
    /// map to the same source line.
    #[test]
    fn a_lone_carriage_return_opens_a_row_without_advancing_the_source_line() {
        assert_eq!(line_table("a: one\rb: two\nc: three\n"), vec![1, 1, 2, 3]);
    }

    /// `\r\n` is one line break, not two.
    ///
    /// Testing characters one at a time opens a row for each half and over-counts,
    /// which is the same defect as under-counting, one line the other way. The
    /// lookahead on the `\r` is what prevents it.
    #[test]
    fn a_carriage_return_before_a_newline_opens_one_row_not_two() {
        // Two carriage returns: the first is a break on its own, the second pairs
        // with the newline. Three breaks, four rows, and `b` on source line 2 —
        // the line an editor shows it on, since one newline precedes it.
        assert_eq!(line_table("a: one\r\r\nb: two\n"), vec![1, 1, 2, 3]);
        // A plain CRLF file has one break where this has two, and `b` lands on the
        // same source line either way.
        assert_eq!(line_table("a: one\r\nb: two\n"), vec![1, 2, 3]);
    }

    /// `\n\r` is two breaks, not one — the mirror of the case above, and the arm
    /// that had no test.
    ///
    /// The lookahead suppressing a duplicate row belongs on the `\r` and only
    /// there. Writing the symmetric suppression on the `\n` instead passes every
    /// other test in this suite while collapsing two rows the parser counts
    /// separately, which desynchronizes the table exactly as under-counting did.
    #[test]
    fn a_newline_before_a_carriage_return_opens_two_rows() {
        assert_eq!(
            line_table("a: 1\n\rb: 2\n"),
            vec![1, 2, 2, 3],
            "the newline ends source line 1; the carriage return opens a second \
             row on source line 2 without advancing it"
        );
    }

    /// Only `\n` and `\r` open a row, because those are the only characters the
    /// parser breaks on.
    ///
    /// Every other test here asserts that something *does* open a row, which
    /// leaves the break set pinned in one direction: widening it to Unicode's
    /// other separators reads as a correctness fix, passes the whole suite, and
    /// desynchronizes the table on any block containing one.
    #[test]
    fn unicode_separators_do_not_open_a_row() {
        for separator in ['\u{2028}', '\u{0085}', '\u{000c}', '\u{2029}', '\u{000b}'] {
            assert_eq!(
                line_table(&format!("a: 1{separator}b: 2\n")),
                vec![1, 2],
                "{separator:?} is not a line break to this parser"
            );
        }
    }

    #[test]
    fn a_code_span_crossing_the_fence_does_not_move_the_block() {
        // A backtick opened in frontmatter and closed in the body, which used to
        // move the boundary when the block was found in a masked copy of the whole
        // file: blanking the span took the closing fence with it.
        //
        // **This asserts the outcome, not the boundary.** Both candidate readings
        // of this input are unreadable, so no assertion here can tell them apart.
        // Where the boundary lands is observable through the markdown parser's
        // mask, and `a_second_block_below_frontmatter_is_prose` is what sees it.
        let content = "---\npurpose: `a: b\n---\nc: d` tail\nkey: v\n---\n\n# Body\n";
        assert!(parse(content).metadata.is_none());
        assert!(
            !parse(content).diagnostics.is_empty(),
            "the block is the first fence pair, and it is reported rather than dropped"
        );
    }

    #[test]
    fn a_longer_closing_fence_does_not_close_the_block() {
        // `rest.find("\n---")` matched the first three characters of `----`, so a
        // block "closed" by a thematic break was claimed here and rendered as a
        // setext heading by the markdown parser — the same text being metadata
        // and an address at once.
        assert!(parse("---\ntitle: Doc\n----\n\nBody\n").metadata.is_none());
        assert!(parse("---\ntitle: Doc\n---x\n\nBody\n").metadata.is_none());
    }

    #[test]
    fn a_closing_fence_takes_trailing_spaces_but_not_a_tab() {
        // Mirrors the markdown library, which scans spaces after the fence and
        // stops at anything else. A tab is not a space to either of them.
        assert!(parse("---\ntitle: Doc\n---  \n\nBody\n").metadata.is_some());
        assert!(parse("---\ntitle: Doc\n---\t\n\nBody\n").metadata.is_none());
    }

    #[test]
    fn a_document_end_marker_closes_the_block() {
        // `...` is YAML's document-end marker and the library accepts it wherever
        // it accepts `---`. Keys below it are outside the block.
        //
        // saphyr stops at `...` on its own, so metadata alone cannot tell where the
        // block ended — a parser running to the later `---` still yields
        // `{title: Doc}`. What separates them is what the markdown parser is left
        // to render, and `a_document_end_marker_ends_the_masked_region` in that
        // parser's tests asserts it: the text below `...` keeps its heading.
        let content = "---\ntitle: Doc\n...\npurpose: below\n---\n\nBody\n";
        let metadata = parse(content)
            .metadata
            .expect("the block above `...` is read");
        assert!(metadata.get("title").is_some());
        assert!(metadata.get("purpose").is_none());
    }

    #[test]
    fn an_opening_fence_is_exactly_three_dashes() {
        // A fourth dash makes the line a thematic break to the renderer, so
        // opening a block on it claims a span the renderer publishes as content.
        assert!(parse("----\ntitle: Doc\n----\n\nBody\n").metadata.is_none());
        assert!(parse("----\ntitle: Doc\n---\n\nBody\n").metadata.is_none());
    }

    #[test]
    fn an_opening_fence_may_carry_trailing_whitespace() {
        // The renderer allows whitespace after the opening fence and still opens
        // the block. Rejecting it leaves the block claimed there and unread here,
        // which is a declared derivation lost with nothing said.
        assert!(parse("--- \ntitle: Doc\n---\n\nBody\n").metadata.is_some());
        assert!(parse("---\t\ntitle: Doc\n---\n\nBody\n").metadata.is_some());
    }

    #[test]
    fn a_first_line_of_exotic_whitespace_is_still_blank() {
        // The renderer counts vertical tab and form feed as blank-line
        // whitespace. Counting only space and tab opens a block it does not, so
        // the file would report unreadable frontmatter here while publishing an
        // anchor slugged from the same text there.
        for whitespace in ["\u{0b}", "\u{0c}", " ", "\t"] {
            let content = format!("---\n{whitespace}\nkey: value\n---\n\nBody\n");
            assert!(
                parse(&content).metadata.is_none(),
                "a first line of {whitespace:?} is blank, so this is a thematic break"
            );
            assert!(
                parse(&content).diagnostics.is_empty(),
                "and a thematic break is not an unreadable block"
            );
        }
    }

    #[test]
    fn a_blank_first_line_makes_it_a_thematic_break() {
        // The library will not open a block whose first line is blank, and
        // matching that is what stops this parser claiming a span the markdown
        // parser renders. `key: v` above a `---` is a setext heading there, so
        // reading it as frontmatter here publishes metadata and an address for
        // one piece of text.
        assert!(parse("---\n\nkey: v\n---\n\nBody\n").metadata.is_none());
        // A blank line lower in the block is ordinary YAML.
        assert!(
            parse("---\nkey: v\n\nother: w\n---\n\nBody\n")
                .metadata
                .is_some()
        );
    }

    #[test]
    fn an_unreadable_block_is_reported_rather_than_dropped() {
        // The markdown parser skips whatever the fences claim, so a block that
        // fails the mapping gate reaches neither graph. Saying nothing about it
        // is the failure this rule exists to prevent.
        let result = parse("---\nJust A Title\n---\n\nBody\n");
        assert!(result.metadata.is_none());
        let diagnostic = result
            .diagnostics
            .first()
            .expect("a claimed block that is not a mapping is reported");
        assert_eq!(diagnostic.rule, UNREADABLE_FRONTMATTER);
        assert_eq!(diagnostic.line, Some(1));

        // A file with no block at all has nothing to report.
        assert!(parse("# Body\n\ntext\n").diagnostics.is_empty());
        // Neither does one whose block reads cleanly.
        assert!(
            parse("---\ntitle: Doc\n---\n\nBody\n")
                .diagnostics
                .is_empty()
        );
    }

    #[test]
    fn a_literal_nul_makes_the_whole_recognized_block_unreadable() {
        for block in [
            "\0\nsources: ./target.md",
            "note: \0\nsources: ./target.md",
            "note: before\0after\nsources: ./target.md",
            "note: ok # before\0after\nsources: ./target.md",
            "note: |\n  before\0after\nsources: ./target.md",
            "sour\0ces: ./target.md",
            "sources: ./target.md\n\0",
            "{sources: ./target.md}\0",
        ] {
            let result = parse_with(&format!("---\n{block}\n---\nbody\n"), &["sources"]);
            assert!(
                result.metadata.is_none(),
                "prefix metadata survived {block:?}"
            );
            assert!(result.links.is_empty(), "prefix edges survived {block:?}");
            assert_eq!(result.diagnostics.len(), 1, "diagnostic for {block:?}");
            assert_eq!(result.diagnostics[0].rule, UNREADABLE_FRONTMATTER);
            assert_eq!(result.diagnostics[0].line, Some(1));
        }
    }

    #[test]
    fn escaped_or_outside_nuls_do_not_make_frontmatter_unreadable() {
        for content in [
            "---\nnote: \\\\0\n---\nbody\n",
            "---\nnote: clean\n---\nbody\0\n",
            "---\n{note: clean} # trailing comment\n---\nbody\n",
            "---\nnote: café\n---\nbody\n",
        ] {
            let result = parse_with(content, &[]);
            assert!(
                result.metadata.is_some(),
                "valid block rejected: {content:?}"
            );
            assert!(
                result.diagnostics.is_empty(),
                "valid block reported: {content:?}"
            );
        }
    }

    #[test]
    fn an_opening_fence_owns_its_line_even_when_a_closer_follows() {
        // `---key: v\n---` is rejected by the first-line guard as well, so it
        // cannot tell whether the opening-line check is doing anything. A block
        // with a second line separates them: without the opening-line check this
        // claims a block, and the renderer claims none.
        assert!(
            parse("---key: v\nfoo: bar\n---\n\nBody\n")
                .metadata
                .is_none()
        );
        assert!(
            parse("---key: v\nfoo: bar\n---\n\nBody\n")
                .diagnostics
                .is_empty(),
            "text that is not a block is not an unreadable block"
        );
    }

    #[test]
    fn a_first_line_that_is_a_closing_fence_opens_nothing() {
        // The other half of the first-line guard, and the blank-line test does
        // not reach it. `---` above `---` is two thematic breaks to every
        // renderer; claiming a block there reports unreadable frontmatter on a
        // document that has none.
        for content in [
            "---\n---\n\n# Head\n",
            "---\n...\nkey: v\n---\n\nBody\n",
            "---\n---\nkey: v\n---\n\nBody\n",
        ] {
            let result = parse(content);
            assert!(result.metadata.is_none(), "no block in {content:?}");
            assert!(
                result.diagnostics.is_empty(),
                "and nothing to report in {content:?}"
            );
        }
    }

    #[test]
    fn the_opening_fence_line_is_fence_rather_than_content() {
        // The renderer opens a fence on whitespace that excludes the vertical tab
        // and includes the form feed, and neither character reaches the YAML —
        // the fence line is the fence. This is the one test that fails when the
        // block text is sliced from the fence characters instead of from the
        // newline that ends their line.
        //
        // A vertical tab opens nothing, so there is no block and nothing to say
        // about one.
        let vertical_tab = parse("---\u{0b}\nkey: v\n---\n\nBody\n");
        assert!(vertical_tab.metadata.is_none());
        assert!(
            vertical_tab.diagnostics.is_empty(),
            "a vertical tab does not open a fence, so there is no block to report"
        );

        // A form feed does, and the block reads: the fence line is the fence, so
        // its trailing whitespace is outside the text handed to the YAML parser.
        // Taking the block from just past the `---` left the form feed inside it
        // and reported a document every renderer accepts as unreadable.
        let form_feed = parse("---\u{0c}\nkey: v\n---\n\nBody\n");
        assert!(form_feed.diagnostics.is_empty());
        assert_eq!(
            form_feed.metadata.and_then(|m| m.get("key").cloned()),
            Some(serde_json::json!("v")),
            "a form feed after the opening fence does not reach the YAML"
        );
    }

    #[test]
    fn a_block_below_a_blank_first_line_is_not_frontmatter() {
        // The library claims a block wherever it finds one and reports where it
        // starts; this parser reads only one at byte 0. The input matters: with
        // both guards gone the slice is a document separator followed by a
        // mapping, so the block parses and the file becomes metadata here and
        // prose to the parser that renders it. A block below a blank line that is
        // *not* a mapping would decline for the wrong reason and assert nothing.
        //
        // **Removing either guard alone leaves this green**, because the two cover
        // the same inputs — see `frontmatter_block`. This asserts the behaviour,
        // not either mechanism.
        let content = "   \n---\nsources:\n  - ./target.md\n---\n\n# Body\n";
        let result = parse_with(content, &["sources"]);
        assert!(result.metadata.is_none());
        assert!(result.links.is_empty());
        assert!(
            result.diagnostics.is_empty(),
            "there is no block here to report as unreadable"
        );
    }

    #[test]
    fn an_opening_fence_must_own_its_line() {
        // `---key: v` is a paragraph, not frontmatter; reading it as one invents
        // metadata out of a document's first line.
        let result = parse("---key: v\n---\n\n# Body\n");
        assert!(result.metadata.is_none());
    }

    #[test]
    fn extracts_frontmatter_links() {
        let content =
            "---\nsources:\n  - ../shared/glossary.md\n  - ./prior-art.md\n---\n\n# Hello\n";
        let result = parse(content);
        assert_eq!(result.links.len(), 2);
        assert_eq!(result.links[0].target, "../shared/glossary.md");
        assert_eq!(result.links[1].target, "./prior-art.md");
    }

    #[test]
    fn extracts_same_directory_links() {
        let content = "---\nsources:\n  - setup.md\n  - config.rs\n---\n";
        let result = parse(content);
        assert_eq!(result.links.len(), 2);
        assert_eq!(result.links[0].target, "setup.md");
        assert_eq!(result.links[1].target, "config.rs");
    }

    #[test]
    fn records_link_line_numbers() {
        // Lines are 1-based and file-accurate: the opening `---` is line 1.
        let content = "---\ntitle: Doc\nsources:\n  - setup.md\n  - other.md\n---\n";
        let result = parse(content);
        let setup = result
            .links
            .iter()
            .find(|l| l.target == "setup.md")
            .unwrap();
        assert_eq!(setup.line, Some(4));
        let other = result
            .links
            .iter()
            .find(|l| l.target == "other.md")
            .unwrap();
        assert_eq!(other.line, Some(5));
    }

    #[test]
    fn malformed_yaml_contributes_nothing() {
        // Invalid YAML yields no links and no metadata — drft is not a linter,
        // and there is no stderr warning (the `eprintln` is gone).
        let result = parse("---\nsources: [a, b\n---\n");
        assert!(result.links.is_empty());
        assert!(result.metadata.is_none());
    }

    /// Parse with the given edge keys, returning the edge targets.
    fn scoped(content: &str, edge_keys: &[&str]) -> Vec<String> {
        let parser = FrontmatterParser {
            file_filter: None,
            edge_keys: edge_keys.iter().map(|k| k.to_string()).collect(),
        };
        parser
            .parse("doc.md", content)
            .links
            .into_iter()
            .map(|l| l.target)
            .collect()
    }

    #[test]
    fn edge_keys_scope_excludes_other_keys() {
        // The two real collisions from #73: an API route and a rule's glob scope,
        // both path-shaped, neither a derivation.
        let content = "---\nsources:\n  - ../src/lib.rs\nroute: /customers\npaths:\n  - \"api/openapi.yaml\"\n---\n";
        assert_eq!(scoped(content, &["sources"]), vec!["../src/lib.rs"]);
    }

    #[test]
    fn edge_keys_scope_takes_whole_subtree() {
        // A matched key hands its entire subtree over, so nesting under it still
        // yields every path beneath.
        let content = "---\nsources:\n  primary:\n    - ../a.rs\n  secondary: ../b.rs\n---\n";
        let mut got = scoped(content, &["sources"]);
        got.sort();
        assert_eq!(got, vec!["../a.rs", "../b.rs"]);
    }

    #[test]
    fn edge_keys_scope_finds_nested_key() {
        // The key scopes the walk, not its depth — `sources` under an unrelated
        // parent is still found.
        let content = "---\nmeta:\n  sources:\n    - ../a.rs\n---\n";
        assert_eq!(scoped(content, &["sources"]), vec!["../a.rs"]);
    }

    #[test]
    fn edge_keys_scope_keeps_line_numbers() {
        let content = "---\ntitle: T\nsources:\n  - ../a.rs\n---\n";
        let parser = FrontmatterParser {
            file_filter: None,
            edge_keys: vec!["sources".to_string()],
        };
        let links = parser.parse("doc.md", content).links;
        assert_eq!(links[0].line, Some(4));
    }

    #[test]
    fn edge_keys_scope_leaves_metadata_whole() {
        // `edge_keys` scopes edges only — the metadata namespace keeps the full block.
        let content = "---\ntitle: T\nroute: /customers\nsources:\n  - ../a.rs\n---\n";
        let parser = FrontmatterParser {
            file_filter: None,
            edge_keys: vec!["sources".to_string()],
        };
        let meta = parser.parse("doc.md", content).metadata.unwrap();
        assert_eq!(meta["title"], "T");
        assert_eq!(meta["route"], "/customers");
    }

    #[test]
    fn frontmatter_skips_code_block_examples() {
        let content = "# Doc\n\n```markdown\n---\nsources:\n  - ./fake.md\n---\n```\n";
        let result = parse(content);
        assert!(
            result.links.is_empty(),
            "frontmatter inside code block should be ignored"
        );
        assert!(result.metadata.is_none());
    }

    #[test]
    fn extracts_metadata() {
        let content =
            "---\ntitle: My Doc\nstatus: draft\ntags:\n  - rust\n  - cli\n---\n\n# Hello\n";
        let result = parse(content);
        let meta = result.metadata.unwrap();
        assert_eq!(meta["title"], "My Doc");
        assert_eq!(meta["status"], "draft");
        assert_eq!(meta["tags"], serde_json::json!(["rust", "cli"]));
    }

    #[test]
    fn no_metadata_without_frontmatter() {
        let result = parse("# Just a heading\n");
        assert!(result.metadata.is_none());
    }

    #[test]
    fn metadata_keeps_code_spans_in_values() {
        // The edge-scan mask blanked code spans in captured metadata; metadata now
        // carries the raw value across every field, not just one — the issue flagged
        // `title`/`description` too, so assert a second field, not only `purpose`.
        let content = "---\npurpose: A code span like `widget-loader` and a path `claims.md`.\ntitle: the `Foo` service\n---\n";
        let meta = parse(content).metadata.unwrap();
        assert_eq!(
            meta["purpose"],
            "A code span like `widget-loader` and a path `claims.md`."
        );
        assert_eq!(meta["title"], "the `Foo` service");
    }

    #[test]
    fn a_stray_backtick_keeps_both_the_metadata_and_the_edges() {
        // An unclosed backtick in a value pairs with one in the body. Finding the
        // block in a masked copy of the whole file blanked everything between —
        // the closing `---` included — so the edge scan found no block and the
        // declared `sources:` entry produced nothing, on a file whose metadata
        // reported that same entry. The boundary comes from the raw content now,
        // so both agree.
        let content =
            "---\npurpose: a `widget\nsources:\n  - b.md\n---\n\n# Body with a `code span`\n";
        let result = parse(content);
        let meta = result.metadata.expect("metadata comes from the raw block");
        assert_eq!(meta["purpose"], "a `widget");
        assert_eq!(
            result.links.len(),
            1,
            "the declared source is still an edge"
        );
        assert_eq!(result.links[0].target, "b.md");
        assert_eq!(
            result.links[0].line,
            Some(4),
            "and its line is file-accurate"
        );
    }

    #[test]
    fn a_span_crossing_the_fence_neither_drops_nor_invents_an_edge() {
        // Direction one: a span that swallows the closing fence used to pull body
        // prose into the block, so a `deps:` line below it became a declared
        // source — with a line number off by the newlines the mask ate.
        let lifted = "---\na: `x\n---\ny`\ndeps: ./t.md\n---\n\n# Body\n";
        let result = parse(lifted);
        assert!(
            result.links.is_empty(),
            "body prose is not a declared source"
        );
        assert!(
            result.metadata.is_none(),
            "and the block is not frontmatter"
        );

        // Direction two: one backtick inside a properly quoted string, plus an
        // ordinary code span in the body, silently deleted every declared edge.
        let dropped =
            "---\nsources:\n  - ./t.md\nnote: \"has a ` backtick\"\n---\n\n# B\n\nA `code` span.\n";
        let result = parse(dropped);
        assert_eq!(result.links.len(), 1, "the declared source survives");
        assert_eq!(result.links[0].target, "./t.md");
    }

    #[test]
    fn a_span_hiding_a_colon_makes_the_block_unreadable() {
        // A code span hiding a `:` makes the block invalid YAML. Blanking the span
        // and reading what was left recovered `status` and reported `title` as
        // null — a value the file does not contain, invented by the recovery. The
        // block is reported instead, and the author's fix is to quote the value.
        // The value does not *open* with the span, so the reserved-indicator
        // rule does not fire and the hidden `:` is what breaks the mapping. A
        // fixture opening with a backtick duplicates the test below instead.
        let content = "---\ntitle: x `key: value`\nstatus: draft\n---\n";
        let result = parse(content);
        assert!(result.metadata.is_none());
        assert_eq!(result.diagnostics.len(), 1);
        assert_eq!(result.diagnostics[0].rule, UNREADABLE_FRONTMATTER);
    }

    #[test]
    fn a_value_opening_with_a_code_span_makes_the_block_unreadable() {
        // A value that *starts* with a code span is invalid YAML — the backtick is
        // a reserved indicator there, and Psych, saphyr and js-yaml all reject it.
        // This is the commonest block the mask used to recover, and the recovery
        // published a value the file does not contain: `purpose` came out as "is
        // the entry point", with the span the author wrote silently removed. The
        // author's fix is to quote the value or use a `|` block scalar, both
        // captured verbatim (see the tests above).
        let content = "---\npurpose: `widget-loader` is the entry point\nstatus: ok\n---\n";
        let result = parse(content);
        assert!(result.metadata.is_none());
        assert_eq!(result.diagnostics[0].rule, UNREADABLE_FRONTMATTER);
    }

    #[test]
    fn block_scalar_with_code_span_survives_whole() {
        // The reported case: a multi-line `purpose` block scalar whose spans were
        // blanked. Every character now comes back, newline included.
        let content = "---\npurpose: |\n  Plain words. A span like `widget-loader` stays.\nsources:\n  - b.md\n---\n";
        let result = parse(content);
        assert_eq!(
            result.metadata.unwrap()["purpose"],
            "Plain words. A span like `widget-loader` stays.\n"
        );
        // The real derivation is still an edge.
        assert_eq!(result.links.len(), 1);
        assert_eq!(result.links[0].target, "b.md");
    }

    #[test]
    fn metadata_handles_nested_yaml() {
        let content = "---\ntitle: Test\nauthor:\n  name: Alice\n  role: dev\n---\n";
        let result = parse(content);
        let meta = result.metadata.unwrap();
        assert_eq!(meta["author"]["name"], "Alice");
        assert_eq!(meta["author"]["role"], "dev");
    }

    #[test]
    fn no_filter_matches_everything() {
        let parser = FrontmatterParser {
            file_filter: None,
            edge_keys: Vec::new(),
        };
        assert!(parser.matches("index.md"));
        assert!(parser.matches("main.rs"));
    }

    #[test]
    fn file_filter_restricts_matching() {
        let mut builder = globset::GlobSetBuilder::new();
        builder.add(globset::Glob::new("*.md").unwrap());
        let parser = FrontmatterParser {
            file_filter: Some(builder.build().unwrap()),
            edge_keys: Vec::new(),
        };
        assert!(parser.matches("index.md"));
        assert!(!parser.matches("main.rs"));
    }

    #[test]
    fn extracts_uris() {
        let content = "---\nsources:\n  - https://example.com\n  - ./local.md\n---\n";
        let result = parse(content);
        assert_eq!(result.links.len(), 2);
        assert_eq!(result.links[0].target, "https://example.com");
        assert_eq!(result.links[1].target, "./local.md");
    }

    #[test]
    fn accepts_paths_without_prefix() {
        let content = "---\nsources:\n  - config.rs\n  - docs/setup.md\n---\n";
        let result = parse(content);
        assert_eq!(result.links.len(), 2);
        assert_eq!(result.links[0].target, "config.rs");
        assert_eq!(result.links[1].target, "docs/setup.md");
    }

    #[test]
    fn emits_absolute_paths() {
        let content = "---\nsource: /usr/local/config.toml\n---\n";
        let result = parse_with(content, &["source"]);
        assert_eq!(result.links.len(), 1);
        assert_eq!(result.links[0].target, "/usr/local/config.toml");
    }

    // The declared key is the whole of the signal. Nothing below asks what a
    // value looks like, which is the property the deleted shape filter broke.

    #[test]
    fn prose_under_a_declared_key_is_an_edge() {
        // Each of these was silently dropped for failing an extension test. A
        // value naming nothing that resolves is an edge that resolves to nothing,
        // and the rules layer says so — it does not vanish here.
        let content = "---\nsources:\n  - TBD\n  - needs review\n  - v2.0\n  - 2026-01-01\n---\n";
        assert_eq!(
            scoped(content, &["sources"]),
            vec!["TBD", "needs review", "v2.0", "2026-01-01"]
        );
    }

    #[test]
    fn a_value_naming_a_directory_is_an_edge() {
        // drft has directory nodes, so a derivation naming one is legitimate. The
        // extension test discarded it for having no dot.
        let content = "---\nsources: docs\n---\n";
        assert_eq!(scoped(content, &["sources"]), vec!["docs"]);
    }

    #[test]
    fn a_markdown_link_value_is_not_unwrapped() {
        // The target is the literal text. Unwrapping it would be inference about
        // what the author meant; naming it is what lets the finding be read.
        let content = "---\nsources: \"[Design notes](real.md)\"\n---\n";
        assert_eq!(
            scoped(content, &["sources"]),
            vec!["[Design notes](real.md)"]
        );
    }

    #[test]
    fn a_path_carrying_a_fragment_keeps_it() {
        let content = "---\nsources: ./real.md#section\n---\n";
        assert_eq!(scoped(content, &["sources"]), vec!["./real.md#section"]);
    }

    #[test]
    fn non_string_scalars_are_not_edges() {
        // Only strings are collected; a number or a boolean has no target to name.
        let content = "---\nsources:\n  - 42\n  - true\n  - null\n---\n";
        assert!(scoped(content, &["sources"]).is_empty());
    }

    #[test]
    fn an_undeclared_key_yields_no_edge() {
        // The scoping is the only filter left, so it has to hold on its own.
        let content = "---\nroute: /customers\nsources: ./a.md\n---\n";
        assert_eq!(scoped(content, &["sources"]), vec!["./a.md"]);
    }

    #[test]
    fn no_declared_keys_yields_no_edges() {
        // A metadata-only graph: the block is still captured, and nothing is an
        // edge. The config layer hints about this shape; the parser just obeys.
        let content = "---\ntitle: T\nsources: ./a.md\n---\n";
        let result = parse_with(content, &[]);
        assert!(result.links.is_empty());
        assert_eq!(result.metadata.unwrap()["title"], "T");
    }
}
