use super::*;
use serde_json::json;
const ID: &str = "12121212121212121212121212121212";
const START: &[u8] = include_bytes!("../../../tests/fixtures/usage/start-v1.json");
const FINISH: &[u8] = include_bytes!("../../../tests/fixtures/usage/finish-v1.json");
fn state(v: &Value, kind: EventKind) -> RecordState {
    classify(&serde_json::to_vec(v).unwrap(), ID, kind)
}
fn fixture(kind: EventKind) -> Value {
    serde_json::from_slice(match kind {
        EventKind::Start => START,
        EventKind::Finish => FINISH,
    })
    .unwrap()
}
#[test]
fn literal_fixtures_supply_times_including_clock_rollback() {
    assert_eq!(
        classify(START, ID, EventKind::Start),
        RecordState::Supported {
            entry: Some(WallTime {
                seconds: 100,
                nanoseconds: 123
            }),
            collected: Some(WallTime {
                seconds: 101,
                nanoseconds: 456
            }),
        }
    );
    assert_eq!(
        classify(FINISH, ID, EventKind::Finish),
        RecordState::Supported {
            entry: Some(WallTime {
                seconds: 100,
                nanoseconds: 123
            }),
            collected: Some(WallTime {
                seconds: 99,
                nanoseconds: 456
            }),
        }
    );
}
#[test]
fn every_required_field_and_nested_unknown_field_is_checked() {
    fn mutate_objects(root: &Value, path: &str, kind: EventKind) {
        match root.pointer(path).unwrap() {
            Value::Object(map) => {
                let mut changed = root.clone();
                changed
                    .pointer_mut(path)
                    .unwrap()
                    .as_object_mut()
                    .unwrap()
                    .insert("surprise".into(), json!(0));
                assert_eq!(
                    state(&changed, kind),
                    RecordState::Malformed,
                    "unknown at {path}"
                );
                for key in map.keys() {
                    let mut changed = root.clone();
                    changed
                        .pointer_mut(path)
                        .unwrap()
                        .as_object_mut()
                        .unwrap()
                        .remove(key);
                    assert_eq!(
                        state(&changed, kind),
                        RecordState::Malformed,
                        "missing {path}/{key}"
                    );
                    mutate_objects(root, &format!("{path}/{key}"), kind);
                }
            }
            Value::Array(a) => {
                for i in 0..a.len() {
                    mutate_objects(root, &format!("{path}/{i}"), kind);
                }
            }
            _ => {}
        }
    }
    for kind in [EventKind::Start, EventKind::Finish] {
        mutate_objects(&fixture(kind), "", kind);
    }
}
#[test]
fn headers_sizes_duplicates_depth_and_trailing_data_fail_closed() {
    assert_eq!(
        classify(&vec![b' '; EVENT_LIMIT + 1], ID, EventKind::Start),
        RecordState::Oversized
    );
    for bytes in [
        b"null".as_slice(),
        b"[]",
        b"{}",
        b"{\"schema\":\"drft-usage\",\"schema\":\"drft-usage\"}",
    ] {
        assert_eq!(
            classify(bytes, ID, EventKind::Start),
            RecordState::Malformed
        );
    }
    let mut trailing = START.to_vec();
    trailing.extend_from_slice(b" true");
    assert_eq!(
        classify(&trailing, ID, EventKind::Start),
        RecordState::Malformed
    );
    let duplicate = String::from_utf8(START.to_vec()).unwrap().replacen(
        "\"seconds\": 100",
        "\"seconds\": 100, \"seconds\": 100",
        1,
    );
    assert_eq!(
        classify(duplicate.as_bytes(), ID, EventKind::Start),
        RecordState::Malformed
    );
    let escaped = String::from_utf8(START.to_vec()).unwrap().replacen(
        "\"seconds\": 100",
        "\"seconds\": 100, \"\\u0073econds\": 100",
        1,
    );
    assert_eq!(
        classify(escaped.as_bytes(), ID, EventKind::Start),
        RecordState::Malformed
    );
    for revision in [json!(0), json!(-1), json!(1.0), json!("1"), json!(null)] {
        let mut v = fixture(EventKind::Start);
        v["revision"] = revision;
        assert_eq!(state(&v, EventKind::Start), RecordState::Malformed);
    }
    let mut v =
        json!({"schema":"drft-usage","revision":2,"event":"start","id":ID,"future":[1,2,3]});
    assert_eq!(state(&v, EventKind::Start), RecordState::Unsupported);
    v["id"] = json!("00000000000000000000000000000000");
    assert_eq!(state(&v, EventKind::Start), RecordState::Malformed);
    let deep = format!(
        "{{\"schema\":\"drft-usage\",\"revision\":2,\"event\":\"start\",\"id\":\"{ID}\",\"x\":{}0{}}}",
        "[".repeat(200),
        "]".repeat(200)
    );
    assert_eq!(
        classify(deep.as_bytes(), ID, EventKind::Start),
        RecordState::Malformed
    );
    assert_eq!(
        classify(START, ID, EventKind::Finish),
        RecordState::Malformed
    );
}
#[test]
fn malformed_timestamps_identity_and_native_units_are_rejected() {
    for (path, replacement) in [
        (
            "/entry_wall_time/value/nanoseconds",
            json!(1_000_000_000u64),
        ),
        ("/entry_wall_time/value/seconds", json!(u64::MAX)),
        ("/entry_wall_time/status", json!("invented")),
        ("/config_fingerprint", json!("b3:abcd")),
        ("/original_cwd/os_encoding", json!("other")),
        ("/caller/authenticated", json!(true)),
        ("/argv/3/value", json!("/w")),
        ("/argv/3/value", json!("YR==")),
        ("/argv/3/value", json!("YQ==")),
    ] {
        let mut v = fixture(EventKind::Start);
        *v.pointer_mut(path).unwrap() = replacement;
        assert_eq!(
            state(&v, EventKind::Start),
            RecordState::Malformed,
            "{path}"
        );
    }
    let mut v = fixture(EventKind::Start);
    v["argv"][3] = json!({"os_encoding":"windows-utf16le","encoding":"base64","value":"YQAA2A=="});
    assert!(matches!(
        state(&v, EventKind::Start),
        RecordState::Supported { .. }
    ));
    // ANhh decodes to an unpaired surrogate plus one trailing byte. Ignoring
    // that remainder would accept it as a valid base64 UTF-16 representation.
    for s in ["YQ==", "YQA=", "%%==", "ANhh"] {
        v["argv"][3]["value"] = json!(s);
        assert_eq!(state(&v, EventKind::Start), RecordState::Malformed);
    }
    v["argv"][3] = json!({"os_encoding":"windows-utf16le","encoding":"utf8","value":"😀"});
    v["entry_wall_time"] = json!({"status":"unavailable","value":"not_observed"});
    assert!(matches!(
        state(&v, EventKind::Start),
        RecordState::Supported { entry: None, .. }
    ));
}

