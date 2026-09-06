use super::*;
use std::fs as stdfs;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};

struct Fixture {
    _temp: tempfile::TempDir,
    graph: PathBuf,
    cache: PathBuf,
    partition: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let graph = root.join("graph");
        stdfs::create_dir(&graph).unwrap();
        let cache = root.join("missing").join("cache");
        let id = crate::usage::identity::partition_id(graph.as_os_str()).unwrap();
        let partition = cache.join(id);
        Self {
            _temp: temp,
            graph,
            cache,
            partition,
        }
    }

    fn bootstrap(&self) -> Result<Partition, StoreError> {
        Partition::bootstrap(&self.cache, &self.graph).map(|x| x.0)
    }

    fn lock(&self) -> PathBuf {
        self.partition.join(LOCK_NAME)
    }
}

#[test]
fn creates_private_suffix_and_one_stable_empty_lock() {
    let f = Fixture::new();
    let mut opened = crate::usage::store::Partition::open_or_create(&f.cache, &f.graph).unwrap();
    assert_eq!(opened.canonical_graph_root(), f.graph);
    opened.try_lock().unwrap().validate().unwrap();
    for path in [f.cache.parent().unwrap(), &f.cache, &f.partition] {
        assert_eq!(stdfs::metadata(path).unwrap().mode() & 0o777, 0o700);
    }
    let lock = stdfs::metadata(f.lock()).unwrap();
    assert_eq!(lock.mode() & 0o777, 0o600);
    assert_eq!(lock.len(), 0);
    assert_eq!(stdfs::read_dir(&f.partition).unwrap().count(), 1);
    f.bootstrap().unwrap();
    let reopened = stdfs::metadata(f.lock()).unwrap();
    assert_eq!((lock.dev(), lock.ino()), (reopened.dev(), reopened.ino()));
}

#[test]
fn incomplete_creator_is_not_repaired_by_contender() {
    let f = Fixture::new();
    let mut observed = false;
    let (mut winner, _, _) = Partition::bootstrap_with(&f.cache, &f.graph, |path| {
        if path == f.partition {
            observed = true;
            assert!(f.bootstrap().is_err());
            assert!(!f.lock().exists());
        }
        Ok(())
    })
    .unwrap();
    assert!(observed);
    let mut loser = f.bootstrap().unwrap();
    let guard = winner.try_lock().unwrap();
    assert!(matches!(loser.try_lock(), Err(StoreError::Busy)));
    drop(guard);
    loser.try_lock().unwrap().validate().unwrap();
}

#[test]
fn failed_creator_leaves_unavailable_partition_without_rollback() {
    let f = Fixture::new();
    assert!(
        Partition::bootstrap_with(&f.cache, &f.graph, |path| {
            if path == f.partition {
                return Err(StoreError::Busy);
            }
            Ok(())
        })
        .is_err()
    );
    assert!(f.partition.is_dir());
    assert!(!f.lock().exists());
    for _ in 0..2 {
        assert!(f.bootstrap().is_err());
    }
    assert_eq!(stdfs::read_dir(&f.partition).unwrap().count(), 0);
}

#[test]
fn removed_established_lock_is_never_recreated_while_guard_survives() {
    let f = Fixture::new();
    let mut original = f.bootstrap().unwrap();
    let _guard = original.try_lock().unwrap();
    stdfs::remove_file(f.lock()).unwrap();
    assert!(f.bootstrap().is_err());
    assert!(!f.lock().exists());
}

#[test]
fn rejects_placement_and_parent_traversal_before_any_creation() {
    let f = Fixture::new();
    for path in [
        f.graph.join("a/b"),
        f.graph.clone(),
        PathBuf::from("relative"),
        f.cache.join("../other"),
    ] {
        assert!(matches!(
            Partition::bootstrap(&path, &f.graph),
            Err(StoreError::Placement)
        ));
    }
    assert!(!f.cache.parent().unwrap().exists());
    assert_eq!(stdfs::read_dir(&f.graph).unwrap().count(), 0);
}

