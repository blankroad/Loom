//! Deterministic sim tests (manual clock). Times are in ms unless noted.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use loom_can_frame::{CanFrame, CanId, Direction, parse_frame};
use loom_can_sim::{
    EndpointId, HubLimits, LocalReceiver, ManualClock, NodeConfig, NodeState, Origin, Periodic, ReplyData,
    Rule, Sender, SimError, SimHub, SimRecord, parse_nodes,
};

const MS: u64 = 1_000_000;
const T0: u64 = 1_000 * MS;

fn f(text: &str) -> CanFrame {
    parse_frame(text, false).unwrap()
}

fn bcm() -> NodeConfig {
    let text =
        std::fs::read_to_string(format!("{}/tests/fixtures/bcm.toml", env!("CARGO_MANIFEST_DIR"))).unwrap();
    parse_nodes(&text).unwrap().remove(0)
}

fn hub() -> (SimHub, ManualClock) {
    let clock = ManualClock::new(T0);
    (SimHub::new(clock.clone(), HubLimits::default()), clock)
}

/// (seq, origin, dir, frame text, time since T0 in µs)
fn view(recs: &[SimRecord]) -> Vec<(u64, Origin, Direction, String, u64)> {
    recs.iter()
        .map(|r| {
            (
                r.source_seq,
                r.origin,
                r.frame.dir,
                r.frame.frame.to_string(),
                (r.frame.ingest_mono_ns - T0) / 1_000,
            )
        })
        .collect()
}

fn all(rx: &LocalReceiver) -> Vec<SimRecord> {
    rx.recv(usize::MAX)
}

#[test]
fn walking_skeleton_scenario() {
    // Plan §2.0, reduced: agent request → fake ECU reply → user request → reply.
    let (mut hub, clock) = hub();
    hub.start_node(bcm()).unwrap();
    let (agent, a_rx) = hub.attach_local("sim1", "agent:claude-1", Origin::Agent).unwrap();
    let (user, b_rx) = hub.attach_local("sim1", "user:web", Origin::User).unwrap();

    assert_eq!(hub.send(agent, f("300#01")).unwrap(), 1);
    clock.advance_ms(4);
    hub.poll();
    clock.advance_ms(1);
    hub.poll();
    assert_eq!(hub.send(user, f("300#00")).unwrap(), 3);
    clock.advance_ms(5);
    hub.poll();

    let expected = vec![
        (1, Origin::Agent, Direction::Tx, "300#01".to_string(), 0),
        (2, Origin::Device, Direction::Rx, "301#01".to_string(), 5_000),
        (3, Origin::User, Direction::Tx, "300#00".to_string(), 5_000),
        (4, Origin::Device, Direction::Rx, "301#00".to_string(), 10_000),
    ];
    let a = all(&a_rx);
    let b = all(&b_rx);
    assert_eq!(view(&a), expected);
    // Observer and sender see exactly the same records (Tx echo included).
    assert_eq!(a, b);
    assert_eq!(a[0].sender, Sender::Endpoint(agent));
    assert!(matches!(a[1].sender, Sender::Node(_)));
    assert!(a.iter().all(|r| r.frame.device_ts.is_none()));
}

#[test]
fn periodic_interval_and_follow() {
    let (mut hub, clock) = hub();
    hub.start_node(bcm()).unwrap();
    let (agent, rx) = hub.attach_local("sim1", "agent", Origin::Agent).unwrap();
    // One coarse step: events still get their exact deadline times.
    clock.advance_ms(1_000);
    hub.poll();
    let recs = all(&rx);
    let times: Vec<u64> = recs.iter().map(|r| (r.frame.ingest_mono_ns - T0) / MS).collect();
    assert_eq!(times, [50, 150, 250, 350, 450, 550, 650, 750, 850, 950]);
    assert!(recs.iter().all(|r| r.frame.frame.data()[0] == 0));

    // byte0 follows the last 0x300 byte0.
    hub.send(agent, f("300#07")).unwrap();
    clock.advance_ms(100);
    hub.poll();
    let recs = all(&rx);
    let status: Vec<_> = recs
        .iter()
        .filter(|r| r.frame.frame.id() == CanId::standard(0x310).unwrap())
        .collect();
    assert_eq!(status.len(), 1);
    assert_eq!(status[0].frame.frame.data()[0], 0x07);
    assert_eq!(hub.next_deadline_ns(), Some(T0 + 1_150 * MS));
}

