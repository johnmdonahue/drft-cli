//! Policies shared by dispatch, result serializers, and the operational guide.

use crate::cli::OutputFormat;
use drft::{config::RuleSeverity, diagnostic::Finding};
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitStatus {
    Success,
    Violations,
    Failure,
}

impl ExitStatus {
    pub const fn code(self) -> i32 {
        match self {
            Self::Success => 0,
            Self::Violations => 1,
            Self::Failure => 2,
        }
    }

    pub const fn meaning(self) -> &'static str {
        match self {
            Self::Success => {
                "Command completed; check may report warnings, and impact may report warnings or errors."
            }
            Self::Violations => "check found at least one finding configured as an error.",
            Self::Failure => {
                "Usage, configuration, path-resolution, output-budget, or runtime failure."
            }
        }
    }
}

pub fn check_status(findings: &[Finding]) -> ExitStatus {
    if findings.iter().any(|f| f.severity == RuleSeverity::Error) {
        ExitStatus::Violations
    } else {
        ExitStatus::Success
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct OutputGuard {
    pub kind: &'static str,
    pub truncates: bool,
    pub atomic_refusal_before_stdout: bool,
    pub failure_exit: i32,
}

pub const OUTPUT_GUARD: OutputGuard = OutputGuard {
    kind: "output-guard",
    truncates: false,
    atomic_refusal_before_stdout: true,
    failure_exit: ExitStatus::Failure.code(),
};

impl OutputGuard {
    pub fn exceeds(self, bytes: usize, limit: Option<usize>) -> bool {
        limit.is_some_and(|limit| bytes > limit)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputMode {
    Silent,
    Envelope,
    BareJgf,
    RawGraphSet,
    IgnoreSources,
    Guide,
}

impl OutputMode {
    pub fn is_json(self, format: OutputFormat) -> bool {
        matches!(self, Self::RawGraphSet) || matches!(format, OutputFormat::Json)
    }

    pub fn allows_color(self, format: OutputFormat) -> bool {
        !self.is_json(format)
    }

    pub const fn embeds_hints(self) -> bool {
        matches!(self, Self::Envelope)
    }

    pub const fn document(self) -> Option<&'static str> {
        match self {
            Self::Silent => None,
            Self::Envelope => Some("result-document"),
            Self::BareJgf => Some("bare-jgf"),
            Self::RawGraphSet => Some("raw-graph-set-json"),
            Self::IgnoreSources => Some("ignore-source-report"),
            Self::Guide => Some("this-contract"),
        }
    }
}

pub const HINTS_FIELD: &str = "hints";

pub fn output_contract() -> serde_json::Value {
    use serde_json::json;
    json!({
        "result_channel": "stdout",
        "error_channel": "stderr",
        "text_hint_channel": "stderr",
        "json_hint_location": if OutputMode::Envelope.embeds_hints() { "result-document" } else { "stderr" },
        "clap_usage_error_format": "clap-text-on-stderr",
        "truncates": OUTPUT_GUARD.truncates,
        "json_colorized": OutputMode::Envelope.allows_color(OutputFormat::Json),
        "exceptions": [
            {"mode": "init success", "result_document": OutputMode::Silent.document()},
            {"mode": "graph --format json", "result_document": OutputMode::BareJgf.document(), "hint_channel": "stderr"},
            {"mode": "graph --raw", "result_document": OutputMode::RawGraphSet.document(), "ignores_format": OutputMode::RawGraphSet.is_json(OutputFormat::Text), "hint_channel": "stderr", "hint_format": "selected-format"},
            {"mode": "config --show-ignores", "result_document": OutputMode::IgnoreSources.document(), "hints": "none"},
            {"mode": "guide", "result_document": OutputMode::Guide.document(), "hints": "none"}
        ]
    })
}

// Each declaration owns both the serializer's keys and the guide's field names.
// No serde renames or skipped fields are permitted in this macro.
macro_rules! result_document {
    ($name:ident { $($field:ident: $ty:ty),* $(,)? }) => {
        #[derive(Serialize)]
        pub struct $name<'a> { $(pub $field: $ty,)* }
        impl $name<'_> {
            pub const FIELDS: &'static [&'static str] = &[$(stringify!($field)),*];
        }
    };
}

result_document!(LockResult { locked: &'a [String], dropped: &'a [String] });
result_document!(NodesResult { total: usize, nodes: &'a [drft::nodes::NodeProjection] });
result_document!(EdgesResult { total: usize, edges: &'a [drft::edges::EdgeProjection] });
result_document!(ImpactResult { seeds: &'a [String], total: usize, impacted: &'a [drft::impact::Impacted], diagnostics: &'a [Finding] });
result_document!(CheckResult { diagnostics: &'a [Finding], summary: CheckSummary });

#[derive(Serialize)]
pub struct CheckSummary {
    pub errors: usize,
    pub warnings: usize,
}

#[derive(Clone, Copy)]
pub enum ResultShape {
    Lock,
    Nodes,
    Edges,
    Impact,
    Check,
}

impl ResultShape {
    pub fn fields(self) -> Vec<&'static str> {
        let mut fields = match self {
            Self::Lock => LockResult::FIELDS,
            Self::Nodes => NodesResult::FIELDS,
            Self::Edges => EdgesResult::FIELDS,
            Self::Impact => ImpactResult::FIELDS,
            Self::Check => CheckResult::FIELDS,
        }
        .to_vec();
        fields.push(HINTS_FIELD);
        fields
    }
}

/// Serialize completely before handing any bytes to the output writer.
pub fn emit_json<T: Serialize>(
    value: &T,
    write: impl FnOnce(&str) -> anyhow::Result<()>,
) -> anyhow::Result<i32> {
    let rendered = format!("{}\n", serde_json::to_string_pretty(value)?);
    write(&rendered)?;
    Ok(ExitStatus::Success.code())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warnings_do_not_gate_but_errors_do() {
        let mut finding = Finding::warn("test", "a", vec![], "test");
        assert_eq!(check_status(&[]).code(), 0);
        assert_eq!(check_status(&[finding.clone()]).code(), 0);
        finding.severity = RuleSeverity::Error;
        assert_eq!(check_status(&[finding]).code(), 1);
        assert_eq!(ExitStatus::Failure.code(), 2);
    }

    #[test]
    fn serialization_failure_reaches_the_failure_path_without_writing() {
        struct Fails;
        impl Serialize for Fails {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                use serde::ser::SerializeSeq;
                let mut seq = serializer.serialize_seq(Some(2))?;
                seq.serialize_element("a prefix that must not escape")?;
                Err(serde::ser::Error::custom("injected serialization failure"))
            }
        }
        let mut output = String::new();
        let result = emit_json(&Fails, |text| {
            output.push_str(text);
            Ok(())
        });
        assert_eq!(result.unwrap_or(ExitStatus::Failure.code()), 2);
        assert!(output.is_empty());
    }
}
