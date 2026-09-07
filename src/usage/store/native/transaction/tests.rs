use super::*;
use serde_json::{Value, json};
use std::fs as stdfs;
use std::os::unix::fs::MetadataExt;

const ID: &str = "11111111111111111111111111111111";
const OLD: &str = "22222222222222222222222222222222";
const TEMP: &str = "33333333333333333333333333333333";

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
    fn pair(&self) {
        self.put(&format!("{OLD}.start.json"), &event(OLD, EventKind::Start));
        let mut finish: Value = serde_json::from_slice(&event(OLD, EventKind::Finish)).unwrap();
        finish["collected_wall_time"]["value"]["seconds"] = json!(200);
        self.put(
            &format!("{OLD}.finish.json"),
            &serde_json::to_vec(&finish).unwrap(),
        );
    }
}
fn event(id: &str, kind: EventKind) -> Vec<u8> {
    let fixture = match kind {
        EventKind::Start => include_str!("../../../../../tests/fixtures/usage/start-v1.json"),
        EventKind::Finish => include_str!("../../../../../tests/fixtures/usage/finish-v1.json"),
    };
    let mut value: Value = serde_json::from_str(fixture).unwrap();
    value["id"] = json!(id);
    serde_json::to_vec(&value).unwrap()
}
fn expired() -> Option<WallTime> {
    Some(WallTime {
        seconds: i64::MAX,
        nanoseconds: 0,
    })
}
fn injected() -> StoreError {
    StoreError::Io(std::io::Error::other("deterministic failure"))
}
struct Hook<F>(F);
impl<F: FnMut(Point) -> Result<(), StoreError>> Operations for Hook<F> {
    fn checkpoint(&mut self, point: Point) -> Result<(), StoreError> {
        self.0(point)
    }
}
fn start(guard: &Guard<'_>) -> PublishOutcome {
    guard.publish_with(
        ID,
        &event(ID, EventKind::Start),
        None,
        None,
        TEMP,
        &mut Native,
    )
}

#[test]
fn start_and_finish_publish_exact_bytes_with_receipt_and_stable_lock() {
    let mut f = Fixture::new();
    let lock = stdfs::metadata(f.root.join(LOCK_NAME)).unwrap().ino();
    let receipt = {
        let guard = f.partition.try_lock().unwrap();
        let result = start(&guard);
        assert!(result.error.is_none(), "{:?}", result.error);
        assert_eq!(result.publication, Publication::Published);
        assert_eq!(result.staging, Staging::Published);
        assert_eq!(result.occupancy.unwrap().files, 2);
        result.receipt.unwrap()
    };
    let guard = f.partition.try_lock().unwrap();
    let result = guard.publish_with(
        ID,
        &event(ID, EventKind::Finish),
        expired(),
        Some(&receipt),
        TEMP,
        &mut Native,
    );
    assert!(result.error.is_none(), "{:?}", result.error);
    assert_eq!(result.publication, Publication::Published);
    assert!(result.receipt.is_none());
    assert_eq!(result.occupancy.unwrap().files, 3);
    for (suffix, kind) in [("start", EventKind::Start), ("finish", EventKind::Finish)] {
        let path = f.root.join(format!("{ID}.{suffix}.json"));
        assert_eq!(stdfs::read(&path).unwrap(), event(ID, kind));
        assert_eq!(stdfs::metadata(&path).unwrap().mode() & 0o077, 0);
    }
    assert_eq!(stdfs::metadata(f.root.join(LOCK_NAME)).unwrap().ino(), lock);
}

#[test]
fn initial_invalid_state_and_collisions_do_not_earn_cleanup() {
    for case in 0..5 {
        let mut f = Fixture::new();
        f.pair();
        let temporary = format!(".tmp.{TEMP}");
        f.put(&temporary, b"abandoned");
        match case {
            0 => f.put("z-unknown", b""),
            1 => stdfs::create_dir(f.root.join("z-unsafe")).unwrap(),
            2 => f.put(&format!("{ID}.start.json"), b"rejected"),
            3 => f.put(&format!("{ID}.finish.json"), b"rejected"),
            _ => stdfs::OpenOptions::new()
                .write(true)
                .open(f.root.join(&temporary))
                .unwrap()
                .set_len(crate::usage::store::PARTITION_BYTE_LIMIT + 1)
                .unwrap(),
        }
        let guard = f.partition.try_lock().unwrap();
        let outcome = guard.publish_with(
            ID,
            &event(ID, EventKind::Start),
            expired(),
            None,
            TEMP,
            &mut Native,
        );
        assert!(outcome.error.is_some());
        assert_eq!(outcome.removed_files, 0);
        assert_eq!(outcome.staging, Staging::NotCreated);
        assert!(f.root.join(&temporary).exists());
        assert!(f.root.join(format!("{OLD}.start.json")).exists());
        assert!(f.root.join(format!("{OLD}.finish.json")).exists());
    }
}

