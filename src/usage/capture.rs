//! Observe existing `write_all` inputs and outcomes without owning a writer.

use std::io;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Serialize;

pub const STDOUT_LIMIT: usize = 64 * 1024;
pub const STDERR_LIMIT: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "status", content = "bytes", rename_all = "snake_case")]
pub enum ByteCount {
    Exact(u64),
    Overflow,
}

impl ByteCount {
    fn add(&mut self, bytes: usize) {
        *self = match (*self, u64::try_from(bytes)) {
            (Self::Exact(previous), Ok(bytes)) => previous
                .checked_add(bytes)
                .map(Self::Exact)
                .unwrap_or(Self::Overflow),
            _ => Self::Overflow,
        };
    }
}

/// Acceptance reported by the supplied writer, which may buffer its input.
/// This never establishes OS acceptance or downstream consumption.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "status", content = "bytes", rename_all = "snake_case")]
pub enum AcceptedBytes {
    NotAttempted,
    Known(ByteCount),
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DownstreamConsumption {
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum WriteOutcome {
    NotAttempted,
    AllSucceeded,
    UnknownAcceptance { failed: bool, unfinished: bool },
}

#[derive(Debug, Serialize)]
pub struct CaptureSnapshot {
    pub prefix_base64: String,
    pub observed_input_bytes: ByteCount,
    pub retained_bytes: usize,
    pub truncated: bool,
    pub write_outcome: WriteOutcome,
    /// Bytes accepted by `write_all`; OS acceptance remains unobserved.
    pub writer_accepted_bytes: AcceptedBytes,
    pub os_accepted_bytes: DownstreamConsumption,
    pub downstream_consumption: DownstreamConsumption,
}

/// Fixed per-stream storage; no per-attempt log is accumulated.
pub struct StreamCapture {
    prefix: Vec<u8>,
    limit: usize,
    observed: ByteCount,
    attempted: bool,
    failed: bool,
    unfinished: bool,
    pending: bool,
    truncated: bool,
}

impl StreamCapture {
    pub fn stdout() -> Self {
        Self::new(STDOUT_LIMIT)
    }

    pub fn stderr() -> Self {
        Self::new(STDERR_LIMIT)
    }

    fn new(limit: usize) -> Self {
        Self {
            prefix: Vec::with_capacity(limit),
            limit,
            observed: ByteCount::Exact(0),
            attempted: false,
            failed: false,
            unfinished: false,
            pending: false,
            truncated: false,
        }
    }

    /// Call immediately before the existing writer's `write_all(input)` and
    /// pass its result to `WriteAttempt::finish`. The original writer and its
    /// error handling stay with the caller. Dropping an unfinished attempt
    /// preserves uncertainty, including during unwinding.
    pub fn begin_write(&mut self, input: &[u8]) -> WriteAttempt<'_> {
        // A forgotten attempt cannot erase uncertainty on a subsequent call.
        self.unfinished |= self.pending;
        self.pending = true;
        self.attempted = true;
        self.observed.add(input.len());
        let retain = input.len().min(self.limit - self.prefix.len());
        self.prefix.extend_from_slice(&input[..retain]);
        self.truncated |= retain < input.len();
        WriteAttempt { capture: self }
    }

    pub fn snapshot(&self) -> CaptureSnapshot {
        self.snapshot_prefix(self.prefix.len())
    }

    /// Preserve observations while retaining only the requested raw-byte prefix.
    pub(crate) fn snapshot_prefix(&self, retained: usize) -> CaptureSnapshot {
        let retained = retained.min(self.prefix.len());
        let unfinished = self.unfinished || self.pending;
        let write_outcome = if !self.attempted {
            WriteOutcome::NotAttempted
        } else if self.failed || unfinished {
            WriteOutcome::UnknownAcceptance {
                failed: self.failed,
                unfinished,
            }
        } else {
            WriteOutcome::AllSucceeded
        };
        let writer_accepted_bytes = match write_outcome {
            WriteOutcome::NotAttempted => AcceptedBytes::NotAttempted,
            WriteOutcome::AllSucceeded => AcceptedBytes::Known(self.observed),
            WriteOutcome::UnknownAcceptance { .. } => AcceptedBytes::Unknown,
        };
        CaptureSnapshot {
            prefix_base64: STANDARD.encode(&self.prefix[..retained]),
            observed_input_bytes: self.observed,
            retained_bytes: retained,
            truncated: self.truncated || retained < self.prefix.len(),
            write_outcome,
            writer_accepted_bytes,
            os_accepted_bytes: DownstreamConsumption::Unknown,
            downstream_consumption: DownstreamConsumption::Unknown,
        }
    }
}

#[must_use = "finish the observation with the existing write_all result"]
pub struct WriteAttempt<'a> {
    capture: &'a mut StreamCapture,
}

