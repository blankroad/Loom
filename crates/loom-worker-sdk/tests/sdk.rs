//! LatchFlag, RateGuard, frame codec, non-blocking Emitter.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use loom_proto::bytes::Bytes;
use loom_proto::error::ProtoError;
use loom_proto::event::{GapCount, Origin, TsSource, WriteState, payload_write_state};
use loom_proto::num::U64;
use loom_proto::worker::{
    Credit, Events, HEADER_LEN, Input, Lane, Latch, MAX_FRAME, Reply, ReplyResult, WorkerMsg,
};
use loom_worker_sdk::codec::{CodecError, decode_frame, encode_frame};
use loom_worker_sdk::*;
use serde_json::json;

fn emitter(cap: usize, clock: &SimClock) -> (Emitter, Arc<MemorySink>) {
    let sink = Arc::new(MemorySink::new(cap));
    let e = Emitter::new(
        "s1",
        "uart",
        "uart-dut",
        U64(2),
        100,
        "dut",
        sink.clone(),
        Arc::new(clock.clone()),
    );
    (e, sink)
}

#[test]
fn latch_flag_shared() {
    let a = LatchFlag::new();
    let b = a.clone();
    assert!(!b.is_on());
    a.set(true);
    assert!(b.is_on());
    a.set(false);
    assert!(!b.is_on());
}

#[test]
fn rate_guard_token_bucket() {
    let t0 = Instant::now();
    let mut g = RateGuard::new_at(10, 5, t0);
    for _ in 0..5 {
        assert!(g.try_take_at(t0, 1));
    }
    assert!(!g.try_take_at(t0, 1), "burst exhausted");
    assert!(!g.try_take_at(t0 + Duration::from_millis(50), 1), "0.5 token");
    assert!(g.try_take_at(t0 + Duration::from_millis(100), 1), "refilled 1");
    assert!(g.try_take_at(t0 + Duration::from_secs(10), 5), "capped at burst");
    assert!(!g.try_take_at(t0 + Duration::from_secs(10), 1));
    // Time going backwards never adds tokens.
    assert!(!g.try_take_at(t0, 1));
}

fn sample_msgs(clock: &SimClock) -> Vec<WorkerMsg> {
    let (e, sink) = emitter(16, clock);
    e.rx(
        "uart.data",
        clock.ts(TsSource::HostReadComplete),
        json!({"bytes_len": 3}),
    )
    .unwrap();
    let records = sink.drain().into_iter().map(|(_, r)| r).collect();
    vec![
        WorkerMsg::Latch(Latch {
            on: true,
            scope: "all".into(),
            reason: Some("user".into()),
            latch_seq: U64(7),
        }),
        WorkerMsg::Input(Input {
            conn_id: None,
            channel: None,
            input_seq: None,
            call_id: None,
            lease_id: "l1".into(),
            bytes: Bytes(vec![3]),
            priority: true,
        }),
        WorkerMsg::Credit(Credit { bytes: U64(1 << 20) }),
        WorkerMsg::Reply(Reply {
            call_id: U64(1),
            result: ReplyResult::Ok(json!({"ok": true})),
            covers: None,
        }),
        WorkerMsg::Events(Events {
            first_source_seq: U64(100),
            count: 1,
            records,
        }),
    ]
}

#[test]
fn frame_roundtrip_and_stream() {
    let clock = SimClock::new(1_000);
    let msgs = sample_msgs(&clock);
    let mut stream = Vec::new();
    for m in &msgs {
        let f = encode_frame(m).unwrap();
        assert!(f.len() <= MAX_FRAME);
        let (back, used) = decode_frame(&f).unwrap();
        assert_eq!(&back, m);
        assert_eq!(used, f.len());
        stream.extend_from_slice(&f);
    }
    // Decode back-to-back frames from one buffer.
    let mut off = 0;
    for m in &msgs {
        let (back, used) = decode_frame(&stream[off..]).unwrap();
        assert_eq!(&back, m);
        off += used;
    }
    assert_eq!(off, stream.len());
    assert_eq!(msgs[2].lane(), Lane::Data);
}

#[test]
fn truncated_frames_are_incomplete() {
    let clock = SimClock::new(0);
    for m in sample_msgs(&clock) {
        let f = encode_frame(&m).unwrap();
        for cut in 0..f.len() {
            let err = decode_frame(&f[..cut]).unwrap_err();
            assert!(err.is_incomplete(), "cut {cut}: {err}");
        }
    }
}

