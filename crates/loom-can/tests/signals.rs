//! M2a signals without a daemon: DBC load/info and caps, decoded trace rows, `sig.get`,
//! `sig.wait_for`, condition parsing and the `can.signal_range` verifier.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use loom_can::signals::{KIND_DB_CHANGED, Op, PRED_SIGNAL_RANGE, parse_condition};
use loom_can::{RPC_DB_INFO, RPC_DB_LOAD, RPC_LAST, RPC_SIG_GET, RPC_SIG_WAIT_FOR, RPC_TRACE_QUERY};
use loom_domain_api::{Coverage, EvalBudget, Predicate, Verification};
use loom_proto::error::ErrorCode;
use loom_proto::event::{Origin, PublishedRecord, SeqRange};
use loom_proto::num::U64;
use loom_proto::rpc::Role;
use serde_json::{Value, json};

mod common;
use common::*;

fn fixture(name: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../loom-can-dbc/tests/fixtures")
        .join(name);
    std::path::absolute(p).unwrap().to_str().unwrap().to_owned()
}

/// A small DBC file in the test temp dir; `tag` makes its hash unique.
fn temp_dbc(name: &str, body: &str) -> String {
    let p = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("loom-can-signals-{name}.dbc"));
    std::fs::write(&p, body).unwrap();
    p.to_str().unwrap().to_owned()
}

fn variant(tag: u32) -> String {
    temp_dbc(
        &format!("v{tag}"),
        &format!(
            "VERSION \"v{tag}\"\n\nBU_: ECU\n\nBO_ {} M{tag}: 8 ECU\n SG_ S{tag} : 0|8@1+ (1,0) [0|255] \"\" ECU\n",
            0x200 + tag
        ),
    )
}

/// EngineData (0x100): EngRPM = raw × 0.25 (bytes 0–1, LE), EngTemp = -10, Gear in bits 25..27.
fn engine(rpm: f64, gear: u8) -> String {
    let raw = (rpm / 0.25) as u16;
    format!(
        "100#{:02X}{:02X}F6{:02X}00000000",
        raw & 0xFF,
        raw >> 8,
        gear << 1
    )
}

fn load(env: &Env_, ch: &str, path: &str) -> Result<Value, loom_domain_api::DomainError> {
    env.call(RPC_DB_LOAD, Role::Agent, json!({"channel": ch, "path": path}))
}

fn loaded() -> Env_ {
    let env = setup();
    load(&env, "ch0", &fixture("basic.dbc")).unwrap();
    env.published();
    env
}

fn push(env: &Env_, text: &str, origin: Origin) -> u64 {
    env.svc.journal.push(frame(text, origin, "ch0"))
}

// ---- db.load / db.info ------------------------------------------------------------------------

#[test]
fn db_load_info_and_change_events() {
    let env = setup();
    let d = load(&env, "ch0", &fixture("basic.dbc")).unwrap();
    assert_eq!(d["loaded"], json!(true));
    assert_eq!(d["messages"], json!(4));
    assert_eq!(d["signals"], json!(10));
    assert_eq!(d["changed"], json!(true));
    assert_eq!(d["previous_sha256"], Value::Null);
    let sha = d["db_sha256"].as_str().unwrap().to_owned();
    assert_eq!(sha.len(), 64);
    let ev = env.published();
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0].kind, KIND_DB_CHANGED);
    assert_eq!(ev[0].channel.as_deref(), Some("ch0"));
    assert_eq!(ev[0].payload["db_sha256"], json!(sha));

    // Same file again: no change event.
    let d = load(&env, "ch0", &fixture("basic.dbc")).unwrap();
    assert_eq!(d["changed"], json!(false));
    assert_eq!(d["previous_sha256"], json!(sha));
    assert!(env.published().is_empty());

    // Unparsable statements are warnings (listed ≤ 20), not a failed load.
    let d = load(&env, "ch1", &fixture("broken.dbc")).unwrap();
    assert!(d["warnings_total"].as_u64().unwrap() >= 1);
    assert!(d["warnings"].as_array().unwrap().len() <= 20);
    let ws = d["warnings"].as_array().unwrap();
    assert!(
        ws.iter()
            .any(|w| w["kind"] == json!("StatementDropped") && w["line"].is_u64()),
        "{ws:?}"
    );

    let info = env
        .call(RPC_DB_INFO, Role::Agent, json!({"channel": "ch0"}))
        .unwrap();
    assert_eq!(info["channels"][0]["db_sha256"], json!(sha));
    let none = env
        .call(RPC_DB_INFO, Role::Agent, json!({"channel": "ch5"}))
        .unwrap();
    assert_eq!(none["channels"], json!([{"channel": "ch5", "loaded": false}]));
    let all = env.call(RPC_DB_INFO, Role::Agent, json!({})).unwrap();
    assert_eq!(all["channels"].as_array().unwrap().len(), 2);
}

#[test]
fn db_load_errors() {
    let env = setup();
    let code = |params: Value| env.call(RPC_DB_LOAD, Role::Agent, params).unwrap_err().code;
    assert_eq!(code(json!({"path": "relative/body.dbc"})), ErrorCode::Usage);
    assert_eq!(
        code(json!({"path": fixture("basic.dbc"), "encoding": "ebcdic"})),
        ErrorCode::Usage
    );
    assert_eq!(code(json!({"path": fixture("missing.dbc")})), ErrorCode::Db);
    assert_eq!(
        code(json!({"path": env!("CARGO_TARGET_TMPDIR")})),
        ErrorCode::Db,
        "a directory"
    );
    let junk = temp_dbc("junk", "this is not a DBC file at all {{{\n");
    assert_eq!(code(json!({"path": junk})), ErrorCode::Db);

    // Over the file size limit: refused from the metadata, before reading.
    let big = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("loom-can-signals-big.dbc");
    std::fs::File::create(&big)
        .unwrap()
        .set_len(16 * 1024 * 1024 + 1)
        .unwrap();
    let e = env
        .call(RPC_DB_LOAD, Role::Agent, json!({"path": big.to_str().unwrap()}))
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Db);
    assert_eq!(e.detail["limit"], json!("file size (bytes)"));
    assert!(env.published().is_empty(), "failed loads change nothing");
    let info = env.call(RPC_DB_INFO, Role::Agent, json!({})).unwrap();
    assert_eq!(info["channels"], json!([]));
}

#[test]
fn db_caps_channels_and_distinct_files() {
    let env = setup();
    for i in 0..4 {
        load(&env, &format!("ch{i}"), &variant(i)).unwrap();
    }
    let e = load(&env, "ch4", &variant(4)).unwrap_err();
    assert_eq!(e.code, ErrorCode::Db, "a 5th different file");
    // Replacing a channel's only copy keeps the count at 4.
    load(&env, "ch0", &variant(4)).unwrap();
    // A file that is already loaded does not count again.
    for i in 4..8 {
        load(&env, &format!("ch{i}"), &variant(1)).unwrap();
    }
    let e = load(&env, "ch8", &variant(1)).unwrap_err();
    assert_eq!(e.code, ErrorCode::Db, "a 9th channel");
}

