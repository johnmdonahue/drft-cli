use saphyr::{LoadableYamlNode, MarkedYaml, Scalar, YamlData};
use std::borrow::Cow;

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
/// The lookahead is what makes `\r\n` one break rather than two. `str::lines`
/// strips exactly one `\r`, so a source line ending in two of them arrives here
/// as a masked `\r\n` — and testing characters one at a time opened a row for
/// each, over-counting by one and reintroducing the same defect a line in the
/// other direction.
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

/// A code-masked copy of a block, with the source lines its columns came from.
///
/// The mask blanks a code span to spaces, newlines included, because fusing a
/// span's lines is part of what lets a block parse: a span can hide a `:` that
/// would otherwise break the mapping. That fusing is also why a masked line
/// number is not a source line number — the copy is shorter by one line per
/// newline swallowed, so every value below a span resolved that many lines too
/// high.
///
/// Recording the correspondence fixes the line without touching the text. The
/// alternative — keeping the newlines so the copy stays the same shape — changes
/// what the mask *says*, not just where it says it: `collect_links` reads a
/// scalar's value out of this copy, so a span inside a link value would fold to a
/// different target string, and the edge, the lockfile entry, and the resolution
/// would move with it.
struct Masked {
    text: String,
    /// Per masked line, the source lines its columns came from: `(col, line)`
    /// pairs meaning "from 0-based column `col` onward, the source line is
    /// `line`". Every masked line opens with one; a fused line gains one per
    /// newline the mask swallowed.
    ///
    /// **Zero-based, because `saphyr::Marker::col()` is** — its own doc comment
    /// says 1-indexed and the scanner initialises the column to 0 and resets it to
    /// 0 on a newline. Building this table 1-based and comparing it against that
    /// leaves the first column of every masked line resolving to nothing.
    ///
    /// Per column rather than per line, because a value can sit *after* a
    /// multi-line span closes and share a masked line with the span's opening.
    /// Mapping the whole line to where it began reports the line the span opened
    /// on, for a value the mask did not touch.
    lines: Vec<Vec<(usize, usize)>>,
}

#[cfg(test)]
impl Masked {
    /// The source line a value reported at 1-based `line` and 0-based `col` came
    /// from — saphyr's own convention for each, whatever its doc comments say.
    ///
    /// Test-only sugar over [`source_line_in`], which is what the parser calls: a
    /// mask and a raw block build the same table shape and are read by one lookup.
    fn source_line(&self, line: usize, col: usize) -> usize {
        source_line_in(&self.lines, line, col)
    }
}

/// The source line a value reported at 1-based `line` and 0-based `col` came from,
/// against any table built by the walk below.
fn source_line_in(lines: &[Vec<(usize, usize)>], line: usize, col: usize) -> usize {
    let Some(spans) = lines.get(line.wrapping_sub(1)) else {
        return line;
    };
    // The first pair sits at column 0 and `col` is zero-based, so a line the
    // table holds always resolves. The fallbacks cover a line it does not
    // hold: saphyr reports line 0 for a scalar carrying a bare `!` tag, and
    // there is no zeroth masked line to correct against.
    spans
        .iter()
        .rev()
        .find(|(start, _)| *start <= col)
        .map_or(line, |(_, source)| *source)
}

/// The line table for a block nothing has been blanked out of: one row per line
/// saphyr counts, carrying the file line that row sits on.
///
/// **A raw block is not already numbered the way the file is.** saphyr breaks on
/// `\n` and on a lone `\r`; a file's lines are counted by `\n` alone, which is what
/// an editor and `grep -n` report. So a stray carriage return above a value opens a
/// row here without advancing the file line, and reporting saphyr's line unaltered
/// moves that value and every one below it — the same shift the mask's table exists
/// to correct, arriving by a route that does no masking at all.
///
/// This is the `else` arm of [`strip_code`]'s walk with the span handling removed,
/// and it has to stay that: the two tables are read by one lookup and any
/// disagreement about what opens a line desynchronizes them.
fn raw_line_table(text: &str) -> Vec<Vec<(usize, usize)>> {
    let chars: Vec<char> = text.chars().collect();
    let mut lines = Vec::new();
    let mut current = vec![(0usize, 1usize)];
    let mut source_line = 1usize;
    for i in 0..chars.len() {
        if opens_line(&chars, i) {
            lines.push(std::mem::take(&mut current));
            if chars[i] == '\n' {
                source_line += 1;
            }
            current = vec![(0, source_line)];
        }
    }
    lines.push(current);
    lines
}

