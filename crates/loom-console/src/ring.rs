//! `ring` — fixed per-channel backlog ring (UART plan §3.3, §3.6, §3.13 "backlog 링" row).
//!
//! Holds the most recent RX/TX chunks and gap marks of one channel in memory. Three limits are
//! enforced at the same time — payload bytes, entry count and metadata bytes — and the oldest
//! entries are evicted first. Eviction is normal retention expiry, **not** record loss: it never
//! creates a gap mark. Older data is read from the record files (P1).
//!
//! Coordinates: every data chunk has a strictly increasing `source_seq`; RX chunks also carry
//! `rx_offset`, the cumulative RX byte offset of their first byte. RX offsets only grow and
//! continue across port generations (the `generation` field tells generations apart). A jump
//! in `rx_offset` is a hole (data that never reached the ring), not an eviction.

use std::collections::VecDeque;

use loom_proto::event::{Dir, Origin};
use serde::{Deserialize, Serialize};

/// Payload bytes per channel ring (plan §3.6, §3.13 backlog 행 "payload 16 MiB").
pub const DEFAULT_MAX_PAYLOAD: usize = 16 << 20;

/// Entries per channel ring (plan §3.6, §3.13 backlog 행 "32,768이벤트").
pub const DEFAULT_MAX_ENTRIES: usize = 32_768;

/// Metadata pool per channel ring (plan §3.13 backlog 행 "메타데이터 pool 4 MiB").
pub const DEFAULT_MAX_META: usize = 4 << 20;

/// Longest single metadata string (`client_id`, `request_id`, gap `cause`). Longer strings are
/// rejected, not truncated — they are a caller bug (task p0-console-core §A).
pub const MAX_META_FIELD_BYTES: usize = 128;

/// Limits of one [`Ring`]. All three apply at once; the first one reached evicts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RingLimits {
    /// Payload bytes (sum of chunk buffer capacities).
    pub max_payload: usize,
    /// Entries (chunks + gap marks).
    pub max_entries: usize,
    /// Metadata bytes (sum of metadata string capacities).
    pub max_meta: usize,
}

impl Default for RingLimits {
    fn default() -> Self {
        Self {
            max_payload: DEFAULT_MAX_PAYLOAD,
            max_entries: DEFAULT_MAX_ENTRIES,
            max_meta: DEFAULT_MAX_META,
        }
    }
}

/// One RX or TX chunk as produced by the worker (plan §3.3 청크).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chunk {
    /// Worker source sequence number (strictly increasing within the ring).
    pub source_seq: u64,
    /// Direction.
    pub dir: Dir,
    /// Who caused it.
    pub origin: Origin,
    /// Sending client (`client_name#pid`), TX only in practice. ≤ [`MAX_META_FIELD_BYTES`].
    pub client_id: Option<String>,
    /// Request id of the write, if any. ≤ [`MAX_META_FIELD_BYTES`].
    pub request_id: Option<String>,
    /// Port generation.
    pub generation: u32,
    /// Host monotonic time (ns).
    pub mono_ns: u64,
    /// Wall time (ns since Unix epoch).
    pub wall_ns: u64,
    /// RX only: cumulative RX offset of the first byte of this chunk. Must be `None` for TX.
    pub rx_offset: Option<u64>,
    /// Raw bytes (never modified).
    pub bytes: Vec<u8>,
}

/// Loss layer of a gap (plan §3.6 손실 3층).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GapLayer {
    /// Live display skipped data (`display.gap`).
    Display,
    /// Data did not reach the record (`record.gap`).
    Record,
    /// Data was lost before Loom read it (`source.gap`).
    Source,
}

/// A loss mark stored in the ring at the position where the loss happened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GapMark {
    /// Loss layer.
    pub layer: GapLayer,
    /// Cause code (e.g. `reader_bridge_overflow`). ≤ [`MAX_META_FIELD_BYTES`].
    pub cause: String,
    /// First lost RX offset, if known.
    pub rx_offset_from: Option<u64>,
    /// Lost bytes, if known.
    pub bytes: Option<u64>,
    /// Lost producer `source_seq` range (inclusive), if known.
    pub source_seq_range: Option<(u64, u64)>,
    /// Port generation in which the loss happened.
    pub generation: u32,
    /// Host monotonic time of the mark (ns).
    pub mono_ns: u64,
}

/// One ring entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Entry {
    /// RX/TX data.
    Data(Chunk),
    /// Loss mark.
    Gap(GapMark),
}

impl Entry {
    fn mono_ns(&self) -> u64 {
        match self {
            Entry::Data(c) => c.mono_ns,
            Entry::Gap(g) => g.mono_ns,
        }
    }
}

