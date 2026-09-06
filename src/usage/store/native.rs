//! Descriptor-relative infrastructure access for macOS and Linux.

use std::ffi::OsString;
use std::os::fd::OwnedFd;
use std::path::{Component, Path, PathBuf};

use rustix::fs::{self, AtFlags, FileType, FlockOperation, Mode, OFlags, Stat};

use super::StoreError;

const DIRECTORY: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);
const LOCK: OFlags = OFlags::RDWR
    .union(OFlags::NOFOLLOW)
    .union(OFlags::NONBLOCK)
    .union(OFlags::CLOEXEC);
const LOCK_NAME: &str = ".lock";

fn io(error: rustix::io::Errno) -> StoreError {
    StoreError::Io(error.into())
}

fn same(left: &Stat, right: &Stat) -> bool {
    left.st_dev == right.st_dev && left.st_ino == right.st_ino
}

fn stat_entry(parent: &OwnedFd, name: &std::ffi::OsStr) -> Result<Stat, StoreError> {
    fs::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW).map_err(io)
}

fn directory(stat: &Stat) -> Result<(), StoreError> {
    if FileType::from_raw_mode(stat.st_mode) != FileType::Directory {
        return Err(StoreError::UnsafeEntry);
    }
    Ok(())
}

fn owner_controlled(stat: &Stat) -> Result<(), StoreError> {
    if stat.st_uid != rustix::process::geteuid().as_raw() || stat.st_mode & 0o022 != 0 {
        return Err(StoreError::UnsafeEntry);
    }
    Ok(())
}

fn lock_file(stat: &Stat) -> Result<(), StoreError> {
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile
        || stat.st_nlink != 1
        || stat.st_size != 0
    {
        return Err(StoreError::UnsafeEntry);
    }
    owner_controlled(stat)
}

struct Link {
    name: OsString,
    fd: OwnedFd,
    identity: Stat,
}

/// Each link's name is relative to the preceding retained descriptor.
struct Chain {
    root: OwnedFd,
    links: Vec<Link>,
}

impl Chain {
    fn open(path: &Path) -> Result<Self, StoreError> {
        if !path.is_absolute() {
            return Err(StoreError::Placement);
        }
        let mut chain = Self {
            root: fs::open("/", DIRECTORY, Mode::empty()).map_err(io)?,
            links: Vec::new(),
        };
        for part in path.components() {
            let name = match part {
                Component::RootDir => continue,
                Component::Normal(name) => name,
                _ => return Err(StoreError::Placement),
            };
            let before = stat_entry(chain.last(), name)?;
            directory(&before)?;
            let fd = fs::openat(chain.last(), name, DIRECTORY, Mode::empty()).map_err(io)?;
            let opened = fs::fstat(&fd).map_err(io)?;
            if !same(&before, &opened) {
                return Err(StoreError::IdentityChanged);
            }
            chain.links.push(Link {
                name: name.to_owned(),
                fd,
                identity: opened,
            });
        }
        chain.validate()?;
        Ok(chain)
    }

    fn last(&self) -> &OwnedFd {
        self.links.last().map_or(&self.root, |link| &link.fd)
    }

    fn validate(&self) -> Result<(), StoreError> {
        let mut parent = &self.root;
        for link in &self.links {
            let current = stat_entry(parent, &link.name)?;
            if !same(&current, &link.identity)
                || FileType::from_raw_mode(current.st_mode) != FileType::Directory
            {
                return Err(StoreError::IdentityChanged);
            }
            parent = &link.fd;
        }
        Ok(())
    }
}

pub(super) struct Partition {
    graph: Chain,
    cache: Chain,
    partition: Link,
    lock: Link,
}

impl Partition {
    pub(super) fn open(cache: &Path, graph: &Path) -> Result<(Self, PathBuf, String), StoreError> {
        Self::open_with(cache, graph, || {})
    }