#[test]
fn seq_is_monotonic_and_same_name_shares_the_bus() {
    let (mut hub, clock) = hub();
    hub.start_node(bcm()).unwrap();
    let (ch1, rx1) = hub.attach_local("sim1", "ch1", Origin::Agent).unwrap();
    let (_ch2, rx2) = hub.attach_local("sim1", "ch2", Origin::Script).unwrap();
    let (other, rx3) = hub.attach_local("sim2", "other", Origin::Agent).unwrap();
    assert_eq!(ch1.bus(), _ch2.bus());
    assert_ne!(ch1.bus(), other.bus());

    for i in 0..200u64 {
        hub.send(ch1, f(&format!("300#{:02X}", i % 256))).unwrap();
        clock.advance_ns(700_000 + i * 13_000);
        hub.poll();
    }
    clock.advance_ms(20);
    hub.poll();
    let r1 = all(&rx1);
    let r2 = all(&rx2);
    assert_eq!(r1, r2);
    let seqs: Vec<u64> = r1.iter().map(|r| r.source_seq).collect();
    assert_eq!(seqs, (1..=r1.len() as u64).collect::<Vec<_>>());
    assert!(
        r1.windows(2)
            .all(|w| w[0].frame.ingest_mono_ns <= w[1].frame.ingest_mono_ns)
    );
    assert_eq!(
        r1.iter().filter(|r| r.origin == Origin::Device).count(),
        r1.len() - 200
    );

    // The other bus has its own numbering and saw nothing from sim1.
    hub.send(other, f("123#")).unwrap();
    let r3 = all(&rx3);
    assert_eq!(view(&r3).len(), 1);
    assert_eq!(r3[0].source_seq, 1);
    let st = hub.bus_stats("sim1").unwrap();
    assert_eq!(st.last_seq, r1.len() as u64);
    assert_eq!(st.frames_total, r1.len() as u64);
    assert_eq!(st.frames_by_origin, [200, 0, r1.len() as u64 - 200, 0]);
}

#[test]
fn stop_latch() {
    let (mut hub, clock) = hub();
    hub.start_node(bcm()).unwrap();
    let (agent, rx) = hub.attach_local("sim1", "agent", Origin::Agent).unwrap();
    let (script, _) = hub.attach_local("sim1", "script", Origin::Script).unwrap();
    let (user, _) = hub.attach_local("sim1", "user", Origin::User).unwrap();

    hub.latch("sim1", &[Origin::Agent, Origin::Script, Origin::Device])
        .unwrap();
    assert_eq!(
        hub.send(agent, f("300#01")),
        Err(SimError::Latched {
            origin: Origin::Agent
        })
    );
    assert!(hub.send(script, f("300#01")).is_err());
    // The user can still intervene, and the fake ECU keeps answering and sending.
    hub.send(user, f("300#05")).unwrap();
    clock.advance_ms(60);
    hub.poll();
    let ids: Vec<String> = all(&rx).iter().map(|r| r.frame.frame.to_string()).collect();
    assert_eq!(ids, ["300#05", "301#05", "310#0500000000000000"]);

    let st = hub.bus_stats("sim1").unwrap();
    assert_eq!(st.latched, [Origin::Agent, Origin::Script]); // Device cannot be latched
    assert_eq!(st.latch_rejected, [1, 0, 0, 1]);
    // Still latched until released explicitly.
    clock.advance_ms(1_000);
    hub.poll();
    assert!(hub.send(agent, f("300#01")).is_err());
    hub.release("sim1").unwrap();
    assert!(hub.send(agent, f("300#01")).is_ok());
    assert!(hub.bus_stats("sim1").unwrap().latched.is_empty());
}