// ---- decoded rows --------------------------------------------------------------------------------

#[test]
fn trace_and_last_decode_on_request_only() {
    let env = loaded();
    push(&env, &engine(3200.0, 2), Origin::Device);
    push(&env, "123#01", Origin::Device); // not in the DBC
    push(&env, "18FEF100#0040250000000000", Origin::Device); // J1939 (extended)
    push(&env, "100#00", Origin::Device); // shorter than the DBC length

    let d = env
        .call(
            RPC_TRACE_QUERY,
            Role::Agent,
            json!({"channel": "ch0", "since": "seq:1", "decode": true}),
        )
        .unwrap();
    let rows = d["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 4);
    let dec = &rows[0]["decoded"];
    assert_eq!(dec["message"], json!("EngineData"));
    let sig = |name: &str| {
        dec["signals"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == json!(name))
            .unwrap()
            .clone()
    };
    assert_eq!(
        sig("EngRPM"),
        json!({"name": "EngRPM", "value": 3200.0, "unit": "rpm", "raw": 12800})
    );
    assert_eq!(sig("EngTemp")["value"], json!(-10.0));
    assert_eq!(sig("Gear")["label"], json!("N"));
    assert!(rows[1].get("decoded").is_none(), "unknown id: no decoded field");
    assert_eq!(rows[2]["decoded"]["signals"][0]["value"], json!(37.25));
    assert_eq!(
        rows[3]["decoded"]["length_mismatch"],
        json!({"expected": 8, "actual": 1})
    );
    assert!(
        rows[3]["decoded"]["signals"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["unavailable"] == json!("short_frame"))
    );
    assert_eq!(d["db_sha256"].as_str().unwrap().len(), 64);

    // Off by default, and the stored events never change.
    let plain = env
        .call(
            RPC_TRACE_QUERY,
            Role::Agent,
            json!({"channel": "ch0", "since": "seq:1"}),
        )
        .unwrap();
    assert!(plain["rows"][0].get("decoded").is_none());
    assert!(
        env.svc
            .journal
            .events
            .lock()
            .unwrap()
            .iter()
            .all(|e| e.payload.get("decoded").is_none())
    );

    let last = env
        .call(
            RPC_LAST,
            Role::Agent,
            json!({"channel": "ch0", "id": "0x100", "decode": true}),
        )
        .unwrap();
    assert_eq!(last["rows"][0]["last"]["decoded"]["message"], json!("EngineData"));

    // Decoding without a DBC on the channel is an error that says how to load one.
    let e = env
        .call(
            RPC_TRACE_QUERY,
            Role::Agent,
            json!({"channel": "ch3", "decode": true}),
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Db);
    assert!(e.detail["hint"].as_str().unwrap().contains("loom can db load"));
}

// ---- sig.get -----------------------------------------------------------------------------------------

#[test]
fn sig_get_returns_newest_value_with_evidence() {
    let env = loaded();
    push(&env, &engine(1000.0, 1), Origin::Device);
    push(&env, &engine(2000.0, 3), Origin::Device);
    let seq = push(&env, &engine(3200.0, 2), Origin::Device);
    push(&env, "123#01", Origin::Device);

    let d = env
        .call(
            RPC_SIG_GET,
            Role::Agent,
            json!({"channel": "ch0", "signal": "EngRPM"}),
        )
        .unwrap();
    assert_eq!(
        (d["value"].clone(), d["unit"].clone(), d["raw"].clone()),
        (json!(3200.0), json!("rpm"), json!(12800))
    );
    assert_eq!(d["signal"], json!("EngineData.EngRPM"));
    assert_eq!(
        (d["message"].clone(), d["id"].clone()),
        (json!("EngineData"), json!("0x100"))
    );
    assert!(d["age_ms"].as_u64().unwrap() < 5_000);
    assert_eq!(d["frame_ref"], json!(format!("can:ch0@e1:seq:{seq}")));
    assert_eq!(
        d["refs"],
        json!([format!("can:ch0@e1:seq:{seq}#/signals/EngRPM")])
    );
    let ev = &d["evidence"][0];
    assert_eq!(ev["kind"], json!("can.signal"));
    assert_eq!(ev["pointer"], json!("/signals/EngRPM"));
    assert_eq!(ev["observed"]["value"], json!(3200.0));
    assert_eq!(ev["observed"]["db_sha256"], d["db_sha256"]);
    assert_eq!(
        ev["ref"]["seq_range"],
        json!({"from": seq.to_string(), "to": seq.to_string()})
    );

    let gear = env
        .call(RPC_SIG_GET, Role::Agent, json!({"signal": "EngineData.Gear"}))
        .unwrap();
    assert_eq!(
        (gear["value"].clone(), gear["label"].clone()),
        (json!(2.0), json!("N"))
    );

    // Origin filter.
    push(&env, &engine(5000.0, 0), Origin::Agent);
    let any = env
        .call(RPC_SIG_GET, Role::Agent, json!({"signal": "EngRPM"}))
        .unwrap();
    assert_eq!(any["value"], json!(5000.0));
    let dev = env
        .call(
            RPC_SIG_GET,
            Role::Agent,
            json!({"signal": "EngRPM", "origin": "device"}),
        )
        .unwrap();
    assert_eq!(dev["value"], json!(3200.0));
}

#[test]
fn sig_get_errors_and_window() {
    let env = loaded();
    let call = |p: Value| env.call(RPC_SIG_GET, Role::Agent, p).unwrap_err();
    let e = call(json!({"signal": "EngRPM"}));
    assert_eq!(e.code, ErrorCode::NoMatch);
    assert_eq!(e.code.exit_code(), 3);
    assert_eq!(call(json!({"signal": "Nope"})).code, ErrorCode::Usage);
    assert_eq!(call(json!({"signal": "EngineData.Nope"})).code, ErrorCode::Usage);
    assert_eq!(
        call(json!({"signal": "EngRPM", "window_ms": 60_001})).code,
        ErrorCode::Usage
    );
    assert_eq!(
        call(json!({"channel": "ch2", "signal": "EngRPM"})).code,
        ErrorCode::Db
    );

    // A value older than the window is not "current".
    push(&env, &engine(1000.0, 1), Origin::Device);
    thread::sleep(Duration::from_millis(40));
    assert_eq!(
        call(json!({"signal": "EngRPM", "window_ms": 10})).code,
        ErrorCode::NoMatch
    );
    env.call(
        RPC_SIG_GET,
        Role::Agent,
        json!({"signal": "EngRPM", "window_ms": 10_000}),
    )
    .unwrap();

    // A name in several messages needs Message.Signal.
    let amb = temp_dbc(
        "amb",
        "VERSION \"\"\n\nBU_: ECU\n\nBO_ 1 A: 8 ECU\n SG_ Speed : 0|8@1+ (1,0) [0|255] \"\" ECU\n\n\
         BO_ 2 B: 8 ECU\n SG_ Speed : 0|8@1+ (1,0) [0|255] \"\" ECU\n",
    );
    load(&env, "ch1", &amb).unwrap();
    let e = env
        .call(
            RPC_SIG_GET,
            Role::Agent,
            json!({"channel": "ch1", "signal": "Speed"}),
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Usage);
    assert!(
        e.message.contains("A.Speed") && e.message.contains("B.Speed"),
        "{}",
        e.message
    );
}

// ---- sig.wait_for ---------------------------------------------------------------------------------------

fn later(env: &Env_, delay_ms: u64, frames: Vec<(String, Origin)>) -> thread::JoinHandle<()> {
    let j = env.svc.journal.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(delay_ms));
        for (f, o) in frames {
            j.push(frame(&f, o, "ch0"));
        }
    })
}

