//! Record inventory and retention planning. Plans never mutate files.

use std::collections::BTreeMap;
use std::ffi::OsStr;

use super::{
    Inventory, InventoryEntry, InventoryRead, PARTITION_BYTE_LIMIT, PARTITION_FILE_LIMIT,
    StoreError,
};
use crate::usage::bounded::EVENT_LIMIT;
use crate::usage::record::{EventKind, RecordState, WallTime, classify};

const RETENTION_SECONDS: i128 = 7 * 24 * 60 * 60;

fn valid_id(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[derive(Debug, PartialEq, Eq)]
enum Name<'a> {
    Lock,
    Temporary,
    Final { id: &'a str, kind: EventKind },
}

fn name(value: &OsStr) -> Result<Name<'_>, StoreError> {
    let text = value.to_str().ok_or(StoreError::UnknownName)?;
    if text == ".lock" {
        return Ok(Name::Lock);
    }
    if text.strip_prefix(".tmp.").is_some_and(valid_id) {
        return Ok(Name::Temporary);
    }
    for (suffix, kind) in [
        (".start.json", EventKind::Start),
        (".finish.json", EventKind::Finish),
    ] {
        if let Some(id) = text.strip_suffix(suffix).filter(|id| valid_id(id)) {
            return Ok(Name::Final { id, kind });
        }
    }
    Err(StoreError::UnknownName)
}

/// Pairing uses exact filenames, including rejected envelopes. It never proves
/// invocation provenance or supports comparing findings by itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pairing {
    Paired,
    Incomplete,
    Orphan,
}

/// Only a supported pair with known, agreeing, nondecreasing endpoints has
/// known time coverage. Unknown coverage must be treated conservatively by export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coverage {
    Known { first: WallTime, last: WallTime },
    Unknown,
}

#[derive(Debug)]
pub struct Record {
    index: usize,
    bytes: u64,
    state: RecordState,
}
impl Record {
    pub fn index(&self) -> usize {
        self.index
    }
    pub fn state(&self) -> &RecordState {
        &self.state
    }
}

#[derive(Debug)]
pub struct Group {
    id: String,
    start: Option<Record>,
    finish: Option<Record>,
}
impl Group {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn start(&self) -> Option<&Record> {
        self.start.as_ref()
    }
    pub fn finish(&self) -> Option<&Record> {
        self.finish.as_ref()
    }
    pub fn pairing(&self) -> Pairing {
        match (&self.start, &self.finish) {
            (Some(_), Some(_)) => Pairing::Paired,
            (Some(_), None) => Pairing::Incomplete,
            (None, Some(_)) => Pairing::Orphan,
            (None, None) => unreachable!("groups are created with a record"),
        }
    }
    pub fn coverage(&self) -> Coverage {
        let (Some(start), Some(finish)) = (&self.start, &self.finish) else {
            return Coverage::Unknown;
        };
        match (&start.state, &finish.state) {
            (
                RecordState::Supported {
                    entry: Some(entry),
                    collected: Some(start_time),
                },
                RecordState::Supported {
                    entry: Some(repeated),
                    collected: Some(finish_time),
                },
            ) if entry == repeated && entry <= start_time && start_time <= finish_time => {
                Coverage::Known {
                    first: *entry,
                    last: *finish_time,
                }
            }
            _ => Coverage::Unknown,
        }
    }
    fn records(&self) -> impl Iterator<Item = &Record> {
        self.start.iter().chain(self.finish.iter())
    }
    fn eviction_key(&self) -> (u8, Option<WallTime>, &str) {
        match self.coverage() {
            Coverage::Known { last, .. } => (0, Some(last), &self.id),
            Coverage::Unknown => (1, None, &self.id),
        }
    }
}

struct RecordSet {
    groups: BTreeMap<String, Group>,
    temporary: Vec<(usize, u64)>,
    bytes: u64,
    files: usize,
}

