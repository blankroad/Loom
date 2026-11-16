//! Operator upgrade UDS batch 2 — pure / single-type contracts (Opus-authored specification).
//! U1 TesterPresent parameters + scheduling, U6 closed/forgotten state, U8 row route stamps,
//! backward compatibility of every new optional field. Contracts: work/reports/operator-upgrade/uds/FUNCTIONS.md (B2-*).

use std::sync::Arc;
use std::time::Duration;

use loom_domain_api::Reducer;
use loom_proto::event::{Origin, PublishedRecord, WriteState};
use loom_proto::rpc::SubFilter;
use loom_uds::lanes::{Lane, PriorityGate};
use loom_uds::module::StateReducer;
use loom_uds::payload::{LinkState, RouteStamp, RowPayload, StatePayload};
use loom_uds::session::{Lease, TargetSession};
use loom_uds::tester_present::{
    TP_MAX_INTERVAL_MS, TP_MIN_INTERVAL_MS, TpCancel, TpConfig, TpRequest, TpTick, parse_tp_params,
    tp_decide, tp_exchange_outcome,
};
use loom_uds_runner::{AbortKind, RejectReason, TimeoutPhase, TxnResult, TxnStatus, TxnTimes};
use serde_json::{Value, json};

const S: fn(u64) -> Duration = Duration::from_secs;
const MS: fn(u64) -> Duration = Duration::from_millis;

fn agent() -> Lease {
    Lease {
        owner_origin: Origin::Agent,
        actor_id: "claude-1".into(),
        client_id: "claude-1".into(),
    }
}

fn up(name: &str) -> TargetSession {
    let mut s = TargetSession::new(name);
    s.set_endpoint("sim", "DoIP 0x1001");
    s.link_up(Duration::ZERO);
    s
}

fn json_of(p: &StatePayload) -> Value {
    serde_json::to_value(p).expect("state serializes")
}

// ------------------------------------------------------------------ B2-T1..T5
#[test]
fn b2_t1_pdu() {
    assert_eq!(
        TpConfig {
            interval: S(2),
            suppress: true
        }
        .pdu(),
        vec![0x3E, 0x80]
    );
    assert_eq!(
        TpConfig {
            interval: S(2),
            suppress: false
        }
        .pdu(),
        vec![0x3E, 0x00]
    );
}

#[test]
fn b2_t2_params() {
    assert_eq!(parse_tp_params(&json!({"on": false})), Ok(TpRequest::Off));
    assert_eq!(
        parse_tp_params(&json!({"on": false, "interval_ms": 1})),
        Ok(TpRequest::Off),
        "off ignores the rest"
    );
    assert_eq!(
        parse_tp_params(&json!({"on": true, "interval_ms": 2000})),
        Ok(TpRequest::On(TpConfig {
            interval: MS(2000),
            suppress: true
        })),
        "suppress defaults to true (3E 80)"
    );
    assert_eq!(
        parse_tp_params(&json!({"on": true, "interval_ms": TP_MIN_INTERVAL_MS, "suppress": false})),
        Ok(TpRequest::On(TpConfig {
            interval: MS(TP_MIN_INTERVAL_MS),
            suppress: false
        }))
    );
    assert!(parse_tp_params(&json!({"on": true, "interval_ms": TP_MAX_INTERVAL_MS})).is_ok());
    for bad in [
        json!({}),
        json!({"on": "yes"}),
        json!({"on": true}),
        json!({"on": true, "interval_ms": 499}),
        json!({"on": true, "interval_ms": 30_001}),
        json!({"on": true, "interval_ms": "2000"}),
        json!({"on": true, "interval_ms": 2000, "suppress": "no"}),
    ] {
        assert!(parse_tp_params(&bad).is_err(), "{bad} must be E_USAGE");
    }
    let e = parse_tp_params(&json!({"on": true})).expect_err("no default interval");
    assert!(e.contains("interval_ms"), "{e}");
}