#[test]
fn slow_endpoint_drops_alone() {
    let (mut hub, _clock) = hub();
    let (agent, fast) = hub.attach_local("quiet", "agent", Origin::Agent).unwrap();
    let (slow_id, slow) = hub
        .attach_local_with_capacity("quiet", "slow-ui", Origin::User, 4)
        .unwrap();
    for i in 0..10 {
        hub.send(agent, f(&format!("100#{i:02X}"))).unwrap();
    }
    assert_eq!(fast.len(), 10);
    let got: Vec<u64> = slow.recv(100).iter().map(|r| r.source_seq).collect();
    assert_eq!(got, [1, 2, 3, 4]);
    for i in 0..6 {
        hub.send(agent, f(&format!("100#{i:02X}"))).unwrap();
    }
    let st = hub.bus_stats("quiet").unwrap();
    let ep = st.endpoints.iter().find(|e| e.id == slow_id).unwrap();
    assert_eq!(ep.delivered, 8);
    assert_eq!(ep.drops.total, 8);
    let gaps: Vec<(u64, u64, u64)> = ep
        .drops
        .gaps
        .iter()
        .map(|g| (g.first_seq, g.last_seq, g.count))
        .collect();
    assert_eq!(gaps, [(5, 10, 6), (15, 16, 2)]);
    // The fast endpoint lost nothing.
    let fast_seqs: Vec<u64> = all(&fast).iter().map(|r| r.source_seq).collect();
    assert_eq!(fast_seqs, (1..=16).collect::<Vec<_>>());
    assert!(
        st.endpoints
            .iter()
            .find(|e| e.name == "agent")
            .unwrap()
            .drops
            .total
            == 0
    );

    // Throttle fault: the consumer reads one record per call.
    slow.set_throttle(Some(1));
    assert_eq!(slow.recv(100).len(), 1);
    slow.set_throttle(None);
    assert_eq!(slow.recv(100).len(), 3);
}

#[test]
fn faults() {
    let mut cfg = bcm();
    cfg.faults.drop_ids = vec![CanId::standard(0x310).unwrap()];
    cfg.faults.extra_delay_ns = 3 * MS;
    cfg.faults.crash_after_ns = Some(250 * MS);
    let (mut hub, clock) = hub();
    let node = hub.start_node(cfg).unwrap();
    let (agent, rx) = hub.attach_local("sim1", "agent", Origin::Agent).unwrap();
    hub.send(agent, f("300#01")).unwrap();
    clock.advance_ms(7);
    hub.poll();
    assert_eq!(all(&rx).len(), 1); // reply not yet: 5 + 3 ms
    clock.advance_ms(1);
    hub.poll();
    assert_eq!(view(&all(&rx))[0].4, 8_000);
    clock.advance_ms(1_000);
    hub.poll();
    assert!(all(&rx).is_empty()); // 0x310 dropped by fault, then crash at 250 ms
    let info = hub.nodes().into_iter().find(|n| n.id == node).unwrap();
    assert_eq!(info.state, NodeState::Crashed);
    assert_eq!(info.fault_dropped, 2); // 0x310 at 50 and 150 ms
    assert_eq!(info.pending, 0);
    hub.send(agent, f("300#02")).unwrap();
    clock.advance_ms(20);
    hub.poll();
    assert_eq!(all(&rx).len(), 1); // only the echo; the crashed node is silent

    // Stop / crash API and restart in the same slot.
    let node2 = hub.start_node(bcm()).unwrap();
    hub.stop_node(node2).unwrap();
    assert!(
        hub.nodes()
            .iter()
            .any(|n| n.id == node2 && n.state == NodeState::Stopped)
    );
    assert_eq!(hub.next_deadline_ns(), None);
    assert_eq!(hub.stop_node(node2), Ok(()));
    let node3 = hub.start_node(bcm()).unwrap();
    hub.crash_node(node3).unwrap();
    hub.remove_node(node3).unwrap();
    assert_eq!(hub.crash_node(node3), Err(SimError::UnknownNode));
}

#[test]
fn jitter_is_seeded() {
    let delays = |seed: u64| {
        let mut cfg = bcm();
        cfg.seed = seed;
        cfg.periodic.clear();
        cfg.faults.jitter_ns = 2 * MS;
        let (mut hub, clock) = hub();
        hub.start_node(cfg).unwrap();
        let (agent, rx) = hub.attach_local("sim1", "agent", Origin::Agent).unwrap();
        let mut out = Vec::new();
        for _ in 0..20 {
            let sent = hub.now_ns();
            hub.send(agent, f("300#01")).unwrap();
            clock.advance_ms(10);
            hub.poll();
            let recs = all(&rx);
            out.push(recs[1].frame.ingest_mono_ns - sent);
        }
        out
    };
    let a = delays(42);
    assert_eq!(a, delays(42));
    assert_ne!(a, delays(43));
    assert!(a.iter().all(|d| (5 * MS..=7 * MS).contains(d)));
}

