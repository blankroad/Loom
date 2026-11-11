//! data 필드·예약 순서·표준 gap의 스트림 좌표를 검증한다.
#![allow(clippy::unwrap_used)]

use loom_proto::event::{Dir, Env, GapCount, GapData, Metric, Origin, ProducerRange, Risk, TsSource};
use loom_proto::num::U64;
use loom_proto::worker::Lane;
use loom_worker_sdk::{Clock, EmitError, Emitter, EventDraft, MemorySink, SimClock};
use serde_json::json;
use std::sync::Arc;

fn emitter(first: u64) -> (Emitter, Arc<MemorySink>, SimClock) {
    let clock = SimClock::new(1000);
    let sink = Arc::new(MemorySink::new(100));
    (
        Emitter::new(
            "s",
            "can",
            "can-a",
            U64(2),
            first,
            "ch0",
            sink.clone(),
            Arc::new(clock.clone()),
        ),
        sink,
        clock,
    )
}

fn draft(clock: &SimClock) -> EventDraft {
    EventDraft {
        kind: "can.frame".into(),
        ts: clock.ts(TsSource::Driver),
        dir: Some(Dir::Tx),
        origin: Origin::User,
        client_id: Some("viewer".into()),
        request_id: Some("r1".into()),
        job_id: Some("j1".into()),
        generation: Some(3),
        purpose: Some("interactive".into()),
        risk: Some(Risk::R1),
        env: Some(Env::Sim),
        payload: json!({"id":123}),
        metrics: vec![Metric {
            name: "load".into(),
            value: 0.5,
            unit: Some("ratio".into()),
        }],
        quality: vec!["simulated".into()],
    }
}

#[test]
fn data_and_reserved_data_keep_all_draft_fields() {
    let (emitter, sink, clock) = emitter(10);
    let d = draft(&clock);
    emitter.data(d.clone()).unwrap();
    let seq = emitter.reserve_seq().unwrap();
    emitter.data_with_seq(seq, d.clone()).unwrap();
    for (lane, rec) in sink.drain() {
        assert_eq!(lane, Lane::Data);
        assert_eq!(rec.kind, d.kind);
        assert_eq!(rec.ts, d.ts);
        assert_eq!(rec.dir, d.dir);
        assert_eq!(rec.origin, d.origin);
        assert_eq!(rec.client_id, d.client_id);
        assert_eq!(rec.request_id, d.request_id);
        assert_eq!(rec.job_id, d.job_id);
        assert_eq!(rec.generation, d.generation);
        assert_eq!(rec.purpose, d.purpose);
        assert_eq!(rec.risk, d.risk);
        assert_eq!(rec.env, d.env);
        assert_eq!(rec.payload, d.payload);
        assert_eq!(rec.metrics, d.metrics);
        assert_eq!(rec.quality, d.quality);
        assert_eq!(rec.domain, "can");
        assert_eq!(rec.channel.as_deref(), Some("ch0"));
    }
}

#[test]
fn reservation_order_is_shared_across_channels_and_rejects_duplicates_and_unreserved_ids() {
    let (e, sink, clock) = emitter(10);
    let old = e.reserve_seq().unwrap();
    let newer = e.for_channel("ch1").reserve_seq().unwrap();
    e.for_channel("ch1")
        .rx_with_seq(newer, "can.frame", clock.ts(TsSource::Driver), json!({}))
        .unwrap();
    assert_eq!(
        e.data_with_seq(old, draft(&clock)),
        Err(EmitError::OutOfOrder { seq: 10, last: 11 })
    );
    assert!(matches!(
        e.rx_with_seq(newer, "can.frame", clock.ts(TsSource::Driver), json!({})),
        Err(EmitError::OutOfOrder { .. })
    ));
    assert_eq!(
        e.data_with_seq(U64(999), draft(&clock)),
        Err(EmitError::UnreservedSeq)
    );
    assert_eq!(e.data(draft(&clock)).unwrap(), U64(12));
    assert_eq!(
        sink.drain()
            .iter()
            .map(|(_, r)| r.source_seq.0)
            .collect::<Vec<_>>(),
        [11, 12]
    );
}

#[test]
fn sequence_exhaustion_never_wraps() {
    let (e, _, clock) = emitter(u64::MAX);
    let seq = e.reserve_seq().unwrap();
    assert_eq!(seq, U64::MAX);
    assert_eq!(e.reserve_seq(), Err(EmitError::SeqExhausted));
    e.rx_with_seq(seq, "can.frame", clock.ts(TsSource::Driver), json!({}))
        .unwrap();
    assert_eq!(e.data(draft(&clock)), Err(EmitError::SeqExhausted));
}

#[test]
fn partial_sink_failure_cannot_allow_an_older_reserved_record_into_the_journal() {
    #[derive(Debug, Default)]
    struct JournalThenFull(std::sync::Mutex<Option<u64>>);
    impl loom_worker_sdk::EmitSink for JournalThenFull {
        fn try_push(&self, _lane: Lane, record: loom_proto::event::SourceRecord) -> Result<(), EmitError> {
            let mut last = self.0.lock().unwrap();
            assert!(last.is_none_or(|previous| record.source_seq.0 > previous));
            *last = Some(record.source_seq.0);
            Err(EmitError::Full)
        }
    }
    let clock = SimClock::new(0);
    let sink = Arc::new(JournalThenFull::default());
    let e = Emitter::new(
        "s",
        "can",
        "can-a",
        U64(1),
        1,
        "ch0",
        sink.clone(),
        Arc::new(clock.clone()),
    );
    let older = e.reserve_seq().unwrap();
    assert_eq!(e.data(draft(&clock)), Err(EmitError::Full));
    assert_eq!(
        e.data_with_seq(older, draft(&clock)),
        Err(EmitError::OutOfOrder { seq: 1, last: 2 })
    );
    assert_eq!(*sink.0.lock().unwrap(), Some(2));
}

#[test]
fn all_standard_gaps_keep_affected_domain_channel_range_and_details() {
    let (e, sink, _) = emitter(10);
    let range = ProducerRange {
        producer_id: "can-a".into(),
        producer_epoch: U64(2),
        from: U64(1),
        to: U64(4),
    };
    e.source_gap_with(
        "driver_overrun",
        GapCount::Exact(U64(4)),
        Some(range.clone()),
        json!({"queue":"driver"}),
    )
    .unwrap();
    e.record_gap(
        Some(range.clone()),
        GapCount::Exact(U64(4)),
        "disk_full",
        Some(32),
        json!({"queue":"journal"}),
    )
    .unwrap();
    e.display_gap_with(
        Some(range.clone()),
        GapCount::Exact(U64(4)),
        "slow_client",
        Some(32),
        json!({"queue":"mirror"}),
    )
    .unwrap();
    let rows = sink.drain();
    for ((lane, r), kind) in rows.into_iter().zip(["source.gap", "record.gap", "display.gap"]) {
        assert_eq!(lane, Lane::Ctl);
        assert_eq!(r.kind, kind);
        assert_eq!(r.domain, "can");
        assert_eq!(r.channel.as_deref(), Some("ch0"));
        let gap: GapData = serde_json::from_value(r.payload).unwrap();
        assert_eq!(gap.producer_range, Some(range.clone()));
        assert_eq!(gap.count, GapCount::Exact(U64(4)));
        assert!(gap.detail["queue"].is_string());
    }
}