    fn open_with(
        cache: &Path,
        graph: &Path,
        resolved: impl FnOnce(),
    ) -> Result<(Self, PathBuf, String), StoreError> {
        // Resolve ancestors once, but keep the cache entry itself subject to
        // NOFOLLOW. Missing infrastructure is never created by this primitive.
        if !cache.is_absolute() {
            return Err(StoreError::Placement);
        }
        let name = cache.file_name().ok_or(StoreError::Placement)?;
        let parent = cache.parent().ok_or(StoreError::Placement)?;
        let cache_path = parent.canonicalize()?.join(name);
        let graph_path = graph.canonicalize()?;
        if cache_path.starts_with(&graph_path) {
            return Err(StoreError::Placement);
        }
        // Pin endpoint identities at resolution, before reopening any chain.
        // The hook is an internal deterministic test seam; production supplies
        // only the no-op above and has no fault-injection environment variable.
        let graph_identity = fs::stat(&graph_path).map_err(io)?;
        let cache_identity = fs::lstat(&cache_path).map_err(io)?;
        directory(&graph_identity)?;
        directory(&cache_identity)?;
        owner_controlled(&cache_identity)?;
        resolved();
        let graph = Chain::open(&graph_path)?;
        let cache = Chain::open(&cache_path)?;
        if !same(&fs::fstat(graph.last()).map_err(io)?, &graph_identity)
            || !same(&fs::fstat(cache.last()).map_err(io)?, &cache_identity)
        {
            return Err(StoreError::IdentityChanged);
        }
        // Also reject a graph-root alias encountered in the cache ancestry.
        if same(&fs::fstat(&cache.root).map_err(io)?, &graph_identity)
            || cache
                .links
                .iter()
                .any(|link| same(&link.identity, &graph_identity))
        {
            return Err(StoreError::Placement);
        }
        let id = crate::usage::identity::partition_id(graph_path.as_os_str())
            .map_err(|_| StoreError::Unsupported)?;
        let partition = open_directory(cache.last(), id.into())?;
        owner_controlled(&partition.identity)?;
        let lock = open_lock(&partition.fd)?;
        let result = Self {
            graph,
            cache,
            partition,
            lock,
        };
        result.validate()?;
        let id = result.partition.name.to_string_lossy().into_owned();
        Ok((result, graph_path, id))
    }

    fn validate(&self) -> Result<(), StoreError> {
        self.graph.validate()?;
        self.cache.validate()?;
        owner_controlled(&fs::fstat(self.cache.last()).map_err(io)?)?;
        let partition = stat_entry(self.cache.last(), &self.partition.name)?;
        if !same(&partition, &self.partition.identity) {
            return Err(StoreError::IdentityChanged);
        }
        directory(&partition)?;
        owner_controlled(&partition)?;
        let lock = stat_entry(&self.partition.fd, &self.lock.name)?;
        if !same(&lock, &self.lock.identity) {
            return Err(StoreError::IdentityChanged);
        }
        lock_file(&lock)
    }

    pub(super) fn try_lock(&mut self) -> Result<Guard<'_>, StoreError> {
        self.try_lock_with(|_| {})
    }

    fn try_lock_with(&mut self, acquired: impl FnOnce(&OwnedFd)) -> Result<Guard<'_>, StoreError> {
        self.validate()?;
        // Reopening gives flock an independent open-file description. Reusing
        // the retained descriptor could turn overlapping acquisitions into one.
        let lock = open_lock(&self.partition.fd)?;
        if !same(&lock.identity, &self.lock.identity) {
            return Err(StoreError::IdentityChanged);
        }
        match fs::flock(&lock.fd, FlockOperation::NonBlockingLockExclusive) {
            Ok(()) => {}
            Err(rustix::io::Errno::WOULDBLOCK) => return Err(StoreError::Busy),
            Err(error) => return Err(io(error)),
        }
        let guard = Guard {
            partition: self,
            _lock: lock.fd,
        };
        acquired(&guard._lock);
        guard.validate()?;
        Ok(guard)
    }
}

fn open_directory(parent: &OwnedFd, name: OsString) -> Result<Link, StoreError> {
    open_directory_with(parent, name, || {})
}

fn open_directory_with(
    parent: &OwnedFd,
    name: OsString,
    checked: impl FnOnce(),
) -> Result<Link, StoreError> {
    let before = stat_entry(parent, &name)?;
    directory(&before)?;
    checked();
    let fd = fs::openat(parent, &name, DIRECTORY, Mode::empty()).map_err(io)?;
    let identity = fs::fstat(&fd).map_err(io)?;
    if !same(&before, &identity) {
        return Err(StoreError::IdentityChanged);
    }
    Ok(Link { name, fd, identity })
}

fn open_lock(parent: &OwnedFd) -> Result<Link, StoreError> {
    open_lock_with(parent, || {})
}

fn open_lock_with(parent: &OwnedFd, checked: impl FnOnce()) -> Result<Link, StoreError> {
    let name = OsString::from(LOCK_NAME);
    let before = stat_entry(parent, &name)?;
    lock_file(&before)?;
    checked();
    let fd = fs::openat(parent, &name, LOCK, Mode::empty()).map_err(io)?;
    let identity = fs::fstat(&fd).map_err(io)?;
    lock_file(&identity)?;
    if !same(&before, &identity) {
        return Err(StoreError::IdentityChanged);
    }
    Ok(Link { name, fd, identity })
}

pub(super) struct Guard<'a> {
    partition: &'a mut Partition,
    // Explicit unlock matters if a concurrent fork inherited this open-file
    // description before exec closes its CLOEXEC copy.
    _lock: OwnedFd,
}

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        // Drop cannot report an unlock failure; closing remains the fallback.
        // Construct the guard before any fallible post-acquisition work.
        let _ = fs::flock(&self._lock, FlockOperation::Unlock);
    }
}

impl Guard<'_> {
    pub(super) fn validate(&self) -> Result<(), StoreError> {
        self.partition.validate()
    }
}

#[cfg(test)]
mod tests;

mod bootstrap;
