//! Bounded native mutations. A refreshed snapshot is accepted only if every
//! name and surviving identity agrees with the ledger of confirmed own changes.

use std::collections::BTreeMap;
use std::ffi::OsStr;

use super::*;
use crate::usage::bounded::EVENT_LIMIT;
use crate::usage::identity::InvocationId;
use crate::usage::record::{EventKind, RecordState, WallTime, classify};
use crate::usage::store::records::Request;
use crate::usage::store::{
    InventoryRead, Occupancy, Publication, PublishOutcome, Staging, StartReceipt,
};

const CREATE: OFlags = OFlags::RDWR
    .union(OFlags::CREATE)
    .union(OFlags::EXCL)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::NONBLOCK)
    .union(OFlags::CLOEXEC);

#[derive(Debug)]
pub(in crate::usage::store) struct Receipt {
    partition: Stat,
    lock: Stat,
    start: Stat,
    digest: blake3::Hash,
}

// Internal static-dispatch seams, with no environment variable or public hook.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Point {
    BeforeUnlink(usize),
    AfterUnlink(usize),
    BeforeCreate,
    CreateReady,
    Created,
    BeforeWrite(usize),
    BeforeRename,
    RenameReady,
    AfterRename,
    BeforeStageCleanup,
}

trait Operations {
    fn checkpoint(&mut self, _point: Point) -> Result<(), StoreError> {
        Ok(())
    }
    fn write(&mut self, fd: &OwnedFd, bytes: &[u8]) -> Result<usize, rustix::io::Errno> {
        rustix::io::write(fd, bytes)
    }
}
struct Native;
impl Operations for Native {}

struct Ledger {
    entries: BTreeMap<OsString, Stat>,
}

impl Ledger {
    fn from_inventory(inventory: &Inventory<'_>) -> Self {
        Self {
            entries: inventory
                .entries()
                .iter()
                .zip(&inventory.identities)
                .map(|(entry, stat)| (entry.name.clone(), *stat))
                .collect(),
        }
    }

    fn occupancy(&self) -> Occupancy {
        Occupancy {
            bytes: self.entries.values().map(|stat| stat.st_size as u64).sum(),
            files: self.entries.len(),
        }
    }

    fn validate<'a>(&self, guard: &'a Guard<'a>) -> Result<Inventory<'a>, StoreError> {
        let current = guard.inventory()?;
        if current.entries().len() != self.entries.len()
            || current
                .entries()
                .iter()
                .zip(&current.identities)
                .any(|(entry, stat)| {
                    self.entries
                        .get(&entry.name)
                        .is_none_or(|expected| !scan::unchanged(stat, expected))
                })
        {
            return Err(StoreError::IdentityChanged);
        }
        Ok(current)
    }
}

