use crate::config::RuleSeverity;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Diagnostic {
    pub rule: String,
    pub severity: RuleSeverity,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub via: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
}

impl Default for Diagnostic {
    fn default() -> Self {
        Diagnostic {
            rule: String::new(),
            severity: RuleSeverity::Warn,
            message: String::new(),
            source: None,
            target: None,
            node: None,
            via: None,
            path: None,
            graph: None,
            fix: None,
        }
    }
}

impl Diagnostic {
    pub fn format_text(&self) -> String {
        let severity = match self.severity {
            RuleSeverity::Error => "error",
            RuleSeverity::Warn => "warn",
            RuleSeverity::Off => "off",
        };

        match (&self.source, &self.target) {
            (Some(source), Some(target)) => {
                let via_suffix = self
                    .via
                    .as_ref()
                    .map(|v| format!(" via {v}"))
                    .unwrap_or_default();
                format!(
                    "{severity}[{}]: {source} \u{2192} {target} ({}{})",
                    self.rule, self.message, via_suffix
                )
            }
            _ if self.path.is_some() => {
                let path = self.path.as_ref().unwrap();
                let cycle = path.join(" \u{2192} ");
                format!("{severity}[{}]: {}: {cycle}", self.rule, self.message)
            }
            _ if self.node.is_some() => {
                let node = self.node.as_ref().unwrap();
                match &self.via {
                    Some(via) => {
                        format!("{severity}[{}]: {node} ({} {via})", self.rule, self.message)
                    }
                    None => format!("{severity}[{}]: {node} ({})", self.rule, self.message),
                }
            }
            _ => format!("{severity}[{}]: {}", self.rule, self.message),
        }
    }

    pub fn format_text_color(&self) -> String {
        let (severity_str, color) = match self.severity {
            RuleSeverity::Error => ("error", "\x1b[1;31m"), // bold red
            RuleSeverity::Warn => ("warn", "\x1b[1;33m"),   // bold yellow
            RuleSeverity::Off => ("off", "\x1b[0m"),
        };
        let reset = "\x1b[0m";
        let bold = "\x1b[1m";
        let cyan = "\x1b[36m";

        match (&self.source, &self.target) {
            (Some(source), Some(target)) => {
                let via_suffix = self
                    .via
                    .as_ref()
                    .map(|v| format!(" via {cyan}{v}{reset}"))
                    .unwrap_or_default();
                format!(
                    "{color}{severity_str}{reset}[{bold}{}{reset}]: {cyan}{source}{reset} \u{2192} {cyan}{target}{reset} ({}{})",
                    self.rule, self.message, via_suffix
                )
            }
            _ if self.path.is_some() => {
                let path = self.path.as_ref().unwrap();
                let cycle = path
                    .iter()
                    .map(|p| format!("{cyan}{p}{reset}"))
                    .collect::<Vec<_>>()
                    .join(" \u{2192} ");
                format!(
                    "{color}{severity_str}{reset}[{bold}{}{reset}]: {}: {cycle}",
                    self.rule, self.message
                )
            }
            _ if self.node.is_some() => {
                let node = self.node.as_ref().unwrap();
                match &self.via {
                    Some(via) => format!(
                        "{color}{severity_str}{reset}[{bold}{}{reset}]: {cyan}{node}{reset} ({} {cyan}{via}{reset})",
                        self.rule, self.message
                    ),
                    None => format!(
                        "{color}{severity_str}{reset}[{bold}{}{reset}]: {cyan}{node}{reset} ({})",
                        self.rule, self.message
                    ),
                }
            }
            _ => format!(
                "{color}{severity_str}{reset}[{bold}{}{reset}]: {}",
                self.rule, self.message
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_format_edge_rule() {
        let d = Diagnostic {
            rule: "dangling-edge".into(),
            severity: RuleSeverity::Warn,
            message: "file not found".into(),
            source: Some("index.md".into()),
            target: Some("gone.md".into()),
            ..Default::default()
        };
        assert_eq!(
            d.format_text(),
            "warn[dangling-edge]: index.md \u{2192} gone.md (file not found)"
        );
    }

    #[test]
    fn json_serialization() {
        let d = Diagnostic {
            rule: "dangling-edge".into(),
            severity: RuleSeverity::Warn,
            message: "file not found".into(),
            source: Some("index.md".into()),
            target: Some("gone.md".into()),
            ..Default::default()
        };
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains("\"rule\":\"dangling-edge\""));
        assert!(json.contains("\"severity\":\"warn\""));
        assert!(json.contains("\"source\":\"index.md\""));
        assert!(json.contains("\"target\":\"gone.md\""));
        assert!(!json.contains("\"node\""));
    }

    #[test]
    fn text_format_with_graph_prefix() {
        let d = Diagnostic {
            rule: "orphan-node".into(),
            severity: RuleSeverity::Warn,
            message: "no inbound links".into(),
            node: Some("orphan.md".into()),
            graph: Some("beta".into()),
            ..Default::default()
        };
        // Scope is handled by the output loop, not format_text
        assert_eq!(
            d.format_text(),
            "warn[orphan-node]: orphan.md (no inbound links)"
        );
    }
}
