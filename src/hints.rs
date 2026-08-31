//! `hints` — a run-level advisory channel.
//!
//! A hint is a statement about the *invocation* rather than about any item in
//! its result: an unknown rule name in `drft.toml`, a selector that matched
//! nothing, a projection large enough to crowd out the task it was meant to
//! ground. Findings describe the graph; hints describe the run that read it.
//!
//! Hints are structured rather than prose so a consumer can act on or suppress
//! one by `name` — the whole point for a reader that is a model. They carry an
//! optional `locus`, which is not necessarily a path: a selector, a config key
//! like `rules.stale-nodes`, or nothing at all.
//!
//! **Hints are advisory and never replace a guard.** A hint annotates output and
//! lets it stand, so anything that must stop a caller stays an error with a
//! nonzero exit — `drft lock`'s empty argument list and `drft nodes`' unresolved
//! path both refuse rather than hint. A hint must never quietly downgrade a
//! refusal into a note.
//!
//! Hints do not affect exit codes. Advisory means advisory.

use serde::Serialize;

/// One advisory statement about the run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hint {
    /// Stable identifier — what a consumer matches on to act or suppress.
    pub name: String,
    /// What the hint is about, when there is something to point at: a path, a
    /// selector, a config key. Absent when the hint is about the run as a whole.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locus: Option<String>,
    pub message: String,
    /// The move that resolves it, when there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
}

impl Hint {
    pub fn new(name: impl Into<String>, message: impl Into<String>) -> Self {
        Hint {
            name: name.into(),
            locus: None,
            message: message.into(),
            next: None,
        }
    }

    /// Attach what the hint points at.
    pub fn at(mut self, locus: impl Into<String>) -> Self {
        self.locus = Some(locus.into());
        self
    }

    /// Attach the resolving move, rendered as an indented line beneath the hint.
    pub fn with_next(mut self, next: impl Into<String>) -> Self {
        self.next = Some(next.into());
        self
    }

    /// `hint[name]: locus (message)`, mirroring a finding's head line so the two
    /// read as one output vocabulary. The `next` line indents beneath it.
    pub fn format_text(&self) -> String {
        let head = match &self.locus {
            // A locus is often a node path, which can carry anything a filename
            // can. One hint is one line, as one finding is.
            Some(locus) => format!(
                "hint[{}]: {} ({})",
                self.name,
                crate::util::one_line(locus),
                crate::util::one_line(&self.message)
            ),
            None => format!(
                "hint[{}]: {}",
                self.name,
                crate::util::one_line(&self.message)
            ),
        };
        match &self.next {
            Some(next) => format!("{head}\n  next: {}", crate::util::one_line(next)),
            None => head,
        }
    }

    pub fn format_text_color(&self) -> String {
        let blue = "\x1b[1;34m";
        let reset = "\x1b[0m";
        let bold = "\x1b[1m";
        let cyan = "\x1b[36m";
        let head = match &self.locus {
            Some(locus) => format!(
                "{blue}hint{reset}[{bold}{}{reset}]: {cyan}{}{reset} ({})",
                self.name,
                crate::util::one_line(locus),
                crate::util::one_line(&self.message)
            ),
            None => format!(
                "{blue}hint{reset}[{bold}{}{reset}]: {}",
                self.name,
                crate::util::one_line(&self.message)
            ),
        };
        match &self.next {
            Some(next) => format!(
                "{head}\n  {bold}next{reset}: {}",
                crate::util::one_line(next)
            ),
            None => head,
        }
    }
}

/// The hints accumulated over one run, in the order they were raised, and
/// whether they have already reached the reader.
///
/// Threaded explicitly rather than kept in a global: a hint is part of what a
/// command produced, and the places that raise one (config load, lockfile read,
/// selector resolution, rendering) are few enough to pass a collector to.
///
/// `delivered` is what keeps a hint from being lost. A command that prints a
/// result document embeds its hints there; one that prints nothing — `init`,
/// `lock`, or any command that errored before rendering — has no document to
/// embed them in, and the caller has to notice and route them somewhere else.
/// Tracking delivery on the collector rather than inferring it from the command
/// means a command added later cannot silently drop them.
#[derive(Debug, Clone, Default)]
pub struct Hints {
    items: Vec<Hint>,
    delivered: bool,
}

impl Hints {
    pub fn push(&mut self, hint: Hint) {
        self.items.push(hint);
    }