impl WriteAttempt<'_> {
    pub fn finish(self, result: &io::Result<()>) {
        self.capture.failed |= result.is_err();
        self.capture.pending = false;
    }
}

impl Drop for WriteAttempt<'_> {
    fn drop(&mut self) {
        self.capture.unfinished |= self.capture.pending;
        self.capture.pending = false;
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    // Derived fixtures: no attempt versus empty success, exact/over cap,
    // split UTF-8, failed partial acceptance followed by success, unfinished
    // attempts, count overflow, and many attempts with constant storage.
    #[test]
    fn unattempted_differs_from_empty_success() {
        let mut capture = StreamCapture::stdout();
        assert_eq!(
            capture.snapshot().writer_accepted_bytes,
            AcceptedBytes::NotAttempted
        );
        capture.begin_write(b"").finish(&Ok(()));
        let snapshot = capture.snapshot();
        assert_eq!(snapshot.write_outcome, WriteOutcome::AllSucceeded);
        assert_eq!(
            snapshot.writer_accepted_bytes,
            AcceptedBytes::Known(ByteCount::Exact(0))
        );
        assert_eq!(
            snapshot.downstream_consumption,
            DownstreamConsumption::Unknown
        );
    }

    #[test]
    fn snapshot_retained_count_and_truncation_survive_empty_writes() {
        let mut capture = StreamCapture::stderr();
        capture.begin_write(b"abc").finish(&Ok(()));
        assert_eq!(capture.snapshot().retained_bytes, 3);
        capture
            .begin_write(&vec![b'x'; STDERR_LIMIT])
            .finish(&Ok(()));
        assert!(capture.snapshot().truncated);
        capture.begin_write(b"").finish(&Ok(()));
        let snapshot = capture.snapshot();
        assert_eq!(snapshot.retained_bytes, STDERR_LIMIT);
        assert_eq!(
            STANDARD.decode(snapshot.prefix_base64).unwrap().len(),
            snapshot.retained_bytes
        );
        assert!(snapshot.truncated);
    }

    #[test]
    fn successful_buffering_writer_does_not_establish_os_acceptance() {
        let mut writer = io::BufWriter::with_capacity(32, Vec::new());
        let mut capture = StreamCapture::stdout();
        let attempt = capture.begin_write(b"abc");
        let result = writer.write_all(b"abc");
        attempt.finish(&result);
        result.unwrap();
        assert!(writer.get_ref().is_empty());
        let snapshot = serde_json::to_value(capture.snapshot()).unwrap();
        assert_eq!(
            snapshot["writer_accepted_bytes"],
            serde_json::json!({
                "status": "known", "bytes": { "status": "exact", "bytes": 3 }
            })
        );
        assert!(snapshot.get("accepted_bytes").is_none());
        assert_eq!(snapshot["downstream_consumption"], "unknown");
    }

    #[test]
    fn fixed_caps_and_raw_utf8_boundary() {
        for (mut capture, cap) in [
            (StreamCapture::stdout(), STDOUT_LIMIT),
            (StreamCapture::stderr(), STDERR_LIMIT),
        ] {
            capture.begin_write(&vec![b'x'; cap - 1]).finish(&Ok(()));
            capture.begin_write("é".as_bytes()).finish(&Ok(()));
            let snapshot = capture.snapshot();
            let raw = STANDARD.decode(snapshot.prefix_base64).unwrap();
            assert_eq!(raw.len(), cap);
            assert_eq!(raw[cap - 1], 0xc3);
            assert!(std::str::from_utf8(&raw).is_err());
            assert!(snapshot.truncated);
            assert_eq!(
                snapshot.observed_input_bytes,
                ByteCount::Exact((cap + 1) as u64)
            );
            assert_eq!(capture.prefix.capacity(), cap);
        }
        let mut capture = StreamCapture::stdout();
        capture.begin_write(&vec![0; STDOUT_LIMIT]).finish(&Ok(()));
        assert!(!capture.snapshot().truncated);
    }

    #[test]
    fn partial_failure_then_success_cannot_restore_known_acceptance() {
        struct PartialWriter {
            accepted: Vec<u8>,
        }
        impl Write for PartialWriter {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                if self.accepted.is_empty() {
                    self.accepted.extend_from_slice(&bytes[..2]);
                    Ok(2)
                } else {
                    Err(io::Error::new(io::ErrorKind::BrokenPipe, "fixture"))
                }
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut capture = StreamCapture::stdout();
        let mut writer = PartialWriter {
            accepted: Vec::new(),
        };
        let attempt = capture.begin_write(b"abcdef");
        let result = writer.write_all(b"abcdef");
        attempt.finish(&result);
        assert!(result.is_err());
        assert_eq!(writer.accepted, b"ab");
        capture.begin_write(b"later").finish(&Ok(()));
        let snapshot = capture.snapshot();
        assert_eq!(snapshot.writer_accepted_bytes, AcceptedBytes::Unknown);
        assert_eq!(snapshot.observed_input_bytes, ByteCount::Exact(11));
        assert_eq!(
            STANDARD.decode(snapshot.prefix_base64).unwrap(),
            b"abcdeflater"
        );
        assert_eq!(
            snapshot.write_outcome,
            WriteOutcome::UnknownAcceptance {
                failed: true,
                unfinished: false
            }
        );
    }

    #[test]
    fn unfinished_and_forgotten_attempts_remain_unknown() {
        let mut capture = StreamCapture::stderr();
        drop(capture.begin_write(b"x"));
        capture.begin_write(b"y").finish(&Ok(()));
        assert_eq!(
            capture.snapshot().write_outcome,
            WriteOutcome::UnknownAcceptance {
                failed: false,
                unfinished: true
            }
        );
        let mut forgotten = StreamCapture::stdout();
        std::mem::forget(forgotten.begin_write(b"x"));
        assert_eq!(
            forgotten.snapshot().writer_accepted_bytes,
            AcceptedBytes::Unknown
        );
        forgotten.begin_write(b"y").finish(&Ok(()));
        assert_eq!(
            forgotten.snapshot().writer_accepted_bytes,
            AcceptedBytes::Unknown
        );
    }

    #[test]
    fn many_successes_keep_fixed_memory_and_known_total() {
        let mut capture = StreamCapture::stderr();
        for _ in 0..100_000 {
            capture.begin_write(b"abc").finish(&Ok(()));
        }
        assert_eq!(capture.prefix.len(), STDERR_LIMIT);
        assert_eq!(capture.prefix.capacity(), STDERR_LIMIT);
        assert_eq!(
            capture.snapshot().writer_accepted_bytes,
            AcceptedBytes::Known(ByteCount::Exact(300_000))
        );
    }

    #[test]
    fn overflow_is_explicit_and_sticky() {
        let mut capture = StreamCapture::stdout();
        capture.observed = ByteCount::Exact(u64::MAX - 1);
        capture.begin_write(b"xx").finish(&Ok(()));
        capture.begin_write(b"").finish(&Ok(()));
        assert_eq!(capture.snapshot().observed_input_bytes, ByteCount::Overflow);
        assert_eq!(
            capture.snapshot().writer_accepted_bytes,
            AcceptedBytes::Known(ByteCount::Overflow)
        );
    }
}
