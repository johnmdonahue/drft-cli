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
//! The algorithm is undocumented; this follows `html-pipeline`'s
//! `TableOfContentsFilter`, which is what GitHub renders with. Its surprises are
//! pinned by the tests below rather than described here, because the tests are
//! what a future change is measured against.

use std::collections::HashMap;

/// The anchor a heading's rendered text answers to.
///
/// Downcase, drop every character that is not a letter, digit, underscore,
/// hyphen, or space, then turn spaces into hyphens. Punctuation is **removed
/// rather than replaced**, so a character sitting between two spaces leaves both
/// behind and yields a double hyphen — `Sizing — notes` becomes
/// `sizing--notes`, which is the trap worth knowing about when a heading is also
/// an identity.
pub fn slug(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars().flat_map(char::to_lowercase) {
        if ch == ' ' {
            out.push('-');
        } else if ch == '-' || ch == '_' || ch.is_alphanumeric() {
            out.push(ch);
        }
    }
    out
}

/// Resolve a document's headings, in order, to the anchors they define.
///
/// A slug that repeats takes GitHub's `-1`, `-2` disambiguator, so the second
/// `## Notes` answers to `#notes-1`. Order therefore decides identity for a
/// repeated heading — worth knowing before a heading becomes a node key.
pub fn anchors<'a>(headings: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    headings
        .into_iter()
        .map(|text| {
            let base = slug(text);
            let count = seen.entry(base.clone()).or_insert(0);
            let anchor = if *count == 0 {
                base.clone()
            } else {
                format!("{base}-{count}")
            };
            *count += 1;
            anchor
        })
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
    fn headings_differing_only_in_case_collide() {
        // Downcasing happens before disambiguation, so these are one slug space.
        assert_eq!(anchors(["Setup", "SETUP"]), vec!["setup", "setup-1"]);
    }
}
