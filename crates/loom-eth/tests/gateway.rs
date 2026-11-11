//! Gateway piece: registration, hooks, journal-backed handlers and evidence predicates.

mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use loom_domain_api::{Coverage, EvalBudget, Instances, Predicate, Registry, Verification};
use loom_eth::gateway::Eth;
use loom_eth::gateway::hooks::uds_risk;
use loom_proto::error::ErrorCode;
use loom_proto::event::{Risk, SeqRange};
use loom_testkit::{HookHarness, OwnedWriteRequest, RegistryProbe};
use serde_json::{Value, json};
use support::*;

fn registry() -> Registry {
    RegistryProbe::assert_ok(&Eth).0
}

/// A registry whose reducer saw `capture_state` events (feeds the shared channel book).
fn registry_with_channels(chans: &[(&str, &str, &str, u32)]) -> Registry {
    let mut reg = registry();
    let mut reducers = reg.take_reducers();
    let r = reducers.first_mut().expect("eth reducer");
    for (i, (ch, backend, iface, generation)) in chans.iter().enumerate() {
        let mut ev = record(
            i as u64 + 1,
            ch,
            "eth.capture_state",
            wall_now(),
            json!({"ch": ch, "state": "live", "backend": backend, "iface": iface}),
        );
        ev.generation = Some(*generation);
        r.apply(&ev);
    }
    reg
}

#[test]
fn registry_probe_passes() {
    let (reg, report) = RegistryProbe::assert_ok(&Eth);
    assert!(report.warnings.is_empty(), "{report}");
    for m in [
        "eth.ifaces",
        "eth.capture.start",
        "eth.capture.stop",
        "eth.capture.status",
        "eth.query",
        "eth.show",
        "eth.wait_for",
        "eth.stats",
        "eth.follow",
        "eth.send",
        "eth.export",
        "eth.filter.check",
    ] {
        assert!(report.commands.iter().any(|c| c == m), "{m} missing");
    }
    assert_eq!(
        report.write_commands,
        ["eth.capture.start", "eth.capture.stop", "eth.send"]
    );
    assert_eq!(
        report.event_kinds,
        [
            "eth.pkt",
            "eth.batch",
            "eth.tx",
            "eth.link",
            "eth.capture_state",
            "eth.stats"
        ]
    );
    for k in reg.event_kinds() {
        for p in &k.pointers {
            assert!(p.path.starts_with("/payload/"), "{}: {}", k.kind, p.path);
        }
    }
    let pkt = reg
        .event_kinds()
        .iter()
        .find(|k| k.kind == "eth.pkt")
        .expect("eth.pkt");
    for p in [
        "/payload/proto",
        "/payload/info",
        "/payload/len",
        "/payload/attributed_origin",
        "/payload/link/request_id",
    ] {
        assert!(pkt.pointers.iter().any(|x| x.path == p), "{p}");
    }
    let w = &reg.workers()[0];
    assert_eq!(w.kind, "eth");
    assert_eq!(w.instances, Instances::PerChannel { max: 4 });
    let wait = reg
        .commands()
        .iter()
        .find(|c| c.method == "eth.wait_for")
        .expect("wait_for");
    assert_eq!(wait.bounds.max_wait, Duration::from_secs(3600));
    assert!(
        reg.commands()
            .iter()
            .all(|c| c.bounds.max_rows == 1000 && c.bounds.default_rows == 100)
    );
    let ids: Vec<&str> = reg
        .panel_manifests()
        .iter()
        .filter_map(|p| p["id"].as_str())
        .collect();
    assert_eq!(ids, ["eth.packets", "eth.stream", "eth.stats"]);
    let stop = &reg.panel_manifests()[0]["actions"][1];
    assert_eq!(stop["safetyDirection"], "reduce");
    let help = reg.help_ref().expect("help");
    assert!(help.notes.iter().any(|n| n.contains("--bpf")));
    let preds = reg.evidence_verifiers()[0].predicates();
    assert_eq!(preds, ["eth.match", "eth.count", "eth.absent", "eth.period"]);
}