#[test]
fn wait_for_returns_first_new_sample_that_holds() {
    let env = loaded();
    push(&env, &engine(4000.0, 3), Origin::Device); // already true, but old: must not count
    thread::sleep(Duration::from_millis(5));
    let t = later(
        &env,
        30,
        vec![
            (engine(1000.0, 3), Origin::Device),
            (engine(9000.0, 3), Origin::Agent), // own frame: not the device's value
            (engine(2000.0, 3), Origin::Device),
            (engine(3500.0, 3), Origin::Device),
        ],
    );
    let started = Instant::now();
    let d = env
        .call(
            RPC_SIG_WAIT_FOR,
            Role::Agent,
            json!({"channel": "ch0", "expr": "EngRPM > 3000", "within_ms": 3000}),
        )
        .unwrap();
    t.join().unwrap();
    assert!(started.elapsed() < Duration::from_millis(2_000));
    assert_eq!(d["matched"], json!(true));
    assert_eq!(d["value"], json!(3500.0));
    assert_eq!(d["expr"], json!("EngineData.EngRPM > 3000"));
    assert_eq!(d["samples_before"], json!(2));
    assert_eq!(d["previous"]["value"], json!(2000.0));
    assert_eq!(d["seq"], json!("5"));
    assert_eq!(d["refs"], json!(["can:ch0@e1:seq:5#/signals/EngRPM"]));
    let args = &d["predicate"]["args"];
    assert_eq!(d["predicate"]["name"], json!(PRED_SIGNAL_RANGE));
    assert_eq!(args["op"], json!(">"));
    assert_eq!(args["origin"], json!("device"));
    assert_eq!(d["evidence"][0]["expected"], *args);

    let acts = env.published();
    assert_eq!(acts.len(), 1);
    assert_eq!(acts[0].payload["action"], json!("wait"));
    assert_eq!(acts[0].payload["waiting"]["predicate"], json!(PRED_SIGNAL_RANGE));
}

#[test]
fn wait_for_between_after_cursor_and_any_origin() {
    let env = loaded();
    let s1 = push(&env, &engine(1000.0, 0), Origin::Device);
    push(&env, &engine(2000.0, 0), Origin::Device);
    // An explicit cursor reads from there, so older-but-after samples count at once.
    let d = env
        .call(
            RPC_SIG_WAIT_FOR,
            Role::Agent,
            json!({"expr": "EngineData.EngRPM between 1500 2500", "after": format!("seq:{s1}"), "within_ms": 500}),
        )
        .unwrap();
    assert_eq!(d["value"], json!(2000.0));
    assert_eq!(d["predicate"]["args"]["between"], json!([1500.0, 2500.0]));

    let t = later(&env, 20, vec![(engine(7000.0, 0), Origin::Agent)]);
    let d = env
        .call(
            RPC_SIG_WAIT_FOR,
            Role::Agent,
            json!({"expr": "EngRPM >= 7000", "any_origin": true, "within_ms": 2000}),
        )
        .unwrap();
    t.join().unwrap();
    assert_eq!(d["origin"], json!("agent"));
    assert!(d["predicate"]["args"].get("origin").is_none());
}

#[test]
fn wait_for_timeout_cites_only_seen_samples() {
    let env = loaded();
    let t = later(
        &env,
        10,
        vec![
            (engine(1000.0, 0), Origin::Device),
            (engine(1200.0, 0), Origin::Device),
        ],
    );
    let e = env
        .call(
            RPC_SIG_WAIT_FOR,
            Role::Agent,
            json!({"expr": "EngRPM > 3000", "within_ms": 120}),
        )
        .unwrap_err();
    t.join().unwrap();
    assert_eq!(e.code, ErrorCode::Timeout);
    assert_eq!(e.code.exit_code(), 3);
    assert_eq!(e.detail["samples"], json!(2));
    assert_eq!(e.detail["last"]["value"], json!(1200.0));
    assert_eq!(e.detail["last"]["ref"], json!("can:ch0@e1:seq:2#/signals/EngRPM"));
    assert_eq!(e.detail["refs"], json!(["can:ch0@e1:seq:1..2#/signals/EngRPM"]));
    assert_eq!(e.detail["evidence"][0]["kind"], json!("can.signal_wait_timeout"));

    // Nothing seen: nothing cited.
    let e = env
        .call(
            RPC_SIG_WAIT_FOR,
            Role::Agent,
            json!({"expr": "EngRPM > 3000", "within_ms": 20}),
        )
        .unwrap_err();
    assert_eq!(e.detail["samples"], json!(0));
    assert_eq!(e.detail["evidence"], json!([]));
    assert!(e.detail.get("last").is_none());

    for bad in [
        json!({"expr": "EngRPM >", "within_ms": 10}),
        json!({"expr": "EngRPM = 5", "within_ms": 10}),
        json!({"expr": "Nope > 5", "within_ms": 10}),
        json!({"expr": "EngRPM > 5", "within_ms": 0}),
        json!({"expr": "EngRPM > 5", "within_ms": 3_600_001}),
    ] {
        assert_eq!(
            env.call(RPC_SIG_WAIT_FOR, Role::Agent, bad.clone())
                .unwrap_err()
                .code,
            ErrorCode::Usage,
            "{bad}"
        );
    }
}