impl Guard<'_> {
    pub(in crate::usage::store) fn publish(
        &self,
        id: &str,
        bytes: &[u8],
        now: Option<WallTime>,
        receipt: Option<&StartReceipt>,
    ) -> PublishOutcome {
        let staging = match InvocationId::generate() {
            Ok(id) => id,
            Err(_) => {
                return PublishOutcome {
                    error: Some(StoreError::RandomUnavailable),
                    ..Default::default()
                };
            }
        };
        self.publish_with(id, bytes, now, receipt, staging.as_str(), &mut Native)
    }

    fn publish_with(
        &self,
        id: &str,
        bytes: &[u8],
        now: Option<WallTime>,
        receipt: Option<&StartReceipt>,
        staging_id: &str,
        operations: &mut impl Operations,
    ) -> PublishOutcome {
        let mut outcome = PublishOutcome::default();
        let mut ledger = None;
        let mut staging_fd = None;
        let staging_name = OsString::from(format!(".tmp.{staging_id}"));
        let result = self.execute(
            id,
            bytes,
            now,
            receipt,
            &staging_name,
            operations,
            &mut outcome,
            &mut ledger,
            &mut staging_fd,
        );
        if let Err(error) = result {
            outcome.error = Some(error);
            if outcome.publication == Publication::NotPublished
                && let Some(fd) = staging_fd.as_ref()
            {
                let cleanup = (|| {
                    operations.checkpoint(Point::BeforeStageCleanup)?;
                    self.validate()?;
                    let opened = fs::fstat(fd).map_err(io)?;
                    scan::regular(&opened)?;
                    let named = stat_entry(&self.partition.partition.fd, &staging_name)?;
                    if !scan::unchanged(&opened, &named) {
                        return Err(StoreError::IdentityChanged);
                    }
                    fs::unlinkat(
                        &self.partition.partition.fd,
                        &staging_name,
                        AtFlags::empty(),
                    )
                    .map_err(io)?;
                    Ok(())
                })();
                if cleanup.is_ok() {
                    outcome.staging = Staging::Removed;
                    if let Some(ledger) = ledger.as_mut() {
                        ledger.entries.remove(&staging_name);
                    }
                }
            }
        }
        // This observation never replaces the primary error. In particular,
        // unexplained membership changes cannot be laundered into accounting.
        if let Some(ledger) = ledger.as_ref() {
            match ledger.validate(self) {
                Ok(_) => outcome.occupancy = Some(ledger.occupancy()),
                Err(error) => {
                    outcome.receipt = None;
                    if outcome.error.is_none() {
                        outcome.error = Some(error);
                    }
                }
            }
        }
        outcome
    }

    #[allow(clippy::too_many_arguments)]
    fn execute(
        &self,
        id: &str,
        bytes: &[u8],
        now: Option<WallTime>,
        receipt: Option<&StartReceipt>,
        staging_name: &OsStr,
        operations: &mut impl Operations,
        outcome: &mut PublishOutcome,
        retained_ledger: &mut Option<Ledger>,
        staging_fd: &mut Option<OwnedFd>,
    ) -> Result<(), StoreError> {
        let kind = if receipt.is_some() {
            EventKind::Finish
        } else {
            EventKind::Start
        };
        if !matches!(classify(bytes, id, kind), RecordState::Supported { .. }) {
            return Err(StoreError::InvalidEvent);
        }
        let physical = crate::usage::store::Inventory {
            native: self.inventory()?,
        };
        let records = physical.records()?;
        if let Some(receipt) = receipt {
            self.verify_receipt(&physical.native, receipt)?;
        }
        let request = if receipt.is_some() {
            Request::ReserveFinish {
                id,
                bytes: bytes.len() as u64,
            }
        } else {
            Request::ReserveStart {
                id,
                bytes: bytes.len() as u64,
            }
        };
        let plan = records.plan(request, now)?;
        plan.validate()?;
        // Clone only bounded native metadata. Indices are resolved exclusively
        // against the exact snapshot used by the planner.
        let removals: Vec<_> = plan
            .removal_indices()
            .iter()
            .map(|&index| {
                let group = plan
                    .removal_groups()
                    .iter()
                    .find(|group| group.contains(&index));
                (
                    physical.entries()[index].name.clone(),
                    group.map(|group| group.as_slice()),
                )
            })
            .collect();
        *retained_ledger = Some(Ledger::from_inventory(&physical.native));
        let ledger = retained_ledger.as_mut().expect("initialized above");
        let mut group_progress: BTreeMap<usize, usize> = BTreeMap::new();
        for (step, (name, group)) in removals.iter().enumerate() {
            operations.checkpoint(Point::BeforeUnlink(step))?;
            ledger.validate(self)?;
            let stat = ledger
                .entries
                .get(name)
                .ok_or(StoreError::IdentityChanged)?;
            // This is intentionally adjacent to unlink; the advisory lock and
            // stable owner-controlled directory remain the concurrency contract.
            let current = stat_entry(&self.partition.partition.fd, name)?;
            if !scan::unchanged(stat, &current) {
                return Err(StoreError::IdentityChanged);
            }
            fs::unlinkat(&self.partition.partition.fd, name, AtFlags::empty()).map_err(io)?;
            outcome.removed_files += 1;
            outcome.removed_bytes += stat.st_size as u64;
            ledger.entries.remove(name);
            if let Some(group) = group {
                let removed = group_progress.entry(group[0]).or_default();
                *removed += 1;
                outcome.partial_group = *removed < group.len();
            }
            operations.checkpoint(Point::AfterUnlink(step))?;
            ledger.validate(self)?;
        }
        operations.checkpoint(Point::BeforeCreate)?;
        ledger.validate(self)?;
        let occupancy = ledger.occupancy();
        if occupancy.files + 1 > crate::usage::store::PARTITION_FILE_LIMIT
            || occupancy.bytes + bytes.len() as u64 > crate::usage::store::PARTITION_BYTE_LIMIT
        {
            return Err(StoreError::Capacity);
        }
        operations.checkpoint(Point::CreateReady)?;
        let fd = fs::openat(
            &self.partition.partition.fd,
            staging_name,
            CREATE,
            Mode::from_raw_mode(0o600),
        )
        .map_err(io)?;
        outcome.staging = Staging::MayRemain;
        *staging_fd = Some(fd);
        let fd = staging_fd.as_ref().expect("created above");
        let initial = fs::fstat(fd).map_err(io)?;
        scan::regular(&initial)?;
        if initial.st_size != 0 {
            return Err(StoreError::IdentityChanged);
        }
        ledger.entries.insert(staging_name.to_owned(), initial);
        operations.checkpoint(Point::Created)?;
        ledger.validate(self)?;
        let mut written = 0;
        while written < bytes.len() {
            operations.checkpoint(Point::BeforeWrite(written))?;
            let count = match operations.write(fd, &bytes[written..]) {
                Ok(0) => return Err(StoreError::Io(std::io::ErrorKind::WriteZero.into())),
                Ok(count) => count,
                Err(rustix::io::Errno::INTR) => continue,
                Err(error) => return Err(io(error)),
            };
            written += count;
        }
        let completed = fs::fstat(fd).map_err(io)?;
        scan::regular(&completed)?;
        if !same_file_attributes(&initial, &completed) || completed.st_size as usize != bytes.len()
        {
            return Err(StoreError::IdentityChanged);
        }
        ledger.entries.insert(staging_name.to_owned(), completed);
        let staged = ledger.validate(self)?;
        verify_bytes(&staged, staging_name, bytes)?;
        operations.checkpoint(Point::BeforeRename)?;
        ledger.validate(self)?;
        // Verify the source immediately before publication. Destination safety
        // is enforced by the native no-replace primitive, never an exists check.
        let named = stat_entry(&self.partition.partition.fd, staging_name)?;
        if !scan::unchanged(&named, &completed) {
            return Err(StoreError::IdentityChanged);
        }
        let final_name = OsString::from(format!(
            "{id}.{}.json",
            if receipt.is_some() { "finish" } else { "start" }
        ));
        operations.checkpoint(Point::RenameReady)?;
        fs::renameat_with(
            &self.partition.partition.fd,
            staging_name,
            &self.partition.partition.fd,
            &final_name,
            fs::RenameFlags::NOREPLACE,
        )
        .map_err(|error| {
            if error == rustix::io::Errno::EXIST {
                StoreError::Collision
            } else {
                io(error)
            }
        })?;
        outcome.publication = Publication::Published;
        outcome.staging = Staging::Published;
        operations.checkpoint(Point::AfterRename)?;
        let published = fs::fstat(fd).map_err(io)?;
        scan::regular(&published)?;
        // Rename can change ctime, but it cannot account for changed content
        // modification time, ownership, permissions, length, or link count.
        if !same_file_attributes(&completed, &published)
            || published.st_size != completed.st_size
            || published.st_mtime != completed.st_mtime
            || published.st_mtime_nsec != completed.st_mtime_nsec
        {
            return Err(StoreError::IdentityChanged);
        }
        ledger.entries.remove(staging_name);
        ledger.entries.insert(final_name.clone(), published);
        let current = ledger.validate(self)?;
        verify_bytes(&current, &final_name, bytes)?;
        if let Some(receipt) = receipt {
            self.verify_start(&current, receipt)?;
        } else {
            outcome.receipt = Some(StartReceipt {
                id: id.to_owned(),
                native: Receipt {
                    partition: self.partition.partition.identity,
                    lock: self.partition.lock.identity,
                    start: published,
                    digest: blake3::hash(bytes),
                },
            });
        }
        Ok(())
    }

    fn verify_receipt(
        &self,
        inventory: &Inventory<'_>,
        receipt: &StartReceipt,
    ) -> Result<(), StoreError> {
        if !same(
            &receipt.native.partition,
            &self.partition.partition.identity,
        ) || !same(&receipt.native.lock, &self.partition.lock.identity)
        {
            return Err(StoreError::ReceiptMismatch);
        }
        let finish = format!("{}.finish.json", receipt.id);
        if inventory
            .entries()
            .iter()
            .any(|entry| entry.name == finish.as_str())
        {
            return Err(StoreError::Collision);
        }
        self.verify_start(inventory, receipt)
    }

    fn verify_start(
        &self,
        inventory: &Inventory<'_>,
        receipt: &StartReceipt,
    ) -> Result<(), StoreError> {
        let name = format!("{}.start.json", receipt.id);
        let index = inventory
            .entries()
            .iter()
            .position(|entry| entry.name == name.as_str())
            .ok_or(StoreError::MissingStart)?;
        if !scan::unchanged(&inventory.identities[index], &receipt.native.start) {
            return Err(StoreError::ReceiptMismatch);
        }
        match inventory.read(index, EVENT_LIMIT)? {
            InventoryRead::Complete(bytes) if blake3::hash(&bytes) == receipt.native.digest => {
                Ok(())
            }
            _ => Err(StoreError::ReceiptMismatch),
        }
    }
}

fn same_file_attributes(a: &Stat, b: &Stat) -> bool {
    same(a, b)
        && a.st_mode == b.st_mode
        && a.st_uid == b.st_uid
        && a.st_gid == b.st_gid
        && a.st_nlink == b.st_nlink
}

fn verify_bytes(
    inventory: &Inventory<'_>,
    name: &OsStr,
    expected: &[u8],
) -> Result<(), StoreError> {
    let index = inventory
        .entries()
        .iter()
        .position(|entry| entry.name == name)
        .ok_or(StoreError::IdentityChanged)?;
    match inventory.read(index, EVENT_LIMIT)? {
        InventoryRead::Complete(bytes) if bytes == expected => Ok(()),
        _ => Err(StoreError::IdentityChanged),
    }
}

#[cfg(test)]
mod tests;
