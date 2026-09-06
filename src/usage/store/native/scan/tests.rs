use super::*;
use std::fs as stdfs;
use std::os::unix::fs::{PermissionsExt, symlink};

struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    partition: Partition,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        let graph = base.join("graph");
        stdfs::create_dir(&graph).unwrap();
        let cache = base.join("cache");
        let (partition, _, id) = Partition::bootstrap(&cache, &graph).unwrap();
        Self {
            _temp: temp,
            root: cache.join(id),
            partition,
        }
    }

    fn put(&self, name: &str, bytes: &[u8]) {
        stdfs::write(self.root.join(name), bytes).unwrap();
    }
}

fn index(inventory: &Inventory<'_>, name: &str) -> usize {
    inventory
        .entries()
        .iter()
        .position(|entry| entry.name() == name)
        .unwrap()
}

#[test]
fn inventory_accounts_all_regular_names_in_native_order_without_mutation() {
    let mut f = Fixture::new();
    for (name, bytes) in [
        ("z.finish.json", b"invalid".as_slice()),
        (".tmp-any", b"abc"),
        ("a\nunknown", b"{}"),
        (".health", b"x"),
    ] {
        f.put(name, bytes);
    }
    let guard = f.partition.try_lock().unwrap();
    for _ in 0..2 {
        let inventory = guard.inventory().unwrap();
        assert_eq!(inventory.bytes(), 13);
        assert_eq!(
            inventory
                .entries()
                .iter()
                .map(|e| e.name().to_str().unwrap())
                .collect::<Vec<_>>(),
            [
                ".health",
                ".lock",
                ".tmp-any",
                "a\nunknown",
                "z.finish.json"
            ]
        );
        assert_eq!(
            inventory
                .read(index(&inventory, "z.finish.json"), EVENT_LIMIT)
                .unwrap(),
            InventoryRead::Complete(b"invalid".to_vec())
        );
        assert_eq!(
            inventory.read(index(&inventory, ".lock"), 0).unwrap(),
            InventoryRead::Complete(vec![])
        );
        inventory.validate().unwrap();
    }
    assert_eq!(stdfs::read_dir(&f.root).unwrap().count(), 5);
}

#[test]
fn count_and_byte_bounds_include_lock_and_reject_external_excess_without_cleanup() {
    let mut f = Fixture::new();
    f.put("a", b"12");
    f.put("b", b"345");
    let guard = f.partition.try_lock().unwrap();
    assert_eq!(guard.inventory_with(3, 5, || {}).unwrap().bytes(), 5);
    assert!(matches!(
        guard.inventory_with(2, 5, || {}),
        Err(StoreError::ScanLimit)
    ));
    assert!(matches!(
        guard.inventory_with(3, 4, || {}),
        Err(StoreError::ScanLimit)
    ));
    assert_eq!(stdfs::read(f.root.join("a")).unwrap(), b"12");
    assert_eq!(stdfs::read(f.root.join("b")).unwrap(), b"345");
    assert_eq!(stdfs::read_dir(&f.root).unwrap().count(), 3);
}

#[test]
fn public_scan_limits_apply_to_sparse_file_without_reading_payload() {
    let mut f = Fixture::new();
    stdfs::File::create(f.root.join("huge"))
        .unwrap()
        .set_len(PARTITION_BYTE_LIMIT)
        .unwrap();
    let guard = f.partition.try_lock().unwrap();
    let inventory = guard.inventory().unwrap();
    assert_eq!(inventory.bytes(), PARTITION_BYTE_LIMIT);
    assert_eq!(
        inventory
            .read(index(&inventory, "huge"), EVENT_LIMIT)
            .unwrap(),
        InventoryRead::Oversized {
            bytes: PARTITION_BYTE_LIMIT
        }
    );
    drop(inventory);
    stdfs::OpenOptions::new()
        .write(true)
        .open(f.root.join("huge"))
        .unwrap()
        .set_len(PARTITION_BYTE_LIMIT + 1)
        .unwrap();
    assert!(matches!(guard.inventory(), Err(StoreError::ScanLimit)));
}