#[test]
fn condition_parser() {
    let c = parse_condition("EngRPM > 3000").unwrap();
    assert_eq!((c.signal.as_str(), c.op, c.value), ("EngRPM", Op::Gt, 3000.0));
    let c = parse_condition("  EngineData.EngRPM>=-1.5e2 ").unwrap();
    assert_eq!(
        (c.signal.as_str(), c.op, c.value),
        ("EngineData.EngRPM", Op::Ge, -150.0)
    );
    for (e, op) in [
        ("A<1", Op::Lt),
        ("A <= 1", Op::Le),
        ("A == 1", Op::Eq),
        ("A != 0x10", Op::Ne),
    ] {
        assert_eq!(parse_condition(e).unwrap().op, op, "{e}");
    }
    assert_eq!(parse_condition("A != 0x10").unwrap().value, 16.0);
    let b = parse_condition("Volt between 11.5 and 14.5").unwrap();
    assert_eq!((b.op, b.value, b.hi), (Op::Between, 11.5, 14.5));
    assert!(b.holds(11.5) && b.holds(14.5) && !b.holds(14.6));
    assert!(!parse_condition("A > 1").unwrap().holds(f64::NAN));
    for bad in [
        "",
        "> 3",
        "1A > 3",
        "A.B.C > 1",
        "A. > 1",
        "A = 3",
        "A => 3",
        "A > ",
        "A > 3 4",
        "A > nan",
        "A > inf",
        "A between 5 1",
        "A between 1",
        "A betweenx 1 2",
        "A > 1; rm -rf /",
    ] {
        assert!(parse_condition(bad).is_err(), "{bad:?}");
    }
    assert!(parse_condition(&format!("A > {}", "1".repeat(300))).is_err());
}

// ---- verifier ------------------------------------------------------------------------------------------

fn seqd(env: &Env_) -> Vec<PublishedRecord> {
    env.svc.journal.events.lock().unwrap().clone()
}

#[test]
fn verifier_signal_range() {
    let env = loaded();
    let sha = env
        .call(RPC_DB_INFO, Role::Agent, json!({"channel": "ch0"}))
        .unwrap()["channels"][0]["db_sha256"]
        .as_str()
        .unwrap()
        .to_owned();
    push(&env, &engine(1000.0, 0), Origin::Device);
    push(&env, &engine(3200.0, 0), Origin::Agent);
    push(&env, &engine(2500.0, 0), Origin::Device);
    let events = seqd(&env);
    let v = env.reg.evidence_verifiers()[0].clone();
    let cov = |complete: bool| Coverage {
        window: SeqRange {
            from: U64(1),
            to: U64(3),
        },
        complete,
        causes: if complete {
            vec![]
        } else {
            vec!["record_gap".into()]
        },
    };
    let check = |args: Value, complete: bool| {
        let p = Predicate {
            name: PRED_SIGNAL_RANGE.into(),
            args,
            expect_id: None,
        };
        v.verify(&p, &events, &cov(complete), &mut EvalBudget::default())
    };

    match check(
        json!({"signal": "EngRPM", "op": ">", "value": 2000, "db_sha256": sha}),
        true,
    ) {
        Verification::Match { matched, observed } => {
            assert_eq!(matched[0].seq, U64(2));
            assert_eq!(observed["value"], json!(3200.0));
        }
        other => panic!("{other:?}"),
    }
    // Origin pins it to device frames; tolerance widens equality.
    match check(
        json!({"signal": "EngRPM", "op": ">", "value": 2000, "origin": "device", "db_sha256": sha}),
        true,
    ) {
        Verification::Match { matched, .. } => assert_eq!(matched[0].seq, U64(3)),
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        check(
            json!({"signal": "EngRPM", "op": "==", "value": 2499.9, "tol": 0.25, "db_sha256": sha}),
            true
        ),
        Verification::Match { .. }
    ));
    assert!(matches!(
        check(
            json!({"signal": "EngineData.EngRPM", "between": [5000, 6000], "db_sha256": sha}),
            true
        ),
        Verification::Mismatch { .. }
    ));
    assert!(matches!(
        check(
            json!({"signal": "EngRPM", "between": [5000, 6000], "db_sha256": sha}),
            false
        ),
        Verification::Inconclusive { .. }
    ));
    // The claim was made with another DBC: never judged with this one.
    match check(
        json!({"signal": "EngRPM", "op": ">", "value": 0, "db_sha256": "00".repeat(32)}),
        true,
    ) {
        Verification::Inconclusive { reason } => assert_eq!(reason, "db_changed"),
        other => panic!("{other:?}"),
    }
    for bad in [
        json!({"signal": "EngRPM", "op": ">", "value": 0}),
        json!({"signal": "EngRPM", "op": "~", "value": 0, "db_sha256": sha}),
        json!({"signal": "EngRPM", "between": [5, 1], "db_sha256": sha}),
        json!({"signal": "EngRPM", "op": ">", "value": 0, "tol": -1, "db_sha256": sha}),
        json!({"signal": "Nope", "op": ">", "value": 0, "db_sha256": sha}),
    ] {
        assert!(
            matches!(check(bad.clone(), true), Verification::Inconclusive { .. }),
            "{bad}"
        );
    }
}

// ---- periodic start from the DBC, IG `sig set` ---------------------------------------------------

#[test]
fn periodic_start_from_dbc_message() {
    let env = loaded();
    let d = env
        .call(
            "can.periodic.start",
            Role::Agent,
            json!({"msg": "EngineData", "signals": {"EngRPM": 3000, "Gear": "D", "Volt": 12}}),
        )
        .unwrap();
    let op = env.svc.worker.writes.lock().unwrap()[0].1.clone();
    assert_eq!(op["method"], json!("can.periodic.start"));
    assert_eq!(op["frame"]["id"], json!(0x100));
    // EngRPM 3000/0.25 = 0x2EE0, EngTemp start −10, Gear D = 3, Volt (12+1.5)/0.01 = 0x0546 (Motorola).
    assert_eq!(op["frame"]["bytes"]["data"], json!("e02ef60600000546"));
    assert_eq!(op["period_ms"], json!(10), "GenMsgCycleTime of EngineData");
    assert_eq!(d["period_from_dbc"], json!(true));
    assert_eq!(d["msg"], json!("EngineData"));
    assert_eq!(d["db_sha256"].as_str().unwrap().len(), 64);

    let d = env
        .call(
            "can.periodic.start",
            Role::Agent,
            json!({"msg": "EngineData", "signals": {"Volt": 0}, "period_ms": 50}),
        )
        .unwrap();
    assert_eq!(
        (d["period_ms"].clone(), d["period_from_dbc"].clone()),
        (json!(50), json!(false))
    );
    // Outside the DBC range only with force_range (the raw value must still fit).
    let over = json!({"msg": "EngineData", "signals": {"Volt": 0, "EngRPM": 16_001}});
    let e = env
        .call("can.periodic.start", Role::Agent, over.clone())
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Usage);
    assert!(e.message.contains("range"), "{}", e.message);
    let mut forced = over;
    forced["force_range"] = json!(true);
    env.call("can.periodic.start", Role::Agent, forced).unwrap();

    for (bad, what) in [
        (json!({"msg": "EngineData"}), "Volt has no start value"),
        (
            json!({"msg": "Wide", "signals": {"U64": 1}}),
            "no GenMsgCycleTime",
        ),
        (json!({"msg": "Nope", "period_ms": 10}), "unknown message"),
        (
            json!({"msg": "EngineData", "signals": {"Volt": 0, "Gear": "X"}}),
            "unknown label",
        ),
        (
            json!({"msg": "EngineData", "signals": {"Volt": 0, "Nope": 1}}),
            "unknown signal",
        ),
    ] {
        assert_eq!(
            env.call("can.periodic.start", Role::Agent, bad).unwrap_err().code,
            ErrorCode::Usage,
            "{what}"
        );
    }
    let e = env
        .call(
            "can.periodic.start",
            Role::Agent,
            json!({"channel": "ch2", "msg": "EngineData"}),
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Db, "no DBC on ch2");
    assert_eq!(env.svc.worker.writes.lock().unwrap().len(), 3);
}

