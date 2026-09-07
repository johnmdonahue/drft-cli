//! Explicit command-lifecycle integration for local usage records.
//!
//! Collection is best effort. A failure here disables evidence for the invocation
//! and never changes command output or status.

use super::{
    capture::StreamCapture,
    event::{
        self, AttemptObservation, Availability, BudgetRefusal, Command, Completion,
        ConfigFingerprint, Elapsed, FindingCoverage, GraphSizes, HintEmbedding, HintObservation,
        HintRoute, HintSuppression, IntendedExit, OutputMode, RequestedFormat, ResultSizes,
        Timestamp, UnavailableReason, Unknown,
    },
    identity::InvocationId,
    record::WallTime,
    store::{Partition, StartReceipt},
};
use crate::{diagnostic::Finding, hints::Hint};
use std::{
    env,
    error::Error,
    ffi::OsString,
    fmt,
    io::{self, Write},
    path::{Path, PathBuf},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

/// Process-entry observations and, after opt-in, the retained publication state.
pub struct Invocation {
    entry_instant: Instant,
    entry_wall_time: Availability<Timestamp>,
    argv: Vec<OsString>,
    active: Option<Active>,
}

struct Active {
    id: InvocationId,
    partition: Partition,
    receipt: StartReceipt,
    stdout: StreamCapture,
    stderr: StreamCapture,
    graph_sizes: Availability<GraphSizes>,
    result_sizes: Availability<ResultSizes>,
    findings: Option<event::RetainedFindings>,
    hint_observation: HintObservation,
    budget_refusal: Option<BudgetRefusal>,
    output_mode: OutputMode,
}

impl Invocation {
    /// Capture exact argv, wall time, and monotonic time before clap runs. Cwd is
    /// deferred until opt-in because this process never changes it.
    pub fn capture() -> Self {
        let entry_instant = Instant::now();
        let entry_wall_time = wall_time(SystemTime::now())
            .and_then(|time| Timestamp::new(time.seconds, time.nanoseconds))
            .map(Availability::Available)
            .unwrap_or(Availability::Unavailable(UnavailableReason::NotObserved));
        Self {
            entry_instant,
            entry_wall_time,
            argv: env::args_os().collect(),
            active: None,
        }
    }

    pub fn argv(&self) -> &[OsString] {
        &self.argv
    }

    /// Activate exactly once after the normal config load established opt-in.
    /// Required metadata is bounded before cache lookup or storage bootstrap.
    pub fn activate(
        &mut self,
        enabled: bool,
        fingerprint: Option<&ConfigFingerprint>,
        effective_directory: &Path,
        graph_root: &Path,
        command: Command,
        requested_format: RequestedFormat,
    ) {
        if !enabled || self.active.is_some() {
            return;
        }
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        self.activate_supported(
            fingerprint,
            effective_directory,
            graph_root,
            command,
            requested_format,
        );
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            let _ = (
                fingerprint,
                effective_directory,
                graph_root,
                command,
                requested_format,
            );
        }
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn activate_supported(
        &mut self,
        fingerprint: Option<&ConfigFingerprint>,
        effective_directory: &Path,
        graph_root: &Path,
        command: Command,
        requested_format: RequestedFormat,
    ) {
        let Some(fingerprint) = fingerprint else {
            return;
        };
        // The CLI never changes cwd: `-C` resolves another effective directory.
        // Deferring this lookup therefore preserves the entry cwd while ensuring
        // disabled and excluded invocations perform no collector filesystem call.
        let Some(original_cwd) = env::current_dir().ok() else {
            return;
        };

        let activated = (|| {
            let id = InvocationId::generate().ok()?;
            let collected = SystemTime::now();
            let collected_wall = wall_time(collected);
            let collected_timestamp = collected_wall
                .and_then(|time| Timestamp::new(time.seconds, time.nanoseconds))
                .map(Availability::Available)
                .unwrap_or(Availability::Unavailable(UnavailableReason::NotObserved));
            let bytes = event::start(event::StartInput {
                id: &id,
                entry_wall_time: self.entry_wall_time,
                collected_wall_time: collected_timestamp,
                binary_version: env!("CARGO_PKG_VERSION"),
                original_cwd: original_cwd.as_os_str(),
                effective_directory: effective_directory.as_os_str(),
                canonical_graph_root: graph_root.as_os_str(),
                command,
                requested_format,
                argv: &self.argv,
                caller_id: None,
                config_fingerprint: fingerprint,
            })
            .ok()?;

            let cache = usage_cache_path()?;
            let mut partition = Partition::open_or_create(&cache, graph_root).ok()?;
            let outcome = {
                let guard = partition.try_lock().ok()?;
                guard.publish_start(&id, &bytes, collected_wall)
            };
            let receipt = outcome.receipt?;
            Some(Active {
                id,
                partition,
                receipt,
                stdout: StreamCapture::stdout(),
                stderr: StreamCapture::stderr(),
                graph_sizes: Availability::Unavailable(UnavailableReason::NotObserved),
                result_sizes: Availability::Unavailable(UnavailableReason::NotObserved),
                findings: None,
                hint_observation: HintObservation {
                    embedding: HintEmbedding::NotEmbedded,
                    route: HintRoute::None,
                    suppression: HintSuppression::None,
                    write_attempt: AttemptObservation::NotAttempted,
                    writer_acceptance: Unknown::Unknown,
                    os_acceptance: Unknown::Unknown,
                    downstream_consumption: Unknown::Unknown,
                },
                budget_refusal: None,
                output_mode: OutputMode::NoDocument,
            })
        })();
        self.active = activated;
    }

    /// Observe the exact bytes and result of the command's existing `write_all`.
    pub fn write_stdout<W: Write>(
        &mut self,
        writer: &mut W,
        mode: OutputMode,
        bytes: &[u8],
    ) -> io::Result<()> {
        let Some(active) = self.active.as_mut() else {
            return writer.write_all(bytes);
        };
        active.output_mode = mode;
        if matches!(
            active.hint_observation.embedding,
            HintEmbedding::ResultDocument
        ) {
            active.hint_observation.write_attempt = AttemptObservation::Attempted;
        }
        let attempt = active.stdout.begin_write(bytes);
        let result = writer.write_all(bytes);
        attempt.finish(&result);
        if result.is_err() && matches!(active.hint_observation.route, HintRoute::StdoutDocument) {
            active.hint_observation.suppression = HintSuppression::EarlierWriteFailure;
        }
        result
    }

    /// Format once through the real stderr writer while retaining bounded chunks.
    pub fn write_stderr<W: Write>(
        &mut self,
        writer: &mut W,
        arguments: fmt::Arguments<'_>,
    ) -> io::Result<()> {
        let capture = self.active.as_mut().map(|active| &mut active.stderr);
        let mut observer = StderrObserver {
            writer,
            capture,
            error: None,
        };
        match fmt::write(&mut observer, arguments) {
            Ok(()) => Ok(()),
            Err(_) => Err(observer.error.unwrap_or_else(|| {
                io::Error::other("formatter returned an error while writing stderr")
            })),
        }
    }

    pub fn observe_graph_sizes(&mut self, graphs: usize, nodes: usize, edges: usize) {
        if let Some(active) = self.active.as_mut() {
            active.graph_sizes = Availability::Available(GraphSizes {
                graphs,
                nodes,
                edges,
            });
        }
    }

    pub fn observe_result_sizes(&mut self, sizes: ResultSizes) {
        if let Some(active) = self.active.as_mut() {
            active.result_sizes = Availability::Available(sizes);
        }
    }

    pub fn observe_findings(&mut self, coverage: FindingCoverage, findings: &[Finding]) {
        if let Some(active) = self.active.as_mut() {
            active.findings = event::RetainedFindings::capture(coverage, findings).ok();
        }
    }

    pub fn observe_hints_embedded(&mut self) {
        if let Some(active) = self.active.as_mut() {
            active.hint_observation.embedding = HintEmbedding::ResultDocument;
            active.hint_observation.route = HintRoute::StdoutDocument;
        }
    }

    pub fn observe_hint_stderr_attempt(&mut self, route: HintRoute) {
        if let Some(active) = self.active.as_mut() {
            active.hint_observation.route = route;
            active.hint_observation.write_attempt = AttemptObservation::Attempted;
        }
    }

    pub fn observe_hint_suppression(&mut self, suppression: HintSuppression) {
        if let Some(active) = self
            .active
            .as_mut()
            .filter(|active| matches!(active.hint_observation.suppression, HintSuppression::None))
        {
            active.hint_observation.suppression = suppression;
        }
    }

    pub fn observe_budget_refusal(&mut self, rendered: usize, budget: usize) {
        if let Some(active) = self.active.as_mut() {
            active.hint_observation.suppression = HintSuppression::BudgetRefusal;
            active.budget_refusal = Some(BudgetRefusal {
                rendered_bytes: u64::try_from(rendered).unwrap_or(u64::MAX),
                budget_bytes: u64::try_from(budget).unwrap_or(u64::MAX),
            });
        }
    }

    pub fn stdout_write_failed(&self) -> bool {
        self.active.as_ref().is_some_and(|active| {
            matches!(
                active.stdout.snapshot().write_outcome,
                super::capture::WriteOutcome::UnknownAcceptance { failed: true, .. }
            )
        })
    }

    /// Prepare and publish finish only after all normal output handling is done.
    pub fn finish(
        &mut self,
        exit_code: i32,
        completion: Completion,
        error: Option<&(dyn Error + 'static)>,
        hints: &[Hint],
    ) {
        let Some(mut active) = self.active.take() else {
            return;
        };
        let collected = SystemTime::now();
        let input = event::FinishInput {
            id: &active.id,
            entry_wall_time: self.entry_wall_time,
            collected_wall_time: wall_time(collected)
                .and_then(|time| Timestamp::new(time.seconds, time.nanoseconds))
                .map(Availability::Available)
                .unwrap_or(Availability::Unavailable(UnavailableReason::NotObserved)),
            elapsed: Availability::Available(Elapsed::from(self.entry_instant.elapsed())),
            // The immutable envelope cannot include its own serialization work.
            collector_work_through_preparation: Availability::Unavailable(
                UnavailableReason::NotObserved,
            ),
            intended_exit: match exit_code {
                0 => IntendedExit::Clean,
                1 => IntendedExit::Violations,
                _ => IntendedExit::UsageError,
            },
            completion,
            output_mode: active.output_mode,
            graph_sizes: active.graph_sizes,
            result_sizes: active.result_sizes,
            findings: Availability::Unavailable(UnavailableReason::NotObserved),
            hints: Availability::Available(hints),
            hint_observation: active.hint_observation,
            error: Availability::Available(error),
            budget_refusal: active.budget_refusal,
            stdout: &active.stdout,
            stderr: &active.stderr,
        };
        let bytes = match active.findings.as_ref() {
            Some(findings) => event::finish_retained(input, findings),
            None => event::finish(input),
        };
        let Ok(bytes) = bytes else {
            return;
        };
        if let Ok(guard) = active.partition.try_lock() {
            let _ = guard.publish_finish(&active.receipt, &bytes, wall_time(collected));
        }
    }
}

struct StderrObserver<'a, W> {
    writer: &'a mut W,
    capture: Option<&'a mut StreamCapture>,
    error: Option<io::Error>,
}

impl<W: Write> fmt::Write for StderrObserver<'_, W> {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        let attempt = self
            .capture
            .as_deref_mut()
            .map(|capture| capture.begin_write(value.as_bytes()));
        let result = self.writer.write_all(value.as_bytes());
        if let Some(attempt) = attempt {
            attempt.finish(&result);
        }
        if let Err(error) = result {
            self.error = Some(error);
            return Err(fmt::Error);
        }
        Ok(())
    }
}