#[test]
fn public_file_limit_is_enforced_including_zero_byte_files() {
    let mut f = Fixture::new();
    for n in 1..PARTITION_FILE_LIMIT {
        f.put(&format!("{n}"), b"");
    }
    let guard = f.partition.try_lock().unwrap();
    assert_eq!(
        guard.inventory().unwrap().entries().len(),
        PARTITION_FILE_LIMIT
    );
    stdfs::write(f.root.join("one-too-many"), []).unwrap();
    assert!(matches!(guard.inventory(), Err(StoreError::ScanLimit)));
}

#[test]
fn bounded_read_distinguishes_complete_oversized_and_invalid_request() {
    let mut f = Fixture::new();
    f.put("a", &vec![b'x'; EVENT_LIMIT]);
    let guard = f.partition.try_lock().unwrap();
    let inventory = guard.inventory().unwrap();
    let a = index(&inventory, "a");
    assert_eq!(
        inventory.read(a, EVENT_LIMIT).unwrap(),
        InventoryRead::Complete(vec![b'x'; EVENT_LIMIT])
    );
    assert_eq!(
        inventory.read(a, EVENT_LIMIT - 1).unwrap(),
        InventoryRead::Oversized {
            bytes: EVENT_LIMIT as u64
        }
    );
    assert!(matches!(
        inventory.read(a, EVENT_LIMIT + 1),
        Err(StoreError::InvalidRead)
    ));
    assert!(matches!(
        inventory.read(inventory.entries().len(), 1),
        Err(StoreError::InvalidRead)
    ));
}

#[test]
fn unsafe_entries_reject_inventory_without_touching_earlier_files() {
    for kind in [
        "symlink",
        "hardlink",
        "directory",
        "fifo",
        "socket",
        "writable",
    ] {
        let mut f = Fixture::new();
        f.put("a", b"preserve");
        let bad = f.root.join("z");
        match kind {
            "symlink" => symlink(f.root.join("a"), &bad).unwrap(),
            "hardlink" => stdfs::hard_link(f.root.join("a"), &bad).unwrap(),
            "directory" => stdfs::create_dir(&bad).unwrap(),
            "fifo" => assert!(
                std::process::Command::new("mkfifo")
                    .arg(&bad)
                    .status()
                    .unwrap()
                    .success()
            ),
            "socket" => {
                // sockaddr_un has a shorter limit than filesystem paths.
                let short = tempfile::tempdir_in("/tmp").unwrap();
                let path = short.path().join("socket");
                let _socket = std::os::unix::net::UnixListener::bind(&path).unwrap();
                stdfs::rename(path, &bad).unwrap();
            }
            "writable" => {
                stdfs::write(&bad, []).unwrap();
                stdfs::set_permissions(&bad, stdfs::Permissions::from_mode(0o666)).unwrap();
            }
            _ => unreachable!(),
        }
        let guard = f.partition.try_lock().unwrap();
        assert!(
            matches!(guard.inventory(), Err(StoreError::UnsafeEntry)),
            "{kind}"
        );
        assert_eq!(stdfs::read(f.root.join("a")).unwrap(), b"preserve");
        assert!(bad.symlink_metadata().is_ok());
    }
}

#[test]
fn scan_rechecks_records_and_directory_after_enumeration() {
    for action in ["insert", "remove", "resize", "mode", "replace", "lock"] {
        let mut f = Fixture::new();
        f.put("a", b"original");
        let guard = f.partition.try_lock().unwrap();
        let result =
            guard.inventory_with(
                PARTITION_FILE_LIMIT,
                PARTITION_BYTE_LIMIT,
                || match action {
                    "insert" => stdfs::write(f.root.join("new"), []).unwrap(),
                    "remove" => stdfs::remove_file(f.root.join("a")).unwrap(),
                    "resize" => stdfs::write(f.root.join("a"), b"longer-than-before").unwrap(),
                    "mode" => stdfs::set_permissions(
                        f.root.join("a"),
                        stdfs::Permissions::from_mode(
                            stdfs::metadata(f.root.join("a"))
                                .unwrap()
                                .permissions()
                                .mode()
                                ^ 0o100,
                        ),
                    )
                    .unwrap(),
                    "replace" => {
                        stdfs::rename(f.root.join("a"), f.root.join("old")).unwrap();
                        stdfs::write(f.root.join("a"), b"original").unwrap();
                    }
                    "lock" => {
                        stdfs::rename(f.root.join(".lock"), f.root.join("old")).unwrap();
                        stdfs::write(f.root.join(".lock"), []).unwrap();
                    }
                    _ => unreachable!(),
                },
            );
        assert!(result.is_err(), "{action}");
    }
}

