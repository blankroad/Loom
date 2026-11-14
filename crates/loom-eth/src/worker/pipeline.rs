use super::{shared::*, *};
use crate::model::*;
use loom_eth_capture::{BoundedFrameQueue, CaptureLoop, CapturedFrame, GapReport, TimestampSource};
use loom_eth_decode::{CompactLayers, FieldValue, FrameMeta, StreamTable};
use loom_proto::event::{TsSource, WriteState};
use loom_worker_sdk::{Emitter, RateGuard};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

/// Bound protocol counters per coverage window. Excess names share `other`.
pub const MAX_BATCH_PROTOCOLS: usize = 32;
/// Bounded gap draining prevents loss reporting from starving packet decode or STOP.
pub const MAX_GAPS_PER_POLL: usize = 64;
/// Publication window / retry cadence; no unbounded event retry queue.
pub const CAP_BATCH_NS: u64 = 100_000_000;
pub const CAP_STATS_NS: u64 = 1_000_000_000;
/// Input retained for decode matches M1 inline ceiling; oversized originals are marked partial.
pub const MAX_DECODE_INPUT: usize = MAX_INLINE_FRAME;
#[derive(Debug, Default)]
struct Window {
    first: Option<u64>,
    last: Option<u64>,
    total: u64,
    shown: u64,
    budget_omitted: u64,
    by_proto: BTreeMap<String, u64>,
}
impl Window {
    fn see(&mut self, seq: u64, proto: &str) {
        self.first.get_or_insert(seq);
        self.last = Some(seq);
        self.total = self.total.saturating_add(1);
        let key = if self.by_proto.contains_key(proto) || self.by_proto.len() < MAX_BATCH_PROTOCOLS - 1 {
            bounded_text(proto, 64)
        } else {
            "other".into()
        };
        let n = self.by_proto.entry(key).or_default();
        *n = n.saturating_add(1);
    }
    fn flush(&mut self, ch: &str, em: &Emitter, m: &Metrics) {
        if self.total > self.shown {
            let payload = BatchPayload {
                ch: ch.into(),
                covered_first_src_seq: loom_proto::num::U64(self.first.unwrap_or(0)),
                covered_last_src_seq: loom_proto::num::U64(self.last.unwrap_or(0)),
                total: loom_proto::num::U64(self.total),
                shown: loom_proto::num::U64(self.shown),
                // M1: no worker journal yet, so every packet not published individually has
                // no stored original (eth.batch makes that count visible).
                unrecorded: loom_proto::num::U64(self.total.saturating_sub(self.shown)),
                by_proto: std::mem::take(&mut self.by_proto)
                    .into_iter()
                    .map(|(k, v)| (k, loom_proto::num::U64(v)))
                    .collect(),
            };
            priority(
                em,
                KIND_BATCH,
                serde_json::to_value(payload).unwrap_or(Value::Null),
                m,
            );
            gap(
                em,
                "record.gap",
                "m1_unpublished_no_journal",
                Some(self.total - self.shown),
                self.first,
                self.last,
                m,
            );
        }
        *self = Self::default();
    }
}
fn scalar(d: &loom_eth_decode::Decoded, name: &str) -> Option<u64> {
    d.field_values(name).find_map(|v| {
        if let FieldValue::Uint(v) = v {
            Some(v)
        } else {
            None
        }
    })
}
fn compact(layers: &[loom_eth_decode::Layer], limit: usize) -> Option<Value> {
    let mut out = LimitedJson::new(limit);
    serde_json::to_writer(&mut out, &CompactLayers(layers)).ok()?;
    serde_json::from_slice(&out.bytes).ok()
}
fn layers(d: &loom_eth_decode::Decoded, f: &CapturedFrame) -> Value {
    // Reserve marker/coordinate headroom. A bounded serializer stops before the JSON grows.
    let limit = MAX_LAYERS_JSON - 256;
    let mut value = compact(&d.layers, limit).unwrap_or_else(|| {
        let mut last = json!({});
        for n in 1..=d.layers.len() {
            if let Some(v) = compact(&d.layers[..n], limit) {
                last = v;
            } else {
                break;
            }
        }
        if let Some(o) = last.as_object_mut() {
            o.insert("_truncated".into(), Value::Bool(true));
        }
        last
    });
    if let Some(o) = value.as_object_mut() {
        o.insert("_capture".into(),json!({"source_seq":f.source_seq.to_string(),"ts_ns":f.raw.ts_ns.to_string(),"interface_id":f.raw.interface_id,"section":f.raw.section_index.to_string(),"replay_src_seq":f.raw.meta.as_ref().map(|m|m.seq.to_string())}));
    }
    value
}
fn map_gaps(em: &Emitter, g: GapReport, shared: &Shared) {
    let m = &shared.metrics;
    if let Some(last) = g.last_seq {
        shared
            .next_capture_seq
            .fetch_max(last.saturating_add(1), Ordering::AcqRel);
    }
    if g.counts.record > 0 {
        Metrics::add(&m.unrecorded, g.counts.record);
        Metrics::add(&m.capture_dropped, g.counts.record);
        gap(
            em,
            "record.gap",
            "capture_queue_overflow",
            Some(g.counts.record),
            g.first_seq,
            g.last_seq,
            m,
        );
    }
    for (cause, count) in [
        ("kernel_drop", g.counts.kernel_drop),
        ("if_drop", g.counts.if_drop),
    ] {
        if count > 0 {
            gap(em, "source.gap", cause, Some(count), None, None, m);
        }
    }
    if g.counts.stats_unavailable > 0 {
        gap(em, "source.gap", "stats_unavailable", None, None, None, m);
    }
    if g.counts.capture_restart > 0 {
        gap(em, "source.gap", "capture_restart", None, None, None, m);
    }
    if g.counts.producer_seq > 0 {
        gap(
            em,
            "source.gap",
            "producer_seq_gap",
            Some(g.counts.producer_seq),
            None,
            None,
            m,
        );
    }
}
fn state(shared: &Shared, em: &Emitter, next: CaptureStateKind, message: Option<String>) {
    if let Some(s) = lock(&shared.state).as_mut() {
        s.state = next;
        s.error = message;
        state_event(em, s, &shared.metrics);
    }
}
#[derive(Debug, Default)]
struct Rates {
    last_ns: u64,
    last_total: u64,
    last_shown: u64,
    last_bytes: u64,
    last_kernel: Option<u64>,
    last_interface: Option<u64>,
}
impl Rates {
    fn emit(&mut self, ch: &str, em: &Emitter, shared: &Shared, now: u64) {
        let dt = now.saturating_sub(self.last_ns).max(1);
        let total =
            Metrics::get(&shared.metrics.total).saturating_add(Metrics::get(&shared.metrics.capture_dropped));
        let shown = Metrics::get(&shared.metrics.shown);
        let bytes = Metrics::get(&shared.metrics.bytes);
        let driver = *lock(&shared.drop_stats);
        let availability = match driver.availability {
            loom_eth_capture::StatsAvailability::Available => "available",
            loom_eth_capture::StatsAvailability::Partial => "partial",
            loom_eth_capture::StatsAvailability::Unavailable => "unavailable",
        };
        if let Some(s) = lock(&shared.state).as_mut()
            && s.drop_stats != availability
        {
            s.drop_stats = availability.into();
            state_event(em, s, &shared.metrics);
        }
        let kd = driver
            .kernel_drop
            .map(|n| n.saturating_sub(self.last_kernel.unwrap_or(0)));
        let id = driver
            .if_drop
            .map(|n| n.saturating_sub(self.last_interface.unwrap_or(0)));
        let rate = |n: u64| u64::try_from(u128::from(n) * 1_000_000_000 / u128::from(dt)).unwrap_or(u64::MAX);
        let payload = StatsPayload {
            ch: ch.into(),
            pps: loom_proto::num::U64(rate(total.saturating_sub(self.last_total))),
            bps: loom_proto::num::U64(rate(bytes.saturating_sub(self.last_bytes)).saturating_mul(8)),
            kernel_drop: kd.map(loom_proto::num::U64),
            if_drop: id.map(loom_proto::num::U64),
            decode_lag_ms: loom_proto::num::U64(Metrics::get(&shared.metrics.decode_lag_ms)),
            shown: loom_proto::num::U64(shown.saturating_sub(self.last_shown)),
            total: loom_proto::num::U64(total.saturating_sub(self.last_total)),
        };
        priority(
            em,
            KIND_STATS,
            serde_json::to_value(payload).unwrap_or(Value::Null),
            &shared.metrics,
        );
        self.last_ns = now;
        self.last_total = total;
        self.last_shown = shown;
        self.last_bytes = bytes;
        if driver.kernel_drop.is_some() {
            self.last_kernel = driver.kernel_drop;
        }
        if driver.if_drop.is_some() {
            self.last_interface = driver.if_drop;
        }
    }
}
pub(crate) fn run(
    capture: CaptureLoop,
    queue: BoundedFrameQueue,
    em: Emitter,
    shared: Arc<Shared>,
    stop: Arc<AtomicBool>,
    config: WorkerConfig,
    ch: String,
) -> Result<(), WorkerError> {
    let start = Instant::now();
    let first_ns = em.clock().mono_ns();
    let mut batch_ns = first_ns;
    let mut stats_ns = first_ns;
    let mut gap_ns = first_ns;
    let mut streams = StreamTable::new();
    let mut identity = None;
    let mut filter_time_ref: Option<u64> = None;
    let mut previous_filter_ts: Option<u64> = None;
    let mut window = Window::default();
    let mut rates = Rates {
        last_ns: first_ns,
        ..Default::default()
    };
    let mut displayed_loss = 0;
    let mut budget = RateGuard::new_at(config.publication_per_sec, config.publication_per_sec, start);
    loop {
        let now = em.clock().mono_ns();
        for _ in 0..MAX_GAPS_PER_POLL {
            let Some(g) = queue.try_gap() else {
                break;
            };
            map_gaps(&em, g, &shared);
        }
        if stop.load(Ordering::Acquire) {
            break;
        }
        if let Some(frame) = queue.pop_timeout(Duration::from_millis(2)) {
            shared
                .next_capture_seq
                .fetch_max(frame.source_seq.saturating_add(1), Ordering::AcqRel);
            let id = (frame.raw.section_index, frame.raw.interface_id);
            if identity.is_some_and(|old| old != id) {
                streams = StreamTable::new();
                Metrics::add(&shared.metrics.decode_resets, 1);
            }
            identity = Some(id);
            let lag =
                u64::try_from(start.elapsed().as_nanos().saturating_sub(frame.host_mono_ns) / 1_000_000)
                    .unwrap_or(u64::MAX);
            shared.metrics.decode_lag_ms.store(lag, Ordering::Relaxed);
            Metrics::add(&shared.metrics.total, 1);
            Metrics::add(&shared.metrics.bytes, u64::from(frame.raw.origlen));
            let historical_replay =
                frame.ts_source == TimestampSource::Replay && frame.raw.section_index != u64::MAX;
            // Frames from an external producer are never Loom's own: only frames Loom genuinely
            // sent (its own echo copies, section u64::MAX) may match the recent-send table.
            let external_stream =
                frame.ts_source == TimestampSource::External && frame.raw.section_index != u64::MAX;
            let foreign = historical_replay || external_stream;
            let mut attributed = if foreign {
                None
            } else {
                lock(&shared.recent).find(&frame.raw.data, frame.raw.origlen, now)
            };
            let probe_bytes = &frame.raw.data[..frame.raw.data.len().min(MAX_DECODE_INPUT)];
            if !foreign && attributed.is_none() && lock(&shared.recent).has_socket() {
                let probe = loom_eth_decode::decode(
                    frame.raw.interface.linktype,
                    probe_bytes,
                    &FrameMeta::default(),
                    &mut StreamTable::new(),
                );
                let mut recent = lock(&shared.recent);
                attributed = recent
                    .find_udp(&probe, now)
                    .or_else(|| recent.find_tcp(&probe, now));
            }
            let replay_meta = historical_replay.then_some(frame.raw.meta.as_ref()).flatten();
            let replay_origin = replay_meta.map(|m| match m.org {
                loom_eth_pcapng::Origin::Agent => loom_eth_decode::Origin::Agent,
                loom_eth_pcapng::Origin::User => loom_eth_decode::Origin::User,
                loom_eth_pcapng::Origin::Script => loom_eth_decode::Origin::Script,
            });
            let meta = FrameMeta {
                seq: frame.source_seq,
                ts_ns: u64::try_from(frame.raw.ts_ns).unwrap_or_else(|_| {
                    Metrics::add(&shared.metrics.timestamp_fallbacks, 1);
                    u64::try_from(frame.host_mono_ns).unwrap_or(u64::MAX)
                }),
                dir: replay_meta
                    .map(|m| {
                        if m.dir == loom_eth_capture::Direction::Tx {
                            loom_eth_decode::Dir::Tx
                        } else {
                            loom_eth_decode::Dir::Rx
                        }
                    })
                    .or_else(|| attributed.as_ref().map(|_| loom_eth_decode::Dir::Tx)),
                attributed_origin: replay_origin.or_else(|| {
                    attributed.as_ref().map(|(r, _)| match r.meta.origin {
                        Origin::Agent => loom_eth_decode::Origin::Agent,
                        Origin::User => loom_eth_decode::Origin::User,
                        _ => loom_eth_decode::Origin::Script,
                    })
                }),
                request_id: replay_meta
                    .and_then(|m| m.req.clone())
                    .or_else(|| attributed.as_ref().map(|(r, _)| r.meta.request_id.clone())),
                orig_len: Some(frame.raw.origlen),
            };
            let bytes = &frame.raw.data[..frame.raw.data.len().min(MAX_DECODE_INPUT)];
            let decoded = loom_eth_decode::decode(frame.raw.interface.linktype, bytes, &meta, &mut streams);
            let proto = if decoded.summary.proto.is_empty() {
                "Unknown"
            } else {
                &decoded.summary.proto
            };
            window.see(frame.source_seq, proto);
            let eval = loom_eth_filter::EvalCtx {
                time_ref_ns: *filter_time_ref.get_or_insert(meta.ts_ns),
                prev_ts_ns: previous_filter_ts,
            };
            let interest = attributed.is_some()
                || lock(&shared.interests)
                    .iter()
                    .any(|filter| filter.matches_with(&decoded, &eval));
            previous_filter_ts = Some(meta.ts_ns);
            let at = start
                .checked_add(Duration::from_nanos(now.saturating_sub(first_ns)))
                .unwrap_or(start);
            let allowed = budget.try_take_at(at, 1) || interest;
            if allowed {
                let origin = replay_meta
                    .map(|m| match m.org {
                        loom_eth_pcapng::Origin::Agent => AttributedOrigin::Agent,
                        loom_eth_pcapng::Origin::User => AttributedOrigin::User,
                        loom_eth_pcapng::Origin::Script => AttributedOrigin::Script,
                    })
                    .or_else(|| {
                        attributed.as_ref().map(|(r, _)| match r.meta.origin {
                            Origin::Agent => AttributedOrigin::Agent,
                            Origin::User => AttributedOrigin::User,
                            _ => AttributedOrigin::Script,
                        })
                    });
                let virtual_copy = attributed.as_ref().is_some_and(|(r, _)| r.virtual_copy);
                let payload = PktPayload {
                    ch: ch.clone(),
                    linktype: frame.raw.interface.linktype,
                    len: frame.raw.origlen,
                    caplen: frame.raw.caplen,
                    dir: replay_meta
                        .map(|m| {
                            if m.dir == loom_eth_capture::Direction::Tx {
                                PktDir::Tx
                            } else {
                                PktDir::Rx
                            }
                        })
                        .or_else(|| origin.map(|_| PktDir::Tx)),
                    attributed_origin: origin,
                    ts_src: if virtual_copy {
                        TsSrc::Sim
                    } else {
                        match frame.ts_source {
                            TimestampSource::Replay => TsSrc::Replay,
                            TimestampSource::Sim => TsSrc::Sim,
                            TimestampSource::PcapHost => TsSrc::PcapHost,
                            TimestampSource::PcapAdapter => TsSrc::PcapAdapter,
                            TimestampSource::External => TsSrc::External,
                        }
                    },
                    proto: proto.into(),
                    src: if decoded.summary.src.is_empty() {
                        "?".into()
                    } else {
                        bounded_text(&decoded.summary.src, 128)
                    },
                    dst: if decoded.summary.dst.is_empty() {
                        "?".into()
                    } else {
                        bounded_text(&decoded.summary.dst, 128)
                    },
                    info: if decoded.summary.info.is_empty() {
                        "undecoded frame".into()
                    } else {
                        bounded_text(&decoded.summary.info, 512)
                    },
                    stream: scalar(&decoded, "tcp.stream").and_then(|v| u32::try_from(v).ok()),
                    layers: layers(&decoded, &frame),
                    link: replay_meta
                        .and_then(|m| m.req.as_ref())
                        .map(|req| PktLink {
                            domain: "eth".into(),
                            request_id: req.clone(),
                            role: "tx".into(),
                            confidence: "exact".into(),
                            target: None,
                        })
                        .or_else(|| {
                            attributed.as_ref().map(|(r, ambiguous)| PktLink {
                                domain: "eth".into(),
                                request_id: r.meta.request_id.clone(),
                                role: "tx".into(),
                                confidence: if *ambiguous { "probable" } else { "exact" }.into(),
                                target: None,
                            })
                        }),
                    pdu_first_src_seq: scalar(&decoded, "loom.pdu_first_seq").map(loom_proto::num::U64),
                    bytes: json!({"encoding":"base64","data":base64::Engine::encode(&base64::engine::general_purpose::STANDARD,bytes),"comments":frame.raw.comments,"loom_comment":replay_meta.and_then(|m|m.to_comment().ok())}),
                    bytes_truncated: bytes.len() < frame.raw.data.len(),
                };
                let timestamp = em
                    .clock()
                    .ts(if virtual_copy || frame.ts_source == TimestampSource::Sim {
                        TsSource::Synthetic
                    } else {
                        TsSource::HostReadComplete
                    });
                let emitted = em
                    .rx(
                        KIND_PKT,
                        timestamp,
                        serde_json::to_value(payload).unwrap_or(Value::Null),
                    )
                    .is_ok();
                if emitted {
                    window.shown += 1;
                    Metrics::add(&shared.metrics.shown, 1);
                } else {
                    Metrics::add(&shared.metrics.emit_failed, 1);
                    Metrics::add(&shared.metrics.display_lost, 1);
                    Metrics::add(&shared.metrics.unrecorded, 1);
                }
                if emitted && bytes.len() < frame.raw.data.len() {
                    Metrics::add(&shared.metrics.unrecorded, 1);
                    gap(
                        &em,
                        "record.gap",
                        "inline_frame_truncated",
                        Some(1),
                        Some(frame.source_seq),
                        Some(frame.source_seq),
                        &shared.metrics,
                    );
                }
            } else {
                window.budget_omitted += 1;
                Metrics::add(&shared.metrics.unrecorded, 1);
            }
            if let Some((recent, _)) = attributed {
                if let Some(o) = recent.observation {
                    let mut o = lock(&o);
                    o.seq = Some(o.seq.map_or(frame.source_seq, |s| s.max(frame.source_seq)));
                    if o.sent {
                        recent.meta.emit(&em, &shared.metrics, WriteState::Sent, o.seq);
                    }
                } else {
                    recent
                        .meta
                        .emit(&em, &shared.metrics, WriteState::Sent, Some(frame.source_seq));
                }
            }
        } else if queue.is_closed() {
            break;
        }
        if now.saturating_sub(batch_ns) >= CAP_BATCH_NS {
            window.flush(&ch, &em, &shared.metrics);
            batch_ns = now;
        }
        if now.saturating_sub(stats_ns) >= CAP_STATS_NS {
            rates.emit(&ch, &em, &shared, now);
            stats_ns = now;
        }
        if now.saturating_sub(gap_ns) >= CAP_BATCH_NS {
            let failed = Metrics::get(&shared.metrics.display_lost);
            if failed > displayed_loss
                && gap(
                    &em,
                    "display.gap",
                    "emitter_full",
                    Some(failed - displayed_loss),
                    None,
                    None,
                    &shared.metrics,
                )
            {
                displayed_loss = failed;
            }
            gap_ns = now;
        }
    }
    // Join the producer first so no late frame escapes final loss accounting.
    let summary = capture.join().map_err(capture_error);
    // STOP never spends time decoding an E2 backlog; account for the abandoned originals.
    let (mut first, mut last, mut count) = (None, None, 0u64);
    while let Some(frame) = queue.pop_timeout(Duration::ZERO) {
        first.get_or_insert(frame.source_seq);
        last = Some(frame.source_seq);
        count += 1;
    }
    if count > 0 {
        Metrics::add(&shared.metrics.unrecorded, count);
        Metrics::add(&shared.metrics.capture_dropped, count);
        gap(
            &em,
            "record.gap",
            "capture_stop_no_journal",
            Some(count),
            first,
            last,
            &shared.metrics,
        );
    }
    for _ in 0..loom_eth_capture::MAX_GAP_REPORTS {
        let Some(g) = queue.try_gap() else {
            break;
        };
        map_gaps(&em, g, &shared);
    }
    window.flush(&ch, &em, &shared.metrics);
    let failed = Metrics::get(&shared.metrics.display_lost);
    if failed > displayed_loss {
        gap(
            &em,
            "display.gap",
            "emitter_full",
            Some(failed - displayed_loss),
            None,
            None,
            &shared.metrics,
        );
    }
    match summary {
        Ok(summary) => {
            shared
                .next_capture_seq
                .fetch_max(summary.next_seq, Ordering::AcqRel);
            *lock(&shared.drop_stats) = summary.final_stats;
            let (replay, stream) = lock(&shared.state)
                .as_ref()
                .map_or((false, false), |s| (s.backend == "replay", s.backend == "stream"));
            let requested = stop.load(Ordering::Acquire);
            // The source was dropped (reader shut down, bounded) before join returned.
            let outstanding = if stream {
                loom_eth_capture::outstanding_stream_readers()
            } else {
                0
            };
            if outstanding > 0 {
                let msg = format!(
                    "stream reader thread not finished within {} ms; {outstanding} outstanding (cleanup incomplete)",
                    loom_eth_capture::STREAM_JOIN_BOUND.as_millis()
                );
                state(&shared, &em, CaptureStateKind::Error, Some(msg.clone()));
                return Err(error(ErrorCode::Internal, msg));
            }
            let next = if requested {
                CaptureStateKind::Stopped
            } else if replay {
                CaptureStateKind::ReplayDone
            } else {
                CaptureStateKind::Stopped
            };
            if stream && let Some(s) = lock(&shared.state).as_mut() {
                s.reason = Some(if requested { "requested" } else { "producer_eof" }.into());
            }
            state(&shared, &em, next, None);
            Ok(())
        }
        Err(e) => {
            state(&shared, &em, CaptureStateKind::Error, Some(e.message.clone()));
            Err(e)
        }
    }
}