#[test]
fn second_member_failure_reports_confirmed_partial_deletion_without_publication() {
    let mut f = Fixture::new();
    f.pair();
    let guard = f.partition.try_lock().unwrap();
    let mut hooks = Hook(|point| {
        if point == Point::BeforeUnlink(1) {
            Err(injected())
        } else {
            Ok(())
        }
    });
    let outcome = guard.publish_with(
        ID,
        &event(ID, EventKind::Start),
        expired(),
        None,
        TEMP,
        &mut hooks,
    );
    assert!(outcome.error.is_some());
    assert_eq!(outcome.removed_files, 1);
    assert_eq!(
        outcome.removed_bytes,
        event(OLD, EventKind::Start).len() as u64
    );
    assert!(outcome.partial_group);
    assert_eq!(outcome.occupancy.unwrap().files, 2);
    assert_eq!(outcome.publication, Publication::NotPublished);
    assert!(!f.root.join(format!("{OLD}.start.json")).exists());
    assert!(f.root.join(format!("{OLD}.finish.json")).exists());
}

#[test]
fn own_deletion_does_not_bless_unexplained_membership_changes() {
    let mut f = Fixture::new();
    f.pair();
    let guard = f.partition.try_lock().unwrap();
    let mut hooks = Hook(|point| {
        if point == Point::AfterUnlink(0) {
            stdfs::write(f.root.join("unknown"), b"external").unwrap();
        }
        Ok(())
    });
    let outcome = guard.publish_with(
        ID,
        &event(ID, EventKind::Start),
        expired(),
        None,
        TEMP,
        &mut hooks,
    );
    assert!(matches!(outcome.error, Some(StoreError::IdentityChanged)));
    assert_eq!(outcome.removed_files, 1);
    assert!(outcome.partial_group);
    assert!(outcome.occupancy.is_none());
    assert_eq!(outcome.staging, Staging::NotCreated);
}

#[test]
fn no_replace_collision_preserves_new_destination_and_cleans_only_own_stage() {
    let mut f = Fixture::new();
    let guard = f.partition.try_lock().unwrap();
    let mut hooks = Hook(|point| {
        if point == Point::RenameReady {
            stdfs::write(f.root.join(format!("{ID}.start.json")), b"winner").unwrap();
        }
        Ok(())
    });
    let outcome = guard.publish_with(
        ID,
        &event(ID, EventKind::Start),
        None,
        None,
        TEMP,
        &mut hooks,
    );
    assert!(matches!(outcome.error, Some(StoreError::Collision)));
    assert_eq!(outcome.publication, Publication::NotPublished);
    assert_eq!(outcome.staging, Staging::Removed);
    assert!(outcome.receipt.is_none());
    assert!(outcome.occupancy.is_none());
    assert_eq!(
        stdfs::read(f.root.join(format!("{ID}.start.json"))).unwrap(),
        b"winner"
    );
}

#[test]
fn exclusive_staging_creation_never_truncates_a_collision() {
    let mut f = Fixture::new();
    let guard = f.partition.try_lock().unwrap();
    let mut hooks = Hook(|point| {
        if point == Point::CreateReady {
            stdfs::write(f.root.join(format!(".tmp.{TEMP}")), b"winner").unwrap();
        }
        Ok(())
    });
    let outcome = guard.publish_with(
        ID,
        &event(ID, EventKind::Start),
        None,
        None,
        TEMP,
        &mut hooks,
    );
    assert!(outcome.error.is_some());
    assert_eq!(outcome.staging, Staging::NotCreated);
    assert_eq!(
        stdfs::read(f.root.join(format!(".tmp.{TEMP}"))).unwrap(),
        b"winner"
    );
}