/// Worker that lists job `j1` for EngineData (revisions from `revs`) and answers updates with `writes`.
fn ig_worker(env: &Env_, data: Option<&str>, revs: Vec<u64>, writes: Vec<Result<u64, u64>>) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let (lists, ups) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let data = data.map(str::to_owned);
    *env.svc.worker.call_reply.lock().unwrap() = Some(Box::new(move |m, _| {
        assert_eq!(m, "can.periodic.list");
        let i = lists.fetch_add(1, Ordering::SeqCst).min(revs.len() - 1);
        let mut j = json!({"job": "j1", "channel": "ch0", "id": 256, "ext": false, "revision": revs[i],
                           "period_ms": 10, "state": "running"});
        if let Some(d) = &data {
            j["data"] = json!(d);
        }
        Ok(json!({"jobs": [{"job": "j9", "channel": "ch0", "id": 0x123, "ext": false, "revision": 0}, j]}))
    }));
    *env.svc.worker.write_reply.lock().unwrap() = Some(Box::new(move |_, op| {
        assert_eq!(op["method"], json!("can.periodic.update"));
        let i = ups.fetch_add(1, Ordering::SeqCst).min(writes.len() - 1);
        match writes[i] {
            Ok(rev) => Ok(json!({"job": "j1", "revision": rev})),
            Err(current) => Err(
                loom_domain_api::DomainError::new(ErrorCode::Locked, "revision changed")
                    .with_detail(json!({"current_rev": current})),
            ),
        }
    }));
}

#[test]
fn sig_set_patches_the_running_job() {
    let env = loaded();
    ig_worker(&env, Some("803ef60400000546"), vec![3], vec![Ok(4)]);
    let d = env
        .call(
            "can.sig.set",
            Role::Agent,
            json!({"signal": "EngineData.Gear", "value": "D"}),
        )
        .unwrap();
    let (rid, op) = env.svc.worker.writes.lock().unwrap()[0].clone();
    assert_eq!(rid, "r1");
    assert_eq!(
        op,
        json!({"method": "can.periodic.update", "job": "j1", "expected_rev": 3,
               "patch": {"data": "803ef60600000546"}}),
        "only Gear changes; other signals keep the job's bytes"
    );
    assert_eq!((d["job"].clone(), d["revision"].clone()), (json!("j1"), json!(4)));
    assert_eq!(d["changed"], json!([{"byte": 3, "old": "04", "new": "06"}]));
    assert_eq!(
        (d["base"].clone(), d["risk"].clone()),
        (json!("job"), json!("R1"))
    );
    assert_eq!(d["signal"], json!("EngineData.Gear"));
    assert!(d.get("warning").is_none());

    // Numbers are physical values; the range is checked; unknown signals are usage errors.
    env.call(
        "can.sig.set",
        Role::Agent,
        json!({"signal": "EngRPM", "value": 3000}),
    )
    .unwrap();
    let op = env.svc.worker.writes.lock().unwrap()[1].1.clone();
    assert_eq!(op["patch"]["data"], json!("e02ef60400000546"));
    for bad in [
        json!({"signal": "EngRPM", "value": 99_999}),
        json!({"signal": "EngineData.Nope", "value": 1}),
        json!({"signal": "Gear", "value": "X"}),
        json!({"signal": "Gear", "value": true}),
    ] {
        assert_eq!(
            env.call("can.sig.set", Role::Agent, bad.clone())
                .unwrap_err()
                .code,
            ErrorCode::Usage,
            "{bad}"
        );
    }
    // A job given explicitly must send that message.
    let e = env
        .call(
            "can.sig.set",
            Role::Agent,
            json!({"signal": "Gear", "value": 1, "job": "j9"}),
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Usage);
}

#[test]
fn sig_set_retries_one_conflict() {
    let env = loaded();
    ig_worker(&env, Some("803ef60400000546"), vec![3, 4], vec![Err(4), Ok(5)]);
    let d = env
        .call("can.sig.set", Role::Agent, json!({"signal": "Gear", "value": 1}))
        .unwrap();
    let writes = env.svc.worker.writes.lock().unwrap().clone();
    assert_eq!(writes.len(), 2);
    assert_eq!(writes[0].1["expected_rev"], json!(3));
    assert_eq!(writes[1].1["expected_rev"], json!(4), "re-read before the retry");
    assert_eq!(
        (d["revision"].clone(), d["previous_revision"].clone()),
        (json!(5), json!(4))
    );

    // Two conflicts in a row: the worker's error is returned.
    let env = loaded();
    ig_worker(
        &env,
        Some("803ef60400000546"),
        vec![3, 4, 5],
        vec![Err(4), Err(5), Ok(9)],
    );
    let e = env
        .call("can.sig.set", Role::Agent, json!({"signal": "Gear", "value": 1}))
        .unwrap_err();
    assert_eq!(e.detail["current_rev"], json!(5));
    assert_eq!(env.svc.worker.writes.lock().unwrap().len(), 2, "one retry only");
}

#[test]
fn sig_set_without_job_or_bytes() {
    let env = loaded();
    // No job for the message.
    *env.svc.worker.call_reply.lock().unwrap() = Some(Box::new(|_, _| Ok(json!({"jobs": []}))));
    let e = env
        .call("can.sig.set", Role::Agent, json!({"signal": "Gear", "value": 1}))
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::NotFound);
    assert!(
        e.detail["hint"]
            .as_str()
            .unwrap()
            .contains("periodic start --msg EngineData")
    );

    // `list` without bytes: fall back to the job's last frame in the journal, with a warning.
    let env = loaded();
    ig_worker(&env, None, vec![3], vec![Ok(4)]);
    let e = env
        .call("can.sig.set", Role::Agent, json!({"signal": "Gear", "value": 1}))
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::NotFound, "no frame sent yet");
    let mut f = frame("100#803EF60400000546", Origin::Agent, "ch0");
    f.job_id = Some("j1".into());
    env.svc.journal.push(f);
    let d = env
        .call("can.sig.set", Role::Agent, json!({"signal": "Gear", "value": 1}))
        .unwrap();
    assert_eq!(d["base"], json!("journal"));
    assert!(d["warning"].as_str().unwrap().contains("overwritten"));
    let op = env.svc.worker.writes.lock().unwrap()[0].1.clone();
    assert_eq!(op["patch"]["data"], json!("803ef60200000546"));

    // No DBC on the channel.
    let e = env
        .call(
            "can.sig.set",
            Role::Agent,
            json!({"channel": "ch3", "signal": "Gear", "value": 1}),
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Db);
}

