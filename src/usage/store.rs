//! Inactive storage infrastructure. Opens existing infrastructure only.
//!
//! A guard establishes placement and synchronization, not record validity.
//! Bootstrap, record scans, quotas, publication, and utility operations remain
//! separate work. No command calls this module.

use std::path::{Path, PathBuf};

#[cfg(any(target_os = "macos", target_os = "linux"))]
mod native;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("usage storage is unsupported on this platform")]
    Unsupported,
    #[error("usage cache must be an absolute path outside the graph")]
    Placement,
    #[error("usage storage contains unsafe infrastructure")]
    UnsafeEntry,
    #[error("usage storage infrastructure identity changed")]
    IdentityChanged,
    #[error("usage storage is busy")]
    Busy,
    #[error("usage storage I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

/// Existing cache/partition/lock handles. This does not initialize storage.
///
/// Cache ancestry must be stable and owner-controlled during use. Existing
/// symlink ancestors resolve once; the cache entry, partition, and lock may not
/// be symlinks. Cache and partition must belong to the effective user and may
/// not be writable by group or others. These checks do not inspect ACLs or
/// establish safety against arbitrary concurrent directory relocation.
pub struct Partition {
    root: PathBuf,
    id: String,
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    native: native::Partition,
}

impl Partition {
    /// Open an existing `<cache>/<partition digest>/.lock` without creating,
    /// truncating, or removing anything. The stable lock must be an empty,
    /// singly linked regular file owned by the effective user, with no group
    /// or other write permission.
    pub fn open_existing(cache: &Path, graph_root: &Path) -> Result<Self, StoreError> {
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            let (native, root, id) = native::Partition::open(cache, graph_root)?;
            Ok(Self { native, root, id })
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            let _ = (cache, graph_root);
            Err(StoreError::Unsupported)
        }
    }

    pub fn canonical_graph_root(&self) -> &Path {
        &self.root
    }

    pub fn partition_id(&self) -> &str {
        &self.id
    }

    /// Attempt once, without waiting or retrying. Each acquisition opens an
    /// independent lock descriptor; the guard also exclusively borrows this
    /// handle. No lock survives guard drop.
    pub fn try_lock(&mut self) -> Result<PartitionGuard<'_>, StoreError> {
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            Ok(PartitionGuard {
                native: self.native.try_lock()?,
            })
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            Err(StoreError::Unsupported)
        }
    }
}

/// Exclusive infrastructure lock. Future record operations must additionally
/// validate the complete partition and reserve quota before mutation.
pub struct PartitionGuard<'a> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    native: native::Guard<'a>,
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    _borrow: std::marker::PhantomData<&'a mut Partition>,
}

impl PartitionGuard<'_> {
    /// Recheck the retained ancestry and synchronization identities. This
    /// detects persistent substitutions; it does not inspect record contents.
    pub fn validate(&self) -> Result<(), StoreError> {
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            self.native.validate()
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            Err(StoreError::Unsupported)
        }
    }
}

#[cfg(all(test, not(any(target_os = "macos", target_os = "linux"))))]
mod tests {
    use super::*;

    #[test]
    fn unsupported_open_precedes_path_access() {
        assert!(matches!(
            Partition::open_existing(Path::new("\0"), Path::new("\0")),
            Err(StoreError::Unsupported)
        ));
    }
}