#[test]
fn rename_commit_point_survives_post_publication_verification_failure() {
    for point in [Point::BeforeRename, Point::AfterRename] {
        let mut f = Fixture::new();
        let guard = f.partition.try_lock().unwrap();
        let mut hooks = Hook(|at| if at == point { Err(injected()) } else { Ok(()) });
        let outcome = guard.publish_with(
            ID,
            &event(ID, EventKind::Start),
            None,
            None,
            TEMP,
            &mut hooks,
        );
        assert!(outcome.error.is_some());
        assert!(outcome.receipt.is_none());
        let published = point == Point::AfterRename;
        assert_eq!(outcome.publication == Publication::Published, published);
        assert_eq!(f.root.join(format!("{ID}.start.json")).exists(), published);
        assert_eq!(
            outcome.staging,
            if published {
                Staging::Published
            } else {
                Staging::Removed
            }
        );
    }
}

struct Writes {
    calls: usize,
    failure: Option<usize>,
    zero: bool,
}
impl Operations for Writes {
    fn write(&mut self, fd: &OwnedFd, bytes: &[u8]) -> Result<usize, rustix::io::Errno> {
        self.calls += 1;
        if self.calls == 1 {
            return Err(rustix::io::Errno::INTR);
        }
        if self.failure == Some(self.calls) {
            return if self.zero {
                Ok(0)
            } else {
                Err(rustix::io::Errno::NOSPC)
            };
        }
        rustix::io::write(fd, &bytes[..bytes.len().min(71)])
    }
}

#[test]
fn interrupted_partial_and_failed_writes_are_bounded_and_never_publish_prefixes() {
    for (failure, zero) in [(None, false), (Some(3), false), (Some(3), true)] {
        let mut f = Fixture::new();
        let guard = f.partition.try_lock().unwrap();
        let mut writes = Writes {
            calls: 0,
            failure,
            zero,
        };
        let outcome = guard.publish_with(
            ID,
            &event(ID, EventKind::Start),
            None,
            None,
            TEMP,
            &mut writes,
        );
        assert!(writes.calls >= 3);
        assert_eq!(outcome.error.is_some(), failure.is_some());
        assert_eq!(
            outcome.publication == Publication::Published,
            failure.is_none()
        );
        if failure.is_some() {
            assert_eq!(outcome.staging, Staging::Removed);
            assert_eq!(outcome.occupancy.unwrap().files, 1);
        }
    }
}

#[test]
fn unsafe_or_failed_staging_cleanup_preserves_primary_error_and_abandoned_entry() {
    for replace in [false, true] {
        let mut f = Fixture::new();
        let guard = f.partition.try_lock().unwrap();
        let path = f.root.join(format!(".tmp.{TEMP}"));
        let mut hooks = Hook(|point| {
            if point == Point::BeforeRename {
                if replace {
                    stdfs::remove_file(&path).unwrap();
                    stdfs::write(&path, b"replacement").unwrap();
                }
                return Err(injected());
            }
            if point == Point::BeforeStageCleanup && !replace {
                return Err(injected());
            }
            Ok(())
        });
        let outcome = guard.publish_with(
            ID,
            &event(ID, EventKind::Start),
            None,
            None,
            TEMP,
            &mut hooks,
        );
        assert_eq!(outcome.error.unwrap().to_string(), injected().to_string());
        assert_eq!(outcome.staging, Staging::MayRemain);
        assert!(path.exists());
        if replace {
            assert_eq!(stdfs::read(path).unwrap(), b"replacement");
        }
    }
}

#[test]
fn missing_changed_foreign_and_replayed_receipts_refuse_before_cleanup() {
    for case in 0..4 {
        let mut f = Fixture::new();
        let mut other = Fixture::new();
        let receipt = start(&f.partition.try_lock().unwrap()).receipt.unwrap();
        let target = if case == 2 { &mut other } else { &mut f };
        match case {
            0 => stdfs::remove_file(target.root.join(format!("{ID}.start.json"))).unwrap(),
            1 => stdfs::write(target.root.join(format!("{ID}.start.json")), b"changed").unwrap(),
            2 => {}
            _ => {
                let outcome = target.partition.try_lock().unwrap().publish_with(
                    ID,
                    &event(ID, EventKind::Finish),
                    None,
                    Some(&receipt),
                    TEMP,
                    &mut Native,
                );
                assert!(outcome.error.is_none(), "{:?}", outcome.error);
            }
        }
        target.put(&format!(".tmp.{TEMP}"), b"abandoned");
        let outcome = target.partition.try_lock().unwrap().publish_with(
            ID,
            &event(ID, EventKind::Finish),
            expired(),
            Some(&receipt),
            TEMP,
            &mut Native,
        );
        assert!(
            matches!(
                (case, &outcome.error),
                (0, Some(StoreError::MissingStart))
                    | (1 | 2, Some(StoreError::ReceiptMismatch))
                    | (3, Some(StoreError::Collision))
            ),
            "case {case}: {:?}",
            outcome.error
        );
        assert_eq!(outcome.removed_files, 0);
        assert!(target.root.join(format!(".tmp.{TEMP}")).exists());
    }
}

