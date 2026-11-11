//! TCP stream table: stream numbers, relative sequence numbers, analysis flags, and bounded
//! per-direction reassembly for application protocols (DoIP 13400, SOME/IP 30490).
//!
//! Policy (plan X8 · E5):
//! - Stream key = VLAN id + both endpoints (direction-independent). A client SYN on a closed/reset
//!   stream, or with a different ISN, starts a new generation (new `tcp.stream` number).
//! - Only streams with a known application port buffer payload. Others are tracked for numbering only.
//! - Per direction: in-order delivery; retransmissions/duplicates ignored; overlaps keep the bytes that
//!   arrived first; out-of-order segments buffered within caps; mid-stream start = first seen seq;
//!   sequence numbers compared with wrapping arithmetic.
//! - A hole that cannot be buffered becomes a gap (`tcp_gap` warning, application framing resyncs on
//!   the next header or stops for that direction). Cap overruns raise `reassembly_truncated`;
//!   the original packet is never altered — only application decoding is skipped.

use std::collections::HashMap;
use std::net::IpAddr;

use crate::{doip, someip};

/// Concurrent streams. Over: idle streams are evicted first, then the least recently used one
/// (warning `stream_evicted`; its later packets get a new stream number).
pub const MAX_STREAMS: usize = 1024;
/// Buffered bytes per stream (out-of-order segments + incomplete PDU, both directions).
/// Over: that direction stops application decoding (`reassembly_truncated`).
pub const MAX_STREAM_BUFFERED_BYTES: usize = 4 * 1024 * 1024;
/// Buffered bytes over all streams. Over: same as the per-stream cap.
pub const MAX_TOTAL_BUFFERED_BYTES: usize = 64 * 1024 * 1024;
/// Out-of-order segments waiting per direction. Over: declared a gap.
pub const MAX_PENDING_SEGMENTS: usize = 256;
/// How far ahead (bytes) an out-of-order segment may be buffered. Further: declared a gap.
pub const MAX_OOO_DISTANCE: u32 = 4 * 1024 * 1024;
/// Largest application PDU reassembled (DoIP / SOME/IP). Over: direction stops app decoding.
pub const MAX_PDU_BYTES: usize = 1_048_576;
/// PDUs emitted for one packet. Over: the rest stays buffered and is emitted with the next packet.
pub const MAX_PDUS_PER_PACKET: usize = 64;
/// Idle time after which a stream is evicted (by packet timestamps).
pub const STREAM_IDLE_NS: u64 = 120_000_000_000;
/// Minimum interval between idle sweeps (by packet timestamps).
pub const SWEEP_INTERVAL_NS: u64 = 1_000_000_000;

/// Application protocol carried by a stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum App {
    Doip,
    SomeIp,
}