#[test]
fn b2_t3_decide() {
    let i = S(2);
    assert_eq!(
        tp_decide(S(10), Some(S(9)), i, false),
        TpTick::Wait(S(1)),
        "recent traffic postpones"
    );
    assert_eq!(
        tp_decide(S(10), Some(S(9)), i, true),
        TpTick::Wait(S(1)),
        "not due: busy is irrelevant"
    );
    assert_eq!(
        tp_decide(S(10), Some(S(8)), i, false),
        TpTick::Send,
        "exactly due"
    );
    assert_eq!(
        tp_decide(S(10), Some(S(7)), i, true),
        TpTick::Busy,
        "due but the target is busy"
    );
    assert_eq!(
        tp_decide(S(10), None, i, false),
        TpTick::Send,
        "no activity known: send now"
    );
    assert_eq!(
        tp_decide(S(1), Some(S(5)), i, false),
        TpTick::Wait(S(6)),
        "activity in the future (clock skew) waits"
    );
}

fn result(status: TxnStatus) -> TxnResult {
    TxnResult {
        status,
        write_state: WriteState::Sent,
        timeout: false,
        nrc: None,
        sent: true,
        may_have_executed: true,
        response: None,
        pending_count: 0,
        late_count: 0,
        generation: 1,
        times: TxnTimes::default(),
        error: None,
    }
}

#[test]
fn b2_t4_outcome() {
    assert_eq!(tp_exchange_outcome(&result(TxnStatus::OkSuppressed)), Ok(()));
    assert_eq!(tp_exchange_outcome(&result(TxnStatus::Positive)), Ok(()));
    assert_eq!(
        tp_exchange_outcome(&result(TxnStatus::Negative { nrc: 0x12 })),
        Err("NRC 0x12 subFunctionNotSupported".into())
    );
    assert_eq!(
        tp_exchange_outcome(&result(TxnStatus::Negative { nrc: 0xFF })),
        Err("NRC 0xFF".into()),
        "unnamed NRC"
    );
    assert_eq!(
        tp_exchange_outcome(&result(TxnStatus::Timeout {
            phase: TimeoutPhase::P2
        })),
        Err("timeout".into())
    );
    assert_eq!(
        tp_exchange_outcome(&result(TxnStatus::LinkLost)),
        Err("link lost".into())
    );
    assert_eq!(
        tp_exchange_outcome(&result(TxnStatus::GenerationChanged)),
        Err("generation changed".into())
    );
    assert_eq!(
        tp_exchange_outcome(&result(TxnStatus::TransportNack { code: 0x03 })),
        Err("transport NACK 0x03".into())
    );
    assert_eq!(
        tp_exchange_outcome(&result(TxnStatus::Aborted {
            kind: AbortKind::Stop
        })),
        Err("aborted (Stop)".into())
    );
    assert_eq!(
        tp_exchange_outcome(&result(TxnStatus::Rejected(RejectReason::QueueFull))),
        Err("not sent: QueueFull".into())
    );
}

#[tokio::test]
async fn b2_t5_cancel() {
    let c = Arc::new(TpCancel::default());
    assert!(!c.is_cancelled());
    let w = {
        let c = c.clone();
        tokio::spawn(async move { c.cancelled().await })
    };
    tokio::task::yield_now().await;
    c.cancel();
    c.cancel(); // idempotent
    tokio::time::timeout(S(1), w).await.expect("woken").expect("task");
    assert!(c.is_cancelled());
    tokio::time::timeout(MS(50), c.cancelled())
        .await
        .expect("already cancelled resolves at once");
}

// ------------------------------------------------------------------ B2-L1/L2
#[tokio::test]
async fn b2_l_try_acquire_never_queues() {
    let g = Arc::new(PriorityGate::default());
    assert!(!g.is_busy());
    let p = g
        .try_acquire(Lane::Agent, Origin::Agent, "tp-1")
        .expect("free gate");
    assert!(g.is_busy());
    assert!(
        g.try_acquire(Lane::Agent, Origin::Agent, "tp-2").is_none(),
        "held"
    );
    drop(p);
    assert!(!g.is_busy());
    let hold = g.acquire(Lane::User, Origin::User, "u0").await.expect("free");
    let waiter = {
        let g = g.clone();
        tokio::spawn(async move { g.acquire(Lane::User, Origin::User, "u1").await.map(|_| ()) })
    };
    while g.depth().1 == 0 {
        tokio::task::yield_now().await;
    }
    drop(hold); // hands over to u1 directly
    waiter.await.expect("task").expect("u1 got the gate");
    assert!(
        g.try_acquire(Lane::Agent, Origin::Agent, "tp-3").is_some(),
        "free again after u1"
    );
}

