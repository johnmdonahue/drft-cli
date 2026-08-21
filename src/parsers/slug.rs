//! GitHub's heading-anchor algorithm: the function that decides what `#fragment`
//! a heading answers to.
//!
//! drft runs this over the **heading** and compares a link's fragment to the
//! result byte-for-byte. It never runs over the fragment. Normalizing the citing
//! side would buy acceptances the platform does not grant — slugging `#OBS 92`
//! yields `obs-92` and would match, where a browser sends `#OBS%2092` and finds
//! nothing — and certifying a link that 404s for a reader is the one thing
//! sub-file addressing must not do.
//!
//! The algorithm is undocumented; this follows `github-slugger`, which is what
//! GitHub's markdown pipeline runs. Its surprises are pinned by the tests below
//! rather than described here, because the tests are what a future change is
//! measured against.

use std::collections::HashMap;
use unicode_general_category::{GeneralCategory, get_general_category};

/// The anchor a heading's rendered text answers to.
///
/// Downcase, drop every character that is not a letter, digit, underscore,
/// hyphen, or space, then turn spaces into hyphens. Punctuation is **removed
/// rather than replaced**, so a character sitting between two spaces leaves both
/// behind and yields a double hyphen — `Sizing — notes` becomes
/// `sizing--notes`, which is the trap worth knowing about when a heading is also
/// an identity.
///
/// "Letter, digit, underscore" approximates the `\p{Word}` class the reference
/// implementation keeps. Combining marks are part of that class and are kept
/// here too, so an NFD-normalized `café` does not lose its accent and slug as
/// `cafe`.
pub fn slug(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars().flat_map(char::to_lowercase) {
        if ch == ' ' {
            out.push('-');
        } else if ch == '-' || ch == '_' || ch.is_alphanumeric() || is_mark(ch) {
            out.push(ch);
        }
    }
    out
}

/// Whether `ch` is a Unicode combining mark (`\p{M}`) — kept by the slug so a
/// decomposed character survives as the character it composes.
fn is_mark(ch: char) -> bool {
    matches!(
        get_general_category(ch),
        GeneralCategory::NonspacingMark
            | GeneralCategory::SpacingMark
            | GeneralCategory::EnclosingMark
    )
}

/// Assigns anchors across one document, disambiguating repeats.
///
/// A slug that repeats takes GitHub's `-1`, `-2` suffix, so the second
/// `## Notes` answers to `#notes-1`. Order therefore decides identity for a
/// repeated heading — worth knowing before a heading becomes a node key.
///
/// The suffix is re-checked rather than merely appended, so a document holding
/// `a`, `a`, and a literal `a-1` yields `a`, `a-1`, `a-1-1` instead of emitting
/// `a-1` twice. Two headings claiming one anchor is a real collision on the
/// page, and a published list that repeats an address cannot be checked against.
#[derive(Debug, Default)]
pub struct Slugger {
    /// Every anchor handed out, plus the suffix counter for each base slug.
    occurrences: HashMap<String, usize>,
}

impl Slugger {
    /// The anchor this heading text answers to, given everything assigned so far.
    pub fn heading(&mut self, text: &str) -> String {
        let base = slug(text);
        let mut anchor = base.clone();
        while self.occurrences.contains_key(&anchor) {
            let counter = self.occurrences.entry(base.clone()).or_insert(0);
            *counter += 1;
            anchor = format!("{base}-{counter}");
        }
        self.occurrences.insert(anchor.clone(), 0);
        anchor
    }
}

/// Resolve a document's headings, in order, to the anchors they define.
#[cfg(test)]
fn anchors<'a>(headings: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut slugger = Slugger::default();
    headings
        .into_iter()
        .map(|text| slugger.heading(text))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_id_heading_slugs_to_itself_lowercased() {
        // The shape a record container uses: the heading is the id and nothing
        // else, so the anchor is the id a reader would write.
        assert_eq!(slug("OBS-92"), "obs-92");
        assert_eq!(slug("W-01"), "w-01");
    }

    #[test]
    fn a_title_joins_with_hyphens() {
        assert_eq!(slug("OBS-92: Sampling gap"), "obs-92-sampling-gap");
        assert_eq!(slug("Reading the graph"), "reading-the-graph");
    }

    #[test]
    fn punctuation_is_removed_not_replaced() {
        // The double-hyphen trap: the em dash goes, both spaces around it stay,
        // and each becomes a hyphen.
        assert_eq!(slug("Sizing — notes"), "sizing--notes");
        assert_eq!(slug("What now?"), "what-now");
        assert_eq!(slug("a.b.c"), "abc");
        assert_eq!(slug("50% done"), "50-done");
    }

    #[test]
    fn underscores_and_hyphens_survive() {
        assert_eq!(
            slug("snake_case and kebab-case"),
            "snake_case-and-kebab-case"
        );
    }

    #[test]
    fn a_stripped_leading_character_leaves_a_leading_hyphen() {
        // An emoji is not a word character, so it is dropped and the space after
        // it becomes the first character of the anchor.
        assert_eq!(slug("🚀 Launch"), "-launch");
    }

    #[test]
    fn non_ascii_letters_and_digits_are_kept() {
        assert_eq!(slug("Café Ω 42"), "café-ω-42");
    }

    #[test]
    fn empty_and_punctuation_only_headings_slug_to_nothing() {
        assert_eq!(slug(""), "");
        assert_eq!(slug("!!!"), "");
    }

    #[test]
    fn a_repeated_slug_takes_the_github_disambiguator() {
        assert_eq!(
            anchors(["Notes", "Notes", "Other", "Notes"]),
            vec!["notes", "notes-1", "other", "notes-2"]
        );
    }

    #[test]
    fn a_suffix_that_would_collide_is_bumped_again() {
        // Appending without re-checking would hand `a-1` to two headings, and an
        // address list that repeats itself cannot be checked against.
        assert_eq!(
            anchors(["a", "a", "a-1", "a"]),
            vec!["a", "a-1", "a-1-1", "a-2"]
        );
    }

    #[test]
    fn combining_marks_survive() {
        // Decomposed `café`: `e` followed by U+0301. Dropping the mark would slug
        // it `cafe` and reject the `#café` a reader's browser resolves.
        assert_eq!(slug("Cafe\u{301}"), "cafe\u{301}");
        assert_eq!(slug("Café"), "café");
    }

    #[test]
    fn headings_differing_only_in_case_collide() {
        // Downcasing happens before disambiguation, so these are one slug space.
        assert_eq!(anchors(["Setup", "SETUP"]), vec!["setup", "setup-1"]);
    }
}