#[test]
fn uds_risk_table_matches_uds_plan() {
    assert_eq!(uds_risk(0x22, None, &[0xF1, 0x90]).0, Risk::R0);
    assert_eq!(uds_risk(0x10, Some(0x03), &[]).0, Risk::R1);
    assert_eq!(uds_risk(0x10, Some(0x02), &[]).0, Risk::R3);
    assert_eq!(uds_risk(0x11, Some(0x01), &[]).0, Risk::R2);
    assert_eq!(uds_risk(0x27, Some(0x01), &[]).0, Risk::R1);
    assert_eq!(uds_risk(0x27, Some(0x02), &[]).0, Risk::R3);
    assert_eq!(uds_risk(0x2E, None, &[0xF1, 0x90, 1]).0, Risk::R3);
    assert_eq!(uds_risk(0x2E, None, &[0x01, 0x00, 1]).0, Risk::R2);
    assert_eq!(uds_risk(0x31, None, &[0x01, 0xFF, 0x00]).0, Risk::R4);
    assert_eq!(uds_risk(0x31, None, &[0x01, 0x02, 0x03]).0, Risk::R3);
    assert_eq!(uds_risk(0x34, None, &[]).0, Risk::R4);
    assert_eq!(uds_risk(0x3E, Some(0x80), &[]).0, Risk::R0);
    assert_eq!(uds_risk(0x55, None, &[]).0, Risk::R4);
}

#[test]
fn hooks_classify_sends_and_stay_fast() {
    let reg = registry_with_channels(&[("sim0", "sim", "sim0", 1), ("dut", "live", "en5", 3)]);
    let h = HookHarness::new(reg.hooks_ref().cloned().expect("hooks"));
    let send = |ch: &str, args: Value| OwnedWriteRequest::new("eth.send", ch).args(args);
    let cases: Vec<(OwnedWriteRequest, Risk)> = vec![
        (
            send(
                "sim0",
                json!({"kind": "udp", "to": "192.168.0.10:13400", "hex": "00"}),
            ),
            Risk::R1,
        ),
        (
            send(
                "dut",
                json!({"kind": "udp", "to": "192.168.0.10:13400", "hex": "00"}),
            ),
            Risk::R2,
        ),
        (
            send("dut", json!({"kind": "udp", "to": "127.0.0.1:9", "hex": "00"})),
            Risk::R1,
        ),
        (
            send(
                "dut",
                json!({"kind": "udp", "to": "255.255.255.255:13400", "hex": "00"}),
            ),
            Risk::R3,
        ),
        (
            send(
                "dut",
                json!({"kind": "udp", "to": "224.224.224.245:30490", "hex": "00"}),
            ),
            Risk::R3,
        ),
        (
            send(
                "dut",
                json!({"kind": "raw", "hex": hex(&doip_uds(&[0x22, 0xF1, 0x90]))}),
            ),
            Risk::R2,
        ),
        (
            send(
                "dut",
                json!({"kind": "raw", "hex": hex(&doip_uds(&[0x11, 0x01]))}),
            ),
            Risk::R2,
        ),
        (
            send(
                "dut",
                json!({"kind": "raw", "hex": hex(&doip_uds(&[0x10, 0x02]))}),
            ),
            Risk::R3,
        ),
        (
            send(
                "sim0",
                json!({"kind": "raw", "hex": hex(&doip_uds(&[0x34, 0x00, 0x44]))}),
            ),
            Risk::R4,
        ),
        (
            send(
                "dut",
                json!({"kind": "raw", "hex": hex(&doip_uds(&[0x2E, 0xF1, 0x90, 0x41]))}),
            ),
            Risk::R3,
        ),
        (
            OwnedWriteRequest::new("eth.capture.start", "dut").args(json!({"iface": "en5"})),
            Risk::R0,
        ),
    ];
    for (req, want) in &cases {
        let c = h.classify(req);
        assert_eq!(
            c.value.risk,
            *want,
            "{:?} → {}",
            req.as_req().args,
            c.value.reason
        );
        assert!(c.value.block.is_none(), "F1: nothing blocked by default");
        // Hard limit of the hook contract (1 ms is only a warning; debug builds under load can pass it).
        assert!(
            c.elapsed < loom_testkit::hooks::FAIL,
            "classify took {:?}",
            c.elapsed
        );
    }
    // Inner UDS reason is shown.
    let c = h.classify(&cases[8].0);
    assert!(c.value.reason.contains("0x34"), "{}", c.value.reason);
}