#[test]
fn read_refuses_substitution_between_stat_and_open() {
    for kind in ["regular", "symlink", "fifo"] {
        let mut f = Fixture::new();
        f.put("a", b"data");
        let guard = f.partition.try_lock().unwrap();
        let inventory = guard.inventory().unwrap();
        let result = inventory.read_with(
            index(&inventory, "a"),
            10,
            || {
                stdfs::rename(f.root.join("a"), f.root.join("old")).unwrap();
                match kind {
                    "regular" => stdfs::write(f.root.join("a"), b"data").unwrap(),
                    "symlink" => symlink(f.root.join("old"), f.root.join("a")).unwrap(),
                    "fifo" => assert!(
                        std::process::Command::new("mkfifo")
                            .arg(f.root.join("a"))
                            .status()
                            .unwrap()
                            .success()
                    ),
                    _ => unreachable!(),
                }
            },
            || {},
        );
        assert!(result.is_err(), "{kind}");
        assert_eq!(stdfs::read(f.root.join("old")).unwrap(), b"data");
    }
}

#[test]
fn read_refuses_growth_shrink_rewrite_and_replacement_after_open() {
    for action in ["grow", "shrink", "rewrite", "replace", "insert", "hardlink"] {
        let mut f = Fixture::new();
        f.put("a", b"data");
        let guard = f.partition.try_lock().unwrap();
        let inventory = guard.inventory().unwrap();
        let result = inventory.read_with(
            index(&inventory, "a"),
            EVENT_LIMIT,
            || {},
            || match action {
                "grow" => stdfs::write(f.root.join("a"), vec![b'x'; EVENT_LIMIT + 1]).unwrap(),
                "shrink" => stdfs::write(f.root.join("a"), b"x").unwrap(),
                "rewrite" => stdfs::write(f.root.join("a"), b"xxxx").unwrap(),
                "replace" => {
                    stdfs::rename(f.root.join("a"), f.root.join("old")).unwrap();
                    stdfs::write(f.root.join("a"), b"data").unwrap();
                }
                "insert" => stdfs::write(f.root.join("new"), []).unwrap(),
                "hardlink" => stdfs::hard_link(f.root.join("a"), f.root.join("new")).unwrap(),
                _ => unreachable!(),
            },
        );
        assert!(result.is_err(), "{action}");
    }
}

#[test]
fn validation_and_reads_reject_changes_since_inventory() {
    let mut f = Fixture::new();
    f.put("a", b"data");
    let guard = f.partition.try_lock().unwrap();
    let inventory = guard.inventory().unwrap();
    stdfs::write(f.root.join("a"), b"changed").unwrap();
    assert!(inventory.validate().is_err());
    assert!(inventory.read(index(&inventory, "a"), 1).is_err());
}

#[cfg(target_os = "linux")]
#[test]
fn non_utf8_names_are_preserved_as_physical_inventory_only() {
    let mut f = Fixture::new();
    let name = std::ffi::OsStr::from_bytes(b"\xff");
    stdfs::write(f.root.join(name), b"raw").unwrap();
    let guard = f.partition.try_lock().unwrap();
    let inventory = guard.inventory().unwrap();
    assert_eq!(inventory.entries()[1].name(), name);
    assert_eq!(
        inventory.read(1, 3).unwrap(),
        InventoryRead::Complete(b"raw".to_vec())
    );
}
