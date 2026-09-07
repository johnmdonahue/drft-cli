//! Bounded, independent revision-1 wire validation. Classification retains only
//! timestamps: supported shape is not evidence that supplied observations are true.
use super::bounded::{EVENT_LIMIT, STRUCTURED_PAYLOAD_LIMIT};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{
    Deserialize,
    de::{self, MapAccess, SeqAccess, Visitor},
};
use serde_json::{Map, Value};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    Start,
    Finish,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct WallTime {
    pub seconds: i64,
    pub nanoseconds: u32,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordState {
    Supported {
        entry: Option<WallTime>,
        collected: Option<WallTime>,
    },
    Malformed,
    Unsupported,
    Oversized,
}

// Value's normal deserializer silently replaces duplicate keys. This visitor
// checks every map, including future-revision payloads, under serde's depth cap.
struct Unique(Value);
impl<'de> Deserialize<'de> for Unique {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Unique;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("JSON without duplicate keys")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Unique, E> {
                serde_json::Number::from_f64(v)
                    .map(|n| Unique(Value::Number(n)))
                    .ok_or_else(|| E::custom("invalid number"))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Unique, E> {
                Ok(Unique(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Unique, A::Error> {
                let mut out = Vec::new();
                while let Some(Unique(v)) = a.next_element()? {
                    out.push(v);
                }
                Ok(Unique(Value::Array(out)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Unique, A::Error> {
                let mut out = Map::new();
                while let Some(k) = a.next_key::<String>()? {
                    if out.contains_key(&k) {
                        return Err(de::Error::custom("duplicate key"));
                    }
                    let Unique(v) = a.next_value()?;
                    out.insert(k, v);
                }
                Ok(Unique(Value::Object(out)))
            }
        }
        d.deserialize_any(V)
    }
}

pub fn classify(bytes: &[u8], expected_id: &str, expected_kind: EventKind) -> RecordState {
    if bytes.len() > EVENT_LIMIT {
        return RecordState::Oversized;
    }
    let Ok(Unique(v)) = serde_json::from_slice(bytes) else {
        return RecordState::Malformed;
    };
    let kind = match expected_kind {
        EventKind::Start => "start",
        EventKind::Finish => "finish",
    };
    if v["schema"] != "drft-usage"
        || v["event"] != kind
        || v["id"] != expected_id
        || !hex(expected_id, 32)
    {
        return RecordState::Malformed;
    }
    match v["revision"].as_u64() {
        Some(1) => {}
        Some(2..) => return RecordState::Unsupported,
        _ => return RecordState::Malformed,
    }
    let valid = match expected_kind {
        EventKind::Start => start(&v),
        EventKind::Finish => finish(&v),
    };
    if valid.is_none() {
        return RecordState::Malformed;
    }
    RecordState::Supported {
        entry: wall(&v["entry_wall_time"]).flatten(),
        collected: wall(&v["collected_wall_time"]).flatten(),
    }
}
fn require(b: bool) -> Option<()> {
    b.then_some(())
}
fn fields(v: &Value, required: &[&str], optional: &[&str]) -> Option<()> {
    let o = v.as_object()?;
    require(
        required.iter().all(|k| o.contains_key(*k))
            && o.keys()
                .all(|k| required.contains(&k.as_str()) || optional.contains(&k.as_str())),
    )
}
fn one(v: &Value, choices: &[&str]) -> Option<()> {
    require(choices.contains(&v.as_str()?))
}
fn hex(s: &str, n: usize) -> bool {
    s.len() == n
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn count(v: &Value) -> Option<()> {
    v.as_u64().map(|_| ())
}
fn text(v: &Value) -> Option<()> {
    v.as_str().map(|_| ())
}
fn reason(v: &Value) -> Option<()> {
    one(
        v,
        &[
            "not_evaluated",
            "execution_stopped",
            "not_observed",
            "requires_start",
            "publication_not_yet_performed",
            "traversal_stopped",
        ],
    )
}
fn available(v: &Value, check: fn(&Value) -> Option<()>) -> Option<()> {
    fields(v, &["status", "value"], &[])?;
    match v["status"].as_str()? {
        "available" => check(&v["value"]),
        "unavailable" => reason(&v["value"]),
        _ => None,
    }
}
fn is_available(v: &Value) -> bool {
    v["status"] == "available"
}
fn stamp(v: &Value, signed: bool) -> Option<()> {
    fields(v, &["seconds", "nanoseconds"], &[])?;
    require(if signed {
        v["seconds"].as_i64().is_some()
    } else {
        v["seconds"].as_u64().is_some()
    })?;
    require(v["nanoseconds"].as_u64()? < 1_000_000_000)
}
fn wall(v: &Value) -> Option<Option<WallTime>> {
    available(v, |v| stamp(v, true))?;
    if !is_available(v) {
        return Some(None);
    }
    Some(Some(WallTime {
        seconds: v["value"]["seconds"].as_i64()?,
        nanoseconds: v["value"]["nanoseconds"].as_u64()? as u32,
    }))
}
fn os(v: &Value) -> Option<()> {
    fields(v, &["os_encoding", "encoding", "value"], &[])?;
    one(&v["os_encoding"], &["unix-bytes", "windows-utf16le"])?;
    let s = v["value"].as_str()?;
    match v["encoding"].as_str()? {
        "utf8" => Some(()),
        "base64" => {
            let raw = STANDARD.decode(s).ok()?;
            if v["os_encoding"] == "unix-bytes" {
                require(std::str::from_utf8(&raw).is_err())
            } else {
                require(raw.len() % 2 == 0)?;
                require(
                    char::decode_utf16(
                        raw.chunks_exact(2)
                            .map(|p| u16::from_le_bytes([p[0], p[1]])),
                    )
                    .any(|c| c.is_err()),
                )
            }
        }
        _ => None,
    }
}
fn start(v: &Value) -> Option<()> {
    fields(
        v,
        &[
            "schema",
            "revision",
            "event",
            "id",
            "entry_wall_time",
            "collected_wall_time",
            "binary_version",
            "original_cwd",
            "effective_directory",
            "canonical_graph_root",
            "command",
            "requested_format",
            "argv",
            "config_fingerprint",
            "caller",
        ],
        &[],
    )?;
    wall(&v["entry_wall_time"])?;
    wall(&v["collected_wall_time"])?;
    text(&v["binary_version"])?;
    for key in [
        "original_cwd",
        "effective_directory",
        "canonical_graph_root",
    ] {
        os(&v[key])?;
    }
    one(
        &v["command"],
        &["check", "graph", "nodes", "edges", "impact", "lock"],
    )?;
    one(&v["requested_format"], &["text", "json"])?;
    for arg in v["argv"].as_array()? {
        os(arg)?;
    }
    require(hex(
        v["config_fingerprint"].as_str()?.strip_prefix("b3:")?,
        64,
    ))?;
    let c = &v["caller"];
    if !c.is_null() {
        fields(c, &["label", "authenticated", "unique_to_invocation"], &[])?;
        os(&c["label"])?;
        require(c["authenticated"] == false && c["unique_to_invocation"] == false)?;
    }
    Some(())
}
fn finding(v: &Value) -> Option<()> {
    fields(
        v,
        &["name", "severity", "subject", "_graphs", "message"],
        &["target", "lines", "cause"],
    )?;
    for key in ["name", "subject", "message"] {
        text(&v[key])?;
    }
    one(&v["severity"], &["error", "warn", "off"])?;
    for s in v["_graphs"].as_array()? {
        text(s)?;
    }
    for k in ["target", "cause"] {
        if let Some(x) = v.get(k) {
            text(x)?;
        }
    }
    if let Some(lines) = v.get("lines") {
        let a = lines.as_array()?;
        require(!a.is_empty())?;
        for n in a {
            count(n)?;
        }
    }
    Some(())
}
fn hint(v: &Value) -> Option<()> {
    fields(v, &["name", "message"], &["locus", "next"])?;
    text(&v["name"])?;
    text(&v["message"])?;
    for k in ["locus", "next"] {
        if let Some(x) = v.get(k) {
            text(x)?;
        }
    }
    Some(())
}
fn prefix(v: &Value, record: fn(&Value) -> Option<()>, unknown_records: bool) -> Option<()> {
    fields(v, &["total", "included", "omitted", "records"], &[])?;
    available(&v["total"], count)?;
    available(&v["omitted"], count)?;
    let n = v["included"].as_u64()?;
    let a = v["records"].as_array()?;
    require(n == a.len() as u64)?;
    if is_available(&v["total"]) {
        require(is_available(&v["omitted"]))?;
        require(n.checked_add(v["omitted"]["value"].as_u64()?) == v["total"]["value"].as_u64())?;
    } else {
        require(v["total"] == v["omitted"] && (unknown_records || n == 0))?;
    }
    for r in a {
        record(r)?;
    }
    Some(())
}
fn structured(v: &Value) -> Option<()> {
    fields(v, &["findings", "hints", "error"], &[])?;
    require(serde_json::to_vec(v).ok()?.len() <= STRUCTURED_PAYLOAD_LIMIT)?;
    let f = &v["findings"];
    fields(f, &["availability", "coverage", "prefix"], &[])?;
    available(&f["coverage"], |v| {
        one(
            v,
            &[
                "full_policy_filtered_evaluation",
                "construction_diagnostics",
                "selected_impact_diagnostics",
            ],
        )
    })?;
    prefix(&f["prefix"], finding, false)?;
    if is_available(&f["coverage"]) {
        require(f["availability"] == "available" && is_available(&f["prefix"]["total"]))?;
    } else {
        require(f["availability"] == "unavailable" && f["coverage"] == f["prefix"]["total"])?;
    }
    prefix(&v["hints"], hint, false)?;
    let e = &v["error"];
    fields(e, &["availability", "present", "traversal", "prefix"], &[])?;
    available(&e["present"], |v| v.as_bool().map(|_| ()))?;
    let p = &e["prefix"];
    prefix(p, text, true)?;
    let n = p["included"].as_u64()?;
    if !is_available(&e["present"]) {
        return require(
            e["availability"] == "unavailable"
                && e["traversal"] == "not_started"
                && e["present"] == p["total"]
                && n == 0,
        );
    }
    require(e["availability"] == "available")?;
    if e["present"]["value"] == false {
        return require(
            e["traversal"] == "complete" && is_available(&p["total"]) && p["total"]["value"] == 0,
        );
    }
    match e["traversal"].as_str()? {
        "complete" => {
            require(n > 0 && n < 64 && is_available(&p["total"]) && p["omitted"]["value"] == 0)
        }
        "budget_stopped" | "formatter_failed" | "depth_stopped" => {
            require(!is_available(&p["total"]) && p["total"]["value"] == "traversal_stopped")?;
            require(if e["traversal"] == "depth_stopped" {
                n == 64
            } else {
                n < 64
            })
        }
        _ => None,
    }
}
fn byte_count(v: &Value) -> Option<()> {
    match v["status"].as_str()? {
        "exact" => {
            fields(v, &["status", "bytes"], &[])?;
            count(&v["bytes"])
        }
        "overflow" => fields(v, &["status"], &[]),
        _ => None,
    }
}
fn stream(v: &Value, limit: usize) -> Option<()> {
    fields(
        v,
        &[
            "prefix_base64",
            "observed_input_bytes",
            "retained_bytes",
            "truncated",
            "write_outcome",
            "writer_accepted_bytes",
            "os_accepted_bytes",
            "downstream_consumption",
        ],
        &[],
    )?;
    let raw = STANDARD.decode(v["prefix_base64"].as_str()?).ok()?;
    require(raw.len() <= limit && v["retained_bytes"].as_u64()? == raw.len() as u64)?;
    let observed = &v["observed_input_bytes"];
    byte_count(observed)?;
    let truncated = v["truncated"].as_bool()?;
    if observed["status"] == "exact" {
        let n = observed["bytes"].as_u64()?;
        require(n >= raw.len() as u64 && truncated == (n > raw.len() as u64))?;
    } else {
        require(truncated)?;
    }
    require(v["os_accepted_bytes"] == "unknown" && v["downstream_consumption"] == "unknown")?;
    let w = &v["write_outcome"];
    let a = &v["writer_accepted_bytes"];
    match w["status"].as_str()? {
        "not_attempted" => {
            fields(w, &["status"], &[])?;
            fields(a, &["status"], &[])?;
            require(
                a["status"] == "not_attempted"
                    && observed["status"] == "exact"
                    && observed["bytes"] == 0,
            )
        }
        "all_succeeded" => {
            fields(w, &["status"], &[])?;
            fields(a, &["status", "bytes"], &[])?;
            require(a["status"] == "known" && a["bytes"] == *observed)
        }
        "unknown_acceptance" => {
            fields(w, &["status", "failed", "unfinished"], &[])?;
            let failed = w["failed"].as_bool()?;
            let unfinished = w["unfinished"].as_bool()?;
            fields(a, &["status"], &[])?;
            require((failed || unfinished) && a["status"] == "unknown")
        }
        _ => None,
    }
}
fn finish(v: &Value) -> Option<()> {
    fields(
        v,
        &[
            "schema",
            "revision",
            "event",
            "id",
            "entry_wall_time",
            "collected_wall_time",
            "elapsed",
            "collector_work_through_preparation",
            "finish_publication_duration",
            "comparison_requires_start",
            "intended_exit",
            "completion",
            "output_mode",
            "graph_sizes",
            "result_sizes",
            "hint_observation",
            "budget_refusal",
            "structured",
            "stdout",
            "stderr",
        ],
        &[],
    )?;
    wall(&v["entry_wall_time"])?;
    wall(&v["collected_wall_time"])?;
    for k in ["elapsed", "collector_work_through_preparation"] {
        available(&v[k], |v| stamp(v, false))?;
    }
    let p = &v["finish_publication_duration"];
    available(p, |v| stamp(v, false))?;
    require(
        !is_available(p)
            && p["value"] == "publication_not_yet_performed"
            && v["comparison_requires_start"] == true,
    )?;
    one(&v["intended_exit"], &["clean", "violations", "usage_error"])?;
    one(
        &v["completion"],
        &[
            "returned",
            "command_error",
            "output_budget_refused",
            "stdout_write_failed",
        ],
    )?;
    one(
        &v["output_mode"],
        &["text", "json", "bare_jgf", "raw_graph_set", "no_document"],
    )?;
    available(&v["graph_sizes"], |v| {
        fields(v, &["graphs", "nodes", "edges"], &[])?;
        count(&v["graphs"])?;
        count(&v["nodes"])?;
        count(&v["edges"])
    })?;
    available(&v["result_sizes"], |v| {
        fields(v, &["nodes", "edges", "findings"], &[])?;
        for k in ["nodes", "edges", "findings"] {
            available(&v[k], count)?;
        }
        Some(())
    })?;
    let h = &v["hint_observation"];
    fields(
        h,
        &[
            "embedding",
            "route",
            "suppression",
            "write_attempt",
            "writer_acceptance",
            "os_acceptance",
            "downstream_consumption",
        ],
        &[],
    )?;
    one(&h["embedding"], &["not_embedded", "result_document"])?;
    one(
        &h["route"],
        &["none", "stdout_document", "stderr_text", "stderr_json"],
    )?;
    one(
        &h["suppression"],
        &[
            "none",
            "explicit",
            "budget_refusal",
            "earlier_write_failure",
        ],
    )?;
    one(
        &h["write_attempt"],
        &["not_attempted", "attempted", "unavailable"],
    )?;
    for k in [
        "writer_acceptance",
        "os_acceptance",
        "downstream_consumption",
    ] {
        require(h[k] == "unknown")?;
    }
    let b = &v["budget_refusal"];
    if !b.is_null() {
        fields(b, &["rendered_bytes", "budget_bytes"], &[])?;
        count(&b["rendered_bytes"])?;
        count(&b["budget_bytes"])?;
    }
    structured(&v["structured"])?;
    stream(&v["stdout"], super::capture::STDOUT_LIMIT)?;
    stream(&v["stderr"], super::capture::STDERR_LIMIT)
}

#[cfg(test)]
mod tests;
