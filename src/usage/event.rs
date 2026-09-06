//! Inactive revision-1 producers. Inputs are observations, never instructions to
//! evaluate a graph, write output, read config, or access the filesystem.

use super::{
    bounded::{BoundedError, EVENT_LIMIT, STRUCTURED_PAYLOAD_LIMIT, to_json_bounded},
    capture::{CaptureSnapshot, StreamCapture},
    identity::{EncodedOs, InvocationId},
};
use crate::{diagnostic::Finding, hints::Hint};
use serde::{Serialize, ser::SerializeSeq};
use serde_json::Value;
use std::{
    error::Error,
    ffi::{OsStr, OsString},
    fmt,
    time::Duration,
};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct Timestamp {
    seconds: i64,
    nanoseconds: u32,
}
impl Timestamp {
    pub fn new(seconds: i64, nanoseconds: u32) -> Option<Self> {
        (nanoseconds < 1_000_000_000).then_some(Self {
            seconds,
            nanoseconds,
        })
    }
}
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Elapsed {
    seconds: u64,
    nanoseconds: u32,
}
impl From<Duration> for Elapsed {
    fn from(value: Duration) -> Self {
        Self {
            seconds: value.as_secs(),
            nanoseconds: value.subsec_nanos(),
        }
    }
}
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(tag = "status", content = "value", rename_all = "snake_case")]
pub enum Availability<T> {
    Available(T),
    Unavailable(UnavailableReason),
}

macro_rules! discriminants {
    ($name:ident { $($variant:ident),* $(,)? }) => {
        #[derive(Debug, Clone, Copy, Serialize)]
        #[serde(rename_all="snake_case")]
        pub enum $name { $($variant),* }
    };
}
discriminants!(UnavailableReason {
    NotEvaluated,
    ExecutionStopped,
    NotObserved,
    RequiresStart,
    PublicationNotYetPerformed,
    TraversalStopped
});

discriminants!(Command {
    Check,
    Graph,
    Nodes,
    Edges,
    Impact,
    Lock
});
discriminants!(RequestedFormat { Text, Json });
discriminants!(OutputMode {
    Text,
    Json,
    BareJgf,
    RawGraphSet,
    NoDocument
});
discriminants!(Completion {
    Returned,
    CommandError,
    OutputBudgetRefused,
    StdoutWriteFailed
});
discriminants!(IntendedExit {
    Clean,
    Violations,
    UsageError
});
discriminants!(FindingCoverage {
    FullPolicyFilteredEvaluation,
    ConstructionDiagnostics,
    SelectedImpactDiagnostics
});
discriminants!(HintEmbedding {
    NotEmbedded,
    ResultDocument
});
discriminants!(HintRoute {
    None,
    StdoutDocument,
    StderrText,
    StderrJson
});
discriminants!(HintSuppression {
    None,
    Explicit,
    BudgetRefusal,
    EarlierWriteFailure
});
discriminants!(AttemptObservation {
    NotAttempted,
    Attempted,
    Unavailable
});
discriminants!(Unknown { Unknown });

/// Compute from the exact bytes already parsed by normal config loading.
#[derive(Debug, Serialize)]
#[serde(transparent)]
pub struct ConfigFingerprint(String);
impl ConfigFingerprint {
    pub fn from_parsed_bytes(bytes: &[u8]) -> Self {
        Self(format!("b3:{}", blake3::hash(bytes).to_hex()))
    }
}

struct OsValue<'a>(&'a OsStr);
impl Serialize for OsValue<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        EncodedOs::from_os(self.0)
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
}
struct Argv<'a>(&'a [OsString]);
impl Serialize for Argv<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for arg in self.0 {
            seq.serialize_element(&OsValue(arg))?;
        }
        seq.end()
    }
}
#[derive(Serialize)]
struct Caller<'a> {
    label: OsValue<'a>,
    authenticated: bool,
    unique_to_invocation: bool,
}