#[test]
fn receipt_digest_rejects_changed_bytes_even_when_metadata_observation_agrees() {
    let mut f = Fixture::new();
    let mut receipt = start(&f.partition.try_lock().unwrap()).receipt.unwrap();
    let path = f.root.join(format!("{ID}.start.json"));
    let mut changed: Value = serde_json::from_slice(&event(ID, EventKind::Start)).unwrap();
    changed["binary_version"] = json!("different");
    stdfs::write(&path, serde_json::to_vec(&changed).unwrap()).unwrap();
    // Model a colliding metadata observation deterministically. The public
    // receipt has no mutation API; its independently retained digest must still
    // reject bytes that the stamp alone would admit.
    receipt.native.start = fs::stat(&path).unwrap();
    let outcome = f.partition.try_lock().unwrap().publish_with(
        ID,
        &event(ID, EventKind::Finish),
        None,
        Some(&receipt),
        TEMP,
        &mut Native,
    );
    assert!(matches!(outcome.error, Some(StoreError::ReceiptMismatch)));
    assert_eq!(outcome.removed_files, 0);
    assert_eq!(outcome.publication, Publication::NotPublished);
}

#[test]
fn staging_attribute_changes_are_not_adopted_as_effects_of_write_or_rename() {
    use std::os::unix::fs::PermissionsExt;
    for point in [Point::BeforeWrite(0), Point::RenameReady] {
        let mut f = Fixture::new();
        let path = f.root.join(format!(".tmp.{TEMP}"));
        let mut hooks = Hook(|at| {
            if at == point {
                stdfs::set_permissions(&path, stdfs::Permissions::from_mode(0o400)).unwrap();
            }
            Ok(())
        });
        let outcome = f.partition.try_lock().unwrap().publish_with(
            ID,
            &event(ID, EventKind::Start),
            None,
            None,
            TEMP,
            &mut hooks,
        );
        assert!(matches!(outcome.error, Some(StoreError::IdentityChanged)));
        assert!(outcome.receipt.is_none());
        assert_eq!(
            outcome.publication == Publication::Published,
            point == Point::RenameReady
        );
    }
}

#[test]
fn full_quota_reserves_stage_and_finish_protects_its_start() {
    let mut f = Fixture::new();
    let receipt = start(&f.partition.try_lock().unwrap()).receipt.unwrap();
    let retained = event(ID, EventKind::Start).len() as u64;
    // Recognized rejected finals remain quota-evictable and are accounted at
    // full logical size without allocating a payload buffer.
    f.put(&format!("{OLD}.start.json"), b"");
    stdfs::OpenOptions::new()
        .write(true)
        .open(f.root.join(format!("{OLD}.start.json")))
        .unwrap()
        .set_len(crate::usage::store::PARTITION_BYTE_LIMIT - retained)
        .unwrap();
    let outcome = f.partition.try_lock().unwrap().publish_with(
        ID,
        &event(ID, EventKind::Finish),
        None,
        Some(&receipt),
        TEMP,
        &mut Native,
    );
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    assert_eq!(outcome.removed_files, 1);
    assert_eq!(
        outcome.occupancy.unwrap().bytes,
        retained + event(ID, EventKind::Finish).len() as u64
    );
    assert!(f.root.join(format!("{ID}.start.json")).exists());
}

#[test]
fn exact_byte_peak_counts_staging_without_a_second_rename_slot() {
    for extra in [0, 1] {
        let mut f = Fixture::new();
        let bytes = event(ID, EventKind::Start);
        f.put(&format!("{OLD}.start.json"), b"");
        stdfs::OpenOptions::new()
            .write(true)
            .open(f.root.join(format!("{OLD}.start.json")))
            .unwrap()
            .set_len(crate::usage::store::PARTITION_BYTE_LIMIT - bytes.len() as u64 + extra)
            .unwrap();
        let mut measured = None;
        let mut hooks = Hook(|point| {
            if point == Point::BeforeRename {
                let size: u64 = stdfs::read_dir(&f.root)
                    .unwrap()
                    .map(|entry| entry.unwrap().metadata().unwrap().len())
                    .sum();
                measured = Some(size);
            }
            Ok(())
        });
        let outcome = f
            .partition
            .try_lock()
            .unwrap()
            .publish_with(ID, &bytes, None, None, TEMP, &mut hooks);
        assert!(outcome.error.is_none(), "{:?}", outcome.error);
        assert_eq!(outcome.removed_files, extra as usize);
        assert_eq!(
            measured.unwrap(),
            if extra == 0 {
                crate::usage::store::PARTITION_BYTE_LIMIT
            } else {
                bytes.len() as u64
            }
        );
        assert_eq!(outcome.occupancy.unwrap().bytes, measured.unwrap());
    }
}