#[test]
fn limits_and_overflow() {
    let (mut hub, clock) = hub();
    let mut ids: Vec<EndpointId> = Vec::new();
    for i in 0..16 {
        ids.push(hub.attach_local("b", &format!("e{i}"), Origin::User).unwrap().0);
    }
    assert!(matches!(
        hub.attach_local("b", "e16", Origin::User),
        Err(SimError::TooManyEndpoints { limit: 16, .. })
    ));
    // A detached id stays invalid even when its slot is reused.
    hub.detach(ids[3]).unwrap();
    let reused = hub.attach_local("b", "new", Origin::User).unwrap().0;
    assert_eq!(hub.send(ids[3], f("1#")), Err(SimError::UnknownEndpoint));
    assert!(hub.send(reused, f("1#")).is_ok());

    for i in 0..15 {
        hub.attach_local(&format!("bus{i}"), "x", Origin::User).unwrap();
    }
    assert!(matches!(
        hub.attach_local("bus15", "x", Origin::User),
        Err(SimError::TooManyBuses { limit: 16 })
    ));

    // Node deadline queue: 2 pending replies at most.
    let limits = HubLimits {
        max_pending_per_node: 2,
        max_events_per_poll: 3,
        ..HubLimits::default()
    };
    let clock2 = ManualClock::new(0);
    let mut hub2 = SimHub::new(clock2.clone(), limits);
    let mut cfg = bcm();
    cfg.periodic.clear();
    let node = hub2.start_node(cfg).unwrap();
    let (agent, rx) = hub2.attach_local("sim1", "agent", Origin::Agent).unwrap();
    for _ in 0..5 {
        hub2.send(agent, f("300#01")).unwrap();
    }
    let info = hub2.nodes().into_iter().find(|n| n.id == node).unwrap();
    assert_eq!((info.pending, info.deadline_overflow), (2, 3));
    clock2.advance_ms(5);
    assert_eq!(hub2.poll().events, 2);
    assert_eq!(all(&rx).len(), 7);

    // max_events_per_poll: periodic 1 ms over 10 ms → 10 events, 3 per poll.
    let mut fast = NodeConfig::new("FAST", "sim9");
    fast.periodic.push(Periodic {
        id: CanId::standard(0x10).unwrap(),
        period_ns: MS,
        offset_ns: 0,
        data: vec![1],
        fd: false,
        follow: vec![],
    });
    hub2.start_node(fast).unwrap();
    clock2.advance_ms(9);
    let r = hub2.poll();
    assert_eq!((r.events, r.more_pending), (3, true));
    let mut total = 3;
    loop {
        let r = hub2.poll();
        total += r.events;
        if !r.more_pending {
            break;
        }
    }
    assert_eq!(total, 10);

    let mut too_many = SimHub::new(clock.clone(), HubLimits::default());
    for _ in 0..16 {
        too_many.start_node(bcm()).unwrap();
    }
    assert_eq!(
        too_many.start_node(bcm()),
        Err(SimError::TooManyNodes { limit: 16 })
    );
}

#[test]
fn rule_matching_and_fixed_replies() {
    let mut cfg = NodeConfig::new("GW", "m");
    cfg.rules.push(Rule {
        id: CanId::standard(0x7E0).unwrap(),
        data: vec![0x02, 0x10],
        mask: vec![0xFF, 0xF0],
        reply_id: CanId::standard(0x7E8).unwrap(),
        reply: ReplyData::Fixed(vec![0x02, 0x50, 0x01]),
        delay_ns: 0,
        fd: false,
    });
    cfg.rules.push(Rule {
        id: CanId::extended(0x18DA_F110).unwrap(),
        data: vec![],
        mask: vec![],
        reply_id: CanId::extended(0x18DA_10F1).unwrap(),
        reply: ReplyData::Copy,
        delay_ns: MS,
        fd: false,
    });
    let (mut hub, clock) = hub();
    hub.start_node(cfg).unwrap();
    let (a, rx) = hub.attach_local("m", "a", Origin::Agent).unwrap();
    hub.send(a, f("7E0#021F")).unwrap(); // 0x1F & 0xF0 = 0x10 → match
    hub.send(a, f("7E0#0220")).unwrap(); // no match
    hub.send(a, f("7E0#02")).unwrap(); // too short → no match
    hub.send(a, f("18DAF110##1AABBCCDDEEFF001122334455")).unwrap(); // FD 12 bytes, copied as FD
    clock.advance_ms(1);
    hub.poll();
    let v: Vec<String> = all(&rx)
        .iter()
        .filter(|r| r.origin == Origin::Device)
        .map(|r| r.frame.frame.to_string())
        .collect();
    assert_eq!(v, ["7E8#025001", "18DA10F1##1AABBCCDDEEFF001122334455"]);
}

