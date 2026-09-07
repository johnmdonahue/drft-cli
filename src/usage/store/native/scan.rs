//! Read-only physical inventory. Record interpretation is a separate gate.

use std::os::unix::ffi::OsStrExt;

use super::*;
use crate::usage::bounded::EVENT_LIMIT;
use crate::usage::store::{
    InventoryEntry, InventoryRead, PARTITION_BYTE_LIMIT, PARTITION_FILE_LIMIT,
};

const NAME_LIMIT: usize = 255;
const READ: OFlags = OFlags::RDONLY
    .union(OFlags::NOFOLLOW)
    .union(OFlags::NONBLOCK)
    .union(OFlags::CLOEXEC);

pub(super) fn regular(stat: &Stat) -> Result<(), StoreError> {
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile
        || stat.st_nlink != 1
        || stat.st_size < 0
    {
        return Err(StoreError::UnsafeEntry);
    }
    owner_controlled(stat)
}

// atime is excluded because this reader can update it. These are observation
// checks, not protection against arbitrary same-owner in-place rewrites.
pub(super) fn unchanged(a: &Stat, b: &Stat) -> bool {
    same(a, b)
        && a.st_size == b.st_size
        && a.st_mode == b.st_mode
        && a.st_nlink == b.st_nlink
        && a.st_uid == b.st_uid
        && a.st_gid == b.st_gid
        && a.st_mtime == b.st_mtime
        && a.st_mtime_nsec == b.st_mtime_nsec
        && a.st_ctime == b.st_ctime
        && a.st_ctime_nsec == b.st_ctime_nsec
}

impl Guard<'_> {
    pub(in crate::usage::store) fn inventory(&self) -> Result<Inventory<'_>, StoreError> {
        self.inventory_with(PARTITION_FILE_LIMIT, PARTITION_BYTE_LIMIT, || {})
    }

    fn inventory_with(
        &self,
        file_limit: usize,
        byte_limit: u64,
        scanned: impl FnOnce(),
    ) -> Result<Inventory<'_>, StoreError> {
        self.validate()?;
        let fd = &self.partition.partition.fd;
        let directory = fs::fstat(fd).map_err(io)?;
        let mut records = Vec::new();
        let mut bytes = 0u64;
        // read_from opens an independent directory description, so a second
        // scan starts at the beginning instead of sharing an exhausted offset.
        let dir = fs::Dir::read_from(fd).map_err(io)?;
        for entry in dir {
            let entry = entry.map_err(io)?;
            let raw = entry.file_name().to_bytes();
            if raw == b"." || raw == b".." {
                continue;
            }
            if records.len() == file_limit || raw.len() > NAME_LIMIT {
                return Err(StoreError::ScanLimit);
            }
            let name = std::ffi::OsStr::from_bytes(raw);
            let stat = stat_entry(fd, name)?;
            regular(&stat)?;
            bytes = bytes
                .checked_add(stat.st_size as u64)
                .ok_or(StoreError::ScanLimit)?;
            if bytes > byte_limit {
                return Err(StoreError::ScanLimit);
            }
            records.push((
                InventoryEntry {
                    name: name.to_owned(),
                    bytes: stat.st_size as u64,
                },
                stat,
            ));
        }
        records.sort_by(|a, b| a.0.name.as_bytes().cmp(b.0.name.as_bytes()));
        if records
            .windows(2)
            .any(|pair| pair[0].0.name == pair[1].0.name)
        {
            return Err(StoreError::IdentityChanged);
        }
        // Every successful inventory includes the permanently retained lock.
        if !records.iter().any(|(entry, stat)| {
            entry.name == LOCK_NAME && same(stat, &self.partition.lock.identity)
        }) {
            return Err(StoreError::IdentityChanged);
        }
        let (entries, identities) = records.into_iter().unzip();
        let inventory = Inventory {
            guard: self,
            directory,
            entries,
            identities,
            bytes,
        };
        scanned();
        inventory.validate()?;
        Ok(inventory)
    }
}

pub(in crate::usage::store) struct Inventory<'a> {
    guard: &'a Guard<'a>,
    directory: Stat,
    entries: Vec<InventoryEntry>,
    pub(super) identities: Vec<Stat>,
    bytes: u64,
}

impl Inventory<'_> {
    pub(in crate::usage::store) fn entries(&self) -> &[InventoryEntry] {
        &self.entries
    }
    pub(in crate::usage::store) fn bytes(&self) -> u64 {
        self.bytes
    }

    fn validate_directory(&self) -> Result<(), StoreError> {
        self.guard.validate()?;
        let current = fs::fstat(&self.guard.partition.partition.fd).map_err(io)?;
        if !unchanged(&current, &self.directory) {
            return Err(StoreError::IdentityChanged);
        }
        Ok(())
    }

    fn validate_entry(&self, index: usize) -> Result<(), StoreError> {
        let current = stat_entry(
            &self.guard.partition.partition.fd,
            &self.entries[index].name,
        )?;
        regular(&current)?;
        if !unchanged(&current, &self.identities[index]) {
            return Err(StoreError::IdentityChanged);
        }
        Ok(())
    }

    pub(in crate::usage::store) fn validate(&self) -> Result<(), StoreError> {
        self.validate_directory()?;
        for index in 0..self.entries.len() {
            self.validate_entry(index)?;
        }
        self.validate_directory()
    }

    pub(in crate::usage::store) fn read(
        &self,
        index: usize,
        limit: usize,
    ) -> Result<InventoryRead, StoreError> {
        self.read_with(index, limit, || {}, || {})
    }

    fn read_with(
        &self,
        index: usize,
        limit: usize,
        checked: impl FnOnce(),
        opened: impl FnOnce(),
    ) -> Result<InventoryRead, StoreError> {
        let entry = self.entries.get(index).ok_or(StoreError::InvalidRead)?;
        if limit > EVENT_LIMIT {
            return Err(StoreError::InvalidRead);
        }
        self.validate_directory()?;
        self.validate_entry(index)?;
        if entry.bytes > limit as u64 {
            return Ok(InventoryRead::Oversized { bytes: entry.bytes });
        }
        checked();
        let parent = &self.guard.partition.partition.fd;
        let fd = fs::openat(parent, &entry.name, READ, Mode::empty()).map_err(io)?;
        let stat = fs::fstat(&fd).map_err(io)?;
        regular(&stat)?;
        if !unchanged(&stat, &self.identities[index]) {
            return Err(StoreError::IdentityChanged);
        }
        opened();
        // Only the observed size is allocated. One extra byte is probed into
        // the stack buffer to detect growth without retaining an oversized read.
        let mut bytes = Vec::with_capacity(entry.bytes as usize);
        let mut buffer = [0u8; 8192];
        loop {
            let remaining = entry.bytes as usize - bytes.len();
            let take = buffer.len().min(remaining + 1);
            let read = match rustix::io::read(&fd, &mut buffer[..take]) {
                Ok(read) => read,
                Err(rustix::io::Errno::INTR) => continue,
                Err(error) => return Err(io(error)),
            };
            if read == 0 {
                break;
            }
            if read > remaining {
                return Err(StoreError::IdentityChanged);
            }
            bytes.extend_from_slice(&buffer[..read]);
        }
        let after = fs::fstat(&fd).map_err(io)?;
        regular(&after)?;
        if bytes.len() as u64 != entry.bytes || !unchanged(&after, &self.identities[index]) {
            return Err(StoreError::IdentityChanged);
        }
        self.validate_entry(index)?;
        self.validate_directory()?;
        Ok(InventoryRead::Complete(bytes))
    }
}

#[cfg(test)]
mod tests;