impl RecordSet {
    fn scan(
        entries: &[InventoryEntry],
        mut read: impl FnMut(usize) -> Result<InventoryRead, StoreError>,
    ) -> Result<Self, StoreError> {
        let mut set = Self {
            groups: BTreeMap::new(),
            temporary: Vec::new(),
            bytes: 0,
            files: entries.len(),
        };
        if entries.len() > PARTITION_FILE_LIMIT {
            return Err(StoreError::ScanLimit);
        }
        let mut lock_seen = false;
        for (index, entry) in entries.iter().enumerate() {
            set.bytes = set
                .bytes
                .checked_add(entry.bytes())
                .ok_or(StoreError::ScanLimit)?;
            if set.bytes > PARTITION_BYTE_LIMIT {
                return Err(StoreError::ScanLimit);
            }
            match name(entry.name())? {
                Name::Lock => {
                    if lock_seen || entry.bytes() != 0 {
                        return Err(StoreError::UnsafeEntry);
                    }
                    lock_seen = true;
                }
                Name::Temporary => set.temporary.push((index, entry.bytes())),
                Name::Final { id, kind } => {
                    let state = match read(index)? {
                        InventoryRead::Complete(bytes) => classify(&bytes, id, kind),
                        InventoryRead::Oversized { .. } => RecordState::Oversized,
                    };
                    let group = set.groups.entry(id.to_owned()).or_insert_with(|| Group {
                        id: id.to_owned(),
                        start: None,
                        finish: None,
                    });
                    let slot = match kind {
                        EventKind::Start => &mut group.start,
                        EventKind::Finish => &mut group.finish,
                    };
                    if slot
                        .replace(Record {
                            index,
                            bytes: entry.bytes(),
                            state,
                        })
                        .is_some()
                    {
                        return Err(StoreError::IdentityChanged);
                    }
                }
            }
        }
        if !lock_seen {
            return Err(StoreError::IdentityChanged);
        }
        Ok(set)
    }

    fn plan(
        &self,
        request: Request<'_>,
        now: Option<WallTime>,
        byte_limit: u64,
        file_limit: usize,
    ) -> Result<Planned, StoreError> {
        if now.is_some_and(|time| time.nanoseconds >= 1_000_000_000) {
            return Err(StoreError::InvalidReservation);
        }
        // A colliding start never earns cleanup, even if the existing group is
        // expired, orphaned, or malformed.
        let (pending_bytes, pending_files) = match request {
            Request::ReserveStart { id, bytes } => {
                if !valid_id(id) || bytes == 0 || bytes > EVENT_LIMIT as u64 {
                    return Err(StoreError::InvalidReservation);
                }
                if self.groups.contains_key(id) {
                    return Err(StoreError::Collision);
                }
                (bytes, 1usize)
            }
            Request::ReserveFinish { id, bytes } => {
                if !valid_id(id) || bytes == 0 || bytes > EVENT_LIMIT as u64 {
                    return Err(StoreError::InvalidReservation);
                }
                let group = self.groups.get(id).ok_or(StoreError::MissingStart)?;
                if group.finish.is_some() {
                    return Err(StoreError::Collision);
                }
                if group.start.is_none() {
                    return Err(StoreError::MissingStart);
                }
                (bytes, 1usize)
            }
            _ => (0, 0),
        };
        // These limits also gate externally oversized state: planning cannot
        // launder it into a valid write by deleting evidence.
        if self.bytes > byte_limit || self.files > file_limit {
            return Err(StoreError::ScanLimit);
        }
        let mut plan = Planned {
            remove: Vec::new(),
            groups: Vec::new(),
            retained_bytes: self.bytes,
            retained_files: self.files,
            peak_bytes: 0,
            peak_files: 0,
        };
        for &(index, bytes) in &self.temporary {
            plan.remove(index, bytes);
        }
        let mut groups: Vec<_> = self.groups.values().collect();
        groups.sort_by_key(|group| group.eviction_key());
        let mut remaining = Vec::new();
        for group in groups {
            if matches!(request, Request::ReserveFinish { id, .. } if id == group.id) {
                continue;
            }
            let expired = match (now, group.coverage()) {
                (Some(now), Coverage::Known { last, .. }) => {
                    nanos(now) - nanos(last) >= RETENTION_SECONDS * 1_000_000_000
                }
                _ => false,
            };
            if matches!(request, Request::PruneAll) || expired {
                plan.remove_group(group);
            } else {
                remaining.push(group);
            }
        }
        for group in remaining {
            if fits(&plan, pending_bytes, pending_files, byte_limit, file_limit) {
                break;
            }
            plan.remove_group(group);
        }
        if !fits(&plan, pending_bytes, pending_files, byte_limit, file_limit) {
            return Err(StoreError::Capacity);
        }
        plan.peak_bytes = plan.retained_bytes + pending_bytes;
        plan.peak_files = plan.retained_files + pending_files;
        Ok(plan)
    }
}

