use super::*;
use serde_json::{Value, json};

const A: &str = "11111111111111111111111111111111";
const B: &str = "22222222222222222222222222222222";
const C: &str = "33333333333333333333333333333333";

fn time(seconds: i64) -> WallTime {
    WallTime {
        seconds,
        nanoseconds: 0,
    }
}
fn supported(entry: i64, collected: i64) -> RecordState {
    RecordState::Supported {
        entry: Some(time(entry)),
        collected: Some(time(collected)),
    }
}
fn group(id: &str, start: Option<RecordState>, finish: Option<RecordState>, index: usize) -> Group {
    Group {
        id: id.into(),
        start: start.map(|state| Record {
            index,
            bytes: 10,
            state,
        }),
        finish: finish.map(|state| Record {
            index: index + 1,
            bytes: 10,
            state,
        }),
    }
}
fn set(groups: Vec<Group>, temporary: Vec<(usize, u64)>) -> RecordSet {
    let bytes = groups
        .iter()
        .flat_map(Group::records)
        .map(|r| r.bytes)
        .sum::<u64>()
        + temporary.iter().map(|(_, bytes)| bytes).sum::<u64>();
    let files = 1 + groups.iter().flat_map(Group::records).count() + temporary.len();
    RecordSet {
        groups: groups.into_iter().map(|g| (g.id.clone(), g)).collect(),
        temporary,
        bytes,
        files,
    }
}
fn fixture(id: &str, kind: EventKind) -> Vec<u8> {
    let fixture = match kind {
        EventKind::Start => include_str!("../../../../tests/fixtures/usage/start-v1.json"),
        EventKind::Finish => include_str!("../../../../tests/fixtures/usage/finish-v1.json"),
    };
    let mut value: Value = serde_json::from_str(fixture).unwrap();
    value["id"] = json!(id);
    serde_json::to_vec(&value).unwrap()
}
fn entry(name: &str, bytes: u64) -> InventoryEntry {
    InventoryEntry {
        name: name.into(),
        bytes,
    }
}

#[test]
fn exact_names_exclude_aliases_and_unsupported_controls() {
    assert_eq!(name(OsStr::new(".lock")).unwrap(), Name::Lock);
    assert_eq!(
        name(OsStr::new(&format!(".tmp.{A}"))).unwrap(),
        Name::Temporary
    );
    assert_eq!(
        name(OsStr::new(&format!("{A}.start.json"))).unwrap(),
        Name::Final {
            id: A,
            kind: EventKind::Start
        }
    );
    assert_eq!(
        name(OsStr::new(&format!("{A}.finish.json"))).unwrap(),
        Name::Final {
            id: A,
            kind: EventKind::Finish
        }
    );
    for bad in [
        ".health.json".into(),
        format!("{}.start.json", "A".repeat(32)),
        format!("{A}.start.json.bak"),
        format!(".tmp.{A}.json"),
        format!("/{A}.start.json"),
        format!("{}.start.json", "f".repeat(31)),
        format!("{}.finish.json", "g".repeat(32)),
        "".into(),
    ] {
        assert!(
            matches!(name(OsStr::new(&bad)), Err(StoreError::UnknownName)),
            "{bad}"
        );
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        assert!(matches!(
            name(OsStr::from_bytes(b"\xff.start.json")),
            Err(StoreError::UnknownName)
        ));
    }
}

#[test]
fn recognized_rejections_remain_grouped_and_accounted() {
    let entries = vec![
        entry(".lock", 0),
        entry(&format!("{A}.start.json"), 1),
        entry(&format!("{A}.finish.json"), EVENT_LIMIT as u64 + 1),
        entry(&format!(".tmp.{B}"), 17),
    ];
    let mut read_indices = Vec::new();
    let records = RecordSet::scan(&entries, |index| {
        read_indices.push(index);
        Ok(if index == 1 {
            InventoryRead::Complete(b"{".to_vec())
        } else {
            InventoryRead::Oversized {
                bytes: entries[index].bytes(),
            }
        })
    })
    .unwrap();
    assert_eq!(read_indices, [1, 2]);
    assert_eq!(records.files, 4);
    assert_eq!(records.bytes, EVENT_LIMIT as u64 + 19);
    let group = &records.groups[A];
    assert_eq!(group.pairing(), Pairing::Paired);
    assert_eq!(group.start().unwrap().state(), &RecordState::Malformed);
    assert_eq!(group.finish().unwrap().state(), &RecordState::Oversized);
    assert_eq!(group.coverage(), Coverage::Unknown);
}

