use crate::config::RuleSeverity;
use serde::Serialize;

/// A v0.8 check finding over the composed graph. Serializes to the diagnostic
/// shape `{name, severity, subject, target?, lines?, _graphs, message, cause?}`:
/// `subject` is the implicated path (the source node for edge-level findings),
/// `target` is what the link points at on edge-level findings (absent on
/// node-level ones) — usually the destination node's path, but
/// `unresolved-fragment` reports `path#fragment`, so a consumer must not assume
/// it is a node id, `lines` is the source line(s) where an edge finding's link
/// appears (absent otherwise), `_graphs` carries the same provenance key the node
/// or edge does, so a consumer never has to parse anything, and `cause` names the
/// likely reason when the finding is correct but reads as the wrong problem.
///
/// A finding is about an item in the graph. A statement about the *run* — an
/// unknown rule name, an oversized projection — is a [`crate::hints::Hint`].
#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub name: String,
    pub severity: RuleSeverity,
    pub subject: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<usize>,
    #[serde(rename = "_graphs")]
    pub graphs: Vec<String>,
    pub message: String,
    /// An optional second line naming the likely cause. Reserved for cases where
    /// the finding is correct but reads as the wrong problem — a path that fails
    /// to resolve looks like a typo when the base is what's wrong.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause: Option<String>,
}

impl Finding {
    /// A finding at the default `warn` severity. The check orchestrator applies
    /// the configured severity afterward.
    pub fn warn(
        name: impl Into<String>,
        subject: impl Into<String>,
        graphs: Vec<String>,
        message: impl Into<String>,
    ) -> Self {
        Finding {
            name: name.into(),
            severity: RuleSeverity::Warn,
            subject: subject.into(),
            target: None,
            lines: Vec::new(),
            graphs,
            message: message.into(),
            cause: None,
        }
    }

    /// Attach a cause line rendered beneath the finding.
    pub fn with_cause(mut self, cause: impl Into<String>) -> Self {
        self.cause = Some(cause.into());
        self
    }

    /// Attach the destination path for an edge-level finding; renders as
    /// `subject → target` and serializes as a `target` field.
    pub fn with_target(mut self, target: impl Into<String>) -> Self {
        self.target = Some(target.into());
        self
    }

    /// Attach the source line(s) where an edge finding's link appears. They
    /// annotate the subject (`subject:line → target`) and serialize as `lines`.
    /// The `subject` path itself is unchanged, so `ignore` globs still match it.
    pub fn with_lines(mut self, lines: Vec<usize>) -> Self {
        self.lines = lines;
        self
    }

    /// The locus as rendered in text: `subject:lines → target` for an edge
    /// finding with known lines, `subject → target` for one without, and just
    /// `subject` for a node finding.
    fn subject_display(&self) -> String {
        let subject = if self.lines.is_empty() {
            crate::util::one_line(&self.subject)
        } else {
            let joined = self
                .lines
                .iter()
                .map(usize::to_string)
                .collect::<Vec<_>>()
                .join(",");
            format!("{}:{joined}", crate::util::one_line(&self.subject))
        };
        match &self.target {
            Some(target) => format!("{subject} → {}", crate::util::one_line(target)),
            None => subject,
        }
    }