fn wall_time(value: SystemTime) -> Option<WallTime> {
    match value.duration_since(UNIX_EPOCH) {
        Ok(duration) => Some(WallTime {
            seconds: i64::try_from(duration.as_secs()).ok()?,
            nanoseconds: duration.subsec_nanos(),
        }),
        Err(error) => {
            let duration = error.duration();
            let seconds = i64::try_from(duration.as_secs()).ok()?;
            Some(if duration.subsec_nanos() == 0 {
                WallTime {
                    seconds: -seconds,
                    nanoseconds: 0,
                }
            } else {
                WallTime {
                    seconds: seconds.checked_add(1)?.checked_neg()?,
                    nanoseconds: 1_000_000_000 - duration.subsec_nanos(),
                }
            })
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn usage_cache_path() -> Option<PathBuf> {
    usage_cache_path_with(|name| env::var_os(name))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn usage_cache_path() -> Option<PathBuf> {
    None
}

#[cfg(target_os = "macos")]
fn usage_cache_path_with(mut get: impl FnMut(&str) -> Option<OsString>) -> Option<PathBuf> {
    absolute(get("HOME")?).map(|home| home.join("Library/Caches/drft/usage"))
}

#[cfg(target_os = "linux")]
fn usage_cache_path_with(mut get: impl FnMut(&str) -> Option<OsString>) -> Option<PathBuf> {
    match get("XDG_CACHE_HOME") {
        Some(xdg) => absolute(xdg).map(|path| path.join("drft/usage")),
        None => absolute(get("HOME")?).map(|home| home.join(".cache/drft/usage")),
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn absolute(value: OsString) -> Option<PathBuf> {
    let path = PathBuf::from(value);
    (path.is_absolute() && !path.as_os_str().is_empty()).then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn wall_time_before_epoch_is_normalized() {
        let value = UNIX_EPOCH - std::time::Duration::new(1, 250_000_000);
        assert_eq!(
            wall_time(value),
            Some(WallTime {
                seconds: -2,
                nanoseconds: 750_000_000
            })
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_cache_path_distinguishes_absent_from_invalid_xdg() {
        let lookup = |pairs: &[(&str, &str)]| {
            let values = pairs
                .iter()
                .map(|(key, value)| ((*key).to_owned(), OsString::from(value)))
                .collect::<std::collections::BTreeMap<_, _>>();
            move |key: &str| values.get(key).cloned()
        };
        assert_eq!(
            usage_cache_path_with(lookup(&[("HOME", "/home/a")])),
            Some(PathBuf::from("/home/a/.cache/drft/usage"))
        );
        assert_eq!(
            usage_cache_path_with(lookup(&[("HOME", "/home/a"), ("XDG_CACHE_HOME", "")])),
            None
        );
        assert_eq!(
            usage_cache_path_with(lookup(&[("HOME", "/home/a"), ("XDG_CACHE_HOME", "cache")])),
            None
        );
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn native_cache_requires_absolute_home_when_no_xdg_is_selected() {
        for home in [None, Some(""), Some("relative-home")] {
            assert_eq!(
                usage_cache_path_with(|key| {
                    if key == "HOME" {
                        home.map(OsString::from)
                    } else {
                        None
                    }
                }),
                None
            );
        }
        let path =
            usage_cache_path_with(|key| (key == "HOME").then(|| OsString::from("/home/example")));
        #[cfg(target_os = "macos")]
        assert_eq!(
            path,
            Some(PathBuf::from("/home/example/Library/Caches/drft/usage"))
        );
        #[cfg(target_os = "linux")]
        assert_eq!(path, Some(PathBuf::from("/home/example/.cache/drft/usage")));
        #[cfg(target_os = "linux")]
        assert_eq!(
            usage_cache_path_with(|key| {
                (key == "XDG_CACHE_HOME").then(|| OsString::from("/cache"))
            }),
            Some(PathBuf::from("/cache/drft/usage"))
        );
    }

    #[test]
    fn stderr_formats_once_and_records_the_exact_failed_write() {
        struct DisplayOnce<'a>(&'a Cell<usize>);
        impl fmt::Display for DisplayOnce<'_> {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.set(self.0.get() + 1);
                formatter.write_str("message")
            }
        }
        struct Broken;
        impl Write for Broken {
            fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
                Err(io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let calls = Cell::new(0);
        let mut capture = StreamCapture::stderr();
        let mut writer = Broken;
        let mut observer = StderrObserver {
            writer: &mut writer,
            capture: Some(&mut capture),
            error: None,
        };
        assert!(fmt::write(&mut observer, format_args!("{}\n", DisplayOnce(&calls))).is_err());
        assert_eq!(calls.get(), 1);
        let snapshot = capture.snapshot();
        assert_eq!(
            snapshot.observed_input_bytes,
            super::super::capture::ByteCount::Exact("message".len() as u64)
        );
        assert!(matches!(
            snapshot.write_outcome,
            super::super::capture::WriteOutcome::UnknownAcceptance {
                failed: true,
                unfinished: false
            }
        ));
    }
}