/// Where the code span opening at `i` ends — one past its closing run — or `None`
/// when nothing closes it.
///
/// Shared so a block mask and a value mask pair backticks by one rule. They mask
/// different things and must not be the same function: a block mask also blanks
/// fenced regions, which is meaningless applied to a single scalar and destroys
/// any value whose text opens with a fence marker. Only the pairing is common.
fn span_end(chars: &[char], i: usize) -> Option<usize> {
    let mut ticks = 0;
    while i + ticks < chars.len() && chars[i + ticks] == '`' {
        ticks += 1;
    }
    let mut j = i + ticks;
    while j + ticks <= chars.len() {
        if chars[j..j + ticks].iter().all(|c| *c == '`') {
            return Some(j + ticks);
        }
        j += 1;
    }
    None
}

/// Mask fenced blocks and inline backtick spans, replacing every character with a
/// space, and record which source lines each masked line's columns came from.
fn strip_code(content: &str) -> Masked {
    // First strip fenced code blocks (``` and ~~~)
    let mut result = String::with_capacity(content.len());
    let mut in_code_block = false;
    let mut fence_marker = "";

    for line in content.lines() {
        let trimmed = line.trim_start();
        if !in_code_block {
            if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                in_code_block = true;
                fence_marker = if trimmed.starts_with("```") {
                    "```"
                } else {
                    "~~~"
                };
                result.push_str(&" ".repeat(line.len()));
            } else {
                result.push_str(line);
            }
        } else if trimmed.starts_with(fence_marker) && trimmed.trim() == fence_marker {
            in_code_block = false;
            result.push_str(&" ".repeat(line.len()));
        } else {
            result.push_str(&" ".repeat(line.len()));
        }
        result.push('\n');
    }

    // Then strip inline code spans (single and double backticks), recording a
    // breakpoint wherever a swallowed newline means the columns after it came from
    // the next source line. Only this pass fuses lines; the fenced pass above
    // rebuilds the text line for line.
    //
    // That pass blanks a line to `" ".repeat(line.len())`, which is a *byte* count,
    // so a fenced line holding multi-byte characters comes back wider in characters
    // than its source. A span opened before a fence encloses the blanked fence lines
    // and does record breakpoints inside them, so the widening is not out of reach —
    // it simply does not matter, because this counter and saphyr both count
    // characters over the same masked text. The widening moves both equally.
    let mut cleaned = String::with_capacity(result.len());
    let mut lines: Vec<Vec<(usize, usize)>> = Vec::new();
    let mut current = vec![(0usize, 1usize)];
    let mut source_line = 1usize;
    let mut col = 0usize;
    let chars: Vec<char> = result.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '`' {
            if let Some(span_end) = span_end(&chars, i) {
                // Blank the entire span — backticks, content, and closing
                // backticks, newlines included.
                //
                // Keeping a span's newlines instead is not an option, for two
                // reasons that are easy to miss: it moves the boundary
                // `parsed_block` decides, and it changes the *text* of a value,
                // because `collect_links` reads a scalar's string out of this copy
                // and YAML folds a preserved newline to a single space. The edge
                // target, the node it resolves to, and the lockfile entry move
                // with it.
                //
                // Blanking is therefore what shortens the masked block, so a value
                // below a span resolves one line too high per newline swallowed —
                // a number that reaches `drft edges`, `drft impact`, and every
                // finding. The table is what corrects it.
                // A blanked newline advances the source line without ending the
                // masked one — which is exactly the shift the table records.
                let total = span_end - i;
                for c in &chars[i..i + total] {
                    cleaned.push(' ');
                    col += 1;
                    if *c == '\n' {
                        // The masked line continues, but everything from the next
                        // column on came from the following source line.
                        source_line += 1;
                        current.push((col, source_line));
                    }
                }
                i += total;
            } else {
                // No closing — keep the backtick as-is. It still occupies a
                // column, so the counter has to advance with it.
                cleaned.push(chars[i]);
                col += 1;
                i += 1;
            }
        } else {
            if opens_line(&chars, i) {
                // A lone `\r` starts a new line for saphyr, so it starts a new
                // row of this table — but it is not a line break in the file, so
                // the source line it maps to does not advance.
                lines.push(std::mem::take(&mut current));
                if chars[i] == '\n' {
                    source_line += 1;
                }
                current = vec![(0, source_line)];
                col = 0;
            } else {
                col += 1;
            }
            cleaned.push(chars[i]);
            i += 1;
        }
    }
    lines.push(current);

    Masked {
        text: cleaned,
        lines,
    }
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
        // Two independent parses over one block, one per job, and both choose
        // their rendering by the same test: the raw block where it is a YAML
        // mapping, the masked copy otherwise. The edge scan keeps a subset of
        // what the metadata pass keeps — only the values under a declared key —
        // but the two never differ in what the block *says*.
        ParseResult {
            links: self.extract_links(content),
            // Frontmatter defines no addressable sub-file positions; anchors come
            // from the markdown body's headings.
            anchors: Vec::new(),
            metadata: extract_metadata(content),
            diagnostics: unreadable_block(content).into_iter().collect(),
        }
    }
}

