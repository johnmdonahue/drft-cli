use super::*;
use std::fs as stdfs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::process::{Command, Stdio};

struct Fixture {
    _temp: tempfile::TempDir,
    graph: PathBuf,
    cache: PathBuf,
    partition: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        let graph = base.join("graph");
        let cache = base.join("cache");
        stdfs::create_dir(&graph).unwrap();
        stdfs::create_dir(&cache).unwrap();
        let id = crate::usage::identity::partition_id(graph.as_os_str()).unwrap();
        let partition = cache.join(id);
        stdfs::create_dir(&partition).unwrap();
        stdfs::write(partition.join(LOCK_NAME), []).unwrap();
        Self {
            _temp: temp,
            graph,
            cache,
            partition,
        }
    }

    fn open(&self) -> Partition {
        Partition::open(&self.cache, &self.graph).unwrap().0
    }

    fn lock(&self) -> PathBuf {
        self.partition.join(LOCK_NAME)
    }
}

#[test]
fn existing_open_preserves_infrastructure_and_exact_root() {
    let fixture = Fixture::new();
    let before = stdfs::metadata(fixture.lock()).unwrap();
    let mut partition =
        super::super::Partition::open_existing(&fixture.cache, &fixture.graph).unwrap();
    assert_eq!(partition.canonical_graph_root(), fixture.graph);
    assert_eq!(
        partition.partition_id(),
        fixture.partition.file_name().unwrap()
    );
    partition.try_lock().unwrap().validate().unwrap();
    let after = stdfs::metadata(fixture.lock()).unwrap();
    assert_eq!(
        (before.dev(), before.ino(), before.len()),
        (after.dev(), after.ino(), after.len())
    );
    assert_eq!(stdfs::read_dir(&fixture.partition).unwrap().count(), 1);
}

#[test]
fn missing_infrastructure_never_creates_entries() {
    let fixture = Fixture::new();
    let missing = fixture.cache.join("missing");
    assert!(Partition::open(&missing, &fixture.graph).is_err());
    assert!(!missing.exists());
    stdfs::remove_file(fixture.lock()).unwrap();
    assert!(Partition::open(&fixture.cache, &fixture.graph).is_err());
    assert!(!fixture.lock().exists());
    stdfs::remove_dir(&fixture.partition).unwrap();
    assert!(Partition::open(&fixture.cache, &fixture.graph).is_err());
    assert!(!fixture.partition.exists());
}

#[test]
fn rejects_relative_equal_descendant_and_missing_descendant_placement() {
    let fixture = Fixture::new();
    for path in [
        PathBuf::from("relative"),
        fixture.graph.clone(),
        fixture.graph.join("missing"),
    ] {
        assert!(matches!(
            Partition::open(&path, &fixture.graph),
            Err(StoreError::Placement)
        ));
    }
    assert_eq!(stdfs::read_dir(&fixture.graph).unwrap().count(), 0);
    let descendant = fixture.graph.join("cache");
    stdfs::create_dir(&descendant).unwrap();
    assert!(matches!(
        Partition::open(&descendant, &fixture.graph),
        Err(StoreError::Placement)
    ));
}

#[test]
fn resolves_existing_ancestor_alias_but_refuses_collector_symlinks() {
    let fixture = Fixture::new();
    let alias = fixture.cache.with_file_name("alias");
    symlink(fixture.cache.parent().unwrap(), &alias).unwrap();
    Partition::open(&alias.join("cache"), &fixture.graph).unwrap();
    stdfs::remove_file(&alias).unwrap();
    symlink(&fixture.graph, &alias).unwrap();
    assert!(matches!(
        Partition::open(&alias.join("cache"), &fixture.graph),
        Err(StoreError::Placement)
    ));
    stdfs::remove_file(&alias).unwrap();
    symlink(&fixture.cache, &alias).unwrap();
    assert!(matches!(
        Partition::open(&alias, &fixture.graph),
        Err(StoreError::UnsafeEntry)
    ));
}