#[test]
fn invalid_publication_bytes_never_mutate_records() {
    let mut f = Fixture::new();
    f.pair();
    let guard = f.partition.try_lock().unwrap();
    for bytes in [
        b"invalid".to_vec(),
        event(OLD, EventKind::Start),
        event(ID, EventKind::Finish),
        vec![b' '; EVENT_LIMIT + 1],
    ] {
        let outcome = guard.publish_with(ID, &bytes, expired(), None, TEMP, &mut Native);
        assert!(matches!(outcome.error, Some(StoreError::InvalidEvent)));
        assert_eq!(outcome.removed_files, 0);
        assert_eq!(outcome.staging, Staging::NotCreated);
    }
}

#[test]
fn interruption_child() {
    let Ok(base) = std::env::var("DRFT_TRANSACTION_TEST_BASE") else {
        return;
    };
    let point: usize = std::env::var("DRFT_TRANSACTION_TEST_POINT")
        .unwrap()
        .parse()
        .unwrap();
    let base = PathBuf::from(base);
    let mut partition = Partition::open(&base.join("cache"), &base.join("graph"))
        .unwrap()
        .0;
    let guard = partition.try_lock().unwrap();
    struct Stop {
        point: usize,
    }
    impl Operations for Stop {
        fn checkpoint(&mut self, at: Point) -> Result<(), StoreError> {
            let stop = match self.point {
                0 => at == Point::BeforeCreate,
                1 => matches!(at, Point::BeforeWrite(n) if n > 0),
                2 => at == Point::BeforeRename,
                _ => at == Point::AfterRename,
            };
            if stop {
                std::process::exit(73);
            }
            Ok(())
        }
        fn write(&mut self, fd: &OwnedFd, bytes: &[u8]) -> Result<usize, rustix::io::Errno> {
            rustix::io::write(fd, &bytes[..bytes.len().min(71)])
        }
    }
    let _ = guard.publish_with(
        ID,
        &event(ID, EventKind::Start),
        None,
        None,
        TEMP,
        &mut Stop { point },
    );
    panic!("interruption checkpoint was not reached");
}

#[test]
fn process_exit_leaves_only_abandoned_stage_or_complete_final_and_releases_lock() {
    for point in 0..4 {
        let mut f = Fixture::new();
        let base = f.root.parent().unwrap().parent().unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "usage::store::native::transaction::tests::interruption_child",
            ])
            .env("DRFT_TRANSACTION_TEST_BASE", base)
            .env("DRFT_TRANSACTION_TEST_POINT", point.to_string())
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(73));
        let staging = f.root.join(format!(".tmp.{TEMP}"));
        let final_path = f.root.join(format!("{ID}.start.json"));
        match point {
            0 => {
                assert!(!staging.exists());
                assert!(!final_path.exists());
            }
            1 => {
                assert_eq!(stdfs::metadata(&staging).unwrap().len(), 71);
                assert!(!final_path.exists());
            }
            2 => {
                assert_eq!(stdfs::read(&staging).unwrap(), event(ID, EventKind::Start));
                assert!(!final_path.exists());
            }
            _ => {
                assert!(!staging.exists());
                assert_eq!(
                    stdfs::read(&final_path).unwrap(),
                    event(ID, EventKind::Start)
                );
            }
        }
        // The next independent lock succeeds and a fresh transaction cleans
        // recognized abandoned bytes without repairing synchronization.
        let outcome = f.partition.try_lock().unwrap().publish_with(
            OLD,
            &event(OLD, EventKind::Start),
            None,
            None,
            TEMP,
            &mut Native,
        );
        assert!(outcome.error.is_none(), "{:?}", outcome.error);
        assert_eq!(outcome.removed_files, usize::from(point == 1 || point == 2));
        assert!(!staging.exists());
    }
}