// ------------------------------------------------------------------ B2-S*
#[test]
fn b2_s1_generation_base_continues_numbering() {
    let mut s = TargetSession::new("ecu1").with_generation_base(7);
    assert_eq!(s.generation(), 7);
    s.link_up(Duration::ZERO);
    assert_eq!(s.generation(), 8);
    assert_eq!(
        s.state_payload().link.cause.as_deref(),
        Some("reconnected"),
        "base > 0 is not a first connect"
    );
}

#[test]
fn b2_s2_link_closed() {
    let mut s = up("ecu1");
    s.set_lease(Some(agent()));
    s.tp_start(S(2), true, agent());
    s.link_closed("disconnected by user");
    let p = s.state_payload();
    assert_eq!(p.link.state, LinkState::Closed);
    assert_eq!(p.link.cause.as_deref(), Some("disconnected by user"));
    assert_eq!(json_of(&p)["link"]["state"], "closed");
    assert_eq!(json_of(&p)["session"]["confidence"], "stale");
    assert_eq!(json_of(&p)["security"]["confidence"], "stale");
    assert!(p.lease.is_none(), "a closed target holds no lease");
    assert!(p.tester_present.is_none());
    assert_eq!(p.tp_stop_cause.as_deref(), Some("link closed"));
    assert_eq!(p.generation, 1, "closing never bumps the generation");
}

#[test]
fn b2_s3_s4_tp_start_stop_payload() {
    let mut s = up("ecu1");
    s.tp_start(MS(1500), true, agent());
    let v = json_of(&s.state_payload());
    assert_eq!(
        v["tester_present"],
        json!({"active": true, "interval_ms": 1500, "ok_count": 0, "suppress": true,
               "owner": {"owner_origin": "agent", "actor_id": "claude-1", "client_id": "claude-1"}})
    );
    assert!(v.get("tp_stop_cause").is_none());
    assert_eq!(s.tp_owner(), Some(&agent()));
    assert_eq!(s.tp_interval(), Some(MS(1500)));
    s.on_tester_present_ok(S(3));
    assert_eq!(s.last_activity(), Some(S(3)));
    assert!(s.tp_stop("stopped by user"));
    let v = json_of(&s.state_payload());
    assert_eq!(v["tester_present"], Value::Null);
    assert_eq!(v["tp_stop_cause"], "stopped by user");
    assert_eq!(s.tp_owner(), None);
    assert!(!s.tp_stop("again"), "nothing running");
    assert_eq!(
        s.state_payload().tp_stop_cause.as_deref(),
        Some("stopped by user"),
        "a no-op stop keeps the cause"
    );
    s.tp_start(S(2), false, agent());
    assert!(
        s.state_payload().tp_stop_cause.is_none(),
        "a new job clears the old cause"
    );
}

#[test]
fn b2_s9_link_down_and_reset_stop_tp_with_cause() {
    let mut s = up("ecu1");
    s.tp_start(S(2), true, agent());
    s.link_down("peer closed");
    assert!(s.state_payload().tester_present.is_none());
    assert_eq!(
        s.state_payload().tp_stop_cause.as_deref(),
        Some("link down (peer closed)")
    );
    s.link_up(S(1));
    assert!(
        s.state_payload().tester_present.is_none(),
        "reconnect never restarts TP"
    );
    assert_eq!(
        s.state_payload().tp_stop_cause.as_deref(),
        Some("link down (peer closed)"),
        "cause survives reconnect"
    );

    s.tp_start(S(2), true, agent());
    assert!(s.on_response(S(2), &[0x11, 0x01], &[0x51, 0x01]) || s.state_payload().tester_present.is_none());
    assert!(s.state_payload().tester_present.is_none());
    assert_eq!(
        s.state_payload().tp_stop_cause.as_deref(),
        Some("session reset to default")
    );
}

#[test]
fn b2_s6_route_stamp() {
    let mut s = up("ecu1");
    assert_eq!(
        s.route_stamp(),
        RouteStamp {
            generation: 1,
            backend: Some("sim".into()),
            addr: Some("DoIP 0x1001".into())
        }
    );
    s.link_down("x");
    s.link_up(S(1));
    assert_eq!(s.route_stamp().generation, 2);
    assert_eq!(
        TargetSession::new("n").route_stamp(),
        RouteStamp {
            generation: 0,
            backend: None,
            addr: None
        }
    );
}