#[test]
fn resolved_endpoint_substitution_is_rejected() {
    for replace_graph in [false, true] {
        let fixture = Fixture::new();
        let selected = if replace_graph {
            &fixture.graph
        } else {
            &fixture.cache
        };
        let result = Partition::open_with(&fixture.cache, &fixture.graph, || {
            stdfs::rename(selected, selected.with_extension("old")).unwrap();
            stdfs::create_dir(selected).unwrap();
        });
        assert!(matches!(result, Err(StoreError::IdentityChanged)));
        assert_eq!(stdfs::read_dir(selected).unwrap().count(), 0);
    }
}

#[test]
fn symlink_substitution_between_resolution_and_reopen_is_rejected() {
    let fixture = Fixture::new();
    let old = fixture.cache.with_extension("old");
    let result = Partition::open_with(&fixture.cache, &fixture.graph, || {
        stdfs::rename(&fixture.cache, &old).unwrap();
        symlink(&old, &fixture.cache).unwrap();
    });
    assert!(result.is_err());
    assert_eq!(stdfs::read_dir(&old).unwrap().count(), 1);
}

#[test]
fn unsafe_partition_and_lock_types_are_rejected_without_mutation() {
    for kind in ["symlink", "directory", "fifo", "hardlink", "nonempty"] {
        let fixture = Fixture::new();
        let target = fixture.cache.with_file_name("target");
        stdfs::write(&target, []).unwrap();
        stdfs::remove_file(fixture.lock()).unwrap();
        match kind {
            "symlink" => symlink(&target, fixture.lock()).unwrap(),
            "directory" => stdfs::create_dir(fixture.lock()).unwrap(),
            "fifo" => assert!(
                Command::new("mkfifo")
                    .arg(fixture.lock())
                    .status()
                    .unwrap()
                    .success()
            ),
            "hardlink" => stdfs::hard_link(&target, fixture.lock()).unwrap(),
            "nonempty" => stdfs::write(fixture.lock(), b"preserve").unwrap(),
            _ => unreachable!(),
        }
        assert!(
            matches!(
                Partition::open(&fixture.cache, &fixture.graph),
                Err(StoreError::UnsafeEntry)
            ),
            "{kind}"
        );
        assert_eq!(stdfs::read(&target).unwrap(), b"");
        if kind == "nonempty" {
            assert_eq!(stdfs::read(fixture.lock()).unwrap(), b"preserve");
        }
    }
    for symlinked in [false, true] {
        let fixture = Fixture::new();
        let old = fixture.partition.with_extension("old");
        stdfs::rename(&fixture.partition, &old).unwrap();
        if symlinked {
            symlink(&old, &fixture.partition).unwrap();
        } else {
            stdfs::write(&fixture.partition, b"preserve").unwrap();
        }
        assert!(matches!(
            Partition::open(&fixture.cache, &fixture.graph),
            Err(StoreError::UnsafeEntry)
        ));
        assert_eq!(stdfs::read(old.join(LOCK_NAME)).unwrap(), b"");
    }
}

#[test]
fn writable_infrastructure_is_rejected_at_open_and_acquisition() {
    for select in ["cache", "partition", "lock"] {
        let fixture = Fixture::new();
        let mut opened = fixture.open();
        let path = match select {
            "cache" => fixture.cache.clone(),
            "partition" => fixture.partition.clone(),
            _ => fixture.lock(),
        };
        let metadata = stdfs::metadata(&path).unwrap();
        stdfs::set_permissions(
            &path,
            stdfs::Permissions::from_mode(metadata.mode() | 0o020),
        )
        .unwrap();
        assert!(matches!(
            Partition::open(&fixture.cache, &fixture.graph),
            Err(StoreError::UnsafeEntry)
        ));
        assert!(matches!(opened.try_lock(), Err(StoreError::UnsafeEntry)));
    }
}

#[test]
fn persistent_ancestry_partition_and_lock_substitutions_invalidate_handles() {
    for select in ["graph", "cache", "partition", "lock"] {
        let fixture = Fixture::new();
        let mut opened = fixture.open();
        let path = match select {
            "graph" => fixture.graph.clone(),
            "cache" => fixture.cache.clone(),
            "partition" => fixture.partition.clone(),
            _ => fixture.lock(),
        };
        stdfs::rename(&path, path.with_extension("old")).unwrap();
        if select == "lock" {
            stdfs::write(&path, []).unwrap();
        } else {
            stdfs::create_dir(&path).unwrap();
        }
        assert!(
            matches!(opened.try_lock(), Err(StoreError::IdentityChanged)),
            "{select}"
        );
    }
}