#[test]
fn config_errors() {
    let bad = |toml: &str| parse_nodes(toml).unwrap_err();
    let base = "[[node]]\nname = \"N\"\nchannel = \"c\"\n";
    let e = bad(&format!(
        "{base}[[node.on]]\nid = 1\nreply_id = 2\nreply = \"0G\"\n"
    ));
    assert_eq!(e.path, "node[0].on[0].reply");
    let e = bad(&format!(
        "{base}[[node.on]]\nid = 1\nreply_id = 2\nreply = \"copy\"\ndata = \"0102\"\nmask = \"FF\"\n"
    ));
    assert!(e.path.ends_with(".on[0].mask"), "{e}");
    let e = bad(&format!(
        "{base}[[node.periodic]]\nid = 1\nperiod_ms = 0.01\ndata = \"00\"\n"
    ));
    assert!(e.path.ends_with("period_ms"), "{e}");
    let e = bad(&format!(
        "{base}[[node.periodic]]\nid = 1\nperiod_ms = 10\ndata = \"00\"\nfollow = [{{ source_id = 2, source_byte = 0, byte = 3 }}]\n"
    ));
    assert!(e.path.ends_with(".follow"), "{e}");
    let e = bad(&format!(
        "{base}[[node.periodic]]\nid = 1\nperiod_ms = 10\ndata = \"000000000000000000\"\n"
    ));
    assert!(e.message.contains("Classic CAN"), "{e}");
    bad(&format!("{base}unknown_key = 1\n"));
    bad(&format!(
        "{base}[[node.on]]\nid = 0x20000000\next = true\nreply_id = 1\nreply = \"copy\"\n"
    ));
    let e = bad(&" ".repeat(64 * 1024 + 1));
    assert!(e.message.contains("larger than"));
    let many: String = (0..17)
        .map(|i| format!("[[node]]\nname = \"N{i}\"\nchannel = \"c\"\n"))
        .collect();
    assert!(bad(&many).message.contains("more than 16"));
    // Valid FD periodic with 29-bit id and faults.
    let ok = parse_nodes(&format!(
        "{base}[[node.periodic]]\nid = 0x18FEF100\nperiod_ms = 10\ndata = \"{}\"\nfd = true\n[node.faults]\ndrop_ids = [0x10]\njitter_ms = 0.5\n",
        "00".repeat(64)
    ))
    .unwrap();
    assert!(ok[0].periodic[0].id.is_extended());
    assert_eq!(ok[0].faults.jitter_ns, 500_000);
}

#[test]
fn hub_is_send() {
    fn send<T: Send>() {}
    fn sync<T: Send + Sync>() {}
    send::<SimHub>();
    sync::<LocalReceiver>();
}

#[test]
fn throughput_smoke() {
    // 1 s of bus time: agent 10k fps + ECU replies 10k fps = 20k fps, two observers
    // drained every 1,000 sends. Must be lossless and stay inside the queue bound.
    let (mut hub, clock) = hub();
    let mut cfg = bcm();
    cfg.rules[0].delay_ns = 50_000;
    hub.start_node(cfg).unwrap();
    let (agent, a_rx) = hub.attach_local("sim1", "agent", Origin::Agent).unwrap();
    let (_ui, ui_rx) = hub.attach_local("sim1", "ui", Origin::User).unwrap();
    let start = std::time::Instant::now();
    let mut seen = Vec::with_capacity(4_096);
    let mut last_seq = 0;
    let mut total = 0usize;
    let mut max_len = 0;
    for i in 0..10_000u32 {
        hub.send(agent, f(&format!("300#{:02X}", i % 256))).unwrap();
        clock.advance_ns(100_000);
        hub.poll();
        if i % 1_000 == 999 {
            max_len = max_len.max(a_rx.len()).max(ui_rx.len());
            seen.clear();
            a_rx.recv_into(&mut seen, usize::MAX);
            assert_eq!(ui_rx.recv(usize::MAX), seen);
            for r in &seen {
                assert_eq!(r.source_seq, last_seq + 1);
                last_seq = r.source_seq;
            }
            total += seen.len();
        }
    }
    let elapsed = start.elapsed();
    let st = hub.bus_stats("sim1").unwrap();
    assert!(st.endpoints.iter().all(|e| e.drops.total == 0));
    assert!(max_len <= loom_can_sim::DEFAULT_QUEUE_FRAMES);
    assert!(total >= 20_000, "{total}");
    eprintln!("throughput smoke: {total} records (20k fps for 1 s of bus time) in {elapsed:?}");
    assert!(elapsed.as_secs() < 10);
}
