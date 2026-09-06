//! Exact OS identity, independent of graph path normalization.

use std::ffi::OsStr;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Serialize;

use super::bounded::EVENT_LIMIT;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum IdentityError {
    #[error("usage identity exceeds the event limit")]
    TooLarge,
    #[error("usage identity is unsupported on this platform")]
    Unsupported,
    #[error("usage random identity is unavailable")]
    RandomUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
enum OsEncoding {
    #[cfg(any(unix, test))]
    #[serde(rename = "unix-bytes")]
    UnixBytes,
    #[cfg(any(windows, test))]
    #[serde(rename = "windows-utf16le")]
    WindowsUtf16Le,
}

impl OsEncoding {
    fn tag(self) -> &'static [u8] {
        match self {
            #[cfg(any(unix, test))]
            Self::UnixBytes => b"unix-bytes",
            #[cfg(any(windows, test))]
            Self::WindowsUtf16Le => b"windows-utf16le",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Encoding {
    Utf8,
    Base64,
}

/// Exact text where representable, otherwise base64 of native OS units.
///
/// Construction bounds an individual value before allocating its encoded copy.
/// The future event assembler must also cap the aggregate metadata, including
/// JSON escaping; successful construction alone does not promise event fit.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct EncodedOs {
    os_encoding: OsEncoding,
    encoding: Encoding,
    value: String,
}

impl EncodedOs {
    pub fn from_os(value: &OsStr) -> Result<Self, IdentityError> {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            Self::from_unix_bytes(value.as_bytes())
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            Self::from_windows_units(value.encode_wide())
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = value;
            Err(IdentityError::Unsupported)
        }
    }

    #[cfg(any(unix, test))]
    fn from_unix_bytes(bytes: &[u8]) -> Result<Self, IdentityError> {
        if bytes.len() > EVENT_LIMIT {
            return Err(IdentityError::TooLarge);
        }
        match std::str::from_utf8(bytes) {
            Ok(text) => Ok(Self {
                os_encoding: OsEncoding::UnixBytes,
                encoding: Encoding::Utf8,
                value: text.to_owned(),
            }),
            Err(_) => Self::from_bytes(OsEncoding::UnixBytes, bytes),
        }
    }

    // Also compiled on Unix so fixtures exercise Windows' unpaired-surrogate
    // representation without claiming a native Windows runtime check.
    #[cfg(any(windows, test))]
    fn from_windows_units(units: impl IntoIterator<Item = u16>) -> Result<Self, IdentityError> {
        let mut raw = Vec::new();
        for unit in units {
            if raw.len() == EVENT_LIMIT / 2 {
                return Err(IdentityError::TooLarge);
            }
            raw.push(unit);
        }
        // Select the exact encoding before applying its size limit. A long
        // valid prefix followed by an unpaired surrogate may fit as base64
        // even when that prefix alone would overflow UTF-8 storage.
        let utf8_len = char::decode_utf16(raw.iter().copied())
            .try_fold(0usize, |len, character| {
                character.map(|c| len + c.len_utf8())
            });
        if let Ok(len) = utf8_len {
            if len > EVENT_LIMIT {
                return Err(IdentityError::TooLarge);
            }
            let text = String::from_utf16(&raw).expect("the same units were validated above");
            Ok(Self {
                os_encoding: OsEncoding::WindowsUtf16Le,
                encoding: Encoding::Utf8,
                value: text,
            })
        } else {
            if raw.len().checked_mul(2).is_none_or(|len| !base64_fits(len)) {
                return Err(IdentityError::TooLarge);
            }
            let bytes: Vec<u8> = raw.iter().flat_map(|unit| unit.to_le_bytes()).collect();
            Self::from_bytes(OsEncoding::WindowsUtf16Le, &bytes)
        }
    }

    fn from_bytes(os_encoding: OsEncoding, bytes: &[u8]) -> Result<Self, IdentityError> {
        if !base64_fits(bytes.len()) {
            return Err(IdentityError::TooLarge);
        }
        Ok(Self {
            os_encoding,
            encoding: Encoding::Base64,
            value: STANDARD.encode(bytes),
        })
    }
}

fn base64_fits(len: usize) -> bool {
    len.checked_add(2)
        .and_then(|n| (n / 3).checked_mul(4))
        .is_some_and(|n| n <= EVENT_LIMIT)
}

fn identity_hasher(encoding: OsEncoding) -> blake3::Hasher {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"drft-usage-partition\0");
    hasher.update(encoding.tag());
    hasher.update(b"\0");
    hasher
}

/// Digest a caller-supplied canonical root without resolving or normalizing it.
/// Canonicalization and placement validation belong to the future storage layer.
pub fn partition_id(canonical_root: &OsStr) -> Result<String, IdentityError> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let mut hasher = identity_hasher(OsEncoding::UnixBytes);
        hasher.update(canonical_root.as_bytes());
        Ok(hasher.finalize().to_hex().to_string())
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let mut hasher = identity_hasher(OsEncoding::WindowsUtf16Le);
        for unit in canonical_root.encode_wide() {
            hasher.update(&unit.to_le_bytes());
        }
        Ok(hasher.finalize().to_hex().to_string())
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = canonical_root;
        Err(IdentityError::Unsupported)
    }
}

/// Random 128-bit invocation identity; uniqueness still requires no-replace
/// publication in the storage layer.
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct InvocationId(String);

