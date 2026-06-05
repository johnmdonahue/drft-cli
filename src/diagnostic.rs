use crate::config::RuleSeverity;
use serde::Serialize;

/// A v0.8 check finding over the composed graph. Serializes to the diagnostic
/// shape `{name, severity, subject, target?, _graphs, message}`: `subject` is the
/// implicated path (the source node for edge-level findings), `target` is the
/// edge's destination on edge-level findings (absent on node-level ones), and
/// `_graphs` carries the same provenance key the node or edge does, so a consumer
/// never has to parse anything.
#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub name: String,
    pub severity: RuleSeverity,
    pub subject: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(rename = "_graphs")]
    pub graphs: Vec<String>,
    pub message: String,
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
            graphs,
            message: message.into(),
        }
    }

    /// Attach the destination path for an edge-level finding; renders as
    /// `subject → target` and serializes as a `target` field.
    pub fn with_target(mut self, target: impl Into<String>) -> Self {
        self.target = Some(target.into());
        self
    }

    /// The locus as rendered in text: `subject → target` for edge findings,
    /// just `subject` otherwise.
    fn subject_display(&self) -> String {
        match &self.target {
            Some(target) => format!("{} → {target}", self.subject),
            None => self.subject.clone(),
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
        format!(
            "{}[{}]: {} ({})",
            self.severity_label(),
            self.name,
            self.subject_display(),
            self.message
        )
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
        format!(
            "{color}{}{reset}[{bold}{}{reset}]: {cyan}{}{reset} ({})",
            self.severity_label(),
            self.name,
            self.subject_display(),
            self.message
        )
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
}