pub struct StartInput<'a> {
    pub id: &'a InvocationId,
    pub entry_wall_time: Availability<Timestamp>,
    pub collected_wall_time: Availability<Timestamp>,
    pub binary_version: &'a str,
    pub original_cwd: &'a OsStr,
    pub effective_directory: &'a OsStr,
    /// Already canonicalized by the caller; the producer performs no I/O.
    pub canonical_graph_root: &'a OsStr,
    pub command: Command,
    pub requested_format: RequestedFormat,
    pub argv: &'a [OsString],
    pub config_fingerprint: &'a ConfigFingerprint,
    /// Only the named DRFT_USAGE_CALLER_ID value, supplied by the caller.
    pub caller_id: Option<&'a OsStr>,
}
#[derive(Serialize)]
struct StartEnvelope<'a> {
    schema: &'static str,
    revision: u8,
    event: &'static str,
    id: &'a InvocationId,
    entry_wall_time: Availability<Timestamp>,
    collected_wall_time: Availability<Timestamp>,
    binary_version: &'a str,
    original_cwd: OsValue<'a>,
    effective_directory: OsValue<'a>,
    canonical_graph_root: OsValue<'a>,
    command: Command,
    requested_format: RequestedFormat,
    argv: Argv<'a>,
    config_fingerprint: &'a ConfigFingerprint,
    caller: Option<Caller<'a>>,
}
/// Required metadata is admitted in serialization order with one encoded OS
/// value alive at a time. Any overflow discards the entire event, without a
/// truncated identity or traversal of the remaining argv.
pub fn start(input: StartInput<'_>) -> Result<Vec<u8>, BoundedError> {
    to_json_bounded(
        &StartEnvelope {
            schema: "drft-usage",
            revision: 1,
            event: "start",
            id: input.id,
            entry_wall_time: input.entry_wall_time,
            collected_wall_time: input.collected_wall_time,
            binary_version: input.binary_version,
            original_cwd: OsValue(input.original_cwd),
            effective_directory: OsValue(input.effective_directory),
            canonical_graph_root: OsValue(input.canonical_graph_root),
            command: input.command,
            requested_format: input.requested_format,
            argv: Argv(input.argv),
            config_fingerprint: input.config_fingerprint,
            caller: input.caller_id.map(|label| Caller {
                label: OsValue(label),
                authenticated: false,
                unique_to_invocation: false,
            }),
        },
        EVENT_LIMIT,
    )
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct GraphSizes {
    pub graphs: usize,
    pub nodes: usize,
    pub edges: usize,
}
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ResultSizes {
    pub nodes: Availability<usize>,
    pub edges: Availability<usize>,
    pub findings: Availability<usize>,
}
#[derive(Debug, Clone, Copy, Serialize)]
pub struct HintObservation {
    pub embedding: HintEmbedding,
    pub route: HintRoute,
    pub suppression: HintSuppression,
    /// Observation at the hint's own write site, not aggregate stream activity.
    pub write_attempt: AttemptObservation,
    pub writer_acceptance: Unknown,
    pub os_acceptance: Unknown,
    pub downstream_consumption: Unknown,
}
#[derive(Debug, Clone, Copy, Serialize)]
pub struct BudgetRefusal {
    pub rendered_bytes: u64,
    pub budget_bytes: u64,
}
pub struct Findings<'a> {
    pub coverage: FindingCoverage,
    pub records: &'a [Finding],
}
pub struct FinishInput<'a> {
    pub id: &'a InvocationId,
    pub entry_wall_time: Availability<Timestamp>,
    pub collected_wall_time: Availability<Timestamp>,
    pub elapsed: Availability<Elapsed>,
    pub collector_work_through_preparation: Availability<Elapsed>,
    pub intended_exit: IntendedExit,
    pub completion: Completion,
    pub output_mode: OutputMode,
    pub graph_sizes: Availability<GraphSizes>,
    pub result_sizes: Availability<ResultSizes>,
    pub findings: Availability<Findings<'a>>,
    pub hints: Availability<&'a [Hint]>,
    pub hint_observation: HintObservation,
    pub error: Availability<Option<&'a (dyn Error + 'static)>>,
    pub budget_refusal: Option<BudgetRefusal>,
    pub stdout: &'a StreamCapture,
    pub stderr: &'a StreamCapture,
}
#[derive(Serialize)]
struct Prefix {
    total: Availability<usize>,
    included: usize,
    omitted: Availability<usize>,
    records: Vec<Value>,
}
impl Prefix {
    fn empty(total: Availability<usize>) -> Self {
        Self {
            total,
            included: 0,
            omitted: total,
            records: Vec::new(),
        }
    }
    fn push(&mut self, value: Value) {
        self.records.push(value);
        self.included += 1;
        self.omitted = match self.total {
            Availability::Available(n) => Availability::Available(n - self.included),
            Availability::Unavailable(reason) => Availability::Unavailable(reason),
        };
    }
}
#[derive(Serialize)]
struct FindingPayload {
    availability: &'static str,
    coverage: Availability<FindingCoverage>,
    prefix: Prefix,
}
#[derive(Serialize)]
struct ErrorPayload {
    availability: &'static str,
    present: Availability<bool>,
    traversal: ErrorTraversal,
    prefix: Prefix,
}
discriminants!(ErrorTraversal {
    Complete,
    NotStarted,
    BudgetStopped,
    DepthStopped,
    FormatterFailed
});
#[derive(Serialize)]
struct Structured {
    findings: FindingPayload,
    hints: Prefix,
    error: ErrorPayload,
}
#[derive(Serialize)]
struct FinishEnvelope<'a> {
    schema: &'static str,
    revision: u8,
    event: &'static str,
    id: &'a InvocationId,
    entry_wall_time: Availability<Timestamp>,
    collected_wall_time: Availability<Timestamp>,
    elapsed: Availability<Elapsed>,
    collector_work_through_preparation: Availability<Elapsed>,
    finish_publication_duration: Availability<Elapsed>,
    /// Orphan finishes deliberately cannot establish comparison provenance.
    comparison_requires_start: bool,
    intended_exit: IntendedExit,
    completion: Completion,
    output_mode: OutputMode,
    graph_sizes: Availability<GraphSizes>,
    result_sizes: Availability<ResultSizes>,
    hint_observation: HintObservation,
    budget_refusal: Option<BudgetRefusal>,
    structured: Structured,
    stdout: CaptureSnapshot,
    stderr: CaptureSnapshot,
}