impl InvocationId {
    pub fn generate() -> Result<Self, IdentityError> {
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            Self::generate_with(getrandom::fill)
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            Err(IdentityError::Unsupported)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[cfg(any(target_os = "macos", target_os = "linux", test))]
    fn generate_with<E>(
        fill: impl FnOnce(&mut [u8]) -> Result<(), E>,
    ) -> Result<Self, IdentityError> {
        use std::fmt::Write as _;
        let mut bytes = [0u8; 16];
        fill(&mut bytes).map_err(|_| IdentityError::RandomUnavailable)?;
        let mut id = String::with_capacity(32);
        for byte in bytes {
            write!(id, "{byte:02x}").expect("writing to a String cannot fail");
        }
        Ok(Self(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn utf8_and_invalid_bytes_remain_distinct() {
        let text = EncodedOs::from_unix_bytes("a\\b\n�".as_bytes()).unwrap();
        assert_eq!(
            serde_json::to_value(text).unwrap(),
            json!({
                "os_encoding": "unix-bytes", "encoding": "utf8", "value": "a\\b\n�"
            })
        );
        let bytes = EncodedOs::from_unix_bytes(b"a\\b\n\xff").unwrap();
        assert_eq!(
            serde_json::to_value(bytes).unwrap(),
            json!({
                "os_encoding": "unix-bytes", "encoding": "base64", "value": "YVxiCv8="
            })
        );
    }

    #[test]
    fn windows_units_preserve_unpaired_surrogates_and_valid_text() {
        let unpaired = EncodedOs::from_windows_units([0x61, 0xd800]).unwrap();
        assert_eq!(
            serde_json::to_value(unpaired).unwrap(),
            json!({
                "os_encoding": "windows-utf16le", "encoding": "base64", "value": "YQAA2A=="
            })
        );
        let text = EncodedOs::from_windows_units("a😀".encode_utf16()).unwrap();
        assert_eq!(
            serde_json::to_value(text).unwrap(),
            json!({
                "os_encoding": "windows-utf16le", "encoding": "utf8", "value": "a😀"
            })
        );
    }

    #[test]
    fn windows_encoding_is_selected_before_its_size_limit() {
        let mut units = vec![0x0800; EVENT_LIMIT / 3 + 1];
        assert_eq!(
            EncodedOs::from_windows_units(units.iter().copied()),
            Err(IdentityError::TooLarge)
        );
        units.push(0xd800);
        let encoded = EncodedOs::from_windows_units(units.iter().copied()).unwrap();
        assert_eq!(encoded.encoding, Encoding::Base64);
        assert_eq!(
            STANDARD.decode(encoded.value).unwrap(),
            units
                .iter()
                .flat_map(|unit| unit.to_le_bytes())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn required_identity_overflow_is_rejected_without_truncation() {
        assert_eq!(
            EncodedOs::from_unix_bytes(&vec![b'a'; EVENT_LIMIT + 1]),
            Err(IdentityError::TooLarge)
        );
        assert_eq!(
            EncodedOs::from_unix_bytes(&vec![0xff; EVENT_LIMIT]),
            Err(IdentityError::TooLarge)
        );
        assert_eq!(
            EncodedOs::from_windows_units(std::iter::repeat_n(0x61, EVENT_LIMIT / 2 + 1)),
            Err(IdentityError::TooLarge)
        );
        assert!(EncodedOs::from_unix_bytes(&vec![b'a'; EVENT_LIMIT]).is_ok());
        assert!(base64_fits(EVENT_LIMIT / 4 * 3));
        assert!(!base64_fits(EVENT_LIMIT / 4 * 3 + 1));
        assert!(!base64_fits(usize::MAX));
    }

    #[test]
    fn partition_domain_includes_os_encoding() {
        let mut unix = identity_hasher(OsEncoding::UnixBytes);
        let mut windows = identity_hasher(OsEncoding::WindowsUtf16Le);
        unix.update(b"a\0");
        windows.update(b"a\0");
        assert_ne!(unix.finalize(), windows.finalize());
    }

    #[cfg(unix)]
    #[test]
    fn native_identity_bypasses_graph_normalization() {
        use std::os::unix::ffi::OsStrExt;
        let raw = OsStr::from_bytes(b"/root/a\\b\xff");
        assert_eq!(
            EncodedOs::from_os(raw).unwrap(),
            EncodedOs::from_unix_bytes(raw.as_bytes()).unwrap()
        );
        assert_eq!(partition_id(raw), partition_id(raw));
        assert_ne!(
            partition_id(raw),
            partition_id(OsStr::from_bytes(b"/root/a/b\xff"))
        );
        assert_ne!(partition_id(raw), partition_id(OsStr::new("/root/a\\b�")));
    }

    #[test]
    fn random_fill_is_exact_and_has_no_failure_fallback() {
        let id = InvocationId::generate_with(|bytes| {
            assert_eq!(bytes.len(), 16);
            bytes.copy_from_slice(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 255]);
            Ok::<_, ()>(())
        })
        .unwrap();
        assert_eq!(id.as_str(), "000102030405060708090a0b0c0d0eff");
        assert_eq!(
            InvocationId::generate_with(|bytes| {
                bytes.fill(42);
                Err(())
            }),
            Err(IdentityError::RandomUnavailable)
        );
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn native_random_source_returns_a_full_id() {
        assert_eq!(InvocationId::generate().unwrap().as_str().len(), 32);
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    #[test]
    fn unsupported_platform_does_not_generate_an_id() {
        assert_eq!(InvocationId::generate(), Err(IdentityError::Unsupported));
    }
}