#[test]
fn presend_checks_channel_and_generation() {
    let reg = registry_with_channels(&[("dut", "live", "en5", 3)]);
    let h = HookHarness::new(reg.hooks_ref().cloned().expect("hooks"));
    let mut req = OwnedWriteRequest::new("eth.send", "dut").args(json!({"kind": "raw", "hex": "00"}));
    req.target.generation = Some(3);
    assert!(h.presend_check(&req, None).value.is_ok());
    req.target.generation = Some(2);
    assert!(h.presend_check(&req, None).value.is_err(), "generation changed");
    let other = OwnedWriteRequest::new("eth.send", "nope").args(json!({"kind": "raw", "hex": "00"}));
    assert!(h.presend_check(&other, None).value.is_err(), "channel not open");
    let stop = OwnedWriteRequest::new("eth.capture.stop", "nope");
    assert!(
        h.presend_check(&stop, None).value.is_ok(),
        "stop is never refused"
    );
}

fn services(events: Vec<loom_proto::event::PublishedRecord>) -> Arc<Services> {
    let s = Arc::new(Services::default());
    *s.journal.events.lock().expect("lock") = events;
    s
}

/// 10 packets on `dut`: odd seqs are UDS 0x22 requests, even seqs SOME/IP-ish UDP noise; seq 9 is our own send.
fn bench() -> Vec<loom_proto::event::PublishedRecord> {
    let t0 = wall_now() - 10_000_000_000;
    (1..=10)
        .map(|i| {
            let f = if i % 2 == 1 {
                doip_uds(&[0x22, 0xF1, 0x90])
            } else {
                udp4(ECU, TESTER, 30501, 30501, &[0; 16])
            };
            pkt(i, "dut", t0 + i * 100_000_000, &f, (i == 9).then_some("agent"))
        })
        .collect()
}