#[test]
fn complete_scan_errors_never_return_partial_classification() {
    let valid = fixture(A, EventKind::Start);
    let entries = vec![
        entry(".lock", 0),
        entry(&format!("{A}.start.json"), valid.len() as u64),
        entry("zzz", 0),
    ];
    let mut reads = 0;
    assert!(matches!(
        RecordSet::scan(&entries, |_| {
            reads += 1;
            Ok(InventoryRead::Complete(valid.clone()))
        }),
        Err(StoreError::UnknownName)
    ));
    assert_eq!(reads, 1);
    let entries = vec![
        entry(".lock", 0),
        entry(&format!("{A}.start.json"), 1),
        entry(&format!("{B}.start.json"), 1),
    ];
    assert!(matches!(
        RecordSet::scan(&entries, |index| if index == 1 {
            Ok(InventoryRead::Complete(vec![b'{']))
        } else {
            Err(StoreError::IdentityChanged)
        }),
        Err(StoreError::IdentityChanged)
    ));
    assert!(matches!(
        RecordSet::scan(&[entry(".lock", 1)], |_| unreachable!()),
        Err(StoreError::UnsafeEntry)
    ));
    assert!(matches!(
        RecordSet::scan(&[], |_| unreachable!()),
        Err(StoreError::IdentityChanged)
    ));
    assert!(matches!(
        RecordSet::scan(
            &[
                entry(".lock", 0),
                entry(&format!(".tmp.{A}"), PARTITION_BYTE_LIMIT + 1)
            ],
            |_| unreachable!()
        ),
        Err(StoreError::ScanLimit)
    ));
}

#[test]
fn pair_coverage_requires_every_endpoint_and_agreement() {
    assert_eq!(
        group(A, Some(supported(10, 11)), Some(supported(10, 12)), 1).coverage(),
        Coverage::Known {
            first: time(10),
            last: time(12)
        }
    );
    let cases = [
        group(A, Some(supported(10, 11)), None, 1),
        group(A, None, Some(supported(10, 12)), 1),
        group(A, Some(supported(10, 11)), Some(supported(9, 12)), 1),
        group(A, Some(supported(10, 11)), Some(supported(10, 9)), 1),
        group(A, Some(supported(10, 9)), Some(supported(10, 12)), 1),
        group(
            A,
            Some(RecordState::Supported {
                entry: None,
                collected: Some(time(11)),
            }),
            Some(supported(10, 12)),
            1,
        ),
        group(
            A,
            Some(RecordState::Unsupported),
            Some(supported(10, 12)),
            1,
        ),
    ];
    for group in cases {
        assert_eq!(group.coverage(), Coverage::Unknown);
    }
    assert_eq!(
        group(A, None, Some(supported(0, 1)), 1).pairing(),
        Pairing::Orphan
    );
    assert_eq!(
        group(A, Some(supported(0, 1)), None, 1).pairing(),
        Pairing::Incomplete
    );
}

#[test]
fn expiry_uses_latest_endpoint_and_exact_nanosecond_boundary() {
    let records = set(
        vec![group(
            A,
            Some(supported(-100, -99)),
            Some(supported(-100, 1)),
            1,
        )],
        vec![],
    );
    let boundary = 1 + RETENTION_SECONDS as i64;
    let before = WallTime {
        seconds: boundary - 1,
        nanoseconds: 999_999_999,
    };
    assert!(
        records
            .plan(Request::PruneExpired, Some(before), 100, 10)
            .unwrap()
            .remove
            .is_empty()
    );
    assert_eq!(
        records
            .plan(Request::PruneExpired, Some(time(boundary)), 100, 10)
            .unwrap()
            .remove,
        [1, 2]
    );
    assert!(
        records
            .plan(Request::PruneExpired, Some(time(0)), 100, 10)
            .unwrap()
            .remove
            .is_empty()
    );
    assert!(
        records
            .plan(Request::PruneExpired, None, 100, 10)
            .unwrap()
            .remove
            .is_empty()
    );
    let records = set(
        vec![group(
            A,
            Some(supported(i64::MIN, i64::MIN)),
            Some(supported(i64::MIN, i64::MIN)),
            1,
        )],
        vec![],
    );
    assert_eq!(
        records
            .plan(Request::PruneExpired, Some(time(i64::MAX)), 100, 10)
            .unwrap()
            .remove,
        [1, 2]
    );
}

#[test]
fn uncertain_groups_survive_age_and_are_evicted_as_whole_groups_for_quota() {
    let records = set(
        vec![group(
            A,
            Some(RecordState::Malformed),
            Some(RecordState::Unsupported),
            1,
        )],
        vec![],
    );
    assert!(
        records
            .plan(Request::PruneExpired, Some(time(i64::MAX)), 100, 10)
            .unwrap()
            .remove
            .is_empty()
    );
    let plan = records
        .plan(
            Request::ReserveStart { id: B, bytes: 1 },
            Some(time(i64::MAX)),
            100,
            3,
        )
        .unwrap();
    assert_eq!(plan.remove, [1, 2]);
    assert_eq!(
        (
            plan.retained_files,
            plan.retained_bytes,
            plan.peak_files,
            plan.peak_bytes
        ),
        (1, 0, 2, 1)
    );
}

