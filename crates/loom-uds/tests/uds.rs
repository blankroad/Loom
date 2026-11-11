//! Registration, classify hook (all 256 SIDs, timing), TargetSession transitions, dry-run.

use std::time::Duration;

use loom_proto::event::Risk;
use loom_proto::policy::{BlockClass, PolicyAction};
use loom_testkit::{HookHarness, OwnedWriteRequest, RegistryProbe};
use loom_uds::UdsModule;
use loom_uds::hooks::dry_run;
use loom_uds::payload::Confidence;
use loom_uds::risk::{RULES, classify_pdu};
use loom_uds::session::TargetSession;
use loom_uds_sim::SimEcu;
use serde_json::json;

#[test]
fn registers_cleanly() {
    let (reg, report) = RegistryProbe::assert_ok(&UdsModule);
    assert!(report.warnings.is_empty(), "{report}");
    for k in ["uds.tx", "uds.rx", "uds.state", "uds.result"] {
        assert!(report.event_kinds.iter().any(|e| e == k), "{k}");
    }
    let t = reg.risk_table_ref().expect("risk table");
    assert_eq!(t.immutable_deny, ["sid=2A", "sid=86"]);
    assert!(t.default_blocked.is_empty(), "F1: nothing default-blocked");
}

#[test]
fn every_defined_sid_has_a_rule_and_all_256_classify_fast() {
    for s in loom_uds_codec::sid::SERVICES {
        assert!(
            RULES.iter().any(|r| r.sid == s.sid),
            "no rule for 0x{:02X}",
            s.sid
        );
    }
    let (reg, _) = RegistryProbe::assert_ok(&UdsModule);
    let h = HookHarness::new(reg.hooks_ref().expect("hooks").clone());
    for sid in 0..=255u8 {
        for sub in [0x00u8, 0x01, 0x02, 0x03, 0x83, 0xFF] {
            let req = OwnedWriteRequest::new("uds.request", "ecu1").bytes(&[sid, sub, 0xF1, 0x90]);
            let t = h.classify(&req);
            assert!(
                t.elapsed <= Duration::from_millis(1),
                "0x{sid:02X} took {:?}",
                t.elapsed
            );
            let blocked = t.value.block.is_some();
            assert_eq!(
                blocked,
                sid == 0x2A || sid == 0x86,
                "F1: only 0x2A/0x86 blocked (0x{sid:02X})"
            );
        }
    }
}

#[test]
fn table_cases() {
    let r = |b: &[u8], f: bool| classify_pdu(b, f).risk;
    assert_eq!(r(&[0x22, 0xF1, 0x90], false), Risk::R0);
    assert_eq!(
        r(&[0x22, 0xF1, 0x90], true),
        Risk::R0,
        "functional 0x22 not raised"
    );
    assert_eq!(r(&[0x10, 0x03], false), Risk::R1);
    assert_eq!(
        r(&[0x10, 0x83], false),
        Risk::R1,
        "SPRMIB cleared before matching"
    );
    assert_eq!(r(&[0x10, 0x02], false), Risk::R3);
    assert_eq!(r(&[0x2E, 0xF1, 0xA0, 1], false), Risk::R2);
    assert_eq!(r(&[0x2E, 0xF1, 0x90, 1], false), Risk::R3);
    assert_eq!(r(&[0x27, 0x01], false), Risk::R1);
    assert_eq!(r(&[0x27, 0x02, 1, 2], false), Risk::R3);
    assert_eq!(r(&[0x2F, 0x40, 0x01, 0x00], false), Risk::R1);
    assert_eq!(r(&[0x2F, 0x40, 0x01, 0x03, 1], false), Risk::R3);
    assert_eq!(r(&[0x31, 0x01, 0xFF, 0x00], false), Risk::R4);
    assert_eq!(
        classify_pdu(&[0x31, 0x01, 0xFF, 0x00], false).block,
        None,
        "F1: erase not blocked"
    );
    assert_eq!(r(&[0x31, 0x01, 0xFF, 0x01], false), Risk::R1);
    assert_eq!(r(&[0x11, 0x01], true), Risk::R3, "functional reset raised");
    assert_eq!(r(&[0x62, 0xF1], false), Risk::R4);
    assert_eq!(r(&[], false), Risk::R4);
    assert_eq!(
        classify_pdu(&[0x2A, 0x01], false).block,
        Some(BlockClass::ImmutableDeny)
    );
}

