//! operator-upgrade C2 (ETH-RT-04) — a journal `E_SCAN_LIMIT` with a well-formed, progressing
//! `resume_from` is one bounded scan step, consumed under the existing byte budget; malformed or
//! non-progressing ones stay errors; `eth.show` never starts its context window at `seq:0`
//! (FUNCTIONS.md FC2.1–FC2.3). Runtime: daemon 56402, `seq:0` → `E_SCAN_LIMIT` resume
//! `e1794778200:seq:3275`, scanned 8411383 B; show of a packet 5 seqs after its SYN said unknown.

mod support;

use std::time::Duration;

use loom_domain_api::DomainError;
use loom_eth::gateway::Eth;
use loom_eth::gateway::common::{SCAN_BUDGET_BYTES, scan_limit_resume};
use loom_eth::gateway::handlers::SHOW_CONTEXT_BYTES;
use loom_proto::cursor::Cursor;
use loom_proto::error::ErrorCode;
use loom_testkit::RegistryProbe;
use serde_json::{Value, json};

use support::*;

fn err(code: ErrorCode, detail: Value) -> DomainError {
    DomainError::new(code, "x").with_detail(detail)
}
fn seq(epoch: Option<u64>, seq: u64) -> Cursor {
    Cursor::Seq { epoch, seq }
}
fn limit(detail: Value) -> DomainError {
    err(ErrorCode::ScanLimit, detail)
}

#[test]
fn fc2_1_accepts_the_runtime_shape_and_numbers() {
    let runtime = json!({"partial": [], "resume_from": "e1794778200:seq:3275", "scanned_bytes": "8411383"});
    assert_eq!(
        scan_limit_resume(&limit(runtime), &seq(None, 0)),
        Some((seq(Some(1_794_778_200), 3275), 8_411_383))
    );
    // partial absent, bytes as a JSON number, resume without epoch, from with the same epoch
    let d = json!({"resume_from": "seq:10", "scanned_bytes": 5});
    assert_eq!(
        scan_limit_resume(&limit(d), &seq(Some(7), 3)),
        Some((seq(None, 10), 5))
    );
    // from that is not a seq cursor (e.g. a relative start): any seq resume is progress
    let d = json!({"partial": [], "resume_from": "e1:seq:4", "scanned_bytes": "0"});
    let rel = "-600s".parse::<Cursor>().expect("relative cursor");
    assert_eq!(scan_limit_resume(&limit(d), &rel), Some((seq(Some(1), 4), 0)));
}

#[test]
fn fc2_1_rejects_everything_else() {
    let ok = json!({"partial": [], "resume_from": "e1:seq:9", "scanned_bytes": "100"});
    let from = seq(Some(1), 0);
    assert!(
        scan_limit_resume(&err(ErrorCode::Usage, ok.clone()), &from).is_none(),
        "other code"
    );
    let bad = [
        json!({"partial": [{"seq": "3"}], "resume_from": "e1:seq:9", "scanned_bytes": "100"}), // events would be lost
        json!({"partial": "x", "resume_from": "e1:seq:9", "scanned_bytes": "100"}),
        json!({"partial": [], "scanned_bytes": "100"}), // no resume
        json!({"partial": [], "resume_from": "now", "scanned_bytes": "100"}), // not a seq cursor
        json!({"partial": [], "resume_from": "garbage", "scanned_bytes": "100"}),
        json!({"partial": [], "resume_from": 9, "scanned_bytes": "100"}),
        json!({"partial": [], "resume_from": "e1:seq:9"}), // no bytes
        json!({"partial": [], "resume_from": "e1:seq:9", "scanned_bytes": "-1"}),
        json!({"partial": [], "resume_from": "e1:seq:9", "scanned_bytes": "lots"}),
        json!({"partial": [], "resume_from": "e1:seq:9", "scanned_bytes": -1}),
        json!({"partial": [], "resume_from": "e2:seq:9", "scanned_bytes": "100"}), // other epoch
        Value::Null,
    ];
    for d in bad {
        assert!(scan_limit_resume(&limit(d.clone()), &from).is_none(), "{d}");
    }
    // no progress: resume at or before the start
    for (start, resume) in [(9, "e1:seq:9"), (12, "e1:seq:9")] {
        let d = json!({"partial": [], "resume_from": resume, "scanned_bytes": "100"});
        assert!(
            scan_limit_resume(&limit(d), &seq(Some(1), start)).is_none(),
            "{start} → {resume}"
        );
    }
}