#[test]
fn included_count_matches_records_independently_of_balanced_totals() {
    for category in ["findings", "hints", "error"] {
        let mut value = fixture(EventKind::Finish);
        let prefix = json!({
            "total": {"status":"available","value":2},
            "included":2,
            "omitted": {"status":"available","value":0},
            "records":[]
        });
        match category {
            "findings" => {
                value["structured"][category] = json!({
                    "availability":"available",
                    "coverage":{"status":"available","value":"construction_diagnostics"},
                    "prefix":prefix
                })
            }
            "hints" => value["structured"][category] = prefix,
            "error" => {
                value["structured"][category] = json!({
                    "availability":"available",
                    "present":{"status":"available","value":true},
                    "traversal":"complete",
                    "prefix":prefix
                })
            }
            _ => unreachable!(),
        }
        // Every other count relationship is valid, so another arithmetic
        // guard cannot substitute for checking the actual array length.
        assert_eq!(
            state(&value, EventKind::Finish),
            RecordState::Malformed,
            "{category}"
        );
    }
}
#[test]
fn structured_records_counts_and_traversal_are_validated() {
    let mut v = fixture(EventKind::Finish);
    v["structured"]["findings"] = json!({"availability":"available","coverage":{"status":"available","value":"construction_diagnostics"},"prefix":{"total":{"status":"available","value":2},"included":1,"omitted":{"status":"available","value":1},"records":[{"name":"rule","severity":"warn","subject":"file","_graphs":["g"],"message":"m","target":"t","cause":"c","lines":[0,2]}]}});
    v["structured"]["hints"] = json!({"total":{"status":"available","value":1},"included":1,"omitted":{"status":"available","value":0},"records":[{"name":"hint","message":"m","locus":"l","next":"n"}]});
    assert!(matches!(
        state(&v, EventKind::Finish),
        RecordState::Supported { .. }
    ));
    for (path, value) in [
        ("/structured/findings/prefix/included", json!(2)),
        ("/structured/findings/prefix/total/value", json!(0)),
        (
            "/structured/findings/prefix/records/0/severity",
            json!("fatal"),
        ),
        ("/structured/findings/prefix/records/0/lines", json!([])),
        ("/structured/findings/prefix/records/0/target", json!(null)),
        ("/structured/hints/records/0/locus", json!(9)),
    ] {
        let mut bad = v.clone();
        *bad.pointer_mut(path).unwrap() = value;
        assert_eq!(
            state(&bad, EventKind::Finish),
            RecordState::Malformed,
            "{path}"
        );
    }
    v["structured"]["error"] = json!({"availability":"available","present":{"status":"available","value":true},"traversal":"formatter_failed","prefix":{"total":{"status":"unavailable","value":"traversal_stopped"},"included":1,"omitted":{"status":"unavailable","value":"traversal_stopped"},"records":["source"]}});
    assert!(matches!(
        state(&v, EventKind::Finish),
        RecordState::Supported { .. }
    ));
    v["structured"]["error"]["traversal"] = json!("complete");
    assert_eq!(state(&v, EventKind::Finish), RecordState::Malformed);
    v["structured"]["error"]["traversal"] = json!("depth_stopped");
    assert_eq!(state(&v, EventKind::Finish), RecordState::Malformed);
}
#[test]
fn stream_correlations_are_checked_without_inventing_caller_correlations() {
    for (path, value) in [
        ("/stdout/retained_bytes", json!(13)),
        ("/stdout/truncated", json!(true)),
        ("/stdout/writer_accepted_bytes/bytes/bytes", json!(13)),
        ("/stdout/os_accepted_bytes", json!("known")),
        ("/stderr/observed_input_bytes/bytes", json!(1)),
        (
            "/stdout/write_outcome",
            json!({"status":"unknown_acceptance","failed":false,"unfinished":false}),
        ),
        ("/stdout/prefix_base64", json!("%%%")),
    ] {
        let mut v = fixture(EventKind::Finish);
        *v.pointer_mut(path).unwrap() = value;
        assert_eq!(
            state(&v, EventKind::Finish),
            RecordState::Malformed,
            "{path}"
        );
    }
    let mut v = fixture(EventKind::Finish);
    v["completion"] = json!("stdout_write_failed");
    v["intended_exit"] = json!("clean");
    v["hint_observation"]["write_attempt"] = json!("attempted");
    v["hint_observation"]["suppression"] = json!("budget_refusal");
    v["budget_refusal"] = json!({"rendered_bytes":0,"budget_bytes":100});
    assert!(matches!(
        state(&v, EventKind::Finish),
        RecordState::Supported { .. }
    ));
    v["stdout"]["observed_input_bytes"] = json!({"status":"overflow"});
    v["stdout"]["writer_accepted_bytes"] = json!({"status":"known","bytes":{"status":"overflow"}});
    v["stdout"]["truncated"] = json!(true);
    assert!(matches!(
        state(&v, EventKind::Finish),
        RecordState::Supported { .. }
    ));
}