impl FrontmatterParser {
    /// Extract frontmatter link edges from the frontmatter block, preferring the
    /// *raw* block and falling back to the masked copy only when the raw block is
    /// not a YAML mapping on its own. An absent or malformed block yields no
    /// links.
    ///
    /// A code span inside a value is part of that value: it is never blanked
    /// out of the target, so the edge names what the file declared.
    fn extract_links(&self, content: &str) -> Vec<Link> {
        // The boundary is the shared one; only the masking is this job's own.
        // Finding the block in a masked copy of the *whole file* let a code span
        // crossing the closing fence move the boundary — lifting a link out of
        // body prose in one direction, and silently dropping every declared
        // `sources:` edge in the other, on a file whose metadata still reported
        // them. `strip_code` blanks whole spans, newlines included, so a moved
        // boundary also misreports the lines it does find.
        let Some((_, block)) = parsed_block(content) else {
            return Vec::new();
        };
        // Raw first, masked second — the order `extract_metadata` already uses,
        // and reading the mask unconditionally is what let the two contradict
        // each other. `strip_code` knows nothing about YAML, so a fence opened
        // inside a block scalar latched its fenced pass and blanked every line
        // below it, `sources:` included. The raw block stayed well-formed
        // throughout, so metadata reported the derivation while the edge scan
        // found none and `detached-node` was the only thing said about it.
        //
        // Nothing fails to parse in that case, so no parse-failure diagnostic can
        // reach it. Reaching the mask only when the raw block *is not a mapping*
        // removes the class instead of reporting it. Note the condition: a raw
        // mapping yielding no candidates at all is still the answer, and does not
        // fall through. Reading this as "when the raw block yields nothing" gives
        // a different program that no test distinguishes.
        if let Some(links) = self.scan(block, &raw_line_table(block)) {
            return links;
        }
        // The same mask the gate and the metadata fallback use, so a value's text
        // here is the text they saw. Only the reported line is corrected, against
        // the table the mask built while fusing.
        let masked = strip_code(block);
        self.scan(&masked.text, &masked.lines).unwrap_or_default()
    }

    /// Scan one candidate rendering of the block, or `None` when it is not a YAML
    /// mapping — the same gate `extract_metadata` applies, so the two agree on
    /// which rendering they read.
    ///
    /// `lines` maps a line of `text` back to the file line it came from. **Both
    /// renderings need one.** The mask needs it because blanking a span's newlines
    /// shortens the copy; the raw block needs it because saphyr breaks on a lone
    /// `\r` and a file does not. Neither table is optional and neither is the
    /// other's.
    fn scan(&self, text: &str, lines: &[Vec<(usize, usize)>]) -> Option<Vec<Link>> {
        // Malformed YAML contributes nothing — drft is not a YAML linter.
        let docs = MarkedYaml::load_from_str(text).ok()?;
        let root = docs.first()?;
        // Mirrors `extract_metadata`'s gate so the two jobs choose the same
        // rendering by the same test. Blanking cannot turn a non-mapping into a
        // mapping, so no input is known to reach this arm; it is here to keep the
        // two conditions identical rather than merely equivalent today.
        if !matches!(root.data, YamlData::Mapping(_)) {
            return None;
        }

        let mut candidates = Vec::new();
        let wanted: std::collections::HashSet<&str> =
            self.edge_keys.iter().map(String::as_str).collect();
        collect_scoped(root, &wanted, &mut candidates);
        Some(
            candidates
                .into_iter()
                // Every string reachable through a declared key is an edge. The
                // key is the whole of the signal, so nothing here asks what a
                // value looks like — one that resolves to nothing becomes an edge
                // that resolves to nothing, and `unresolved-edge` says so.
                .map(|(target, line, col)| Link {
                    target,
                    line: Some(source_line_in(lines, line, col)),
                })
                .collect(),
        )
    }
}