/// Why a push was rejected. Every variant is a caller bug; the ring is unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RingError {
    /// A data chunk with no bytes.
    EmptyChunk,
    /// Chunk payload larger than the whole ring payload limit.
    ChunkTooLarge {
        /// Chunk length.
        len: usize,
        /// `max_payload`.
        limit: usize,
    },
    /// A metadata string longer than [`MAX_META_FIELD_BYTES`].
    MetaFieldTooLong {
        /// Field name.
        field: &'static str,
        /// Its length.
        len: usize,
    },
    /// Entry metadata larger than the whole metadata pool.
    MetaTooLarge,
    /// `source_seq` not greater than the previous chunk's.
    NonMonotonicSeq {
        /// Rejected seq.
        got: u64,
        /// Last accepted seq.
        last: u64,
    },
    /// RX chunk without `rx_offset`, or TX chunk with one.
    RxOffsetMismatch,
    /// RX offset going backwards.
    NonMonotonicRxOffset {
        /// Rejected offset.
        got: u64,
        /// Current RX end.
        end: u64,
    },
}

impl std::fmt::Display for RingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for RingError {}

/// Result of a successful push.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PushOutcome {
    /// Entries evicted to make room.
    pub evicted_entries: usize,
    /// Payload bytes (chunk lengths) evicted.
    pub evicted_bytes: u64,
}

/// Ring statistics. All `*_bytes` are counted from buffer capacities so they bound the real
/// heap held by the ring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RingStats {
    /// Limits in force.
    pub limits: RingLimits,
    /// Entries held.
    pub entries: usize,
    /// Payload bytes held (capacity).
    pub payload_bytes: usize,
    /// Metadata bytes held (capacity).
    pub meta_bytes: usize,
    /// Slot capacity of the entry deque (≤ 2 × `max_entries`).
    pub slot_capacity: usize,
    /// Size of one slot; `slot_capacity × slot_size` is the fixed per-entry overhead.
    pub slot_size: usize,
    /// Entries evicted since creation.
    pub evicted_entries: u64,
    /// Chunk payload bytes evicted since creation.
    pub evicted_bytes: u64,
    /// Lowest RX offset still in the ring (or [`RingStats::rx_end`] if no RX is held).
    pub retained_from_rx_offset: u64,
    /// RX offset just past the newest RX byte.
    pub rx_end: u64,
    /// Lowest `source_seq` that may still be in the ring.
    pub retained_from_source_seq: u64,
    /// Newest `source_seq` pushed.
    pub last_source_seq: Option<u64>,
    /// `mono_ns` of the oldest entry held.
    pub oldest_mono_ns: Option<u64>,
}

#[derive(Debug, Clone)]
struct Slot {
    entry: Entry,
    /// RX data: end offset (exclusive). Others: RX end at push time.
    rx_key: u64,
    /// Data: `source_seq`. Gap: last seq + 1 at push time.
    seq_key: u64,
    payload: usize,
    meta: usize,
}

impl Slot {
    fn is_rx(&self) -> bool {
        matches!(&self.entry, Entry::Data(c) if c.dir == Dir::Rx)
    }
}

/// A view of a chunk starting `skip` bytes in (only the boundary chunk of an offset query is
/// partial).
#[derive(Debug, Clone, Copy)]
pub struct ChunkSlice<'a> {
    /// The chunk.
    pub chunk: &'a Chunk,
    /// Bytes skipped at the front.
    pub skip: usize,
}

impl<'a> ChunkSlice<'a> {
    /// Visible bytes.
    pub fn bytes(&self) -> &'a [u8] {
        &self.chunk.bytes[self.skip..]
    }

    /// RX offset of the first visible byte (RX only).
    pub fn rx_offset(&self) -> Option<u64> {
        self.chunk.rx_offset.map(|o| o + self.skip as u64)
    }
}