#[test]
fn structured_and_stream_caps_are_separate_from_event_cap() {
    let mut v = fixture(EventKind::Finish);
    v["structured"]["error"] = json!({
        "availability":"available", "present":{"status":"available","value":true},
        "traversal":"complete", "prefix":{
            "total":{"status":"available","value":1}, "included":1,
            "omitted":{"status":"available","value":0},
            "records":["a".repeat(STRUCTURED_PAYLOAD_LIMIT)]
        }
    });
    assert!(serde_json::to_vec(&v).unwrap().len() < EVENT_LIMIT);
    assert_eq!(state(&v, EventKind::Finish), RecordState::Malformed);
    for (key, limit) in [
        ("stdout", super::super::capture::STDOUT_LIMIT),
        ("stderr", super::super::capture::STDERR_LIMIT),
    ] {
        let mut v = fixture(EventKind::Finish);
        let n = limit + 1;
        v[key] = json!({
            "prefix_base64":STANDARD.encode(vec![b'a';n]),
            "retained_bytes":n, "observed_input_bytes":{"status":"exact","bytes":n},
            "truncated":false, "write_outcome":{"status":"all_succeeded"},
            "writer_accepted_bytes":{"status":"known","bytes":{"status":"exact","bytes":n}},
            "os_accepted_bytes":"unknown", "downstream_consumption":"unknown"
        });
        assert!(serde_json::to_vec(&v).unwrap().len() < EVENT_LIMIT);
        assert_eq!(state(&v, EventKind::Finish), RecordState::Malformed);
    }
}