/// Capture the frontmatter block as node metadata. Prefers the *raw* block so a
/// code span in a value — the service name or path an author put in backticks —
/// survives as the prose it is, instead of coming back blanked.
///
/// Falls back to the masked block only when the raw block is not valid YAML on its
/// own: an unquoted value that *begins* with a backtick, or hides a `:` inside a
/// span, is rejected by every YAML parser (backtick is a reserved indicator), so
/// drft — not a YAML linter — cannot recover it. The fallback keeps the sibling
/// fields structured at the cost of blanking that one span; the author's fix is to
/// quote the value or use a `|` block scalar, both of which the raw path captures
/// verbatim. Returns `None` when there is no frontmatter block at all.
///
/// The masking applies to the block, not to the file — see [`parsed_block`].
fn extract_metadata(content: &str) -> Option<serde_json::Value> {
    let (_, block) = parsed_block(content)?;
    for candidate in [Cow::Borrowed(block), Cow::Owned(strip_code(block).text)] {
        if let Ok(docs) = MarkedYaml::load_from_str(candidate.as_ref())
            && let Some(root) = docs.first()
            // `parsed_block` already gated on one of these parsing as a mapping,
            // but not necessarily *this* one. Checking here keeps the return type
            // honest without resting on an argument about what blanking can do.
            && matches!(root.data, YamlData::Mapping(_))
        {
            return Some(to_json(root));
        }
    }
    None
}

/// The frontmatter block this parser reads: where it ends in `content`, and its
/// raw text. `None` when the fences name no block, and also when they name one
/// whose YAML is not a mapping — [`unreadable_block`] separates those two, which
/// is what lets an unreadable block be reported instead of dropped.
///
/// The block is found in the raw content. Looking for it in a `strip_code` copy
/// of the **whole file** let a backtick opened in frontmatter and closed in the
/// body blank the closing fence and move the boundary past it, so the edge scan
/// read a block nothing else agreed on.
///
/// Masking is applied to the block text instead, which serves the reason it
/// exists — a code span can hide a `:` that would otherwise break the mapping —
/// without letting a span decide where the block ends.
fn parsed_block(content: &str) -> Option<(usize, &str)> {
    let (block, end) = frontmatter_block(content)?;
    yields_metadata(block).then_some((end, block))
}

/// Whether a recognized block yields metadata, in any rendering this parser will
/// read it in.
///
/// Either rendering counts: the block as written, or the block with code spans
/// blanked, since a span can hide a `:` that breaks the mapping. This is a
/// question of whether *any* reading yields a mapping, so it is unordered —
/// which rendering's values are used is [`extract_metadata`]'s decision, and
/// that one is ordered.
///
/// **No test distinguishes the two disjuncts**, because no input is known where
/// the raw block parses and the masked copy does not. Blanking a span can only
/// remove characters, so such an input may not exist; that is unproven either
/// way, and it is why the raw arm is kept rather than argued away.
///
/// **One spelling, called by both the reader and the diagnostic.** Written out
/// twice, the two drift, and the failure is not a missing finding but a
/// contradictory one: a run reporting that a block's keys were all dropped while
/// printing those same keys under `@frontmatter`.
fn yields_metadata(block: &str) -> bool {
    is_mapping(block) || is_mapping(&strip_code(block).text)
}

/// Whether `block` parses as a YAML **mapping** — the shape that yields metadata.
///
/// This no longer decides what is frontmatter; the fences do, and they are the
/// markdown library's. It decides only whether a block drft already claims has
/// anything a node can carry. A comment-only block parses to no document at all
/// and a bare scalar carries no keys, so neither contributes metadata — and both
/// raise [`UNREADABLE_FRONTMATTER`] rather than falling through in silence.
///
/// The gate used to be the boundary too, which is what made the YAML/markdown
/// collision load-bearing: `# First` is a comment and an ATX heading, and
/// `---\nMy Title\n---` is a block and a setext heading, so a wrong answer here
/// deleted real content. Now the two questions are separate and only this one
/// rests on the YAML.
fn is_mapping(block: &str) -> bool {
    MarkedYaml::load_from_str(block)
        .ok()
        .and_then(|docs| {
            docs.first()
                .map(|doc| matches!(doc.data, YamlData::Mapping(_)))
        })
        .unwrap_or(false)
}

/// The byte offset just past a leading YAML frontmatter block, or `None` when the
/// file does not open with one.
///
/// The block has to parse as a **mapping**, which is what separates a block this
/// parser contributes metadata from (see [`crate::builders::frontmatter`]) from
/// one it only recognizes. Nothing in production calls this: the markdown parser
/// takes its block from the markdown library, and this remains as the offset half
/// of the boundary, read by tests alone.
#[cfg(test)]
pub fn mapping_block_end(content: &str) -> Option<usize> {
    parsed_block(content).map(|(end, _)| end)
}

/// A frontmatter fence is exactly three characters wide. A fourth makes the line
/// a thematic break.
const FENCE_WIDTH: usize = 3;

/// The rule an unreadable block becomes. A rule rather than a hint because a
/// hint cannot change an exit code, and a repository whose derivations are
/// declared in frontmatter needs a dropped block to be able to fail its run.
pub const UNREADABLE_FRONTMATTER: &str = "unreadable-frontmatter";

