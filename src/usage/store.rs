//! Inactive storage infrastructure with explicit bootstrap.
//!
//! A guard establishes placement and synchronization, not record validity.
//! Bounded physical inventories do not validate event contents or authorize
//! retention/publication. No command calls this module.

use std::path::{Path, PathBuf};

pub mod records;

/// Includes synchronization, temporary, malformed, and final files.
pub const PARTITION_FILE_LIMIT: usize = 10_000;
pub const PARTITION_BYTE_LIMIT: u64 = 100 * 1024 * 1024;

#[cfg(any(target_os = "macos", target_os = "linux"))]
mod native;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("usage storage is unsupported on this platform")]
    Unsupported,
    #[error("usage cache must be an absolute path outside the graph")]
    Placement,
    #[error("usage storage contains an unsafe entry")]
    UnsafeEntry,
    #[error("usage storage identity or observed metadata changed")]
    IdentityChanged,
    #[error("usage storage is busy")]
    Busy,
    #[error("usage partition exceeds bounded scan limits")]
    ScanLimit,
    #[error("usage inventory read index or byte limit is invalid")]
    InvalidRead,
    #[error("usage partition contains an unrecognized filename")]
    UnknownName,
    #[error("usage invocation identity already exists")]
    Collision,
    #[error("usage write reservation is invalid")]
    InvalidReservation,
    #[error("usage write cannot fit within partition quota")]
    Capacity,
    #[error("usage storage I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

/// Cache/partition/lock handles. Initialization is explicit.
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
    /// Create missing cache directories and a new partition with its stable
    /// lock. Existing partitions must already have a valid lock. An interrupted
    /// creator can leave an unavailable partition; this never repairs it.
    /// Missing suffix directories are private, and parent traversal is refused.
    /// Successfully created directories remain after later failures.
    pub fn open_or_create(cache: &Path, graph_root: &Path) -> Result<Self, StoreError> {
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            let (native, root, id) = native::Partition::bootstrap(cache, graph_root)?;
            Ok(Self { native, root, id })
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            let _ = (cache, graph_root);
            Err(StoreError::Unsupported)
        }
    }

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
    /// handle. Guard drop explicitly unlocks, then closes its descriptor.
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
    /// Inventory safe regular files under this lock without parsing records.
    /// Unknown names are included for accounting, not authorized for mutation.
    pub fn inventory(&self) -> Result<Inventory<'_>, StoreError> {
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            Ok(Inventory {
                native: self.native.inventory()?,
            })
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            Err(StoreError::Unsupported)
        }
    }

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

/// Physical file metadata. Names are exact native OS values and grant no path
/// authority. Entries are sorted by native filename bytes on supported systems.
#[derive(Debug)]
pub struct InventoryEntry {
    name: std::ffi::OsString,
    bytes: u64,
}

impl InventoryEntry {
    pub fn name(&self) -> &std::ffi::OsStr {
        &self.name
    }

    pub fn bytes(&self) -> u64 {
        self.bytes
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum InventoryRead {
    Complete(Vec<u8>),
    /// No bytes were read. The entry remains accounted for in the inventory.
    Oversized {
        bytes: u64,
    },
}

/// Bounded metadata inventory borrowing the exclusive guard. Payloads remain
/// external and mutable; this is not a frozen export or a validated event set.
pub struct Inventory<'a> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    native: native::Inventory<'a>,
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    _borrow: std::marker::PhantomData<&'a ()>,
}

impl Inventory<'_> {
    pub fn entries(&self) -> &[InventoryEntry] {
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            self.native.entries()
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            &[]
        }
    }

    pub fn bytes(&self) -> u64 {
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            self.native.bytes()
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            0
        }
    }

    /// Recheck all retained metadata and infrastructure. Does not read payloads.
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

    /// Read one entry with a caller limit at most the fixed event limit.
    /// Refuses observed metadata changes and never follows a supplied path.
    pub fn read(&self, index: usize, limit: usize) -> Result<InventoryRead, StoreError> {
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            self.native.read(index, limit)
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            let _ = (index, limit);
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
            Partition::open_or_create(Path::new("\0"), Path::new("\0")),
            Err(StoreError::Unsupported)
        ));
        assert!(matches!(
            Partition::open_existing(Path::new("\0"), Path::new("\0")),
            Err(StoreError::Unsupported)
        ));
    }
}