#[test]
fn typed_args_and_presend() {
    let (reg, _) = RegistryProbe::assert_ok(&UdsModule);
    let h = HookHarness::new(reg.hooks_ref().expect("hooks").clone());
    let w = OwnedWriteRequest::new("uds.write_did", "ecu1").args(json!({"did": "F1A0", "data_hex": "02"}));
    assert_eq!(h.classify(&w).value.risk, Risk::R2);
    let s = OwnedWriteRequest::new("uds.session", "ecu1").args(json!({"session": "programming"}));
    assert_eq!(h.classify(&s).value.risk, Risk::R3);
    let bad = OwnedWriteRequest::new("uds.write_did", "ecu1").args(json!({"did": "zz"}));
    assert_eq!(h.classify(&bad).value.risk, Risk::R4);
    let mut never = OwnedWriteRequest::new("uds.read_did", "ecu1");
    never.target.generation = Some(0);
    assert!(h.presend_check(&never, None).value.is_err());
    assert!(h.presend_check(&w, None).value.is_ok());
}

fn exchange(ecu: &mut SimEcu, s: &mut TargetSession, now: u64, req: &[u8]) -> bool {
    let reply = ecu.handle(ms(now), req);
    let last = reply.last().expect("reply").bytes.clone();
    s.on_response(ms(now), req, &last)
}

fn ms(v: u64) -> Duration {
    Duration::from_millis(v)
}

#[test]
fn target_session_follows_the_sim() {
    let mut ecu = SimEcu::bench();
    let mut s = TargetSession::new("ecu1");
    assert_eq!(s.generation(), 0);
    s.link_up(ms(0));
    assert_eq!((s.generation(), s.session()), (1, (0x01, Confidence::Assumed)));
    assert!(exchange(&mut ecu, &mut s, 1, &[0x10, 0x03]));
    assert_eq!(s.session(), (0x03, Confidence::Confirmed));
    assert_eq!(s.timing(), (50, 5000));
    let seed = ecu
        .handle(ms(2), &[0x27, 0x01])
        .last()
        .expect("seed")
        .bytes
        .clone();
    let k = (u32::from_be_bytes([seed[2], seed[3], seed[4], seed[5]]) ^ loom_uds_sim::SECURITY_XOR_DEFAULT)
        .to_be_bytes();
    exchange(&mut ecu, &mut s, 3, &[0x27, 0x02, k[0], k[1], k[2], k[3]]);
    assert_eq!(s.security(), (Some(0x01), Confidence::Confirmed));
    assert!(s.tick(ms(9_000)), "S3 elapsed without tester present → stale");
    assert_eq!(s.session().1, Confidence::Stale);
    // The sim really fell back to default: the write is refused with 0x7F.
    exchange(&mut ecu, &mut s, 9_001, &[0x2E, 0xF1, 0xA0, 0x02]);
    assert_eq!(ecu.session(), 0x01);
    exchange(&mut ecu, &mut s, 9_002, &[0x11, 0x01]);
    assert_eq!((s.session(), s.security().0), ((0x01, Confidence::Assumed), None));
    s.link_down("peer reset");
    s.link_up(ms(9_100));
    let p = serde_json::to_value(s.state_payload()).expect("serializes");
    assert_eq!(p["generation"], 2);
    assert_eq!(p["link"], json!({"state": "up", "cause": "reconnected"}));
    assert_eq!(p["session"]["confidence"], "assumed");
    assert!(p.get("lease").is_some() && p.get("tester_present").is_some());
}

#[test]
fn dry_run_shapes() {
    let ok = serde_json::to_value(dry_run("ecu1", &json!({"text": "read F190"}))).expect("json");
    assert_eq!(ok["bytes"], json!({"encoding": "hex", "data": "22f190"}));
    assert_eq!(
        (ok["sid"].clone(), ok["risk"].clone(), ok["policy_action"].clone()),
        (json!(0x22), json!("R0"), json!("auto"))
    );
    let deny = dry_run("ecu1", &json!({"bytes_hex": "2A0102"}));
    assert_eq!(
        (deny.block, deny.policy_action),
        (Some(BlockClass::ImmutableDeny), PolicyAction::Deny)
    );
    let bad = dry_run("ecu1", &json!({"text": "22 F1 9"}));
    assert_eq!(bad.parse_error.map(|e| e.at), Some(7));
    assert!(
        loom_uds::module::help_json()["nrc"]
            .as_array()
            .is_some_and(|a| a.len() > 50)
    );
}

#[test]
fn random_input_never_panics() {
    let mut x: u64 = 0x0C1A_551F;
    for _ in 0..100_000 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let b = x.to_le_bytes();
        let len = usize::from(b[7] % 8);
        let _ = classify_pdu(&b[..len], b[6] & 1 == 1);
        let text: String = b[..len]
            .iter()
            .map(|c| char::from(b"0123456789abcdefxrwtps \t"[usize::from(*c) % 24]))
            .collect();
        let _ = loom_uds::text::parse_text(&text);
    }
}