/// A block whose fences are a frontmatter block and whose YAML is not a mapping.
///
/// This is the whole reason the fence scan and the mapping gate are separate
/// questions. The markdown parser skips whatever the fences claim, so a block
/// that fails the mapping gate reaches neither graph: no anchors, no metadata,
/// no edges. Before this it was reported by accident — the block stayed visible
/// to the markdown parser and minted a wrong anchor, which was at least
/// something to notice. Silence is the worse failure, so it is named instead.
fn unreadable_block(content: &str) -> Option<Diagnostic> {
    let (block, _) = frontmatter_block(content)?;
    if yields_metadata(block) {
        return None;
    }
    Some(Diagnostic {
        rule: UNREADABLE_FRONTMATTER,
        // What the reader has to know is that the block was recognized and is
        // being skipped wholesale — not which YAML construct failed, which drft
        // is not a linter for and cannot report faithfully across two renderings.
        message: "frontmatter block is not a YAML mapping, so its keys, values \
                  and edges are all dropped"
            .to_string(),
        // The opening fence, which is the line a reader edits. Line 1 whenever
        // there is a block at all, since the fence has to open the file.
        line: Some(1),
    })
}

/// Extract the YAML frontmatter block — its raw text, and the byte offset in
/// `content` just past the closing fence.
///
/// Called on the **raw** content only, by [`parsed_block`] and [`unreadable_block`]. Calling it on a
/// `strip_code` copy of the whole file finds a different boundary whenever a span
/// crosses the closing fence, because blanking the span takes the fence with it —
/// which is how the edge scan came to read a block nothing else agreed on. The
/// slice keeps the newline after the opening fence, so a node's line within the
/// block equals its line within the file.
///
/// The three fence conditions mirror the markdown library that decides the same
/// block for the [markdown parser](super::markdown), character for character
/// rather than by approximation. A rule derived from the inputs that happened to
/// be tested matches those inputs; a rule copied from the component it has to
/// agree with matches the component. Every place these two drift apart, one
/// parser claims a span the other renders, and the file is both metadata and
/// prose at once.
fn frontmatter_block(content: &str) -> Option<(&str, usize)> {
    let rest = content.strip_prefix("---")?;
    // The opening fence has to be a line of its own, carrying nothing but
    // whitespace after it. `---key: v` is not frontmatter under any convention
    // that writes it, and reading it as one invents metadata out of a document's
    // first paragraph.
    //
    // This also settles the fence width, which is why nothing tests that
    // separately: a fourth dash falls inside this line and is not whitespace, so
    // `----` is rejected here, exactly as the renderer rejects it. A standalone
    // three-dash guard was tried and removed — no input reached it that this did
    // not already reject, and a guard that cannot fail measures nothing.
    let opening_line = rest.find('\n').unwrap_or(rest.len());
    if !rest[..opening_line]
        .bytes()
        .all(|b| b.is_ascii_whitespace())
    {
        return None;
    }

    let mut cursor = 0;
    let mut first_line = true;
    while let Some(offset) = rest[cursor..].find('\n') {
        let line_start = cursor + offset + 1;
        let line = &rest[line_start..];
        let closes = closes_block(line);
        // A block's first line may be neither blank nor the closing fence. This
        // is what makes `---` above a blank line a thematic break rather than an
        // empty block, and dropping it is not cosmetic: `---\n\nkey: v\n---`
        // parses as a mapping, so without this the block is claimed here and
        // rendered as a setext heading there.
        if first_line {
            if closes || is_blank(line) {
                return None;
            }
            first_line = false;
        }
        if closes {
            // Past the opening fence, the block, its newline, and the closing
            // fence characters. Trailing spaces after the fence are outside it,
            // as they were when the closer was matched as a bare prefix.
            return Some((
                &rest[..line_start - 1],
                "---".len() + line_start + FENCE_WIDTH,
            ));
        }
        cursor = line_start;
    }
    None
}

/// Whether `line` opens with a closing frontmatter fence: exactly three `-` or
/// exactly three `.`, then spaces, then the end of the line.
///
/// A fourth fence character disqualifies the line and a tab after the fence
/// disqualifies it; spaces do not, and the end of the file ends a line. Matching
/// a bare `\n---` prefix instead — which is what this replaced — accepts `----`
/// and `---x` as closers, ending the block where the markdown library does not.
fn closes_block(line: &str) -> bool {
    let bytes = line.as_bytes();
    let fence = match bytes.first() {
        // `...` is YAML's document-end marker, and the library accepts it as a
        // closer wherever it accepts `---`.
        Some(b'-') => b'-',
        Some(b'.') => b'.',
        _ => return false,
    };
    if bytes.iter().take_while(|&&b| b == fence).count() != FENCE_WIDTH {
        return false;
    }
    let after = &bytes[FENCE_WIDTH..];
    let spaces = after.iter().take_while(|&&b| b == b' ').count();
    matches!(after.get(spaces), None | Some(b'\n' | b'\r'))
}