    fn severity_label(&self) -> &'static str {
        match self.severity {
            RuleSeverity::Error => "error",
            RuleSeverity::Warn => "warn",
            RuleSeverity::Off => "off",
        }
    }

    pub fn format_text(&self) -> String {
        let head = format!(
            "{}[{}]: {} ({})",
            self.severity_label(),
            self.name,
            self.subject_display(),
            self.message
        );
        match &self.cause {
            Some(cause) => format!("{head}\n  cause: {}", crate::util::one_line(cause)),
            None => head,
        }
    }

    pub fn format_text_color(&self) -> String {
        let color = match self.severity {
            RuleSeverity::Error => "\x1b[1;31m",
            RuleSeverity::Warn => "\x1b[1;33m",
            RuleSeverity::Off => "\x1b[0m",
        };
        let reset = "\x1b[0m";
        let bold = "\x1b[1m";
        let cyan = "\x1b[36m";
        let head = format!(
            "{color}{}{reset}[{bold}{}{reset}]: {cyan}{}{reset} ({})",
            self.severity_label(),
            self.name,
            self.subject_display(),
            self.message
        );
        match &self.cause {
            Some(cause) => format!(
                "{head}\n  {bold}cause{reset}: {}",
                crate::util::one_line(cause)
            ),
            None => head,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_to_diagnostic_shape() {
        let f = Finding::warn(
            "stale-node",
            "src/graph.rs",
            vec!["@fs".to_string()],
            "hash b3:444 ≠ locked b3:222",
        );
        let json = serde_json::to_value(&f).unwrap();
        assert_eq!(json["name"], "stale-node");
        assert_eq!(json["severity"], "warn");
        assert_eq!(json["subject"], "src/graph.rs");
        assert_eq!(json["_graphs"], serde_json::json!(["@fs"]));
        assert!(json.get("graphs").is_none(), "field renamed to _graphs");
        assert!(json.get("target").is_none(), "node finding has no target");
    }

    #[test]
    fn node_finding_has_no_arrow() {
        let f = Finding::warn("detached-node", "orphan.md", vec![], "no connections");
        assert_eq!(
            f.format_text(),
            "warn[detached-node]: orphan.md (no connections)"
        );
    }

    #[test]
    fn edge_finding_with_lines_annotates_subject() {
        let f = Finding::warn("stale-edge", "README.md", vec![], "hash a ≠ locked b")
            .with_target("src/lib.rs")
            .with_lines(vec![12, 49]);
        assert_eq!(
            f.format_text(),
            "warn[stale-edge]: README.md:12,49 → src/lib.rs (hash a ≠ locked b)"
        );
        let json = serde_json::to_value(&f).unwrap();
        // The subject path stays bare so `ignore` globs still match it; lines are
        // a separate field.
        assert_eq!(json["subject"], "README.md");
        assert_eq!(json["lines"], serde_json::json!([12, 49]));
    }

    #[test]
    fn edge_finding_renders_arrow_and_target() {
        let f = Finding::warn("unresolved-edge", "index.md", vec![], "no defining node")
            .with_target("gone.md");
        assert_eq!(
            f.format_text(),
            "warn[unresolved-edge]: index.md → gone.md (no defining node)"
        );
        let json = serde_json::to_value(&f).unwrap();
        assert_eq!(json["subject"], "index.md");
        assert_eq!(json["target"], "gone.md");
    }

    // One finding is one line. Each of these deletions left the whole suite green
    // before the tests existed, and each puts a line into `check` output carrying
    // neither a severity prefix nor the cause indent.

    #[test]
    fn a_node_findings_subject_stays_on_one_line() {
        let f = Finding::warn("detached-node", "we\nird.md", vec![], "no connections");
        assert_eq!(
            f.format_text(),
            "warn[detached-node]: we\\nird.md (no connections)"
        );
    }

    #[test]
    fn an_edge_findings_subject_and_lines_stay_on_one_line() {
        let f = Finding::warn("unresolved-edge", "we\nird.md", vec![], "no defining node")
            .with_lines(vec![2])
            .with_target("t\nt.md");
        assert_eq!(
            f.format_text(),
            "warn[unresolved-edge]: we\\nird.md:2 → t\\nt.md (no defining node)"
        );
    }

    #[test]
    fn a_cause_is_escaped_when_rendered_and_kept_raw_on_the_finding() {
        let f = Finding::warn("unresolved-edge", "a.md", vec![], "no defining node")
            .with_lines(vec![1])
            .with_target("docs/b.md")
            .with_cause("`we\nird.md` resolves from the graph root");
        let text = f.format_text();
        assert!(
            text.lines().count() == 2,
            "head plus one cause line: {text:?}"
        );
        assert!(text.contains("`we\\nird.md`"), "{text:?}");
        // The finding itself keeps the real bytes — it is serialised to JSON,
        // where the value has to survive exactly as written.
        assert!(f.cause.as_deref().unwrap().contains('\n'));
        assert!(!f.cause.as_deref().unwrap().contains("\\n"));
    }

    #[test]
    fn the_colour_renderer_escapes_everything_the_plain_one_does() {
        let f = Finding::warn("unresolved-edge", "we\nird.md", vec![], "no defining node")
            .with_lines(vec![2])
            .with_target("t\nt.md")
            .with_cause("`we\nird.md` resolves from the graph root");
        let colored = f.format_text_color();
        assert!(
            !colored.contains('\n') || colored.lines().count() == 2,
            "{colored:?}"
        );
        for fragment in ["we\\nird.md", "t\\nt.md"] {
            assert!(
                colored.contains(fragment),
                "missing {fragment}: {colored:?}"
            );
        }
        // Two lines exactly: the head and its cause. A raw newline in any of the
        // three interpolated values would make more.
        assert_eq!(colored.lines().count(), 2, "{colored:?}");
    }
}