#[test]
fn eviction_order_is_total_and_not_unknown_chronology() {
    let records = set(
        vec![
            group(A, Some(RecordState::Malformed), None, 1),
            group(C, Some(supported(0, 1)), Some(supported(0, 2)), 3),
            group(B, Some(supported(0, 1)), Some(supported(0, 2)), 5),
        ],
        vec![],
    );
    let plan = records
        .plan(
            Request::ReserveStart {
                id: "44444444444444444444444444444444",
                bytes: 50,
            },
            None,
            50,
            10,
        )
        .unwrap();
    assert_eq!(plan.remove, [5, 6, 3, 4, 1]);
    assert_eq!(plan.peak_bytes, 50);
}

#[test]
fn temporaries_are_reserved_once_and_clear_preserves_lock() {
    let records = set(
        vec![group(A, Some(RecordState::Malformed), None, 1)],
        vec![(2, 20)],
    );
    let plan = records
        .plan(Request::ReserveStart { id: B, bytes: 20 }, None, 30, 3)
        .unwrap();
    assert_eq!(plan.remove, [2]);
    assert_eq!((plan.peak_bytes, plan.peak_files), (30, 3));
    let all = records.plan(Request::PruneAll, None, 30, 3).unwrap();
    assert_eq!(all.remove, [2, 1]);
    assert_eq!((all.retained_bytes, all.retained_files), (0, 1));
    let records = set(vec![], vec![]);
    assert!(matches!(
        records.plan(Request::ReserveStart { id: A, bytes: 1 }, None, 1, 1),
        Err(StoreError::Capacity)
    ));
}

#[test]
fn finish_reservation_protects_start_and_rejects_missing_collision_and_capacity() {
    let id = "11111111111111111111111111111111";
    let start = group(id, Some(supported(1, 2)), None, 1);
    let records = set(vec![start], vec![]);
    let request = Request::ReserveFinish { id, bytes: 50 };
    let plan = records.plan(request, Some(time(i64::MAX)), 60, 3).unwrap();
    assert!(plan.remove.is_empty());
    assert_eq!((plan.peak_bytes, plan.peak_files), (60, 3));
    assert!(matches!(
        records.plan(request, None, 59, 3),
        Err(StoreError::Capacity)
    ));
    assert!(matches!(
        records.plan(request, None, 60, 2),
        Err(StoreError::Capacity)
    ));
    assert!(matches!(
        set(vec![], vec![]).plan(request, None, 150, 3),
        Err(StoreError::MissingStart)
    ));
    let paired = set(
        vec![group(id, Some(supported(1, 2)), Some(supported(1, 3)), 1)],
        vec![],
    );
    assert!(matches!(
        paired.plan(request, None, 1000, 10),
        Err(StoreError::Collision)
    ));
}

#[test]
fn collisions_precede_any_cleanup_and_invalid_state_cannot_be_pruned_to_fit() {
    for existing in [
        group(A, Some(supported(0, 1)), Some(supported(0, 2)), 1),
        group(A, None, Some(RecordState::Malformed), 1),
    ] {
        let records = set(vec![existing], vec![(9, 10)]);
        assert!(matches!(
            records.plan(
                Request::ReserveStart { id: A, bytes: 1 },
                Some(time(i64::MAX)),
                100,
                10
            ),
            Err(StoreError::Collision)
        ));
    }
    let records = set(
        vec![group(A, Some(RecordState::Malformed), None, 1)],
        vec![],
    );
    assert!(matches!(
        records.plan(Request::PruneAll, None, 9, 10),
        Err(StoreError::ScanLimit)
    ));
    assert!(matches!(
        records.plan(Request::PruneAll, None, 100, 1),
        Err(StoreError::ScanLimit)
    ));
    for (id, bytes) in [("bad", 1), (B, 0), (B, EVENT_LIMIT as u64 + 1)] {
        assert!(matches!(
            records.plan(Request::ReserveStart { id, bytes }, None, 100, 10),
            Err(StoreError::InvalidReservation)
        ));
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn native_record_inventory_borrows_snapshot_and_detects_later_changes() {
    use crate::usage::store::Partition;
    let root = tempfile::tempdir().unwrap();
    let graph = root.path().join("graph");
    let cache = root.path().join("cache");
    std::fs::create_dir(&graph).unwrap();
    let mut partition = Partition::open_or_create(&cache, &graph).unwrap();
    let directory = cache.join(partition.partition_id());
    let path = directory.join(format!("{A}.start.json"));
    std::fs::write(&path, fixture(A, EventKind::Start)).unwrap();
    let guard = partition.try_lock().unwrap();
    let physical = guard.inventory().unwrap();
    let records = physical.records().unwrap();
    assert_eq!(
        records.groups().next().unwrap().pairing(),
        Pairing::Incomplete
    );
    assert!(matches!(
        records.groups().next().unwrap().start().unwrap().state(),
        RecordState::Supported { .. }
    ));
    let plan = records.plan(Request::PruneAll, None).unwrap();
    assert_eq!(plan.removal_indices().len(), 1);
    assert_eq!(plan.retained_files(), 1);
    assert!(path.exists());
    std::fs::write(&path, b"{}").unwrap();
    assert!(matches!(plan.validate(), Err(StoreError::IdentityChanged)));
    assert!(matches!(
        records.plan(Request::PruneAll, None),
        Err(StoreError::IdentityChanged)
    ));
}