/// Whether `line` is blank up to its end.
///
/// Blank means the four characters the markdown library counts as non-newline
/// whitespace. **This set is not the one the opening fence uses** — that check is
/// `u8::is_ascii_whitespace`, which excludes vertical tab and includes `\r`,
/// mirroring a different library predicate. Unifying them looks like tidying and
/// changes what both accept.
fn is_blank(line: &str) -> bool {
    let bytes = line.as_bytes();
    let indent = bytes
        .iter()
        // The four the markdown library counts as non-newline whitespace. Space
        // and tab alone read a line holding a form feed as content, which opens a
        // block the renderer does not — so the file reports unreadable
        // frontmatter here while publishing an anchor slugged from that same
        // text there. Mirroring a rule means mirroring its character set.
        .take_while(|&&b| matches!(b, b' ' | b'\t' | 0x0b | 0x0c))
        .count();
    matches!(bytes.get(indent), None | Some(b'\n' | b'\r'))
}

/// Collect string leaf *values* (not keys) with their position in whichever
/// rendering the caller parsed — the frontmatter link candidates. The line is
/// 1-based and the column is 0-based, which is saphyr's convention for each
/// whatever its docs say. Both renderings carry a table mapping that position back
/// to a file line; neither is already numbered the way the file is.
///
/// Mirrors the metadata walk but keeps only strings.
fn collect_links(node: &MarkedYaml, out: &mut Vec<(String, usize, usize)>) {
    match &node.data {
        YamlData::Value(Scalar::String(s)) => {
            out.push((s.to_string(), node.span.start.line(), node.span.start.col()))
        }
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
    out: &mut Vec<(String, usize, usize)>,
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

    /// The mask blanks every character of a span, newlines included, so the copy
    /// is the same length and shorter by a line per newline swallowed. Asserting
    /// the text directly is what stops a change to the blanking passing on the
    /// strength of an integration test that only reads line numbers.
    #[test]
    fn the_mask_blanks_a_span_to_spaces_and_fuses_its_lines() {
        let masked = strip_code("a: `one\ntwo` b\nc: d\n");
        // Nine characters of span — backticks, content, and the newline between —
        // become nine spaces, and the two source lines become one masked line.
        assert_eq!(masked.text, "a:           b\nc: d\n");
    }

    /// The table's exact shape, because the arithmetic behind it is invisible to
    /// any test that only reads a corrected line. Five separate changes to the
    /// column counter left every integration test green; each moves this.
    ///
    /// Columns are **zero-based**, matching `saphyr::Marker::col()` rather than
    /// its doc comment.
    #[test]
    fn the_table_records_a_breakpoint_at_every_swallowed_newline() {
        // Masked line 1 is `a:` plus the blanked span plus ` b`, fusing source
        // lines 1 and 2. Column 0 opens on line 1; from column 8 — just past the
        // space that replaced the newline — the source is line 2.
        let masked = strip_code("a: `one\ntwo` b\nc: d\n");
        assert_eq!(
            masked.lines,
            vec![vec![(0, 1), (8, 2)], vec![(0, 3)], vec![(0, 4)]]
        );
    }

    /// Two newlines inside one span means two breakpoints on one masked line, and
    /// the later one has to win.
    #[test]
    fn a_taller_span_records_one_breakpoint_per_line_it_swallows() {
        let masked = strip_code("a: `one\ntwo\nthree` b\n");
        assert_eq!(masked.lines[0], vec![(0, 1), (8, 2), (12, 3)]);
        assert_eq!(masked.source_line(1, 0), 1);
        assert_eq!(masked.source_line(1, 7), 1);
        assert_eq!(masked.source_line(1, 8), 2);
        assert_eq!(masked.source_line(1, 11), 2);
        assert_eq!(masked.source_line(1, 12), 3);
        assert_eq!(masked.source_line(1, 20), 3);
    }

    /// Column 0 is a real column and must resolve to the line the masked line
    /// opened on. Built one-based and compared against saphyr's zero-based
    /// column, it resolves to nothing and the correction silently does not happen.
    #[test]
    fn column_zero_resolves_to_the_line_the_masked_line_opened_on() {
        let masked = strip_code("a: `one\ntwo` b\nc: d\n");
        assert_eq!(masked.source_line(1, 0), 1);
        assert_eq!(masked.source_line(2, 0), 3);
    }

    /// A span on a later line records its breakpoint at that line's own column.
    /// The counter has to reset at every masked newline for that to hold — without
    /// the reset it carries the preceding lines' width forward, and the breakpoint
    /// lands past every column a value on that line can occupy, so the correction
    /// silently stops happening.
    #[test]
    fn the_column_restarts_on_every_masked_line() {
        let masked = strip_code("a: bbbbbbbb\nc: `one\ntwo` d\n");
        assert_eq!(
            masked.lines,
            vec![vec![(0, 1)], vec![(0, 2), (8, 3)], vec![(0, 4)]]
        );
        assert_eq!(masked.source_line(2, 7), 2);
        assert_eq!(masked.source_line(2, 8), 3);
    }

    /// A line the table does not hold falls back to the masked line rather than
    /// panicking.
    ///
    /// One thing reaches it: saphyr reports line 0 for a scalar carrying a *local*
    /// tag — `!`, `!foo`, or a verbatim `!<…>` — and there is no zeroth masked
    /// line. A global tag such as `!!str` reports its real line. This predates the
    /// table and is identical on the unmodified parser. Line-terminator disagreements
    /// do not — `opens_line` matches saphyr break for break, so the table holds a
    /// row for every line the parser can name.
    #[test]
    fn a_line_the_table_does_not_hold_falls_back() {
        let masked = strip_code("a: b\n");
        assert_eq!(masked.source_line(0, 0), 0);
        assert_eq!(masked.source_line(99, 4), 99);
    }

    /// A lone carriage return opens a new row of the table without advancing the
    /// source line.
    ///
    /// saphyr breaks on it and `str::lines` does not, so the table has to break on
    /// it too or its rows stop lining up with the parser's line numbers. But a
    /// file's lines are counted by `\n` alone — which is what an editor reports —
    /// so both rows map to the same source line.
    #[test]
    fn a_lone_carriage_return_opens_a_row_without_advancing_the_source_line() {
        let masked = strip_code("a: one\rb: two\nc: three\n");
        // Rows one and two both map to source line 1, split by the `\r`. The
        // trailing row is the empty line after the final newline the fenced pass
        // appends — never looked up, and asserted here so it does not read as an
        // off-by-one to the next person auditing this table.
        assert_eq!(
            masked.lines,
            vec![vec![(0, 1)], vec![(0, 1)], vec![(0, 2)], vec![(0, 3)]]
        );
    }

    /// `\r\n` is one line break, not two.
    ///
    /// `str::lines` strips exactly one `\r`, so a source line ending in two of
    /// them arrives at the inline pass as a masked `\r\n` — which saphyr counts
    /// once. Testing characters one at a time opens a row for each and
    /// over-counts, which is the same defect as under-counting, one line the
    /// other way.
    #[test]
    fn a_carriage_return_before_a_newline_opens_one_row_not_two() {
        // Two carriage returns in the source, one surviving `str::lines`.
        let masked = strip_code("a: one\r\r\nb: two\n");
        assert_eq!(
            masked.lines,
            vec![vec![(0, 1)], vec![(0, 2)], vec![(0, 3)]],
            "`b` is on source line 2, as it is for a plain CRLF file"
        );
        assert_eq!(masked.lines, strip_code("a: one\r\nb: two\n").lines);
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
        let masked = strip_code("a: 1\n\rb: 2\n");
        assert_eq!(
            masked.lines,
            vec![vec![(0, 1)], vec![(0, 2)], vec![(0, 2)], vec![(0, 3)]],
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
            let masked = strip_code(&format!("a: 1{separator}b: 2\n"));
            assert_eq!(
                masked.lines,
                vec![vec![(0, 1)], vec![(0, 2)]],
                "{separator:?} is not a line break to this parser"
            );
        }
    }

    /// A backtick with no partner is copied through rather than blanked, and a
    /// span can still follow it — so the counter has to advance across it.
    ///
    /// The tempting argument is that an unmatched backtick must be the last one in
    /// the block, since the scan pairs each with any later one. That is false: a
    /// run of two with no two-run closer is consumed **one at a time**, and the
    /// survivor pairs with a later single backtick. Here the leading run is
    /// ``` `` ```, the first is copied, and the second opens a span that swallows
    /// the newline — putting a breakpoint after a character the counter would
    /// otherwise not have counted.
    #[test]
    fn a_copied_backtick_still_advances_the_column() {
        let masked = strip_code("``\n`\n");
        assert_eq!(masked.text, "`   \n");
        assert_eq!(masked.lines, vec![vec![(0, 1), (3, 2)], vec![(0, 3)]]);
    }

    #[test]
    fn a_code_span_crossing_the_fence_does_not_move_the_block() {
        // A backtick opened in frontmatter and closed in the body. Masking the
        // whole file would blank the closing fence and push the block past it, so
        // this parser would claim a span the markdown parser cannot see — and the
        // file would publish anchors slugged out of what this one had claimed.
        let content = "---\npurpose: `a: b\n---\nc: d` tail\nkey: v\n---\n\n# Body\n";
        assert!(parse(content).metadata.is_none());
        assert!(
            mapping_block_end(content).is_none(),
            "and this parser declines it, though the markdown parser claims it"
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
        // The assertion is on the **boundary offset**, not on the metadata: saphyr
        // stops at `...` on its own, so a parser that ran to the later `---` still
        // yields `{title: Doc}` and a metadata-only test passes with this fence
        // rule deleted. The offset is what actually moves.
        let content = "---\ntitle: Doc\n...\npurpose: below\n---\n\nBody\n";
        assert_eq!(
            mapping_block_end(content),
            Some("---\ntitle: Doc\n...".len()),
            "the block ends at the document-end marker, not at the later fence"
        );
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
    fn the_opening_fence_whitespace_set_is_not_the_blank_line_set() {
        // The renderer's opening-fence scan is `is_ascii_whitespace`, which
        // excludes vertical tab; its blank-line scan includes it. Two predicates,
        // deliberately different, and the temptation is to unify them.
        //
        // The observable is whether a block is *recognized*, not whether it
        // yields metadata: a form feed opens the block's text and saphyr rejects
        // that, so the fence accepting it shows up as a diagnostic rather than as
        // keys. Asserting on metadata alone cannot tell the two fences apart.
        let vertical_tab = parse("---\u{0b}\nkey: v\n---\n\nBody\n");
        assert!(vertical_tab.metadata.is_none());
        assert!(
            vertical_tab.diagnostics.is_empty(),
            "a vertical tab does not open a fence, so there is no block to report"
        );

        let form_feed = parse("---\u{0c}\nkey: v\n---\n\nBody\n");
        assert!(
            !form_feed.diagnostics.is_empty(),
            "a form feed does open a fence, so the block is recognized"
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
    fn mapping_block_end_agrees_with_what_this_parser_extracts() {
        // The offset and the metadata describe one block, so a disagreement means
        // this parser contributed keys from a span it does not claim.
        let frontmatter = "---\ntitle: Doc\n---\n\n# Body\n";
        assert_eq!(
            mapping_block_end(frontmatter),
            Some("---\ntitle: Doc\n---".len())
        );
        assert!(parse(frontmatter).metadata.is_some());

        // A code span hiding a `:`: this parser recovers via the masked copy, so
        // the boundary has to as well.
        let masked_only = "---\npurpose: use `a: b` here\n---\n\n# Body\n";
        assert!(mapping_block_end(masked_only).is_some());
        assert!(parse(masked_only).metadata.is_some());

        // A thematic break above a setext title is not frontmatter either way.
        assert!(mapping_block_end("---\nJust A Title\n---\n\nBody\n").is_none());
        assert!(mapping_block_end("---\n\n# First\n\n---\n\n# Second\n").is_none());
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
    fn metadata_falls_back_to_masked_when_raw_is_not_valid_yaml() {
        // A code span hiding a `:` makes the raw block invalid YAML; the masked
        // parse stands in so the block still yields structured metadata rather than
        // being dropped whole. `title` blanks to null — the tell that the *masked*
        // path ran, not raw (raw would keep the backticks or fail outright) — while
        // the sibling `status` comes through.
        let content = "---\ntitle: `key: value`\nstatus: draft\n---\n";
        let meta = parse(content)
            .metadata
            .expect("masked fallback keeps metadata when raw YAML is invalid");
        assert!(
            meta["title"].is_null(),
            "masked span blanks to null: {meta}"
        );
        assert_eq!(meta["status"], "draft");
    }

    #[test]
    fn unquoted_leading_backtick_is_invalid_yaml_and_blanks_via_fallback() {
        // Known limitation: a value that *starts* with a code span is invalid YAML
        // (backtick is a reserved indicator — Psych, saphyr, and js-yaml all reject
        // it), so the raw parse fails and the masked fallback blanks the leading
        // span. Sibling fields are unharmed. The author's fix is to quote the value
        // or use a `|` block scalar, both captured verbatim (see the tests above).
        let content = "---\npurpose: `widget-loader` is the entry point\nstatus: ok\n---\n";
        let meta = parse(content).metadata.unwrap();
        assert_eq!(meta["purpose"], "is the entry point");
        assert_eq!(meta["status"], "ok");
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
