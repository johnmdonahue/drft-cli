use crate::config::RuleSeverity;
use serde::Serialize;

/// A v0.8 check finding over the composed graph. Serializes to the diagnostic
/// shape `{name, severity, subject, _graphs, message}`: `subject` is the
/// implicated path (the source node for edge-level findings), and `_graphs`
/// carries the same provenance key the node or edge does, so a consumer never
/// has to parse anything.
#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub name: String,
    pub severity: RuleSeverity,
    pub subject: String,
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
            graphs,
            message: message.into(),
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
            self.subject,
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
            self.subject,
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
            "current hash differs from locked",
        );
        let json = serde_json::to_value(&f).unwrap();
        assert_eq!(json["name"], "stale-node");
        assert_eq!(json["severity"], "warn");
        assert_eq!(json["subject"], "src/graph.rs");
        assert_eq!(json["_graphs"], serde_json::json!(["@fs"]));
        assert!(json.get("graphs").is_none(), "field renamed to _graphs");
    }

    #[test]
    fn text_format() {
        let f = Finding::warn(
            "unresolved-edge",
            "index.md",
            vec![],
            "target gone.md has no defining node",
        );
        assert_eq!(
            f.format_text(),
            "warn[unresolved-edge]: index.md (target gone.md has no defining node)"
        );
    }
}