#[test]
fn query_filters_and_pages() {
    let reg = registry();
    let svc = services(bench());
    let q = |p: Value| call(&reg, &svc, "eth.query", p, Duration::from_secs(5));

    let r = q(json!({"ch": "dut", "filter": "uds.sid == 0x22", "range": {"from": "seq:1", "to": "seq:5"}, "max": 2}))
        .expect("query");
    // schema v1: the latest `max` rows up to `to`; older matches are reachable via prev_cursor.
    let refs: Vec<&str> = r["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .filter_map(|x| x["ref"].as_str())
        .collect();
    assert_eq!(refs, ["eth:dut@seq:3", "eth:dut@seq:5"]);
    assert_eq!(r["truncated"], true);
    assert_eq!(r["next_cursor"], "seq:6");
    assert_eq!(r["prev_cursor"], "seq:2");

    // No `from`: the latest rows.
    let r = q(json!({"ch": "dut", "filter": "uds", "max": 2})).expect("latest");
    let seqs: Vec<&str> = r["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .filter_map(|x| x["seq"].as_str())
        .collect();
    assert_eq!(seqs, ["7", "9"]);
    assert_eq!(r["rows"][1]["attributed_origin"], "agent");

    // Contract cap: max is clamped to 1000.
    assert!(q(json!({"ch": "dut", "max": 5000})).is_ok());
    // Filter errors carry the position.
    let e = q(json!({"ch": "dut", "filter": "uds.sid =="})).expect_err("bad filter");
    assert_eq!(e.code, ErrorCode::Usage);
    // No channel and none known → not open.
    let e = q(json!({})).expect_err("no channel");
    assert_eq!(e.code, ErrorCode::NotOpen);
}

#[test]
fn show_builds_a_tree() {
    let reg = registry();
    let svc = services(bench());
    let r = call(
        &reg,
        &svc,
        "eth.show",
        json!({"ref": "eth:dut@seq:3"}),
        Duration::from_secs(5),
    )
    .expect("show");
    assert_eq!(r["ref"], "eth:dut@seq:3");
    let names: Vec<&str> = r["tree"]
        .as_array()
        .expect("tree")
        .iter()
        .filter_map(|n| n["name"].as_str())
        .collect();
    assert!(names.contains(&"doip") && names.contains(&"uds"), "{names:?}");
    let e = call(
        &reg,
        &svc,
        "eth.show",
        json!({"ref": "eth:dut@seq:99"}),
        Duration::from_secs(5),
    )
    .expect_err("missing");
    assert_eq!(e.code, ErrorCode::NotFound);
    let e = call(
        &reg,
        &svc,
        "eth.show",
        json!({"ref": "dut:3"}),
        Duration::from_secs(5),
    )
    .expect_err("bad ref");
    assert_eq!(e.code, ErrorCode::Usage);
}

#[test]
fn stats_filter_check_and_preview() {
    let reg = registry();
    let svc = services(bench());
    let r = call(
        &reg,
        &svc,
        "eth.stats",
        json!({"ch": "dut", "by": "proto", "range": {"from": "seq:1"}}),
        Duration::from_secs(5),
    )
    .expect("stats");
    assert_eq!(r["total_packets"], 10);
    let ok = call(
        &reg,
        &svc,
        "eth.filter.check",
        json!({"expr": "doip && uds.sid == 0x22"}),
        Duration::from_secs(1),
    )
    .expect("check");
    assert_eq!(ok["ok"], true);
    let bad = call(
        &reg,
        &svc,
        "eth.filter.check",
        json!({"expr": "tcp.port =="}),
        Duration::from_secs(1),
    )
    .expect("check");
    assert_eq!(bad["ok"], false);
    assert!(bad["error"]["pos"].is_u64());

    let p = call(
        &reg,
        &svc,
        "eth.send",
        json!({"ch": "dut", "kind": "raw", "hex": hex(&doip_uds(&[0x11, 0x01])), "dry_run": true}),
        Duration::from_secs(1),
    )
    .expect("preview");
    assert_eq!(p["risk"], "R2");
    assert!(p["proto"].as_str().is_some_and(|x| !x.is_empty()));
    assert!(
        svc.worker.calls.lock().expect("lock").is_empty(),
        "preview never reaches the worker"
    );
    // Dry run through eth.send also stays local.
    call(
        &reg,
        &svc,
        "eth.send",
        json!({"ch": "dut", "kind": "raw", "hex": "00112233445566778899aabbccdd", "dry_run": true}),
        Duration::from_secs(1),
    )
    .expect("dry run");
    assert!(svc.worker.calls.lock().expect("lock").is_empty());
    // A real send goes to the worker.
    call(
        &reg,
        &svc,
        "eth.send",
        json!({"ch": "dut", "kind": "udp", "to": "192.168.0.10:13400", "hex": "00"}),
        Duration::from_secs(1),
    )
    .expect("send");
    assert_eq!(svc.worker.calls.lock().expect("lock").len(), 1);
}

#[test]
fn wait_for_counts_new_packets_only() {
    let reg = registry();
    // Old packets (before the command) never satisfy a wait.
    let mut evs = bench();
    let now = wall_now();
    evs.push(pkt(
        11,
        "dut",
        now + 1_000_000_000,
        &doip_uds(&[0x22, 0xF1, 0x90]),
        None,
    ));
    evs.push(pkt(
        12,
        "dut",
        now + 1_100_000_000,
        &doip_uds(&[0x22, 0xF1, 0x90]),
        Some("agent"),
    ));
    let svc = services(evs);
    let w = |p: Value| call(&reg, &svc, "eth.wait_for", p, Duration::from_secs(5));

    let r = w(json!({"ch": "dut", "filter": "uds.sid == 0x22", "timeout_ms": 2000})).expect("matched");
    assert_eq!(r["matched"], true);
    assert_eq!(r["count"], 1, "own send (seq 12) is not evidence");
    assert_eq!(r["evidence"][0]["matched_refs"][0], "eth:dut@seq:11");

    let started = Instant::now();
    let e = w(json!({"ch": "dut", "filter": "uds.sid == 0x22", "count": {"min": 2}, "timeout_ms": 300}))
        .expect_err("timeout");
    assert_eq!(e.code, ErrorCode::Timeout);
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(e.detail["count"], 1);

    // include_own counts our send too.
    let r = w(json!({"ch": "dut", "filter": "uds.sid == 0x22", "count": {"min": 2}, "include_own": true, "timeout_ms": 1000}))
        .expect("with own");
    assert_eq!(r["count"], 2);

    // Anchored window: packets of the bench within 250 ms after seq 3 → seq 5 only (seq 4 is not UDS).
    let r = w(
        json!({"ch": "dut", "filter": "uds.sid == 0x22", "anchor": "eth:dut@seq:3", "within_ms": 250,
                     "count": {"min": 1, "max": 2}, "timeout_ms": 1000}),
    )
    .expect("anchored");
    assert_eq!(r["count"], 2, "seq 3 and seq 5: {r}");
}

fn pred(name: &str, args: Value) -> Predicate {
    Predicate {
        name: name.into(),
        args,
        expect_id: None,
    }
}

fn cov(complete: bool) -> Coverage {
    Coverage {
        window: SeqRange {
            from: 1.into(),
            to: 20.into(),
        },
        complete,
        causes: Vec::new(),
    }
}

#[test]
fn evidence_predicates() {
    let reg = registry();
    let v = &reg.evidence_verifiers()[0];
    let evs = bench();
    let run = |p: &Predicate, evs: &[loom_proto::event::PublishedRecord], c: bool| {
        v.verify(p, evs, &cov(c), &mut EvalBudget::default())
    };

    let m = run(
        &pred("eth.match", json!({"filter": "uds.sid == 0x22"})),
        &evs,
        true,
    );
    let Verification::Match { matched, observed } = m else {
        panic!("match expected: {m:?}")
    };
    assert_eq!(matched.len(), 4, "seq 9 (own send) excluded");
    assert_eq!(observed["excluded_own"], 1);

    let c = run(
        &pred("eth.count", json!({"filter": "uds", "min": 4, "max": 4})),
        &evs,
        true,
    );
    assert!(matches!(c, Verification::Match { .. }), "{c:?}");
    let c = run(&pred("eth.count", json!({"filter": "uds", "min": 5})), &evs, true);
    assert!(matches!(c, Verification::Mismatch { .. }), "{c:?}");

    let a = run(
        &pred("eth.absent", json!({"filter": "uds.sid == 0x11"})),
        &evs,
        true,
    );
    assert!(matches!(a, Verification::Match { .. }), "{a:?}");
    let a = run(
        &pred("eth.absent", json!({"filter": "uds.sid == 0x22"})),
        &evs,
        true,
    );
    assert!(matches!(a, Verification::Mismatch { .. }), "{a:?}");

    // Period: UDS requests every 200 ms (seq 1,3,5,7; 9 is our own).
    let p = run(
        &pred(
            "eth.period",
            json!({"filter": "uds", "period_ms": 200, "tol_ms": 5}),
        ),
        &evs,
        true,
    );
    assert!(matches!(p, Verification::Match { .. }), "{p:?}");
    let p = run(
        &pred(
            "eth.period",
            json!({"filter": "uds", "period_ms": 100, "tol_ms": 5}),
        ),
        &evs,
        true,
    );
    assert!(matches!(p, Verification::Mismatch { .. }), "{p:?}");

    // A loss in the window: absence and count become Inconclusive, a positive match still holds.
    let mut lossy = evs.clone();
    lossy.insert(
        4,
        record(
            100,
            "dut",
            "source.gap",
            wall_now(),
            json!({"cause": "kernel_drop"}),
        ),
    );
    let a = run(
        &pred("eth.absent", json!({"filter": "uds.sid == 0x11"})),
        &lossy,
        true,
    );
    assert!(matches!(a, Verification::Inconclusive { .. }), "{a:?}");
    let c = run(
        &pred("eth.count", json!({"filter": "uds", "min": 4, "max": 4})),
        &lossy,
        true,
    );
    assert!(matches!(c, Verification::Inconclusive { .. }), "{c:?}");
    let m = run(&pred("eth.match", json!({"filter": "uds"})), &lossy, true);
    assert!(matches!(m, Verification::Match { .. }), "{m:?}");
    // Incomplete coverage alone also blocks absence.
    let a = run(
        &pred("eth.absent", json!({"filter": "uds.sid == 0x11"})),
        &evs,
        false,
    );
    assert!(matches!(a, Verification::Inconclusive { .. }), "{a:?}");
    // Over the maximum is final even with losses.
    let c = run(
        &pred("eth.count", json!({"filter": "uds", "max": 2})),
        &lossy,
        true,
    );
    assert!(matches!(c, Verification::Mismatch { .. }), "{c:?}");

    // The budget is respected.
    let mut tiny = EvalBudget::new(3, Duration::from_secs(1));
    let b = v.verify(
        &pred("eth.absent", json!({"filter": "uds"})),
        &evs,
        &cov(true),
        &mut tiny,
    );
    assert!(matches!(b, Verification::Inconclusive { .. }), "{b:?}");

    // Coverage needs.
    assert_eq!(
        v.coverage_need(&pred("eth.match", json!({}))),
        loom_domain_api::CoverageNeed::Positive
    );
    assert_eq!(
        v.coverage_need(&pred("eth.absent", json!({}))),
        loom_domain_api::CoverageNeed::Complete
    );
    assert_eq!(
        v.coverage_need(&pred("eth.period", json!({}))),
        loom_domain_api::CoverageNeed::Measure
    );
}

#[test]
fn reducer_snapshot_and_summarizer() {
    let mut reg = registry();
    let mut reducers = reg.take_reducers();
    let r = reducers.first_mut().expect("reducer");
    for e in bench() {
        r.apply(&e);
    }
    r.apply(&record(
        20,
        "dut",
        "eth.capture_state",
        wall_now(),
        json!({"ch": "dut", "state": "live", "backend": "sim", "iface": "sim0"}),
    ));
    let snap = r.snapshot(&Default::default());
    assert_eq!(snap[0]["state"], "live");
    assert_eq!(
        snap.last().expect("rows")["rows"].as_array().map(Vec::len),
        Some(10)
    );
    let s = reg
        .display_summarizer_ref()
        .expect("summarizer")
        .summarize(&bench());
    assert_eq!(s["packets"], 10);
    assert_eq!(s["own"], 1);
}

#[test]
fn export_round_trips_loom_meta_and_comments() {
    use loom_eth_pcapng::{Block, Direction, Origin as POrigin, PcapNgReader};
    let reg = registry();
    let t0 = wall_now();
    // seq 1: device packet; seq 2: our own send (agent, request q-9); seq 3: replayed frame with
    // user comments and the original file's X10 meta.
    let mut evs = vec![
        pkt(1, "dut", t0, &doip_uds(&[0x22, 0xF1, 0x90]), None),
        pkt(2, "dut", t0 + 1_000, &doip_uds(&[0x11, 0x01]), Some("agent")),
        pkt(
            3,
            "dut",
            t0 + 2_000,
            &udp4(ECU, TESTER, 30501, 30501, &[1, 2, 3]),
            None,
        ),
    ];
    evs[1].payload["dir"] = json!("tx");
    evs[1].payload["link"] =
        json!({"domain": "uds", "request_id": "q-9", "role": "request", "confidence": "exact"});
    evs[2].payload["bytes"]["comments"] = json!(["bench note", "loom1 seq=5 dir=rx org=user"]);
    evs[2].payload["bytes"]["loom_comment"] = json!("loom1 seq=12 dir=tx org=agent req=q-7");
    let svc = services(evs);
    let path = std::env::temp_dir().join(format!("loom-eth-export-test-{}.pcapng", std::process::id()));
    let r = call(
        &reg,
        &svc,
        "eth.export",
        json!({"ch": "dut", "format": "pcapng", "range": {"from": "seq:1"}, "path": path.display().to_string()}),
        Duration::from_secs(5),
    )
    .expect("export");
    assert_eq!(r["packets"], 3);
    assert_eq!(r["comments_dropped"], 0);

    let file = std::fs::File::open(&path).expect("exported file");
    let mut rd = PcapNgReader::new(std::io::BufReader::new(file));
    let mut pkts = Vec::new();
    while let Some(b) = rd.next_block().expect("valid pcapng") {
        if let Block::Packet(p) = b {
            pkts.push(p);
        }
    }
    let _ = std::fs::remove_file(&path);
    assert_eq!(pkts.len(), 3);
    // Device packet: no X10 (no org=device), no comments.
    assert!(pkts[0].meta.is_none() && pkts[0].comments.is_empty());
    // Own packet: X10 with the *stream* seq (REF coordinate), direction, origin, request id.
    let m = pkts[1].meta.as_ref().expect("own packet meta");
    assert_eq!(
        (m.seq, m.dir, m.org, m.req.as_deref()),
        (2, Direction::Tx, POrigin::Agent, Some("q-9"))
    );
    // Replayed frame: original X10 kept as loom1-orig (never re-read as attribution), user
    // comments verbatim, and a user comment that looked like X10 is also demoted to loom1-orig.
    assert!(pkts[2].meta.is_none());
    assert_eq!(
        pkts[2].comments,
        [
            "loom1-orig seq=12 dir=tx org=agent req=q-7",
            "bench note",
            "loom1-orig seq=5 dir=rx org=user"
        ]
    );
    assert_eq!(pkts[2].data.len(), 14 + 20 + 8 + 3);
}

#[test]
fn send_options_are_kind_specific_and_tcp_dry_run_keeps_its_shape() {
    let reg = registry();
    let svc = services(Vec::new());
    let send = |p: Value| call(&reg, &svc, "eth.send", p, Duration::from_secs(1));
    let base = json!({"ch": "dut", "kind": "tcp", "to": "192.168.0.10:13400", "hex": hex(&doip(0x0005, &[0x0E, 0, 0, 0, 0, 0, 0])), "dry_run": true});
    let plain = send(base.clone()).expect("tcp dry run");
    let mut ko = base.clone();
    ko["keep_open"] = json!(true);
    let kept = send(ko).expect("tcp keep_open dry run");
    let keys = |v: &Value| {
        v.as_object()
            .map(|o| o.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default()
    };
    assert_eq!(keys(&plain), keys(&kept));
    assert_eq!(
        (plain["proto"].clone(), plain["risk"].clone()),
        (kept["proto"].clone(), kept["risk"].clone())
    );
    for bad in [
        json!({"ch": "dut", "kind": "udp", "to": "192.168.0.10:13400", "hex": "00", "keep_open": true, "dry_run": true}),
        json!({"ch": "dut", "kind": "raw", "hex": "00", "keep_open": true}),
        json!({"ch": "dut", "kind": "tcp", "to": "192.168.0.10:13400", "hex": "00", "from_port": 5000, "dry_run": true}),
    ] {
        let e = send(bad.clone()).expect_err("kind-specific option");
        assert_eq!(e.code, ErrorCode::Usage, "{bad}");
    }
    assert!(
        svc.worker.calls.lock().expect("lock").is_empty(),
        "nothing reached the worker"
    );
}