#[test]
fn sig_set_uses_listed_frame_and_refuses_pending_patch() {
    let env = loaded();
    // can-sol's list shape: `frame` payload + `revision` / `effective_revision`.
    *env.svc.worker.call_reply.lock().unwrap() = Some(Box::new(|_, _| {
        Ok(
            json!({"jobs": [{"job": "j1", "channel": "ch0", "id": 256, "ext": false, "revision": 2,
                            "effective_revision": 2,
                            "frame": {"id": 256, "bytes": {"encoding": "hex", "data": "803ef60400000546"}}}]}),
        )
    }));
    *env.svc.worker.write_reply.lock().unwrap() =
        Some(Box::new(|_, _| Ok(json!({"job": "j1", "revision": 3}))));
    let d = env
        .call("can.sig.set", Role::Agent, json!({"signal": "Gear", "value": 1}))
        .unwrap();
    assert_eq!(d["base"], json!("job"));
    let op = env.svc.worker.writes.lock().unwrap()[0].1.clone();
    assert_eq!(op["patch"]["data"], json!("803ef60200000546"));

    // A patch still waiting for its tick: never encode on older bytes.
    *env.svc.worker.call_reply.lock().unwrap() = Some(Box::new(|_, _| {
        Ok(
            json!({"jobs": [{"job": "j1", "channel": "ch0", "id": 256, "ext": false, "revision": 3,
                            "effective_revision": 2,
                            "frame": {"id": 256, "bytes": {"encoding": "hex", "data": "803ef60400000546"}}}]}),
        )
    }));
    let e = env
        .call("can.sig.set", Role::Agent, json!({"signal": "Gear", "value": 3}))
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Busy);
    assert_eq!(e.detail["effective_revision"], json!(2));
    assert_eq!(env.svc.worker.writes.lock().unwrap().len(), 1);
}

// ---- UI data queries (G2) ---------------------------------------------------------------------

const MS: u64 = 1_000_000;

fn at(env: &Env_, ms: u64, text: &str) -> u64 {
    env.svc
        .journal
        .push_at(frame(text, Origin::Device, "ch0"), ms * MS)
}

fn gap_at(env: &Env_, ms: u64) {
    let g = record(
        loom_proto::event::kinds::SOURCE_GAP,
        "ch0",
        Origin::Device,
        None,
        json!({"cause": "sim_observer_overflow", "count": {"exact": "3"}}),
    );
    env.svc.journal.push_at(g, ms * MS);
}