#[test]
fn existing_ancestor_alias_resolves_but_cache_symlink_does_not() {
    let f = Fixture::new();
    let alias = f.graph.with_file_name("alias");
    symlink(&f.graph, &alias).unwrap();
    assert!(matches!(
        Partition::bootstrap(&alias.join("a/b"), &f.graph),
        Err(StoreError::Placement)
    ));
    assert_eq!(stdfs::read_dir(&f.graph).unwrap().count(), 0);
    stdfs::remove_file(&alias).unwrap();
    symlink(f.graph.parent().unwrap(), &alias).unwrap();
    Partition::bootstrap(&alias.join("other/cache"), &f.graph).unwrap();
    stdfs::create_dir_all(f.cache.parent().unwrap()).unwrap();
    symlink(f.graph.parent().unwrap().join("other/cache"), &f.cache).unwrap();
    assert!(f.bootstrap().is_err());
}

#[test]
fn suffix_race_opens_safe_winner_and_rejects_unsafe_winners() {
    for kind in ["safe", "symlink", "writable", "file"] {
        let f = Fixture::new();
        let result = Partition::bootstrap_with(&f.cache, &f.graph, |path| {
            if path == f.cache {
                match kind {
                    "safe" => stdfs::create_dir(path).unwrap(),
                    "symlink" => symlink(&f.graph, path).unwrap(),
                    "writable" => {
                        stdfs::create_dir(path).unwrap();
                        stdfs::set_permissions(path, stdfs::Permissions::from_mode(0o777)).unwrap();
                    }
                    "file" => stdfs::write(path, b"keep").unwrap(),
                    _ => unreachable!(),
                }
            }
            Ok(())
        });
        assert_eq!(result.is_ok(), kind == "safe");
        assert_eq!(stdfs::read_dir(&f.graph).unwrap().count(), 0);
        if kind == "file" {
            assert_eq!(stdfs::read(&f.cache).unwrap(), b"keep");
        }
    }
}

#[test]
fn changed_suffix_identity_or_permissions_stops_next_creation() {
    for change_identity in [false, true] {
        let f = Fixture::new();
        let parent = f.cache.parent().unwrap();
        assert!(
            Partition::bootstrap_with(&f.cache, &f.graph, |path| {
                if path == f.cache {
                    if change_identity {
                        stdfs::rename(parent, parent.with_extension("old")).unwrap();
                        symlink(&f.graph, parent).unwrap();
                    } else {
                        stdfs::set_permissions(parent, stdfs::Permissions::from_mode(0o777))
                            .unwrap();
                    }
                }
                Ok(())
            })
            .is_err()
        );
        assert_eq!(stdfs::read_dir(&f.graph).unwrap().count(), 0);
        assert!(!f.cache.exists());
    }
}

#[test]
fn partition_substitution_before_lock_creation_does_not_touch_replacement() {
    let f = Fixture::new();
    assert!(matches!(
        Partition::bootstrap_with(&f.cache, &f.graph, |path| {
            if path == f.partition {
                stdfs::rename(path, path.with_extension("old")).unwrap();
                stdfs::create_dir(path).unwrap();
            }
            Ok(())
        }),
        Err(StoreError::IdentityChanged)
    ));
    assert_eq!(stdfs::read_dir(&f.partition).unwrap().count(), 0);
    assert_eq!(
        stdfs::read_dir(f.partition.with_extension("old"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn lock_collision_during_creation_is_exclusive_and_preserves_bytes() {
    for bytes in [b"".as_slice(), b"keep".as_slice()] {
        let f = Fixture::new();
        assert!(
            Partition::bootstrap_with(&f.cache, &f.graph, |path| {
                if path == f.partition {
                    stdfs::write(f.lock(), bytes).unwrap();
                }
                Ok(())
            })
            .is_err()
        );
        assert_eq!(stdfs::read(f.lock()).unwrap(), bytes);
    }
}

#[test]
fn graph_replacement_before_creation_leaves_cache_absent() {
    let f = Fixture::new();
    assert!(
        Partition::bootstrap_with(&f.cache, &f.graph, |_| {
            stdfs::rename(&f.graph, f.graph.with_extension("old")).unwrap();
            stdfs::create_dir(&f.graph).unwrap();
            Ok(())
        })
        .is_err()
    );
    assert!(!f.cache.parent().unwrap().exists());
    assert_eq!(stdfs::read_dir(&f.graph).unwrap().count(), 0);
}