fn journal(below: Option<BelowRing>) -> std::sync::Arc<Services> {
    let s = std::sync::Arc::new(Services::default());
    let base = wall_now() - 60_000_000_000;
    *s.journal.events.lock().expect("lock") = (5..=8)
        .map(|n| pkt(n, "qa", base + n, &udp4(TESTER, ECU, 40000, 65201, b"ping"), None))
        .collect();
    *s.journal.below_ring.lock().expect("lock") = below;
    s
}

fn query(svc: &std::sync::Arc<Services>, from: &str) -> Result<Value, DomainError> {
    let reg = RegistryProbe::assert_ok(&Eth).0;
    call(
        &reg,
        svc,
        "eth.query",
        json!({"ch": "qa", "range": {"from": from}, "max": 100}),
        Duration::from_secs(5),
    )
}

fn seqs(v: &Value) -> Vec<String> {
    v["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .map(|r| r["seq"].as_str().expect("seq").to_owned())
        .collect()
}

#[test]
fn fc2_2_a_progressing_scan_limit_is_consumed_and_the_read_completes() {
    let svc = journal(Some(BelowRing {
        oldest: 5,
        bytes: 1_000,
        detail: None,
    }));
    let v = query(&svc, "seq:0").expect("no hard E_SCAN_LIMIT any more");
    assert_eq!(seqs(&v), ["5", "6", "7", "8"]);
    assert_eq!(v["complete"], true, "{v}");
    assert_eq!(v["partial_reasons"], json!([]));
    assert!(v["scanned"]["bytes"].as_u64().expect("bytes") >= 1_000, "{v}");
}

#[test]
fn fc2_2_the_existing_budget_still_bounds_it_and_says_where_to_resume() {
    let svc = journal(Some(BelowRing {
        oldest: 5,
        bytes: SCAN_BUDGET_BYTES,
        detail: None,
    }));
    let v = query(&svc, "seq:0").expect("partial, not an error");
    assert_eq!(seqs(&v), Vec::<String>::new());
    assert_eq!(v["complete"], false);
    assert!(
        v["partial_reasons"]
            .as_array()
            .expect("reasons")
            .contains(&json!("scan_budget")),
        "{v}"
    );
    assert_eq!(v["resume_cursor"], "e1:seq:5");
}

#[test]
fn fc2_2_malformed_scan_limit_stays_an_error() {
    let svc = journal(Some(BelowRing {
        oldest: 5,
        bytes: 10,
        detail: Some(json!({"partial": [{"seq": "2"}], "resume_from": "e1:seq:5", "scanned_bytes": "10"})),
    }));
    let e = query(&svc, "seq:0").expect_err("partial events must not be dropped");
    assert_eq!(e.code, ErrorCode::ScanLimit);
    // non-progress: the journal says "resume at 0" for a scan from 0
    let svc = journal(Some(BelowRing {
        oldest: 5,
        bytes: 10,
        detail: Some(json!({"partial": [], "resume_from": "e1:seq:0", "scanned_bytes": "10"})),
    }));
    assert_eq!(
        query(&svc, "seq:0").expect_err("no progress").code,
        ErrorCode::ScanLimit
    );
}

// --- eth.show context (FC2.2 + FC2.3) ------------------------------------------------------------

const C: u16 = 50000;
const S: u16 = 40500;

fn tcp4(to_srv: bool, sq: u32, ack: u32, flags: u8, payload: &[u8]) -> Vec<u8> {
    let (s, d, sp, dp) = if to_srv {
        (TESTER, ECU, C, S)
    } else {
        (ECU, TESTER, S, C)
    };
    let total = (20 + 20 + payload.len()) as u16;
    let mut f = vec![0x02, 0, 0, 0, 0, 0x0A, 0x02, 0, 0, 0, 0, 0x02, 0x08, 0x00];
    f.extend([
        0x45,
        0,
        (total >> 8) as u8,
        total as u8,
        0,
        1,
        0x40,
        0,
        64,
        6,
        0,
        0,
    ]);
    f.extend(s);
    f.extend(d);
    f.extend(sp.to_be_bytes());
    f.extend(dp.to_be_bytes());
    f.extend(sq.to_be_bytes());
    f.extend(ack.to_be_bytes());
    f.extend([0x50, flags, 0xFF, 0xFF, 0, 0, 0, 0]);
    f.extend(payload);
    f
}

/// SYN at `first`, SYN-ACK, ACK, client 15 B, server 17 B, client 32 B (target = first + 5).
fn tcp_journal(first: u64, below: Option<BelowRing>) -> std::sync::Arc<Services> {
    let frames = [
        tcp4(true, 1000, 0, 0x02, &[]),
        tcp4(false, 5000, 1001, 0x12, &[]),
        tcp4(true, 1001, 5001, 0x10, &[]),
        tcp4(true, 1001, 5001, 0x18, &[0xAA; 15]),
        tcp4(false, 5001, 1016, 0x18, &[0xBB; 17]),
        tcp4(true, 1016, 5018, 0x18, &[0xCC; 32]),
    ];
    let s = std::sync::Arc::new(Services::default());
    let base = wall_now() - 60_000_000_000;
    *s.journal.events.lock().expect("lock") = frames
        .iter()
        .enumerate()
        .map(|(i, f)| pkt(first + i as u64, "qa", base + i as u64 * 1_000, f, None))
        .collect();
    *s.journal.below_ring.lock().expect("lock") = below;
    s
}

fn show(svc: &std::sync::Arc<Services>, n: u64) -> Value {
    let reg = RegistryProbe::assert_ok(&Eth).0;
    call(
        &reg,
        svc,
        "eth.show",
        json!({"ref": format!("eth:qa@seq:{n}")}),
        Duration::from_secs(5),
    )
    .expect("show")
}

fn seq_label(r: &Value) -> String {
    fn walk(v: &Value) -> Option<String> {
        for n in v.as_array()? {
            if n["name"] == "tcp.seq" {
                return n["label"].as_str().map(str::to_owned);
            }
            if let Some(l) = walk(&n["children"]) {
                return Some(l);
            }
        }
        None
    }
    walk(&r["tree"]).expect("tcp.seq node")
}

#[test]
fn fc2_3_window_never_starts_at_seq_0_so_a_ring_starting_at_1_is_used() {
    // Ring serves from seq 1; anything below costs the whole show budget (the 56402 situation).
    let svc = tcp_journal(
        1,
        Some(BelowRing {
            oldest: 1,
            bytes: SHOW_CONTEXT_BYTES,
            detail: None,
        }),
    );
    let r = show(&svc, 6);
    assert_eq!(r["tcp_rel"]["window_from"], "seq:1");
    assert_eq!(r["tcp_rel"]["reached"], true, "{}", r["tcp_rel"]);
    assert_eq!(seq_label(&r), "Relative sequence number: 16");
}

#[test]
fn fc2_2_show_consumes_a_below_ring_step_and_reaches_the_packet() {
    // Ring starts at 3 (SYN at 3); the window start 1 is below it → one cheap E_SCAN_LIMIT step.
    let svc = tcp_journal(
        3,
        Some(BelowRing {
            oldest: 3,
            bytes: 64,
            detail: None,
        }),
    );
    let r = show(&svc, 8);
    assert_eq!(
        r["tcp_rel"],
        json!({"window_from": "seq:1", "reached": true, "seq_known": true, "ack_known": true})
    );
    assert_eq!(seq_label(&r), "Relative sequence number: 16");
    assert!(
        r["warnings"]
            .as_array()
            .expect("w")
            .iter()
            .all(|w| w["code"] != "tcp_relative_unknown")
    );
}

#[test]
fn fc2_2_show_budget_exhausted_by_the_step_stays_truthfully_unknown() {
    let svc = tcp_journal(
        3,
        Some(BelowRing {
            oldest: 3,
            bytes: SHOW_CONTEXT_BYTES,
            detail: None,
        }),
    );
    let r = show(&svc, 8);
    assert_eq!(r["tcp_rel"]["reached"], false);
    assert_eq!(r["tcp_rel"]["seq_known"], false);
    assert!(seq_label(&r).contains("unknown"), "{}", seq_label(&r));
    assert!(
        r["warnings"]
            .as_array()
            .expect("w")
            .iter()
            .any(|w| w["code"] == "tcp_relative_unknown")
    );
}
