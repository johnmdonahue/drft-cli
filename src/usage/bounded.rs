//! Serialization budgets for inactive usage primitives, not event envelopes.

use std::io::{self, Write};

use serde::Serialize;

pub const STRUCTURED_PAYLOAD_LIMIT: usize = 128 * 1024;
pub const EVENT_LIMIT: usize = 256 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum BoundedError {
    #[error("requested serialization budget exceeds the fixed maximum")]
    InvalidLimit,
    #[error("serialized value exceeds its byte budget")]
    LimitExceeded,
    #[error("serialization failed: {0}")]
    Serialization(#[source] serde_json::Error),
}

struct CappedWriter {
    bytes: Vec<u8>,
    limit: usize,
    exceeded: bool,
}

impl CappedWriter {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(limit),
            limit,
            exceeded: false,
        }
    }
}

impl Write for CappedWriter {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        if self.exceeded || input.len() > self.limit - self.bytes.len() {
            self.exceeded = true;
            return Err(io::Error::other("serialization byte budget exceeded"));
        }
        self.bytes.extend_from_slice(input);
        Ok(input.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Serialize directly into capped storage, discarding all bytes on failure.
///
/// The writer latches budget failures even if a custom `Serialize` implementation
/// ignores the serializer's error. This bounds storage owned here; a custom
/// serializer can still allocate or compute arbitrarily on its own.
pub fn to_json_bounded<T: Serialize + ?Sized>(
    value: &T,
    limit: usize,
) -> Result<Vec<u8>, BoundedError> {
    if limit > EVENT_LIMIT {
        return Err(BoundedError::InvalidLimit);
    }
    let mut writer = CappedWriter::new(limit);
    let result = serde_json::to_writer(&mut writer, value);
    if writer.exceeded {
        return Err(BoundedError::LimitExceeded);
    }
    result.map_err(BoundedError::Serialization)?;
    Ok(writer.bytes)
}

/// A JSON array containing a deterministic prefix of complete borrowed records.
/// Its counts are metadata; callers must budget them and any event envelope.
#[derive(Debug)]
pub struct RecordPrefix {
    json: Vec<u8>,
    total: usize,
    included: usize,
}

impl RecordPrefix {
    pub fn json(&self) -> &[u8] {
        &self.json
    }

    pub fn total(&self) -> usize {
        self.total
    }

    pub fn included(&self) -> usize {
        self.included
    }

    pub fn omitted(&self) -> usize {
        self.total - self.included
    }
}

/// Admit records in input order, stopping at the first that cannot fit.
/// Brackets and commas count against `limit`. No later record is visited after
/// a budget refusal. A non-budget serialization error discards the whole result.
pub fn record_prefix<T: Serialize>(
    records: &[T],
    limit: usize,
) -> Result<RecordPrefix, BoundedError> {
    if limit > STRUCTURED_PAYLOAD_LIMIT {
        return Err(BoundedError::InvalidLimit);
    }
    if limit < 2 {
        return Err(BoundedError::LimitExceeded);
    }
    let mut json = Vec::with_capacity(limit);
    json.push(b'[');
    let mut included = 0;
    for record in records {
        let comma = usize::from(included != 0);
        let Some(remaining) = limit.checked_sub(json.len() + 1 + comma) else {
            break;
        };
        match to_json_bounded(record, remaining) {
            Ok(bytes) => {
                if comma != 0 {
                    json.push(b',');
                }
                json.extend_from_slice(&bytes);
                included += 1;
            }
            Err(BoundedError::LimitExceeded) => break,
            Err(error) => return Err(error),
        }
    }
    json.push(b']');
    Ok(RecordPrefix {
        json,
        total: records.len(),
        included,
    })
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use serde::ser::SerializeSeq;

    use super::*;

    // Derived boundary fixtures: exact JSON size, one byte short, zero budget,
    // bracket/comma overhead, first-record refusal, and poisoned custom writers.
    #[test]
    fn scalar_budgets_and_fixed_maximum() {
        assert_eq!(to_json_bounded(&"é", 4).unwrap(), b"\"\xc3\xa9\"");
        assert!(matches!(
            to_json_bounded(&"é", 3),
            Err(BoundedError::LimitExceeded)
        ));
        assert!(matches!(
            to_json_bounded(&0, 0),
            Err(BoundedError::LimitExceeded)
        ));
        assert!(matches!(
            to_json_bounded(&0, EVENT_LIMIT + 1),
            Err(BoundedError::InvalidLimit)
        ));
    }

    #[test]
    fn fixed_limits_include_escaping_and_base64_expansion() {
        use base64::{Engine as _, engine::general_purpose::STANDARD};

        let exact = "x".repeat(EVENT_LIMIT - 2);
        assert_eq!(
            to_json_bounded(&exact, EVENT_LIMIT).unwrap().len(),
            EVENT_LIMIT
        );
        assert!(matches!(
            to_json_bounded(&(exact + "x"), EVENT_LIMIT),
            Err(BoundedError::LimitExceeded)
        ));
        // Each NUL needs six JSON bytes. The next small item must not replace
        // this oversized first record even though it would fit by itself.
        let escaped = "\0".repeat(STRUCTURED_PAYLOAD_LIMIT / 6 + 1);
        let records = [escaped, "small".to_owned()];
        let prefix = record_prefix(&records, STRUCTURED_PAYLOAD_LIMIT).unwrap();
        assert_eq!(prefix.json(), b"[]");
        assert_eq!(prefix.omitted(), 2);
        let exact_record = "x".repeat(STRUCTURED_PAYLOAD_LIMIT - 4);
        assert_eq!(
            record_prefix(&[&exact_record], STRUCTURED_PAYLOAD_LIMIT)
                .unwrap()
                .json()
                .len(),
            STRUCTURED_PAYLOAD_LIMIT
        );
        let encoded = STANDARD.encode(vec![0; EVENT_LIMIT / 4 * 3]);
        assert_eq!(encoded.len(), EVENT_LIMIT);
        assert!(matches!(
            to_json_bounded(&encoded, EVENT_LIMIT),
            Err(BoundedError::LimitExceeded)
        ));
    }

    #[test]
    fn prefix_counts_delimiters_without_skipping() {
        let prefix = record_prefix(&[1, 22, 3], 6).unwrap();
        assert_eq!(prefix.json(), b"[1,22]");
        assert_eq!(
            (prefix.total(), prefix.included(), prefix.omitted()),
            (3, 2, 1)
        );
        assert_eq!(record_prefix(&[1, 22, 3], 5).unwrap().json(), b"[1]");
        assert_eq!(record_prefix(&[999, 1], 3).unwrap().json(), b"[]");
        assert_eq!(record_prefix::<u8>(&[], 2).unwrap().json(), b"[]");
        assert!(record_prefix::<u8>(&[], 1).is_err());
        assert!(matches!(
            record_prefix::<u8>(&[], STRUCTURED_PAYLOAD_LIMIT + 1),
            Err(BoundedError::InvalidLimit)
        ));
    }

    struct Instrumented<'a> {
        visited: &'a Cell<usize>,
        swallow: bool,
    }

    impl Serialize for Instrumented<'_> {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            self.visited.set(self.visited.get() + 1);
            let mut sequence = serializer.serialize_seq(None)?;
            for _ in 0..100_000 {
                self.visited.set(self.visited.get() + 1);
                let result = sequence.serialize_element(&12345);
                if self.swallow && result.is_err() {
                    break;
                }
                result?;
            }
            sequence.end()
        }
    }

    #[test]
    fn stops_serializing_at_budget_and_never_visits_later_records() {
        let visited = Cell::new(0);
        let records = [
            Instrumented {
                visited: &visited,
                swallow: false,
            },
            Instrumented {
                visited: &visited,
                swallow: false,
            },
        ];
        assert_eq!(record_prefix(&records, 12).unwrap().json(), b"[]");
        assert!(visited.get() <= 4);
    }

    #[test]
    fn write_error_is_latched_even_when_serializer_swallows_it() {
        struct Swallow;
        impl Serialize for Swallow {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                let mut sequence = serializer.serialize_seq(None)?;
                let _ = sequence.serialize_element(&"too large");
                sequence.end()
            }
        }
        assert!(matches!(
            to_json_bounded(&Swallow, 2),
            Err(BoundedError::LimitExceeded)
        ));
        let mut writer = CappedWriter::new(2);
        assert!(writer.write_all(b"long").is_err());
        assert!(writer.write_all(b"x").is_err());
        assert!(writer.bytes.is_empty());
    }

    #[test]
    fn serializer_errors_return_no_partial_prefix() {
        struct Broken;
        impl Serialize for Broken {
            fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
                Err(serde::ser::Error::custom("broken"))
            }
        }
        assert!(matches!(
            record_prefix(&[Broken], 30),
            Err(BoundedError::Serialization(_))
        ));
    }
}