#[test]
fn b2_s7_forgotten() {
    let mut s = up("ecu1");
    s.link_closed("disconnected by agent");
    assert!(
        json_of(&s.state_payload()).get("forgotten").is_none(),
        "absent unless true"
    );
    s.mark_forgotten();
    let v = json_of(&s.state_payload());
    assert_eq!(v["forgotten"], true);
    assert_eq!(v["link"]["state"], "closed");
}

// ------------------------------------------------------------------ B2-F1 + compatibility
#[test]
fn b2_f1_with_route_and_backward_compat() {
    let stamp = RouteStamp {
        generation: 3,
        backend: Some("doip".into()),
        addr: Some("DoIP 0x1001".into()),
    };
    let row = RowPayload::request(
        "ecu1",
        &[0x22, 0xF1, 0x90],
        false,
        WriteState::Requested,
        (50, 5000),
    )
    .with_route(&stamp);
    let v = serde_json::to_value(&row).expect("row");
    assert_eq!(
        (v["generation"].clone(), v["backend"].clone(), v["addr"].clone()),
        (json!(3), json!("doip"), json!("DoIP 0x1001"))
    );
    assert!(v.get("purpose").is_none(), "only keep-alive rows carry a purpose");

    let plain = serde_json::to_value(RowPayload::request(
        "ecu1",
        &[0x22, 0xF1, 0x90],
        false,
        WriteState::Requested,
        (50, 5000),
    ))
    .expect("row");
    for k in ["generation", "backend", "addr", "purpose"] {
        assert!(
            plain.get(k).is_none(),
            "{k} absent unless stamped (old fixtures unchanged)"
        );
    }
    // a row from an older daemon (no stamp) still deserializes, with unknown route
    let old: RowPayload = serde_json::from_value(json!({
        "target": "ecu1", "write_state": "confirmed", "bytes": {"encoding": "hex", "data": "22f190"}, "sid": 34,
        "summary": "x", "suppress_pos": false, "functional": false, "risk": "R0", "reason": "r", "policy_action": "auto",
        "pending_count": 0, "p2_ms": 50, "p2_star_ms": 5000
    }))
    .expect("old row");
    assert_eq!(
        (old.generation, old.backend, old.addr, old.purpose),
        (None, None, None, None)
    );
    // an old state (no tp_stop_cause/forgotten, legacy TP without suppress/owner) still deserializes
    let st: StatePayload = serde_json::from_value(json!({
        "target": "ecu1", "generation": 1, "backend": "sim", "addr": "DoIP 0x1001",
        "link": {"state": "up"}, "session": {"id": 1, "name": "default", "confidence": "assumed"},
        "security": {"level": null, "confidence": "assumed"},
        "tester_present": {"active": true, "interval_ms": 2000, "ok_count": 1}, "p2_ms": 50, "p2_star_ms": 5000, "lease": null
    }))
    .expect("old state");
    assert!(!st.forgotten && st.tp_stop_cause.is_none());
    assert_eq!(st.tester_present.as_ref().and_then(|t| t.suppress), None);
}

// ------------------------------------------------------------------ B2-R1 reducer
fn record(seq: u64, payload: Value) -> PublishedRecord {
    serde_json::from_value(json!({
        "schema_version": 1, "session_id": "s", "domain": "uds", "channel": payload["target"],
        "kind": "uds.state", "seq": seq.to_string(), "epoch": "1",
        "producer_id": "gateway.uds", "producer_epoch": "1", "source_seq": seq.to_string(),
        "ts": {"mono_ns": (seq * 1000).to_string(), "wall_ns": "0", "clock_id": "c", "source": "gateway"},
        "origin": "agent", "payload": payload,
    }))
    .expect("record")
}

#[test]
fn b2_r1_reducer_drops_forgotten_targets() {
    let mut r = StateReducer::default();
    let mut a = up("ecu1");
    let b = up("ecu2");
    r.apply(&record(1, serde_json::to_value(a.state_payload()).expect("a")));
    r.apply(&record(2, serde_json::to_value(b.state_payload()).expect("b")));
    assert_eq!(r.snapshot(&SubFilter::default()).len(), 2);
    a.link_closed("disconnected by user");
    a.mark_forgotten();
    r.apply(&record(3, serde_json::to_value(a.state_payload()).expect("a")));
    let snap = r.snapshot(&SubFilter::default());
    assert_eq!(snap.len(), 1, "forgotten target leaves the snapshot");
    assert_eq!(snap[0]["payload"]["target"], "ecu2");
}