    pub fn extend(&mut self, hints: impl IntoIterator<Item = Hint>) {
        self.items.extend(hints);
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn as_slice(&self) -> &[Hint] {
        &self.items
    }

    /// Whether these hints already reached the reader inside a result document.
    pub fn delivered(&self) -> bool {
        self.delivered
    }

    /// Record that a result document carried them.
    pub fn mark_delivered(&mut self) {
        self.delivered = true;
    }
}

impl Serialize for Hints {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.items.serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_without_absent_optionals() {
        let h = Hint::new("unknown-rule", "not a built-in rule (ignored)");
        let v = serde_json::to_value(&h).unwrap();
        assert_eq!(v["name"], "unknown-rule");
        assert!(v.get("locus").is_none(), "got: {v}");
        assert!(v.get("next").is_none(), "got: {v}");
    }

    #[test]
    fn serializes_locus_and_next_when_present() {
        let h = Hint::new("zero-match-selector", "matched no nodes")
            .at("docs/*.rs")
            .with_next("widen the pattern");
        let v = serde_json::to_value(&h).unwrap();
        assert_eq!(v["locus"], "docs/*.rs");
        assert_eq!(v["next"], "widen the pattern");
    }

    #[test]
    fn text_renders_locus_and_next() {
        let text = Hint::new("zero-match-selector", "matched no nodes")
            .at("docs/*.rs")
            .with_next("widen the pattern")
            .format_text();
        assert_eq!(
            text,
            "hint[zero-match-selector]: docs/*.rs (matched no nodes)\n  next: widen the pattern"
        );
    }

    #[test]
    fn text_omits_locus_when_absent() {
        let text = Hint::new("large-projection", "412 nodes, 180KB").format_text();
        assert_eq!(text, "hint[large-projection]: 412 nodes, 180KB");
    }

    #[test]
    fn color_rendering_carries_the_same_content() {
        let hint = Hint::new("large-projection", "412 nodes")
            .at("docs/")
            .with_next("narrow it");
        let colored = hint.format_text_color();
        // Same facts, wrapped in escapes — the plain form is the contract.
        for fragment in ["large-projection", "docs/", "412 nodes", "narrow it"] {
            assert!(
                colored.contains(fragment),
                "missing {fragment}: {colored:?}"
            );
        }
        assert!(colored.contains("\x1b["), "expected escapes: {colored:?}");
        assert!(!hint.format_text().contains("\x1b["));
    }

    #[test]
    fn delivery_is_recorded_not_assumed() {
        let mut hints = Hints::default();
        hints.push(Hint::new("a", "one"));
        assert!(!hints.delivered());
        hints.mark_delivered();
        assert!(hints.delivered());
    }

    #[test]
    fn collection_serializes_as_a_bare_list() {
        let mut hints = Hints::default();
        hints.push(Hint::new("a", "one"));
        hints.push(Hint::new("b", "two"));
        let v = serde_json::to_value(&hints).unwrap();
        assert!(v.is_array(), "got: {v}");
        assert_eq!(v.as_array().unwrap().len(), 2);
    }

    #[test]
    fn a_locus_carrying_a_newline_stays_on_one_line() {
        // A locus is often a node path, which can carry whatever a filename can.
        let h = Hint::new("directory-lock", "carries no content").at("we\nird");
        assert_eq!(
            h.format_text(),
            "hint[directory-lock]: we\\nird (carries no content)"
        );
        // Raw on the hint, which is serialised.
        assert!(h.locus.as_deref().unwrap().contains('\n'));
    }

    #[test]
    fn the_colour_renderer_escapes_the_locus_too() {
        let h = Hint::new("directory-lock", "carries no content").at("we\nird");
        let colored = h.format_text_color();
        assert_eq!(colored.lines().count(), 1, "{colored:?}");
        assert!(colored.contains("we\\nird"), "{colored:?}");
    }

    /// A hint message interpolates config values — a graph name, a declared key —
    /// so it can carry whatever `drft.toml` carries, and so can the remedy.
    /// Escaping the locus alone left the same hint splitting across lines.
    #[test]
    fn a_message_and_a_next_carrying_newlines_stay_on_their_own_lines() {
        let h = Hint::new("edge-keys-matched-nothing", "declares `so\nurces`")
            .at("graphs.we\nird")
            .with_next("check `so\nurces`");
        let text = h.format_text();
        assert_eq!(text.lines().count(), 2, "{text:?}");
        for fragment in [
            "graphs.we\\nird",
            "declares `so\\nurces`",
            "check `so\\nurces`",
        ] {
            assert!(text.contains(fragment), "missing {fragment}: {text:?}");
        }
        assert_eq!(h.format_text_color().lines().count(), 2);
        // Raw on the hint, which is serialised.
        assert!(h.message.contains('\n') && h.next.as_deref().unwrap().contains('\n'));
    }
}