// This reserves more than all possible usize count-width increases and enum
// spelling changes from the empty skeleton. It is intentionally conservative;
// revision 1 promises deterministic prefixes, not maximal packing.
const COUNTER_HEADROOM: usize = 1024;
const ERROR_DEPTH_LIMIT: usize = 64;

fn admit<T: Serialize>(
    value: &T,
    remaining: &mut usize,
    prefix: &mut Prefix,
) -> Result<bool, BoundedError> {
    let budget = remaining.saturating_sub(1); // comma, including first-record spare
    match to_json_bounded(value, budget) {
        Ok(bytes) => {
            *remaining -= bytes.len() + 1;
            // The parser sees only already-capped JSON, never command results.
            let value = serde_json::from_slice(&bytes).map_err(BoundedError::Serialization)?;
            prefix.push(value);
            Ok(true)
        }
        Err(BoundedError::LimitExceeded) => Ok(false),
        Err(e) => Err(e),
    }
}
fn admit_records<T: Serialize>(
    records: &[T],
    remaining: &mut usize,
    prefix: &mut Prefix,
) -> Result<(), BoundedError> {
    for record in records {
        if !admit(record, remaining, prefix)? {
            break;
        }
    }
    Ok(())
}
struct ErrorText {
    text: String,
    limit: usize,
    exceeded: bool,
}
impl fmt::Write for ErrorText {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        if self.exceeded || value.len() > self.limit - self.text.len() {
            self.exceeded = true;
            return Err(fmt::Error);
        }
        self.text.push_str(value);
        Ok(())
    }
}
fn admit_error(
    mut error: &(dyn Error + 'static),
    remaining: &mut usize,
    payload: &mut ErrorPayload,
) -> Result<(), BoundedError> {
    use fmt::Write as _;
    for depth in 0..ERROR_DEPTH_LIMIT {
        let mut text = ErrorText {
            text: String::new(),
            limit: remaining.saturating_sub(3),
            exceeded: false,
        };
        let formatted = write!(&mut text, "{error}");
        if text.exceeded {
            payload.traversal = ErrorTraversal::BudgetStopped;
            return Ok(());
        }
        if formatted.is_err() {
            payload.traversal = ErrorTraversal::FormatterFailed;
            return Ok(());
        }
        if !admit(&text.text, remaining, &mut payload.prefix)? {
            payload.traversal = ErrorTraversal::BudgetStopped;
            return Ok(());
        }
        if depth + 1 == ERROR_DEPTH_LIMIT {
            payload.traversal = ErrorTraversal::DepthStopped;
            return Ok(());
        }
        match error.source() {
            Some(source) => error = source,
            None => {
                payload.traversal = ErrorTraversal::Complete;
                payload.prefix.total = Availability::Available(payload.prefix.included);
                payload.prefix.omitted = Availability::Available(0);
                return Ok(());
            }
        }
    }
    unreachable!()
}

/// Construct a bounded finish from borrowed results and actual stream captures.
/// No evaluation or output is performed. Both limits count JSON wrappers.
pub fn finish(input: FinishInput<'_>) -> Result<Vec<u8>, BoundedError> {
    finish_with_limit(input, EVENT_LIMIT)
}

fn finish_with_limit(input: FinishInput<'_>, event_limit: usize) -> Result<Vec<u8>, BoundedError> {
    let (finding_availability, coverage, finding_records, finding_total) = match input.findings {
        Availability::Available(f) => (
            "available",
            Availability::Available(f.coverage),
            Some(f.records),
            Availability::Available(f.records.len()),
        ),
        Availability::Unavailable(reason) => (
            "unavailable",
            Availability::Unavailable(reason),
            None,
            Availability::Unavailable(reason),
        ),
    };
    let (error_availability, present, traversal, error_total) = match input.error {
        Availability::Unavailable(reason) => (
            "unavailable",
            Availability::Unavailable(reason),
            ErrorTraversal::NotStarted,
            Availability::Unavailable(reason),
        ),
        Availability::Available(None) => (
            "available",
            Availability::Available(false),
            ErrorTraversal::Complete,
            Availability::Available(0),
        ),
        Availability::Available(Some(_)) => (
            "available",
            Availability::Available(true),
            ErrorTraversal::NotStarted,
            Availability::Unavailable(UnavailableReason::TraversalStopped),
        ),
    };
    let (hint_records, hint_total) = match input.hints {
        Availability::Available(h) => (Some(h), Availability::Available(h.len())),
        Availability::Unavailable(reason) => (None, Availability::Unavailable(reason)),
    };
    let mut event = FinishEnvelope {
        schema: "drft-usage",
        revision: 1,
        event: "finish",
        id: input.id,
        entry_wall_time: input.entry_wall_time,
        collected_wall_time: input.collected_wall_time,
        elapsed: input.elapsed,
        collector_work_through_preparation: input.collector_work_through_preparation,
        finish_publication_duration: Availability::Unavailable(
            UnavailableReason::PublicationNotYetPerformed,
        ),
        comparison_requires_start: true,
        intended_exit: input.intended_exit,
        completion: input.completion,
        output_mode: input.output_mode,
        graph_sizes: input.graph_sizes,
        result_sizes: input.result_sizes,
        hint_observation: input.hint_observation,
        budget_refusal: input.budget_refusal,
        structured: Structured {
            findings: FindingPayload {
                availability: finding_availability,
                coverage,
                prefix: Prefix::empty(finding_total),
            },
            hints: Prefix::empty(hint_total),
            error: ErrorPayload {
                availability: error_availability,
                present,
                traversal,
                prefix: Prefix::empty(error_total),
            },
        },
        stdout: input.stdout.snapshot_prefix(0),
        stderr: input.stderr.snapshot_prefix(0),
    };
    let baseline = to_json_bounded(&event, event_limit)?.len();
    let structured_baseline = to_json_bounded(&event.structured, STRUCTURED_PAYLOAD_LIMIT)?.len();
    let mut remaining = (event_limit - baseline)
        .min(STRUCTURED_PAYLOAD_LIMIT - structured_baseline)
        .checked_sub(COUNTER_HEADROOM)
        .ok_or(BoundedError::LimitExceeded)?;
    if let Some(records) = finding_records {
        admit_records(
            records,
            &mut remaining,
            &mut event.structured.findings.prefix,
        )?;
    }
    if let Some(records) = hint_records {
        admit_records(records, &mut remaining, &mut event.structured.hints)?;
    }
    if let Availability::Available(Some(error)) = input.error {
        admit_error(error, &mut remaining, &mut event.structured.error)?;
    }
    // Check the shared structured limit independently of whole-event capacity.
    to_json_bounded(&event.structured, STRUCTURED_PAYLOAD_LIMIT)?;
    let mut stream_remaining = (event_limit - to_json_bounded(&event, event_limit)?.len())
        .saturating_sub(COUNTER_HEADROOM);
    // Base64 groups cost four bytes; raw retention need not end on a UTF-8 boundary.
    let stdout = input.stdout.snapshot_prefix(stream_remaining / 4 * 3);
    stream_remaining -= stdout.prefix_base64.len();
    event.stdout = stdout;
    event.stderr = input.stderr.snapshot_prefix(stream_remaining / 4 * 3);
    to_json_bounded(&event, event_limit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use std::{cell::Cell, io};

    fn id() -> InvocationId {
        InvocationId::generate_with(|bytes| {
            bytes.fill(0x12);
            Ok::<_, ()>(())
        })
        .unwrap()
    }
    fn hint_observation() -> HintObservation {
        HintObservation {
            embedding: HintEmbedding::NotEmbedded,
            route: HintRoute::None,
            suppression: HintSuppression::None,
            write_attempt: AttemptObservation::NotAttempted,
            writer_acceptance: Unknown::Unknown,
            os_acceptance: Unknown::Unknown,
            downstream_consumption: Unknown::Unknown,
        }
    }
    fn input<'a>(
        id: &'a InvocationId,
        stdout: &'a StreamCapture,
        stderr: &'a StreamCapture,
    ) -> FinishInput<'a> {
        FinishInput {
            id,
            entry_wall_time: Availability::Available(Timestamp::new(100, 123).unwrap()),
            collected_wall_time: Availability::Available(Timestamp::new(99, 456).unwrap()),
            elapsed: Availability::Available(Duration::new(2, 345).into()),
            collector_work_through_preparation: Availability::Available(
                Duration::new(0, 789).into(),
            ),
            intended_exit: IntendedExit::Clean,
            completion: Completion::Returned,
            output_mode: OutputMode::RawGraphSet,
            graph_sizes: Availability::Available(GraphSizes {
                graphs: 2,
                nodes: 3,
                edges: 1,
            }),
            result_sizes: Availability::Unavailable(UnavailableReason::NotObserved),
            findings: Availability::Unavailable(UnavailableReason::NotEvaluated),
            hints: Availability::Available(&[]),
            hint_observation: hint_observation(),
            error: Availability::Available(None),
            budget_refusal: None,
            stdout,
            stderr,
        }
    }
    fn value(input: FinishInput<'_>) -> Value {
        serde_json::from_slice(&finish(input).unwrap()).unwrap()
    }

    #[test]
    fn timestamp_range_and_monotonic_duration_are_independent() {
        assert!(Timestamp::new(-1, 999_999_999).is_some());
        assert!(Timestamp::new(0, 1_000_000_000).is_none());
        let id = id();
        let stdout = StreamCapture::stdout();
        let stderr = StreamCapture::stderr();
        let event = value(input(&id, &stdout, &stderr));
        assert_eq!(event["entry_wall_time"]["value"]["seconds"], 100);
        assert_eq!(event["collected_wall_time"]["value"]["seconds"], 99);
        assert_eq!(event["elapsed"]["value"]["seconds"], 2);
        assert_eq!(
            event["finish_publication_duration"]["status"],
            "unavailable"
        );
        assert_eq!(event["comparison_requires_start"], true);
    }

    #[test]
    fn unavailable_empty_and_coverage_are_distinct() {
        let id = id();
        let stdout = StreamCapture::stdout();
        let stderr = StreamCapture::stderr();
        let absent = value(input(&id, &stdout, &stderr));
        assert_eq!(
            absent["structured"]["findings"]["availability"],
            "unavailable"
        );
        for (coverage, name) in [
            (
                FindingCoverage::FullPolicyFilteredEvaluation,
                "full_policy_filtered_evaluation",
            ),
            (
                FindingCoverage::ConstructionDiagnostics,
                "construction_diagnostics",
            ),
            (
                FindingCoverage::SelectedImpactDiagnostics,
                "selected_impact_diagnostics",
            ),
        ] {
            let mut i = input(&id, &stdout, &stderr);
            i.findings = Availability::Available(Findings {
                coverage,
                records: &[],
            });
            let event = value(i);
            assert_eq!(event["structured"]["findings"]["coverage"]["value"], name);
            assert_eq!(
                event["structured"]["findings"]["prefix"]["total"]["value"],
                0
            );
        }
    }
    #[test]
    fn raw_and_refused_embedded_hints_do_not_imply_writes() {
        let id = id();
        let stdout = StreamCapture::stdout();
        let mut stderr = StreamCapture::stderr();
        stderr
            .begin_write(b"output exceeds budget\n")
            .finish(&Ok(()));
        let hints = [Hint::new("large-projection", "too large")];
        let mut i = input(&id, &stdout, &stderr);
        i.hints = Availability::Available(&hints);
        i.output_mode = OutputMode::Json;
        i.intended_exit = IntendedExit::UsageError;
        i.completion = Completion::OutputBudgetRefused;
        i.budget_refusal = Some(BudgetRefusal {
            rendered_bytes: 5000,
            budget_bytes: 1000,
        });
        i.hint_observation = HintObservation {
            embedding: HintEmbedding::ResultDocument,
            route: HintRoute::StdoutDocument,
            suppression: HintSuppression::BudgetRefusal,
            ..hint_observation()
        };
        let event = value(i);
        assert_eq!(event["hint_observation"]["embedding"], "result_document");
        assert_eq!(event["hint_observation"]["write_attempt"], "not_attempted");
        assert_eq!(event["stdout"]["write_outcome"]["status"], "not_attempted");
        assert_eq!(event["budget_refusal"]["rendered_bytes"], 5000);
        assert_eq!(
            value(input(&id, &stdout, &stderr))["output_mode"],
            "raw_graph_set"
        );
    }
    #[test]
    fn combined_payloads_share_budget_and_keep_stream_observations() {
        let id = id();
        let mut stdout = StreamCapture::stdout();
        let mut stderr = StreamCapture::stderr();
        stdout
            .begin_write(&vec![b'a'; 100_000])
            .finish(&Err(io::ErrorKind::BrokenPipe.into()));
        stderr.begin_write(&vec![b'b'; 20_000]).finish(&Ok(()));
        let findings = [Finding::warn("stale-node", "a", vec![], "x".repeat(90_000))];
        let hints = [
            Hint::new("large-projection", "x".repeat(90_000)),
            Hint::new("small", "omitted too"),
        ];
        let error = io::Error::other("small error");
        let mut i = input(&id, &stdout, &stderr);
        i.findings = Availability::Available(Findings {
            coverage: FindingCoverage::FullPolicyFilteredEvaluation,
            records: &findings,
        });
        i.hints = Availability::Available(&hints);
        i.error = Availability::Available(Some(&error));
        let bytes = finish(i).unwrap();
        assert!(bytes.len() <= EVENT_LIMIT);
        let event: Value = serde_json::from_slice(&bytes).unwrap();
        assert!(
            serde_json::to_vec(&event["structured"]).unwrap().len() <= STRUCTURED_PAYLOAD_LIMIT
        );
        assert_eq!(event["structured"]["findings"]["prefix"]["included"], 1);
        assert_eq!(event["structured"]["hints"]["included"], 0);
        assert_eq!(event["structured"]["hints"]["omitted"]["value"], 2);
        assert_eq!(event["structured"]["error"]["prefix"]["included"], 1);
        assert_eq!(
            event["stdout"]["writer_accepted_bytes"]["status"],
            "unknown"
        );
        assert_eq!(event["stdout"]["os_accepted_bytes"], "unknown");
        // Reduced internal limit exercises aggregate stream trimming without
        // changing the fixed public limits or relying on huge metadata.
        let event: Value =
            serde_json::from_slice(&finish_with_limit(input(&id, &stdout, &stderr), 5000).unwrap())
                .unwrap();
        let stdout_bytes = STANDARD
            .decode(event["stdout"]["prefix_base64"].as_str().unwrap())
            .unwrap();
        assert!(!stdout_bytes.is_empty());
        assert!(stdout_bytes.len() < 64 * 1024);
        assert_eq!(event["stderr"]["retained_bytes"], 0);
        assert_eq!(event["stdout"]["observed_input_bytes"]["bytes"], 100_000);
        assert_eq!(
            event["stderr"]["writer_accepted_bytes"]["bytes"]["bytes"],
            20_000
        );
        assert_eq!(event["stdout"]["truncated"], true);
    }
    #[test]
    fn admission_stops_at_first_rejected_record() {
        struct Record<'a>(&'a Cell<usize>);
        impl Serialize for Record<'_> {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                self.0.set(self.0.get() + 1);
                "too large".serialize(s)
            }
        }
        let visits = Cell::new(0);
        let records = [Record(&visits), Record(&visits)];
        let mut prefix = Prefix::empty(Availability::Available(2));
        let mut remaining = 5;
        admit_records(&records, &mut remaining, &mut prefix).unwrap();
        assert_eq!(visits.get(), 1);
        assert_eq!(prefix.included, 0);
    }
    #[derive(Debug)]
    struct ObservedError {
        formats: Cell<usize>,
        sources: Cell<usize>,
        fail: bool,
        swallow: bool,
    }
    impl fmt::Display for ObservedError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            self.formats.set(self.formats.get() + 1);
            if self.fail {
                return Err(fmt::Error);
            }
            let result = f.write_str("too large for remaining budget");
            if self.swallow { Ok(()) } else { result }
        }
    }
    impl Error for ObservedError {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            self.sources.set(self.sources.get() + 1);
            Some(self)
        }
    }
    fn error_payload() -> ErrorPayload {
        ErrorPayload {
            availability: "available",
            present: Availability::Available(true),
            traversal: ErrorTraversal::NotStarted,
            prefix: Prefix::empty(Availability::Unavailable(
                UnavailableReason::TraversalStopped,
            )),
        }
    }
    #[test]
    fn bounded_formatter_latches_overflow_and_does_not_traverse_sources() {
        for swallow in [false, true] {
            let error = ObservedError {
                formats: Cell::new(0),
                sources: Cell::new(0),
                fail: false,
                swallow,
            };
            let mut payload = error_payload();
            admit_error(&error, &mut 10, &mut payload).unwrap();
            assert_eq!(error.formats.get(), 1);
            assert_eq!(error.sources.get(), 0);
            assert_eq!(
                serde_json::to_value(&payload).unwrap()["traversal"],
                "budget_stopped"
            );
            assert_eq!(payload.prefix.included, 0);
        }
    }
    #[test]
    fn formatter_failure_and_cyclic_chain_are_explicit() {
        for fail in [true, false] {
            let error = ObservedError {
                formats: Cell::new(0),
                sources: Cell::new(0),
                fail,
                swallow: false,
            };
            let mut payload = error_payload();
            let mut remaining = STRUCTURED_PAYLOAD_LIMIT;
            admit_error(&error, &mut remaining, &mut payload).unwrap();
            let event = serde_json::to_value(&payload).unwrap();
            assert_eq!(event["prefix"]["total"]["status"], "unavailable");
            assert_eq!(
                event["traversal"],
                if fail {
                    "formatter_failed"
                } else {
                    "depth_stopped"
                }
            );
            assert_eq!(
                error.formats.get(),
                if fail { 1 } else { ERROR_DEPTH_LIMIT }
            );
            assert_eq!(
                error.sources.get(),
                if fail { 0 } else { ERROR_DEPTH_LIMIT - 1 }
            );
        }
    }
    fn start_input<'a>(
        id: &'a InvocationId,
        fingerprint: &'a ConfigFingerprint,
        argv: &'a [OsString],
    ) -> StartInput<'a> {
        StartInput {
            id,
            entry_wall_time: Availability::Available(Timestamp::new(100, 123).unwrap()),
            collected_wall_time: Availability::Available(Timestamp::new(101, 456).unwrap()),
            binary_version: "0.18.0",
            original_cwd: OsStr::new("/work"),
            effective_directory: OsStr::new("/work/project"),
            canonical_graph_root: OsStr::new("/work/project"),
            command: Command::Graph,
            requested_format: RequestedFormat::Json,
            argv,
            config_fingerprint: fingerprint,
            caller_id: Some(OsStr::new("caller-example")),
        }
    }
    #[test]
    fn aggregate_metadata_and_escaped_metadata_overflow_are_rejected() {
        let id = id();
        let fingerprint = ConfigFingerprint::from_parsed_bytes(b"config");
        let argv = vec![OsString::from("x".repeat(1000)); 300];
        assert!(start(start_input(&id, &fingerprint, &argv)).is_err());
        let argv = [OsString::from("\0".repeat(EVENT_LIMIT / 6))];
        assert!(start(start_input(&id, &fingerprint, &argv)).is_err());
        let huge = OsString::from("x".repeat(EVENT_LIMIT));
        let mut i = start_input(&id, &fingerprint, &[]);
        i.caller_id = Some(&huge);
        assert!(start(i).is_err());
        assert!(matches!(
            finish_with_limit(
                input(&id, &StreamCapture::stdout(), &StreamCapture::stderr()),
                1
            ),
            Err(BoundedError::LimitExceeded)
        ));
    }
    #[test]
    fn reduced_event_budget_handles_counter_growth_without_underflow() {
        let id = id();
        let stdout = StreamCapture::stdout();
        let stderr = StreamCapture::stderr();
        let findings: Vec<_> = (0..100)
            .map(|_| Finding::warn("a", "b", vec![], "x".repeat(100)))
            .collect();
        let mut i = input(&id, &stdout, &stderr);
        i.findings = Availability::Available(Findings {
            coverage: FindingCoverage::ConstructionDiagnostics,
            records: &findings,
        });
        let bytes = finish_with_limit(i, 5000).unwrap();
        assert!(bytes.len() <= 5000);
        let event: Value = serde_json::from_slice(&bytes).unwrap();
        assert!(
            event["structured"]["findings"]["prefix"]["included"]
                .as_u64()
                .unwrap()
                > 0
        );
        assert!(
            event["structured"]["findings"]["prefix"]["omitted"]["value"]
                .as_u64()
                .unwrap()
                > 0
        );
    }

    #[test]
    fn stream_admission_reserves_counter_growth_across_base64_boundaries() {
        let id = id();
        let mut stdout = StreamCapture::stdout();
        let stderr = StreamCapture::stderr();
        stdout.begin_write(&vec![b'x'; 10_000]).finish(&Ok(()));
        for limit in 5000..5100 {
            let bytes = finish_with_limit(input(&id, &stdout, &stderr), limit)
                .unwrap_or_else(|error| panic!("limit {limit}: {error}"));
            assert!(bytes.len() <= limit);
        }
    }

    #[test]
    fn unavailable_reasons_survive_observation_admission() {
        let id = id();
        let stdout = StreamCapture::stdout();
        let stderr = StreamCapture::stderr();
        let mut i = input(&id, &stdout, &stderr);
        i.findings = Availability::Unavailable(UnavailableReason::ExecutionStopped);
        i.hints = Availability::Unavailable(UnavailableReason::ExecutionStopped);
        i.error = Availability::Unavailable(UnavailableReason::NotObserved);
        let event = value(i);
        assert_eq!(
            event["structured"]["findings"]["coverage"]["value"],
            "execution_stopped"
        );
        assert_eq!(
            event["structured"]["findings"]["prefix"]["total"]["value"],
            "execution_stopped"
        );
        assert_eq!(
            event["structured"]["hints"]["total"]["value"],
            "execution_stopped"
        );
        assert_eq!(
            event["structured"]["error"]["present"]["value"],
            "not_observed"
        );
    }

    #[test]
    fn literal_refused_finish_fixture() {
        let id = id();
        let stdout = StreamCapture::stdout();
        let mut stderr = StreamCapture::stderr();
        stderr
            .begin_write(b"output exceeds budget\n")
            .finish(&Err(io::ErrorKind::BrokenPipe.into()));
        let findings = [Finding::warn(
            "stale-edge",
            "index.md",
            vec!["@fs".into()],
            "source changed",
        )
        .with_target("source.md")
        .with_lines(vec![7])
        .with_cause("review source")];
        let hints = [Hint::new("large-projection", "too large")
            .at("graph")
            .with_next("narrow selection")];
        let error = io::Error::other("output exceeds budget");
        let mut i = input(&id, &stdout, &stderr);
        i.findings = Availability::Available(Findings {
            coverage: FindingCoverage::FullPolicyFilteredEvaluation,
            records: &findings,
        });
        i.hints = Availability::Available(&hints);
        i.error = Availability::Available(Some(&error));
        i.intended_exit = IntendedExit::UsageError;
        i.completion = Completion::OutputBudgetRefused;
        i.output_mode = OutputMode::Json;
        i.budget_refusal = Some(BudgetRefusal {
            rendered_bytes: 5000,
            budget_bytes: 1000,
        });
        i.hint_observation = HintObservation {
            embedding: HintEmbedding::ResultDocument,
            route: HintRoute::StdoutDocument,
            suppression: HintSuppression::BudgetRefusal,
            ..hint_observation()
        };
        let expected: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/usage/finish-refused-v1.json"
        ))
        .unwrap();
        assert_eq!(value(i), expected);
    }

    #[cfg(unix)]
    #[test]
    fn literal_revision_one_fixtures() {
        use std::os::unix::ffi::OsStringExt;
        let id = id();
        let fingerprint = ConfigFingerprint::from_parsed_bytes(b"config");
        let argv = [
            OsString::from("drft"),
            OsString::from("graph"),
            OsString::from("--raw"),
            OsString::from_vec(vec![0xff]),
        ];
        let start = start(start_input(&id, &fingerprint, &argv)).unwrap();
        let mut stdout = StreamCapture::stdout();
        let stderr = StreamCapture::stderr();
        stdout.begin_write(b"{\"graphs\":[]}\n").finish(&Ok(()));
        let finish = finish(input(&id, &stdout, &stderr)).unwrap();
        for (actual, expected) in [
            (
                start,
                include_str!("../../tests/fixtures/usage/start-v1.json"),
            ),
            (
                finish,
                include_str!("../../tests/fixtures/usage/finish-v1.json"),
            ),
        ] {
            let actual: Value = serde_json::from_slice(&actual).unwrap();
            let expected: Value = serde_json::from_str(expected).unwrap();
            assert_eq!(actual, expected);
        }
    }
}