#[test]
fn replacement_after_flock_is_rejected_and_old_lock_is_released() {
    let fixture = Fixture::new();
    let mut opened = fixture.open();
    let old = fixture.lock().with_extension("old");
    let result = opened.try_lock_with(|| {
        stdfs::rename(fixture.lock(), &old).unwrap();
        stdfs::write(fixture.lock(), []).unwrap();
    });
    assert!(matches!(result, Err(StoreError::IdentityChanged)));
    let old_fd = fs::open(&old, LOCK, Mode::empty()).unwrap();
    fs::flock(&old_fd, FlockOperation::NonBlockingLockExclusive).unwrap();
    assert_eq!(stdfs::read(fixture.lock()).unwrap(), b"");
}

#[test]
fn guard_rechecks_substitution_and_lock_content_changes() {
    for replace in [false, true] {
        let fixture = Fixture::new();
        let mut opened = fixture.open();
        let guard = opened.try_lock().unwrap();
        if replace {
            stdfs::rename(fixture.lock(), fixture.lock().with_extension("old")).unwrap();
        }
        stdfs::write(fixture.lock(), b"changed").unwrap();
        assert!(guard.validate().is_err());
        assert_eq!(stdfs::read(fixture.lock()).unwrap(), b"changed");
    }
}

#[test]
fn independently_opened_handles_contend_and_drop_releases() {
    let fixture = Fixture::new();
    let mut first = fixture.open();
    let mut second = fixture.open();
    let guard = first.try_lock().unwrap();
    assert!(matches!(second.try_lock(), Err(StoreError::Busy)));
    drop(guard);
    second.try_lock().unwrap().validate().unwrap();
    first.try_lock().unwrap().validate().unwrap();
}

#[test]
fn root_identity_preserves_invalid_bytes_and_literal_backslashes() {
    let fixture = Fixture::new();
    let names = [
        b"root\\name".as_slice(),
        // macOS filesystems can reject invalid UTF-8 before the collector runs.
        // The identity codec has platform-independent raw-byte fixtures.
        #[cfg(target_os = "linux")]
        b"root\xff".as_slice(),
    ];
    for bytes in names {
        let root = fixture
            .graph
            .with_file_name(std::ffi::OsStr::from_bytes(bytes));
        stdfs::create_dir(&root).unwrap();
        let id = crate::usage::identity::partition_id(root.as_os_str()).unwrap();
        let partition = fixture.cache.join(&id);
        stdfs::create_dir(&partition).unwrap();
        stdfs::write(partition.join(LOCK_NAME), []).unwrap();
        let opened = super::super::Partition::open_existing(&fixture.cache, &root).unwrap();
        assert_eq!(
            opened.canonical_graph_root().as_os_str().as_bytes(),
            root.as_os_str().as_bytes()
        );
        assert_eq!(opened.partition_id(), id);
    }
}

// The child handshake makes process locking/crash release deterministic. The
// only environment variable belongs to this test binary, never the collector.
#[test]
fn lock_child() {
    use std::io::Write;
    let Some(base) = std::env::var_os("DRFT_TEST_LOCK_CHILD") else {
        return;
    };
    let base = PathBuf::from(base);
    let mut opened = Partition::open(&base.join("cache"), &base.join("graph"))
        .unwrap()
        .0;
    let _guard = opened.try_lock().unwrap();
    println!("LOCKED");
    std::io::stdout().flush().unwrap();
    std::thread::park();
}

#[test]
fn processes_contend_and_process_death_releases_lock() {
    use std::io::BufRead;
    let fixture = Fixture::new();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "usage::store::native::tests::lock_child",
            "--nocapture",
        ])
        .env("DRFT_TEST_LOCK_CHILD", fixture.cache.parent().unwrap())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut reader = std::io::BufReader::new(child.stdout.take().unwrap());
    loop {
        let mut line = String::new();
        assert_ne!(
            reader.read_line(&mut line).unwrap(),
            0,
            "child exited before lock handshake"
        );
        if line.trim() == "LOCKED" {
            break;
        }
    }
    let mut opened = fixture.open();
    let contention = matches!(opened.try_lock(), Err(StoreError::Busy));
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(contention);
    opened.try_lock().unwrap().validate().unwrap();
}