fn app_for(pa: u16, pb: u16) -> Option<App> {
    if pa == doip::DOIP_PORT || pb == doip::DOIP_PORT {
        Some(App::Doip)
    } else if pa == someip::SOMEIP_SD_PORT || pb == someip::SOMEIP_SD_PORT {
        Some(App::SomeIp)
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct Key {
    vlan: u16,
    a: (IpAddr, u16),
    b: (IpAddr, u16),
}

#[derive(Debug, Default)]
struct DirState {
    /// Seq that counts as relative 0 (SYN seq, or first seen seq - 1).
    isn: Option<u32>,
    /// Next expected seq for reassembly.
    next: Option<u32>,
    pending: Vec<(u32, u64, Vec<u8>)>,
    buf: Vec<u8>,
    buf_off: usize,
    buf_first_pkt: Option<u64>,
    fin: bool,
    /// Application framing lost (bad header after a gap) — no more app decoding in this direction.
    desync: bool,
    /// A cap was hit — no more app decoding in this direction.
    truncated: bool,
}

impl DirState {
    fn buffered(&self) -> usize {
        self.buf.len().saturating_sub(self.buf_off)
            + self.pending.iter().map(|(_, _, v)| v.len()).sum::<usize>()
    }
    fn stop(&mut self) {
        self.pending.clear();
        self.buf.clear();
        self.buf_off = 0;
        self.buf_first_pkt = None;
    }
}

#[derive(Debug)]
struct Entry {
    id: u64,
    last_ts: u64,
    reset: bool,
    app: Option<App>,
    dirs: [DirState; 2],
}

impl Entry {
    fn buffered(&self) -> usize {
        self.dirs[0].buffered() + self.dirs[1].buffered()
    }
}

/// One TCP segment handed to the table.
#[derive(Debug)]
pub(crate) struct Segment<'a> {
    pub vlan: u16,
    pub src: (IpAddr, u16),
    pub dst: (IpAddr, u16),
    pub seq: u32,
    pub ack: u32,
    pub syn: bool,
    pub ack_flag: bool,
    pub fin: bool,
    pub rst: bool,
    pub payload: &'a [u8],
    pub pkt_seq: u64,
    pub ts_ns: u64,
}

/// A complete application PDU.
#[derive(Debug)]
pub(crate) struct Pdu {
    pub app: App,
    pub bytes: Vec<u8>,
    /// Loom seq of the packet carrying the PDU's first byte.
    pub first_pkt: u64,
    /// Offset inside the current segment payload when the PDU lies entirely in it.
    pub payload_off: Option<usize>,
}

#[derive(Debug, Default)]
pub(crate) struct FeedResult {
    pub stream: u64,
    pub rel_seq: u32,
    pub rel_ack: Option<u32>,
    pub retransmission: bool,
    pub out_of_order: bool,
    pub lost_segment: bool,
    /// Bytes of an incomplete PDU remain buffered after this segment.
    pub partial: bool,
    pub pdus: Vec<Pdu>,
    pub warnings: Vec<(&'static str, String)>,
}

/// TCP stream state for one capture interface. Bounded by the `MAX_*` constants of this module.
#[derive(Debug, Default)]
pub struct StreamTable {
    map: HashMap<Key, Entry>,
    next_id: u64,
    buffered_total: usize,
    last_sweep_ns: u64,
    evicted: u64,
}

impl StreamTable {
    pub fn new() -> StreamTable {
        StreamTable::default()
    }
    /// Streams currently tracked.
    pub fn len(&self) -> usize {
        self.map.len()
    }
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
    /// Bytes currently buffered for reassembly.
    pub fn buffered_bytes(&self) -> usize {
        self.buffered_total
    }
    /// Streams evicted so far (idle or LRU).
    pub fn evicted(&self) -> u64 {
        self.evicted
    }

    fn remove(&mut self, k: &Key) {
        if let Some(e) = self.map.remove(k) {
            self.buffered_total = self.buffered_total.saturating_sub(e.buffered());
        }
    }

    fn sweep_idle(&mut self, now: u64) -> usize {
        let idle: Vec<Key> = self
            .map
            .iter()
            .filter(|(_, e)| e.last_ts.saturating_add(STREAM_IDLE_NS) <= now)
            .map(|(k, _)| *k)
            .collect();
        for k in &idle {
            self.remove(k);
        }
        self.evicted += idle.len() as u64;
        idle.len()
    }

    fn new_entry(&mut self, k: Key, now: u64, warnings: &mut Vec<(&'static str, String)>) {
        if self.map.len() >= MAX_STREAMS && self.sweep_idle(now) == 0 {
            let lru = self
                .map
                .iter()
                .min_by_key(|(_, e)| e.last_ts)
                .map(|(k, e)| (*k, e.id));
            if let Some((lk, lid)) = lru {
                self.remove(&lk);
                self.evicted += 1;
                warnings.push((
                    "stream_evicted",
                    format!("table full ({MAX_STREAMS}), evicted stream {lid}"),
                ));
            }
        }
        let id = self.next_id;
        self.next_id += 1;
        let app = app_for(k.a.1, k.b.1);
        self.map.insert(
            k,
            Entry {
                id,
                last_ts: now,
                reset: false,
                app,
                dirs: Default::default(),
            },
        );
    }

    pub(crate) fn feed(&mut self, s: &Segment<'_>) -> FeedResult {
        let mut r = FeedResult::default();
        if s.ts_ns.saturating_sub(self.last_sweep_ns) >= SWEEP_INTERVAL_NS {
            self.last_sweep_ns = s.ts_ns;
            self.sweep_idle(s.ts_ns);
        }
        let (key, d) = if s.src <= s.dst {
            (
                Key {
                    vlan: s.vlan,
                    a: s.src,
                    b: s.dst,
                },
                0usize,
            )
        } else {
            (
                Key {
                    vlan: s.vlan,
                    a: s.dst,
                    b: s.src,
                },
                1usize,
            )
        };
        // New generation on a fresh client SYN over a finished or differently-numbered stream.
        if s.syn
            && !s.ack_flag
            && let Some(e) = self.map.get(&key)
        {
            let finished = e.reset || (e.dirs[0].fin && e.dirs[1].fin);
            let other_isn = e.dirs[d].isn.is_some_and(|i| i != s.seq);
            if finished || other_isn {
                self.remove(&key);
            }
        }
        if !self.map.contains_key(&key) {
            self.new_entry(key, s.ts_ns, &mut r.warnings);
        }
        let before_total = self.buffered_total;
        let Some(e) = self.map.get_mut(&key) else { return r };
        let before = e.buffered();
        e.last_ts = s.ts_ns;
        r.stream = e.id;
        if s.rst {
            e.reset = true;
        }
        let app = e.app;
        let other_isn = e.dirs[1 - d].isn;
        let other_dir_buffered = e.dirs[1 - d].buffered();
        let st = &mut e.dirs[d];
        if s.syn {
            st.isn = Some(s.seq);
            st.next = Some(s.seq.wrapping_add(1));
        } else if st.isn.is_none() {
            st.isn = Some(s.seq.wrapping_sub(1));
            st.next = Some(s.seq);
        }
        r.rel_seq = s.seq.wrapping_sub(st.isn.unwrap_or(s.seq));
        r.rel_ack = if s.ack_flag {
            other_isn.map(|i| s.ack.wrapping_sub(i))
        } else {
            None
        };
        if s.fin {
            st.fin = true;
        }

        let data_seq = if s.syn { s.seq.wrapping_add(1) } else { s.seq };
        if let Some(app) = app
            && !s.payload.is_empty()
            && !st.desync
            && !st.truncated
        {
            let others_total = before_total.saturating_sub(before);
            let limit = MAX_STREAM_BUFFERED_BYTES
                .saturating_sub(other_dir_buffered)
                .min(MAX_TOTAL_BUFFERED_BYTES.saturating_sub(others_total));
            reassemble(st, app, data_seq, s.payload, s.pkt_seq, limit, &mut r);
        } else if !s.payload.is_empty() {
            // Analysis flags still apply to untracked or stopped directions.
            let next = st.next.get_or_insert(data_seq);
            let diff = data_seq.wrapping_sub(*next) as i32;
            let end = i64::from(diff) + s.payload.len() as i64;
            if end <= 0 {
                r.retransmission = true;
            } else {
                if diff > 0 {
                    r.lost_segment = true;
                }
                *next = data_seq.wrapping_add(s.payload.len() as u32);
            }
        }
        let after = e.buffered();
        self.buffered_total = before_total.saturating_sub(before).saturating_add(after);
        r
    }
}

fn reassemble(
    st: &mut DirState,
    app: App,
    seq: u32,
    payload: &[u8],
    pkt: u64,
    limit: usize,
    r: &mut FeedResult,
) {
    let next = *st.next.get_or_insert(seq);
    let diff = seq.wrapping_sub(next) as i32;
    let end = i64::from(diff) + payload.len() as i64;
    if end <= 0 {
        r.retransmission = true;
        return;
    }
    if diff > 0 {
        r.lost_segment = true;
        if st.buffered() + payload.len() > limit {
            // Memory cap (per stream or global): stop app decoding for this direction.
            r.warnings
                .push(("reassembly_truncated", "stream buffer cap reached".into()));
            st.truncated = true;
            st.stop();
            r.partial = false;
            return;
        }
        if (diff as u32) <= MAX_OOO_DISTANCE && st.pending.len() < MAX_PENDING_SEGMENTS {
            if !st.pending.iter().any(|(s, _, _)| *s == seq) {
                st.pending.push((seq, pkt, payload.to_vec()));
            }
            r.partial = true;
            return;
        }
        // Cannot buffer: give up on the hole.
        r.warnings
            .push(("tcp_gap", format!("{diff} bytes not captured before seq {seq}")));
        st.pending.clear();
        st.buf.clear();
        st.buf_off = 0;
        st.buf_first_pkt = None;
        st.next = Some(seq);
        deliver(st, app, payload, pkt, Some(0), limit, r);
    } else {
        if !st.pending.is_empty() {
            r.out_of_order = true;
        }
        let skip = (-diff) as usize;
        let fresh = payload.get(skip..).unwrap_or(&[]);
        deliver(st, app, fresh, pkt, Some(skip), limit, r);
    }
    // Drain buffered segments that are now in order.
    while !st.desync && !st.truncated {
        let Some(n) = st.next else { break };
        let ready = st
            .pending
            .iter()
            .enumerate()
            .filter(|(_, (s, _, _))| (s.wrapping_sub(n) as i32) <= 0)
            .min_by_key(|(_, (s, _, _))| s.wrapping_sub(n) as i32)
            .map(|(i, _)| i);
        let Some(i) = ready else { break };
        let (s, from_pkt, v) = st.pending.swap_remove(i);
        let d = s.wrapping_sub(n) as i32;
        let skip = (-d) as usize;
        if let Some(fresh) = v.get(skip..).filter(|f| !f.is_empty()) {
            deliver(st, app, fresh, from_pkt, None, limit, r);
        }
    }
    if st.buf_off > 0 && st.buf_off >= st.buf.len() {
        st.buf.clear();
        st.buf_off = 0;
    } else if st.buf_off > st.buf.len() / 2 {
        st.buf.drain(..st.buf_off);
        st.buf_off = 0;
    }
    r.partial = st.buf.len() > st.buf_off || !st.pending.is_empty();
}

/// Appends in-order bytes and cuts complete PDUs. `from_payload` = offset of `data` inside the
/// current segment payload (None when the bytes come from an earlier buffered segment).
fn deliver(
    st: &mut DirState,
    app: App,
    data: &[u8],
    pkt: u64,
    from_payload: Option<usize>,
    limit: usize,
    r: &mut FeedResult,
) {
    if data.is_empty() {
        return;
    }
    if st.buffered() + data.len() > limit {
        r.warnings
            .push(("reassembly_truncated", "stream buffer cap reached".into()));
        st.truncated = true;
        st.stop();
        return;
    }
    if st.buf.len() == st.buf_off {
        st.buf_first_pkt = Some(pkt);
    }
    let start = st.buf.len();
    st.buf.extend_from_slice(data);
    st.next = st.next.map(|n| n.wrapping_add(data.len() as u32));
    loop {
        let avail = st.buf.get(st.buf_off..).unwrap_or(&[]);
        if avail.is_empty() {
            break;
        }
        let framed = match app {
            App::Doip => doip::frame_len(avail),
            App::SomeIp => someip::frame_len(avail),
        };
        let total = match framed {
            Ok(Some(t)) => t,
            Ok(None) => break,
            Err(why) => {
                r.warnings.push(("app_desync", why.into()));
                st.desync = true;
                st.stop();
                return;
            }
        };
        if total > MAX_PDU_BYTES {
            r.warnings
                .push(("reassembly_truncated", format!("PDU of {total} bytes over cap")));
            st.truncated = true;
            st.stop();
            return;
        }
        if avail.len() < total {
            break;
        }
        if r.pdus.len() >= MAX_PDUS_PER_PACKET {
            break;
        }
        let pdu_start = st.buf_off;
        let bytes = avail.get(..total).map(<[u8]>::to_vec).unwrap_or_default();
        let payload_off = match from_payload {
            Some(skip) if pdu_start >= start && pdu_start + total <= st.buf.len() => {
                Some(skip + (pdu_start - start))
            }
            _ => None,
        };
        r.pdus.push(Pdu {
            app,
            bytes,
            first_pkt: st.buf_first_pkt.unwrap_or(pkt),
            payload_off,
        });
        st.buf_off += total;
        st.buf_first_pkt = if st.buf_off < st.buf.len() {
            Some(pkt)
        } else {
            None
        };
    }
}