#[test]
fn catalog_pages_and_metadata() {
    let env = loaded();
    let d = env
        .call("can.db.catalog", Role::Viewer, json!({"max": 2}))
        .unwrap();
    assert_eq!(d["total_messages"], json!(4));
    assert_eq!(d["messages"].as_array().unwrap().len(), 2);
    assert_eq!(
        (d["next_cursor"].clone(), d["truncated"].clone()),
        (json!(2), json!(true))
    );
    let m = &d["messages"][0];
    assert_eq!(
        (m["name"].clone(), m["id"].clone(), m["size"].clone()),
        (json!("EngineData"), json!("0x100"), json!(8))
    );
    assert_eq!(
        (m["cycle_ms"].clone(), m["transmitter"].clone()),
        (json!(10), json!("ECU"))
    );
    assert_eq!(m["e2e_suspect"], Value::Null);
    let sig = |name: &str| {
        m["signals"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == json!(name))
            .unwrap()
            .clone()
    };
    let rpm = sig("EngRPM");
    assert_eq!(
        (
            rpm["factor"].clone(),
            rpm["min"].clone(),
            rpm["max"].clone(),
            rpm["unit"].clone(),
            rpm["initial"].clone()
        ),
        (
            json!(0.25),
            json!(0.0),
            json!(16000.0),
            json!("rpm"),
            json!(800.0)
        )
    );
    assert_eq!(rpm["byte_order"], json!("intel"));
    assert_eq!(sig("Volt")["byte_order"], json!("motorola"));
    assert_eq!(sig("Volt")["initial"], Value::Null, "no start value");
    assert_eq!(sig("EngTemp")["kind"], json!("signed"));
    assert_eq!(
        sig("Gear")["values"],
        json!([[0, "P"], [1, "R"], [2, "N"], [3, "D"]])
    );
    let next = env
        .call("can.db.catalog", Role::Viewer, json!({"cursor": 2}))
        .unwrap();
    let names: Vec<&str> = next["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["WideM", "J1939"]);
    assert_eq!(next["messages"][1]["id"], json!("0x18FEF100"));
    assert_eq!(next["next_cursor"], Value::Null);
    let f = env
        .call("can.db.catalog", Role::Viewer, json!({"filter": "rpm"}))
        .unwrap();
    assert_eq!(f["messages"].as_array().unwrap().len(), 1);

    // Multiplexing.
    load(&env, "ch1", &fixture("mux.dbc")).unwrap();
    let d = env
        .call("can.db.catalog", Role::Viewer, json!({"channel": "ch1"}))
        .unwrap();
    let mux = &d["messages"][0];
    assert_eq!(mux["multiplexed"], json!(true));
    let a = mux["signals"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == json!("A"))
        .unwrap();
    assert_eq!(a["mux"], json!({"multiplexor": "Mode", "ranges": [[0, 0]]}));
    assert!(mux["signals"][0]["multiplexor"].as_bool().unwrap());

    for bad in [json!({"max": 0}), json!({"max": 1001}), json!({"bogus": 1})] {
        assert_eq!(
            env.call("can.db.catalog", Role::Viewer, bad).unwrap_err().code,
            ErrorCode::Usage
        );
    }
    assert_eq!(
        env.call("can.db.catalog", Role::Viewer, json!({"channel": "ch7"}))
            .unwrap_err()
            .code,
        ErrorCode::Db
    );
}

#[test]
fn catalog_pages_stay_under_64_kib() {
    let env = setup();
    let mut dbc = String::from("VERSION \"\"\n\nBU_: ECU\n\n");
    for m in 0..300 {
        dbc.push_str(&format!("BO_ {} Message_{m:03}: 8 ECU\n", 0x100 + m));
        for k in 0..8 {
            dbc.push_str(&format!(
                " SG_ Signal_{m:03}_{k} : {} |8@1+ (0.5,-10) [-10|117.5] \"km/h\" ECU\n",
                k * 8
            ));
        }
        dbc.push('\n');
    }
    load(&env, "ch0", &temp_dbc("big-catalog", &dbc)).unwrap();
    let (mut cursor, mut seen, mut pages) = (Value::Null, 0, 0);
    loop {
        let mut p = json!({"max": 1000});
        if !cursor.is_null() {
            p["cursor"] = cursor.clone();
        }
        let d = env.call("can.db.catalog", Role::Viewer, p).unwrap();
        assert!(serde_json::to_vec(&d["messages"]).unwrap().len() <= 64 * 1024);
        seen += d["messages"].as_array().unwrap().len();
        pages += 1;
        cursor = d["next_cursor"].clone();
        if cursor.is_null() {
            break;
        }
    }
    assert_eq!(seen, 300);
    assert!(pages > 1);
}

#[test]
fn encode_preview_reports_without_sending() {
    let env = loaded();
    let p = |v: Value| env.call("can.encode.preview", Role::Viewer, v).unwrap();
    let d = p(json!({"msg": "EngineData", "signals": {"EngRPM": 3000, "Gear": "D", "Volt": 12}}));
    assert_eq!(
        (d["ok"].clone(), d["sendable"].clone()),
        (json!(true), json!(true))
    );
    assert_eq!(d["bytes"], json!("e02ef60600000546"));
    assert_eq!(d["frame"], json!("100#E02EF60600000546"));
    assert_eq!(
        (d["risk"].clone(), d["risk_periodic"].clone()),
        (json!("R1"), json!("R1"))
    );
    assert_eq!(d["base_source"], json!("initial"));
    let src = |name: &str| {
        d["sources"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["signal"] == json!(name))
            .unwrap()
            .clone()
    };
    assert_eq!(src("EngRPM")["source"], json!("given"));
    assert_eq!(src("EngTemp")["source"], json!("initial"));
    assert_eq!(src("EngTemp")["decoded"]["value"], json!(-10.0));
    assert_eq!(src("Gear")["decoded"]["label"], json!("D"));
    assert!(d["elapsed_us"].is_u64());

    // Every range problem at once; bytes still shown but not sendable.
    let d = p(json!({"msg": "EngineData", "signals": {"EngRPM": 16_001, "EngTemp": -50, "Volt": 0}}));
    assert_eq!(d["ok"], json!(true));
    assert_eq!(d["sendable"], json!(false));
    let errs: Vec<&str> = d["range_errors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["signal"].as_str().unwrap())
        .collect();
    assert_eq!(errs, ["EngRPM", "EngTemp"]);
    assert_eq!(d["range_errors"][1]["min"], json!(-40.0));
    // Structural problems: no bytes, a kind the UI can show inline.
    for (signals, kind) in [
        (json!({"EngRPM": 1}), "missing_signal"),
        (json!({"Volt": 0, "Gear": "X"}), "unknown_label"),
        (json!({"Volt": 0, "Nope": 1}), "unknown_signal"),
        (json!({"Volt": 0, "EngRPM": {"raw": 70_000}}), "raw_overflow"),
    ] {
        let d = p(json!({"msg": "EngineData", "signals": signals}));
        assert_eq!(
            (d["ok"].clone(), d["error"]["kind"].clone()),
            (json!(false), json!(kind))
        );
        assert!(d.get("bytes").is_none());
    }
    // Raw input.
    let d = p(json!({"msg": "EngineData", "signals": {"Volt": 0, "EngRPM": {"raw": 4}}}));
    assert!(d["bytes"].as_str().unwrap().starts_with("0400"));

    // Mux: the multiplexor is set from the branch of the given signals.
    load(&env, "ch1", &fixture("mux.dbc")).unwrap();
    let d = p(json!({"channel": "ch1", "msg": "MuxMsg", "signals": {"B": 5, "C": 6}}));
    assert_eq!(d["mux"], json!([{"multiplexor": "Mode", "value": 1}]));
    let mode = d["sources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["signal"] == json!("Mode"))
        .unwrap()
        .clone();
    assert_eq!(mode["source"], json!("auto_multiplexor"));
    let d = p(json!({"channel": "ch1", "msg": "MuxMsg", "signals": {"A": 1, "B": 2}}));
    assert_eq!(d["ok"], json!(false));

    for bad in [
        json!({"msg": "Nope"}),
        json!({"msg": "EngineData", "base": "zz"}),
        json!({"msg": "EngineData", "base": "0102"}),
        json!({"msg": "EngineData", "signals": {"Volt": true}}),
    ] {
        assert_eq!(
            env.call("can.encode.preview", Role::Viewer, bad.clone())
                .unwrap_err()
                .code,
            ErrorCode::Usage,
            "{bad}"
        );
    }
    assert!(
        env.svc.worker.writes.lock().unwrap().is_empty(),
        "preview never writes"
    );
}

#[test]
fn encode_preview_base_last() {
    let env = loaded();
    let p = |v: Value| env.call("can.encode.preview", Role::Viewer, v).unwrap();
    // Nothing known: DBC start values, said so.
    *env.svc.worker.call_reply.lock().unwrap() = Some(Box::new(|_, _| Ok(json!({"jobs": []}))));
    let d = p(json!({"msg": "EngineData", "signals": {"Gear": "D", "Volt": 0}, "base": "last"}));
    assert_eq!(d["base_source"], json!("initial"));
    assert!(d["base_note"].is_string());
    // Last frame on the bus: other signals keep its bytes (Volt is not needed).
    push(&env, "100#803EF60400000546", Origin::Device);
    let d = p(json!({"msg": "EngineData", "signals": {"Gear": "D"}, "base": "last"}));
    assert_eq!(
        (d["base_source"].clone(), d["bytes"].clone()),
        (json!("bus"), json!("803ef60600000546"))
    );
    // A running job wins.
    *env.svc.worker.call_reply.lock().unwrap() = Some(Box::new(|_, _| {
        Ok(
            json!({"jobs": [{"job": "j4", "id": 256, "ext": false, "revision": 1, "effective_revision": 1,
                            "frame": {"bytes": {"data": "0000f60000000546"}}}]}),
        )
    }));
    let d = p(json!({"msg": "EngineData", "signals": {"Gear": 1}, "base": "last"}));
    assert_eq!(
        (d["base_source"].clone(), d["base_job"].clone()),
        (json!("job"), json!("j4"))
    );
    assert_eq!(d["bytes"], json!("0000f60200000546"));
    // Explicit hex base.
    let d = p(json!({"msg": "EngineData", "signals": {"Gear": 0}, "base": "11 22 33 44 55 66 77 88"}));
    assert_eq!(
        (d["base_source"].clone(), d["bytes"].clone()),
        (json!("given"), json!("1122334055667788"))
    );
}

fn col(s: &Value, name: &str) -> Vec<Value> {
    s["columns"][name].as_array().unwrap().clone()
}

#[test]
fn signal_query_buckets_by_hand() {
    let env = loaded();
    // EngRPM samples (ms → rpm); grid 0..100 ms, 10 px = 10 ms buckets.
    // In journal (time) order: the gap at 50 ms comes between the 35 ms and 55 ms samples.
    for (ms, rpm) in [(1, 1000.0), (2, 3000.0), (5, 500.0), (9, 2000.0), (35, 1500.0)] {
        at(&env, ms, &engine(rpm, 2));
    }
    at(&env, 36, "123#01"); // other id: ignored
    gap_at(&env, 50);
    for (ms, rpm) in [(55, 4000.0), (58, 100.0)] {
        at(&env, ms, &engine(rpm, 2));
    }
    let d = env
        .call(
            "can.signal.query",
            Role::Viewer,
            json!({"signals": ["EngRPM", "Gear"], "t0": "seq:1", "t0_ns": 0, "t1_ns": "100000000", "px": 10}),
        )
        .unwrap();
    assert_eq!(
        d["grid"],
        json!({"start_ns": "0", "width_ns": "10000000", "px": 10, "fixed": true})
    );
    let s = &d["series"][0];
    assert_eq!(s["signal"], json!("EngineData.EngRPM"));
    assert_eq!(col(s, "i"), [json!(0), json!(3), json!(5)]);
    assert_eq!(col(s, "first"), [json!(1000.0), json!(1500.0), json!(4000.0)]);
    assert_eq!(col(s, "last"), [json!(2000.0), json!(1500.0), json!(100.0)]);
    assert_eq!(col(s, "min"), [json!(500.0), json!(1500.0), json!(100.0)]);
    assert_eq!(col(s, "max"), [json!(3000.0), json!(1500.0), json!(4000.0)]);
    assert_eq!(col(s, "n"), [json!(4), json!(1), json!(2)]);
    assert_eq!(
        col(s, "brk"),
        [json!(false), json!(false), json!(true)],
        "line breaks after the gap"
    );
    assert_eq!(s["decimated"], json!({"raw": 7, "points": 7}));
    assert_eq!(
        d["series"][1]["decimated"]["raw"],
        json!(7),
        "Gear from the same frames"
    );
    assert_eq!(d["gaps_total"], json!(1));
    assert_eq!(d["gaps"][0]["t_ns"], json!("50000000"));
    assert_eq!(
        (d["frames_scanned"].clone(), d["truncated"].clone()),
        (json!(8), json!(false))
    );

    // Adaptive grid: extremes and counts survive any merging.
    let d = env
        .call(
            "can.signal.query",
            Role::Viewer,
            json!({"signals": ["EngRPM"], "t0": "seq:1", "px": 3}),
        )
        .unwrap();
    let s = &d["series"][0];
    let nums = |c: &str| col(s, c).iter().map(|v| v.as_f64().unwrap()).collect::<Vec<_>>();
    assert!(col(s, "i").len() <= 3);
    assert_eq!(nums("max").into_iter().fold(f64::MIN, f64::max), 4000.0);
    assert_eq!(nums("min").into_iter().fold(f64::MAX, f64::min), 100.0);
    assert_eq!(nums("n").iter().sum::<f64>(), 7.0);
    assert_eq!(nums("first")[0], 1000.0);
    assert_eq!(*nums("last").last().unwrap(), 100.0);
    assert_eq!(
        d["grid"]["start_ns"],
        json!("1000000"),
        "starts at the first frame"
    );
}

#[test]
fn signal_query_limits_and_size_cut() {
    let env = loaded();
    let q = |p: Value| env.call("can.signal.query", Role::Viewer, p);
    for bad in [
        json!({"signals": ["EngRPM"], "t0": "seq:1", "px": 0}),
        json!({"signals": ["EngRPM"], "t0": "seq:1", "px": 4001}),
        json!({"signals": [], "t0": "seq:1", "px": 10}),
        json!({"signals": vec!["EngRPM"; 17], "t0": "seq:1", "px": 10}),
        json!({"signals": ["EngRPM"], "t0": "seq:1", "px": 10, "mode": "avg"}),
        json!({"signals": ["EngRPM"], "t0": "seq:1", "px": 10, "t0_ns": 5}),
        json!({"signals": ["EngRPM"], "t0": "seq:1", "px": 10, "t0_ns": 5, "t1_ns": 5}),
        json!({"signals": ["Nope"], "t0": "seq:1", "px": 10}),
    ] {
        assert_eq!(q(bad.clone()).unwrap_err().code, ErrorCode::Usage, "{bad}");
    }
    // 4000 samples, one per bucket, two signals: more than 64 KiB → cut by time, same end for both.
    for k in 0..4000u64 {
        at(&env, k, &engine((k % 16000) as f64, (k % 8) as u8));
    }
    let d = q(
        json!({"signals": ["EngRPM", "Gear"], "t0": "seq:1", "t0_ns": 0, "t1_ns": "4000000000", "px": 4000}),
    )
    .unwrap();
    assert_eq!(d["truncated"], json!(true));
    assert!(serde_json::to_vec(&d["series"]).unwrap().len() <= 70 * 1024);
    let (a, b) = (col(&d["series"][0], "i"), col(&d["series"][1], "i"));
    assert_eq!(a.last(), b.last(), "cut at the same instant");
    let last = a.last().unwrap().as_u64().unwrap();
    assert_eq!(d["resume_t0_ns"], json!(((last + 1) * MS).to_string()));
    assert!(last < 3999);
}

#[test]
fn signal_at_gives_neighbours_with_refs() {
    let env = loaded();
    let s10 = at(&env, 10, &engine(1000.0, 1));
    let s20 = at(&env, 20, &engine(2000.0, 1));
    let s30 = at(&env, 30, &engine(3000.0, 1));
    let a = |t: u64| {
        env.call(
            "can.signal.at",
            Role::Viewer,
            json!({"signal": "EngRPM", "t": t * MS, "from": "seq:1"}),
        )
        .unwrap()
    };
    let d = a(25);
    assert_eq!(
        (d["before"]["value"].clone(), d["after"]["value"].clone()),
        (json!(2000.0), json!(3000.0))
    );
    assert_eq!(d["before"]["dt_ns"], json!(-5_000_000));
    assert_eq!(d["after"]["dt_ns"], json!(5_000_000));
    assert_eq!(
        d["before"]["ref"],
        json!(format!("can:ch0@e1:seq:{s20}#/signals/EngRPM"))
    );
    assert_eq!(d["after"]["seq"], json!(s30.to_string()));
    assert_eq!(d["evidence"].as_array().unwrap().len(), 2);
    assert_eq!(d["gap_between"], json!(false));
    let d = a(20);
    assert_eq!(
        d["before"]["seq"],
        json!(s20.to_string()),
        "a sample exactly at t counts as before"
    );
    let d = a(5);
    assert_eq!(
        (d["before"].clone(), d["after"]["seq"].clone()),
        (Value::Null, json!(s10.to_string()))
    );
    assert_eq!(a(35)["after"], Value::Null);
    gap_at(&env, 32);
    at(&env, 40, &engine(4000.0, 1));
    assert_eq!(a(36)["gap_between"], json!(true));
    // String time from the UI, default scan start.
    let d = env
        .call(
            "can.signal.at",
            Role::Viewer,
            json!({"signal": "EngRPM", "t": "25000000"}),
        )
        .unwrap();
    assert_eq!(d["before"]["value"], json!(2000.0));
    assert_eq!(
        env.call(
            "can.signal.at",
            Role::Viewer,
            json!({"signal": "EngRPM", "t": "x"})
        )
        .unwrap_err()
        .code,
        ErrorCode::Usage
    );
}