/// Item of [`Ring::rx_items_from`]: RX data or a gap mark in RX order.
#[derive(Debug, Clone, Copy)]
pub enum RxItem<'a> {
    /// RX bytes.
    Data(ChunkSlice<'a>),
    /// Loss mark.
    Gap(&'a GapMark),
}

/// Fixed per-channel ring.
#[derive(Debug, Clone)]
pub struct Ring {
    limits: RingLimits,
    slots: VecDeque<Slot>,
    payload_bytes: usize,
    meta_bytes: usize,
    rx_end: u64,
    rx_floor: u64,
    last_seq: Option<u64>,
    seq_floor: u64,
    evicted_entries: u64,
    evicted_bytes: u64,
    generation: u32,
}

fn shrink_string(s: &mut Option<String>, field: &'static str) -> Result<usize, RingError> {
    match s {
        Some(v) => {
            if v.len() > MAX_META_FIELD_BYTES {
                return Err(RingError::MetaFieldTooLong { field, len: v.len() });
            }
            v.shrink_to_fit();
            Ok(v.capacity())
        }
        None => Ok(0),
    }
}

impl Ring {
    /// Empty ring whose RX offsets start at 0.
    pub fn new(limits: RingLimits) -> Self {
        Self::new_at(limits, 0)
    }

    /// Empty ring whose RX offsets start at `rx_offset` (e.g. resume after restart).
    pub fn new_at(limits: RingLimits, rx_offset: u64) -> Self {
        Self {
            limits,
            slots: VecDeque::new(),
            payload_bytes: 0,
            meta_bytes: 0,
            rx_end: rx_offset,
            rx_floor: rx_offset,
            last_seq: None,
            seq_floor: 0,
            evicted_entries: 0,
            evicted_bytes: 0,
            generation: 0,
        }
    }

    /// Limits in force.
    pub fn limits(&self) -> RingLimits {
        self.limits
    }

    /// Entries held.
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// `true` when nothing is held.
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// RX offset just past the newest RX byte.
    pub fn rx_end(&self) -> u64 {
        self.rx_end
    }

    /// Lowest RX offset still readable from the ring.
    pub fn retained_from_rx_offset(&self) -> u64 {
        self.rx_floor
    }

    /// Lowest `source_seq` that may still be in the ring.
    pub fn retained_from_source_seq(&self) -> u64 {
        self.seq_floor
    }

    /// Generation of the newest pushed entry.
    pub fn generation(&self) -> u32 {
        self.generation
    }

    /// All entries, oldest first.
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.slots.iter().map(|s| &s.entry)
    }

    /// Append an entry, evicting the oldest ones as needed.
    pub fn push(&mut self, mut entry: Entry) -> Result<PushOutcome, RingError> {
        let (payload, meta, rx_key, seq_key) = match &mut entry {
            Entry::Data(c) => {
                if c.bytes.is_empty() {
                    return Err(RingError::EmptyChunk);
                }
                if c.bytes.len() > self.limits.max_payload {
                    return Err(RingError::ChunkTooLarge {
                        len: c.bytes.len(),
                        limit: self.limits.max_payload,
                    });
                }
                if let Some(last) = self.last_seq
                    && c.source_seq <= last
                {
                    return Err(RingError::NonMonotonicSeq {
                        got: c.source_seq,
                        last,
                    });
                }
                let rx_key = match (c.dir, c.rx_offset) {
                    (Dir::Rx, Some(o)) => {
                        if o < self.rx_end {
                            return Err(RingError::NonMonotonicRxOffset {
                                got: o,
                                end: self.rx_end,
                            });
                        }
                        o + c.bytes.len() as u64
                    }
                    (Dir::Tx, None) => self.rx_end,
                    _ => return Err(RingError::RxOffsetMismatch),
                };
                let meta = shrink_string(&mut c.client_id, "client_id")?
                    + shrink_string(&mut c.request_id, "request_id")?;
                c.bytes.shrink_to_fit();
                if c.bytes.capacity() > self.limits.max_payload {
                    return Err(RingError::ChunkTooLarge {
                        len: c.bytes.capacity(),
                        limit: self.limits.max_payload,
                    });
                }
                (c.bytes.capacity(), meta, rx_key, c.source_seq)
            }
            Entry::Gap(g) => {
                if g.cause.len() > MAX_META_FIELD_BYTES {
                    return Err(RingError::MetaFieldTooLong {
                        field: "cause",
                        len: g.cause.len(),
                    });
                }
                g.cause.shrink_to_fit();
                let seq_key = self.last_seq.map_or(0, |s| s + 1);
                (0, g.cause.capacity(), self.rx_end, seq_key)
            }
        };
        if meta > self.limits.max_meta {
            return Err(RingError::MetaTooLarge);
        }

        let mut outcome = PushOutcome::default();
        while !self.slots.is_empty()
            && (self.slots.len() >= self.limits.max_entries
                || self.payload_bytes + payload > self.limits.max_payload
                || self.meta_bytes + meta > self.limits.max_meta)
        {
            if let Some(len) = self.evict_front() {
                outcome.evicted_entries += 1;
                outcome.evicted_bytes += len;
            }
        }

        match &entry {
            Entry::Data(c) => {
                self.last_seq = Some(c.source_seq);
                self.generation = c.generation;
                if c.dir == Dir::Rx {
                    self.rx_end = rx_key;
                }
            }
            Entry::Gap(g) => self.generation = g.generation,
        }
        self.payload_bytes += payload;
        self.meta_bytes += meta;
        self.slots.push_back(Slot {
            entry,
            rx_key,
            seq_key,
            payload,
            meta,
        });
        Ok(outcome)
    }

    /// Evict the oldest entry; returns its payload length.
    fn evict_front(&mut self) -> Option<u64> {
        let slot = self.slots.pop_front()?;
        self.payload_bytes -= slot.payload;
        self.meta_bytes -= slot.meta;
        self.evicted_entries += 1;
        self.rx_floor = self.rx_floor.max(slot.rx_key);
        let len = match &slot.entry {
            Entry::Data(c) => {
                self.seq_floor = self.seq_floor.max(c.source_seq + 1);
                c.bytes.len() as u64
            }
            Entry::Gap(_) => 0,
        };
        self.evicted_bytes += len;
        Some(len)
    }

    /// Statistics.
    pub fn stats(&self) -> RingStats {
        RingStats {
            limits: self.limits,
            entries: self.slots.len(),
            payload_bytes: self.payload_bytes,
            meta_bytes: self.meta_bytes,
            slot_capacity: self.slots.capacity(),
            slot_size: std::mem::size_of::<Slot>(),
            evicted_entries: self.evicted_entries,
            evicted_bytes: self.evicted_bytes,
            retained_from_rx_offset: self.rx_floor,
            rx_end: self.rx_end,
            retained_from_source_seq: self.seq_floor,
            last_source_seq: self.last_seq,
            oldest_mono_ns: self.slots.front().map(|s| s.entry.mono_ns()),
        }
    }

    /// Index of the first slot at or after RX offset `off` (gap marks at exactly `off` are
    /// included, RX data ending at or before `off` is not).
    fn rx_start_index(&self, off: u64) -> usize {
        self.slots.partition_point(|s| {
            if s.is_rx() {
                s.rx_key <= off
            } else {
                s.rx_key < off
            }
        })
    }

    /// RX chunks from RX offset `off` on; the boundary chunk is partial.
    pub fn iter_from_rx_offset(&self, off: u64) -> impl Iterator<Item = ChunkSlice<'_>> {
        self.rx_items_from(off).filter_map(|i| match i {
            RxItem::Data(s) => Some(s),
            RxItem::Gap(_) => None,
        })
    }

    /// RX chunks and gap marks from RX offset `off` on, in order (TX chunks are skipped).
    pub fn rx_items_from(&self, off: u64) -> impl Iterator<Item = RxItem<'_>> {
        let start = self.rx_start_index(off);
        self.slots.range(start..).filter_map(move |s| match &s.entry {
            Entry::Data(c) if c.dir == Dir::Rx => {
                let o = c.rx_offset.unwrap_or(0);
                let skip = usize::try_from(off.saturating_sub(o)).unwrap_or(usize::MAX);
                Some(RxItem::Data(ChunkSlice {
                    chunk: c,
                    skip: skip.min(c.bytes.len()),
                }))
            }
            Entry::Data(_) => None,
            Entry::Gap(g) => Some(RxItem::Gap(g)),
        })
    }

    /// Entries from the first data chunk with `source_seq >= seq` on, including gap marks
    /// pushed after the last chunk below `seq`.
    pub fn iter_from_source_seq(&self, seq: u64) -> impl Iterator<Item = &Entry> {
        let start = self.slots.partition_point(|s| s.seq_key < seq);
        self.slots.range(start..).map(|s| &s.entry)
    }

    /// RX offset of the first RX byte at or after `source_seq`, or [`Ring::rx_end`] if none.
    pub fn rx_offset_for_source_seq(&self, seq: u64) -> u64 {
        self.iter_from_source_seq(seq)
            .find_map(|e| match e {
                Entry::Data(c) if c.dir == Dir::Rx => c.rx_offset,
                _ => None,
            })
            .unwrap_or(self.rx_end)
    }

    /// The newest `n` payload bytes (RX only, or RX+TX), oldest slice first; the first slice
    /// may be partial.
    pub fn last_n_bytes(&self, n: usize, include_tx: bool) -> Vec<ChunkSlice<'_>> {
        let mut out = Vec::new();
        let mut left = n;
        for s in self.slots.iter().rev() {
            if left == 0 {
                break;
            }
            let Entry::Data(c) = &s.entry else { continue };
            if c.dir == Dir::Tx && !include_tx {
                continue;
            }
            let take = left.min(c.bytes.len());
            left -= take;
            out.push(ChunkSlice {
                chunk: c,
                skip: c.bytes.len() - take,
            });
        }
        out.reverse();
        out
    }
}