fn nanos(time: WallTime) -> i128 {
    i128::from(time.seconds) * 1_000_000_000 + i128::from(time.nanoseconds)
}

fn fits(plan: &Planned, bytes: u64, files: usize, byte_limit: u64, file_limit: usize) -> bool {
    plan.retained_bytes
        .checked_add(bytes)
        .is_some_and(|n| n <= byte_limit)
        && plan
            .retained_files
            .checked_add(files)
            .is_some_and(|n| n <= file_limit)
}

/// Reservation includes one staging file of the final event's full byte length.
/// Rename consumes no additional slot. Finish calculation protects its start;
/// native publication independently requires the opaque successful-start receipt.
#[derive(Debug, Clone, Copy)]
pub enum Request<'a> {
    PruneExpired,
    PruneAll,
    ReserveStart {
        id: &'a str,
        bytes: u64,
    },
    /// Calculation only; publication additionally requires a native receipt.
    ReserveFinish {
        id: &'a str,
        bytes: u64,
    },
}

struct Planned {
    remove: Vec<usize>,
    groups: Vec<Vec<usize>>,
    retained_bytes: u64,
    retained_files: usize,
    peak_bytes: u64,
    peak_files: usize,
}
impl Planned {
    fn remove(&mut self, index: usize, bytes: u64) {
        self.remove.push(index);
        self.retained_bytes -= bytes;
        self.retained_files -= 1;
    }
    fn remove_group(&mut self, group: &Group) {
        self.groups
            .push(group.records().map(|record| record.index).collect());
        for record in group.records() {
            self.remove(record.index, record.bytes);
        }
    }
}

/// Classification metadata borrowing the unchanged physical inventory and lock.
/// Only one bounded event is read at a time; payloads are not retained here.
pub struct RecordInventory<'a> {
    source: &'a Inventory<'a>,
    set: RecordSet,
}
impl Inventory<'_> {
    pub fn records(&self) -> Result<RecordInventory<'_>, StoreError> {
        self.validate()?;
        let set = RecordSet::scan(self.entries(), |index| self.read(index, EVENT_LIMIT))?;
        self.validate()?;
        Ok(RecordInventory { source: self, set })
    }
}

impl RecordInventory<'_> {
    pub fn groups(&self) -> impl Iterator<Item = &Group> {
        self.set.groups.values()
    }
    pub fn temporary_count(&self) -> usize {
        self.set.temporary.len()
    }
    pub fn plan(
        &self,
        request: Request<'_>,
        now: Option<WallTime>,
    ) -> Result<RetentionPlan<'_>, StoreError> {
        self.source.validate()?;
        let planned = self
            .set
            .plan(request, now, PARTITION_BYTE_LIMIT, PARTITION_FILE_LIMIT)?;
        Ok(RetentionPlan {
            inventory: self,
            planned,
        })
    }
}

/// A calculation over a retained read-only inventory, never unlink authority.
/// Native transactions must revalidate identities and track each successful
/// mutation; applying indices to another snapshot is invalid.
pub struct RetentionPlan<'a> {
    inventory: &'a RecordInventory<'a>,
    planned: Planned,
}
impl RetentionPlan<'_> {
    pub(super) fn removal_groups(&self) -> &[Vec<usize>] {
        &self.planned.groups
    }
    pub fn removal_indices(&self) -> &[usize] {
        &self.planned.remove
    }
    pub fn retained_bytes(&self) -> u64 {
        self.planned.retained_bytes
    }
    pub fn retained_files(&self) -> usize {
        self.planned.retained_files
    }
    pub fn peak_bytes(&self) -> u64 {
        self.planned.peak_bytes
    }
    pub fn peak_files(&self) -> usize {
        self.planned.peak_files
    }
    pub fn validate(&self) -> Result<(), StoreError> {
        self.inventory.source.validate()
    }
}

#[cfg(test)]
mod tests;