#[test]
fn oversize_and_corrupt_frames_rejected() {
    let clock = SimClock::new(0);
    let (e, sink) = emitter(10_000, &clock);
    for _ in 0..400 {
        e.rx(
            "uart.data",
            clock.ts(TsSource::Driver),
            json!({"pad": "x".repeat(200)}),
        )
        .unwrap();
    }
    let records: Vec<_> = sink.drain().into_iter().map(|(_, r)| r).collect();
    let big = WorkerMsg::Events(Events {
        first_source_seq: U64(100),
        count: 400,
        records,
    });
    assert!(matches!(encode_frame(&big), Err(CodecError::TooLarge { .. })));

    // Header says Latch (18) but body is Input (17).
    let mut f = encode_frame(&sample_msgs(&clock)[1]).unwrap();
    f[4] = 18;
    assert!(matches!(
        decode_frame(&f),
        Err(CodecError::TypeMismatch { header: 18, body: 17 })
    ));
    // Garbage payload.
    let mut g = encode_frame(&sample_msgs(&clock)[0]).unwrap();
    for b in &mut g[HEADER_LEN..] {
        *b = 0xc1; // never-used msgpack marker
    }
    assert!(matches!(decode_frame(&g), Err(CodecError::Decode(_))));
    // Bad header.
    let mut h = encode_frame(&sample_msgs(&clock)[0]).unwrap();
    h[5] = 1;
    assert!(matches!(
        decode_frame(&h),
        Err(CodecError::Frame(ProtoError::InvalidFrame(_)))
    ));
}

#[test]
fn emitter_is_non_blocking_and_counts_seq() {
    let clock = SimClock::new(5_000);
    let (e, sink) = emitter(3, &clock);
    clock.advance_ns(42);
    assert_eq!(
        e.rx("uart.data", clock.ts(TsSource::HostReadComplete), json!({}))
            .unwrap(),
        U64(100)
    );
    let draft = EventDraft {
        kind: "uart.write".into(),
        ts: clock.ts(TsSource::Driver),
        dir: Some(loom_proto::event::Dir::Tx),
        origin: Origin::Agent,
        client_id: Some("claude#1".into()),
        request_id: Some("q1".into()),
        job_id: None,
        generation: Some(1),
        purpose: Some("agent_send".into()),
        risk: None,
        env: None,
        payload: json!({"write_state": "sent"}),
        metrics: vec![],
        quality: vec![],
    };
    assert_eq!(e.priority(draft.clone()).unwrap(), U64(101));
    assert_eq!(
        e.for_channel("gps")
            .source_gap("usb_reenumeration", GapCount::Unknown)
            .unwrap(),
        U64(102)
    );
    // Full: error, no block, and the seq is consumed (hole at 103).
    assert_eq!(e.priority(draft.clone()), Err(EmitError::Full));
    let got = sink.drain();
    assert_eq!(got.len(), 3);
    assert_eq!(got[0].0, Lane::Data);
    assert_eq!(got[0].1.ts.mono_ns, U64(42));
    assert_eq!(got[0].1.origin, Origin::Device);
    assert_eq!(got[1].0, Lane::Ctl);
    assert_eq!(
        payload_write_state(&got[1].1.payload).unwrap(),
        Some(WriteState::Sent)
    );
    assert_eq!(got[1].1.request_id.as_deref(), Some("q1"));
    assert_eq!(
        (got[2].1.domain.as_str(), got[2].1.kind.as_str()),
        ("uart", "source.gap"),
        "domain/channel of the affected stream (contract 02 §2.4)"
    );
    assert_eq!(got[2].1.channel.as_deref(), Some("gps"));
    assert_eq!(e.priority(draft).unwrap(), U64(104));
    assert_eq!(
        got.iter()
            .map(|(_, r)| r.source_ref().source_seq)
            .collect::<Vec<_>>(),
        [U64(100), U64(101), U64(102)]
    );
}

/// `source.gap` / `display.gap` / `system` events carry the affected stream's domain and channel;
/// an emitter without a channel produces no channel; seqs stay strictly increasing across threads.
#[test]
fn gap_events_use_the_stream_domain_and_order_is_kept() {
    let sink = Arc::new(MemorySink::new(100_000));
    let clock = Arc::new(SimClock::new(0));
    let base = Emitter::new("s", "can", "can-hw", U64(3), 1, "", sink.clone(), clock);
    base.for_channel("ch1")
        .source_gap("driver_overrun", GapCount::Exact(U64(4)))
        .unwrap();
    base.system("record.health", json!({"health": "failed"})).unwrap();
    let got = sink.drain();
    assert_eq!(got[0].1.kind, "source.gap");
    assert_eq!(got[0].1.domain, "can");
    assert_eq!(got[0].1.channel.as_deref(), Some("ch1"));
    let gd: loom_proto::event::GapData = serde_json::from_value(got[0].1.payload.clone()).unwrap();
    assert_eq!(gd.cause, "driver_overrun");
    assert_eq!(
        (got[1].1.domain.as_str(), got[1].1.channel.as_deref()),
        ("can", None)
    );

    // Four threads emitting at once: the sink sees source_seq strictly increasing.
    let threads: Vec<_> = (0..4)
        .map(|t| {
            let e = base.for_channel(format!("ch{t}"));
            std::thread::spawn(move || {
                for i in 0..2_000 {
                    let _ = e.rx("can.frame", e.clock().ts(TsSource::Driver), json!({"i": i}));
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    let seqs: Vec<u64> = sink.drain().iter().map(|(_, r)| r.source_seq.0).collect();
    assert_eq!(seqs.len(), 8_000);
    assert!(seqs.windows(2).all(|w| w[0] < w[1]), "push order = seq order");
}
