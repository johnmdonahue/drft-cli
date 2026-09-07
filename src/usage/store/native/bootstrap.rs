//! Creator-only initialization; existing missing locks are never repaired.

use super::*;

impl Partition {
    pub(in crate::usage::store) fn bootstrap(
        cache: &Path,
        graph: &Path,
    ) -> Result<(Self, PathBuf, String), StoreError> {
        Self::bootstrap_with(cache, graph, |_| Ok(()))
    }

    fn bootstrap_with(
        cache_path: &Path,
        graph_path: &Path,
        mut checkpoint: impl FnMut(&Path) -> Result<(), StoreError>,
    ) -> Result<(Self, PathBuf, String), StoreError> {
        if !cache_path.is_absolute() || cache_path.components().any(|c| c == Component::ParentDir) {
            return Err(StoreError::Placement);
        }
        let cache_name = cache_path.file_name().ok_or(StoreError::Placement)?;
        let mut ancestor = cache_path.parent().ok_or(StoreError::Placement)?;
        let mut suffix = vec![cache_name.to_owned()];
        // Resolve existing ancestors only. A dangling symlink or inaccessible
        // existing entry is an error, never an invitation to create through it.
        let resolved = loop {
            match ancestor.symlink_metadata() {
                Ok(_) => break ancestor.canonicalize()?,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    suffix.push(
                        ancestor
                            .file_name()
                            .ok_or(StoreError::Placement)?
                            .to_owned(),
                    );
                    ancestor = ancestor.parent().ok_or(StoreError::Placement)?;
                }
                Err(e) => return Err(e.into()),
            }
        };
        suffix.reverse();
        let graph_path = graph_path.canonicalize()?;
        let mut destination = resolved.clone();
        for name in &suffix {
            destination.push(name);
        }
        if destination.starts_with(&graph_path) {
            return Err(StoreError::Placement);
        }
        let graph_identity = fs::stat(&graph_path).map_err(io)?;
        let ancestor_identity = fs::stat(&resolved).map_err(io)?;
        let graph = Chain::open(&graph_path)?;
        let mut cache = Chain::open(&resolved)?;
        if !same(&graph_identity, &fs::fstat(graph.last()).map_err(io)?)
            || !same(&ancestor_identity, &fs::fstat(cache.last()).map_err(io)?)
        {
            return Err(StoreError::IdentityChanged);
        }
        if same(&fs::fstat(&cache.root).map_err(io)?, &graph_identity)
            || cache
                .links
                .iter()
                .any(|link| same(&link.identity, &graph_identity))
        {
            return Err(StoreError::Placement);
        }
        let mut current_path = resolved;
        let private_start = cache.links.len();
        for name in suffix {
            graph.validate()?;
            cache.validate()?;
            validate_private_suffix(&cache, private_start)?;
            current_path.push(&name);
            checkpoint(&current_path)?;
            graph.validate()?;
            cache.validate()?;
            validate_private_suffix(&cache, private_start)?;
            match fs::mkdirat(cache.last(), &name, Mode::from_raw_mode(0o700)) {
                Ok(()) | Err(rustix::io::Errno::EXIST) => {}
                Err(e) => return Err(io(e)),
            }
            let link = open_directory(cache.last(), name)?;
            owner_controlled(&link.identity)?;
            if same(&link.identity, &graph_identity) {
                return Err(StoreError::Placement);
            }
            cache.links.push(link);
        }
        let id = crate::usage::identity::partition_id(graph_path.as_os_str())
            .map_err(|_| StoreError::Unsupported)?;
        graph.validate()?;
        cache.validate()?;
        validate_private_suffix(&cache, private_start)?;
        owner_controlled(&fs::fstat(cache.last()).map_err(io)?)?;
        let created = match fs::mkdirat(cache.last(), id.as_str(), Mode::from_raw_mode(0o700)) {
            Ok(()) => true,
            Err(rustix::io::Errno::EXIST) => false,
            Err(e) => return Err(io(e)),
        };
        let partition = open_directory(cache.last(), id.clone().into())?;
        owner_controlled(&partition.identity)?;
        let lock = if created {
            checkpoint(&current_path.join(&id))?;
            graph.validate()?;
            cache.validate()?;
            validate_private_suffix(&cache, private_start)?;
            owner_controlled(&fs::fstat(cache.last()).map_err(io)?)?;
            let now = stat_entry(cache.last(), &partition.name)?;
            if !same(&now, &partition.identity) {
                return Err(StoreError::IdentityChanged);
            }
            directory(&now)?;
            owner_controlled(&now)?;
            // Only successful mkdir grants this authority. In particular,
            // EEXIST plus absent .lock must never reach this creation.
            let fd = fs::openat(
                &partition.fd,
                LOCK_NAME,
                LOCK | OFlags::CREATE | OFlags::EXCL,
                Mode::from_raw_mode(0o600),
            )
            .map_err(io)?;
            let identity = fs::fstat(&fd).map_err(io)?;
            lock_file(&identity)?;
            checkpoint(&current_path.join(&id).join(LOCK_NAME))?;
            // Retain the identity we created. Reopening here could adopt a
            // replacement while another process still holds the original lock.
            Link {
                name: LOCK_NAME.into(),
                fd,
                identity,
            }
        } else {
            open_lock(&partition.fd)?
        };
        let result = Self {
            graph,
            cache,
            partition,
            lock,
        };
        result.validate()?;
        Ok((result, graph_path, id))
    }
}

fn validate_private_suffix(cache: &Chain, start: usize) -> Result<(), StoreError> {
    for link in &cache.links[start..] {
        owner_controlled(&fs::fstat(&link.fd).map_err(io)?)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
