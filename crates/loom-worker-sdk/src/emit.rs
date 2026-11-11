//! Non-blocking event emission (plan §3.3, can-core W2·W7).
//!
//! Domain hot-path threads call [`Emitter`]; it builds a [`SourceRecord`] and hands it to an
//! [`EmitSink`] (P1: journal writer + data/ctl lanes; tests: [`MemorySink`]). `try_push` must
//! never block. On failure the caller gets [`EmitError`] and should only count it; the consumed
//! `source_seq` leaves a visible hole.

use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, Mutex};

use loom_proto::event::{
    Dir, Env, GapCount, GapData, Metric, Origin, ProducerRange, Risk, SourceRecord, Ts, TsSource, kinds,
};
use loom_proto::num::U64;
use loom_proto::worker::Lane;
use serde_json::Value;

use crate::clock::Clock;

/// Emission failure. Never blocks; hot paths count these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EmitError {
    /// Queue full — the event was dropped (journal → `record.gap`, screen → `display.gap`).
    #[error("emit queue full")]
    Full,
    /// SDK shutting down.
    #[error("emitter closed")]
    Closed,
    /// 더 큰 seq를 이미 발행 시도했으므로 저널 순서를 거꾸로 만들 수 없다.
    #[error("source seq {seq} is not after {last}")]
    OutOfOrder {
        /// 요청한 seq.
        seq: u64,
        /// 직전 발행 시도의 seq.
        last: u64,
    },
    /// 이 producer에서 예약한 적 없는 seq.
    #[error("source seq was not reserved")]
    UnreservedSeq,
    /// producer epoch의 u64 seq를 모두 사용했다.
    #[error("source sequence exhausted")]
    SeqExhausted,
}

/// Destination of emitted records. Implementations must not block.
pub trait EmitSink: Send + Sync + fmt::Debug {
    /// Push one record onto `lane` (`Data` = bulk RX, `Ctl` = priority).
    fn try_push(&self, lane: Lane, record: SourceRecord) -> Result<(), EmitError>;
}

/// worker가 작성한 data·priority 이벤트(write 상태, 응답, 장비 데이터 등).
#[derive(Debug, Clone, PartialEq)]
pub struct EventDraft {
    /// Kind `<domain>.<event>`.
    pub kind: String,
    /// Timestamp.
    pub ts: Ts,
    /// Direction.
    pub dir: Option<Dir>,
    /// Origin (for write rows: the requester's origin from `Write.origin`).
    pub origin: Origin,
    /// Client id (write rows).
    pub client_id: Option<String>,
    /// Request id (write rows — required with `payload.write_state`, E8).
    pub request_id: Option<String>,
    /// Job id.
    pub job_id: Option<String>,
    /// Device/link generation.
    pub generation: Option<u32>,
    /// Purpose.
    pub purpose: Option<String>,
    /// Risk.
    pub risk: Option<Risk>,
    /// Environment.
    pub env: Option<Env>,
    /// Payload object.
    pub payload: Value,
    /// Metrics.
    pub metrics: Vec<Metric>,
    /// Quality flags.
    pub quality: Vec<String>,
}

#[derive(Debug)]
struct Producer {
    session_id: String,
    domain: String,
    producer_id: String,
    producer_epoch: U64,
    /// Held while a record gets its seq **and** is pushed into the sink, so sinks (journal)
    /// see `source_seq` strictly increasing even when several threads emit.
    order: Mutex<Sequence>,
}

#[derive(Debug)]
struct Sequence {
    first: u64,
    next: u64,
    last: Option<u64>,
    exhausted: bool,
}
impl Sequence {
    fn reserve(&mut self) -> Result<U64, EmitError> {
        if self.exhausted {
            return Err(EmitError::SeqExhausted);
        }
        let seq = self.next;
        match self.next.checked_add(1) {
            Some(next) => self.next = next,
            None => self.exhausted = true,
        }
        Ok(U64(seq))
    }
}

/// Per-channel emitter. Cheap to clone; clones share the producer sequence.
#[derive(Clone)]
pub struct Emitter {
    producer: Arc<Producer>,
    channel: String,
    generation: Option<u32>,
    env: Option<Env>,
    sink: Arc<dyn EmitSink>,
    clock: Arc<dyn Clock>,
}

impl fmt::Debug for Emitter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Emitter")
            .field("producer", &self.producer)
            .field("channel", &self.channel)
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

impl Emitter {
    /// New emitter. `first_seq` is the next source seq of this producer epoch.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        session_id: impl Into<String>,
        domain: impl Into<String>,
        producer_id: impl Into<String>,
        producer_epoch: U64,
        first_seq: u64,
        channel: impl Into<String>,
        sink: Arc<dyn EmitSink>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Emitter {
            producer: Arc::new(Producer {
                session_id: session_id.into(),
                domain: domain.into(),
                producer_id: producer_id.into(),
                producer_epoch,
                order: Mutex::new(Sequence {
                    first: first_seq,
                    next: first_seq,
                    last: None,
                    exhausted: false,
                }),
            }),
            channel: channel.into(),
            generation: None,
            env: None,
            sink,
            clock,
        }
    }

    /// Same producer (shared seq), another channel.
    pub fn for_channel(&self, channel: impl Into<String>) -> Self {
        Emitter {
            channel: channel.into(),
            ..self.clone()
        }
    }

    /// Set the generation / env stamped on RX records (after a reopen).
    pub fn with_generation(mut self, generation: Option<u32>, env: Option<Env>) -> Self {
        self.generation = generation;
        self.env = env;
        self
    }

    /// Clock used for timestamps.
    pub fn clock(&self) -> &dyn Clock {
        &*self.clock
    }

    /// Assign the next seq and push, atomically with respect to other emitters of the producer.
    fn push(&self, lane: Lane, mut rec: SourceRecord) -> Result<U64, EmitError> {
        let mut order = self
            .producer
            .order
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let seq = order.reserve()?;
        order.last = Some(seq.0);
        rec.source_seq = seq;
        self.sink.try_push(lane, rec).map(|()| seq)
    }

    /// seq만 예약한다. 낮은 예약 seq부터 발행할 책임은 호출자에게 있다.
    pub fn reserve_seq(&self) -> Result<U64, EmitError> {
        self.producer
            .order
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .reserve()
    }

    fn push_reserved(&self, lane: Lane, seq: U64, mut rec: SourceRecord) -> Result<U64, EmitError> {
        let mut order = self
            .producer
            .order
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if seq.0 < order.first || (!order.exhausted && seq.0 >= order.next) {
            return Err(EmitError::UnreservedSeq);
        }
        if let Some(last) = order.last
            && seq.0 <= last
        {
            return Err(EmitError::OutOfOrder { seq: seq.0, last });
        }
        // sink가 저널 수락 후 화면 Full을 반환해도 seq를 되돌리지 않는다.
        order.last = Some(seq.0);
        rec.source_seq = seq;
        self.sink.try_push(lane, rec).map(|()| seq)
    }

    #[allow(clippy::too_many_arguments)]
    fn record(
        &self,
        domain: &str,
        kind: String,
        ts: Ts,
        dir: Option<Dir>,
        origin: Origin,
        payload: Value,
    ) -> SourceRecord {
        SourceRecord {
            schema_version: loom_proto::SCHEMA_VERSION,
            session_id: self.producer.session_id.clone(),
            domain: domain.to_owned(),
            channel: (!self.channel.is_empty()).then(|| self.channel.clone()),
            kind,
            producer_id: self.producer.producer_id.clone(),
            producer_epoch: self.producer.producer_epoch,
            source_seq: U64(0),
            ts,
            dir,
            origin,
            client_id: None,
            actor_id: None,
            external_actor: None,
            original_origin: None,
            request_id: None,
            job_id: None,
            generation: self.generation,
            purpose: None,
            risk: None,
            test_session: None,
            test_step: None,
            env: self.env,
            payload,
            metrics: Vec::new(),
            quality: Vec::new(),
        }
    }

    /// Device RX event on the data lane (`origin = device`, `dir = rx`). Returns its source seq.
    pub fn rx(&self, kind: &str, ts: Ts, payload: Value) -> Result<U64, EmitError> {
        let rec = self.record(
            &self.producer.domain,
            kind.to_owned(),
            ts,
            Some(Dir::Rx),
            Origin::Device,
            payload,
        );
        self.push(Lane::Data, rec)
    }

    /// 예약한 seq로 RX를 발행한다. 이미 더 큰 seq를 시도했으면 오류다.
    pub fn rx_with_seq(&self, seq: U64, kind: &str, ts: Ts, payload: Value) -> Result<U64, EmitError> {
        let rec = self.record(
            &self.producer.domain,
            kind.into(),
            ts,
            Some(Dir::Rx),
            Origin::Device,
            payload,
        );
        self.push_reserved(Lane::Data, seq, rec)
    }

    /// Priority event on the control lane (write states, responses, state changes).
    pub fn priority(&self, d: EventDraft) -> Result<U64, EmitError> {
        self.push(Lane::Ctl, self.draft(d))
    }

    /// data lane에 모든 draft 필드를 보존하여 발행한다.
    pub fn data(&self, d: EventDraft) -> Result<U64, EmitError> {
        self.push(Lane::Data, self.draft(d))
    }

    /// 예약한 seq로 data draft를 발행한다.
    pub fn data_with_seq(&self, seq: U64, d: EventDraft) -> Result<U64, EmitError> {
        self.push_reserved(Lane::Data, seq, self.draft(d))
    }

    fn draft(&self, d: EventDraft) -> SourceRecord {
        let mut rec = self.record(&self.producer.domain, d.kind, d.ts, d.dir, d.origin, d.payload);
        rec.client_id = d.client_id;
        rec.request_id = d.request_id;
        rec.job_id = d.job_id;
        if d.generation.is_some() {
            rec.generation = d.generation;
        }
        rec.purpose = d.purpose;
        rec.risk = d.risk;
        if d.env.is_some() {
            rec.env = d.env;
        }
        rec.metrics = d.metrics;
        rec.quality = d.quality;
        rec
    }

    /// Report loss before Loom received the data (`source.gap`, A13) on the control lane.
    /// Domain and channel are those of the affected stream (this emitter), contract 02 §2.4.
    pub fn source_gap(&self, cause: &str, count: GapCount) -> Result<U64, EmitError> {
        self.source_gap_with(cause, count, None, Value::Null)
    }

    /// source.gap의 producer 범위와 도메인 상세를 함께 발행하는 추가 API.
    pub fn source_gap_with(
        &self,
        cause: &str,
        count: GapCount,
        producer_range: Option<ProducerRange>,
        detail: Value,
    ) -> Result<U64, EmitError> {
        let gap = GapData {
            seq_range: None,
            producer_range,
            count,
            cause: cause.to_owned(),
            bytes: None,
            detail,
        };
        let payload = serde_json::to_value(&gap).unwrap_or(Value::Null);
        let ts = self.clock.ts(TsSource::HostReadComplete);
        let rec = self.record(
            &self.producer.domain,
            kinds::SOURCE_GAP.to_owned(),
            ts,
            None,
            Origin::Device,
            payload,
        );
        self.push(Lane::Ctl, rec)
    }

    /// Producer-level state event on the control lane (`record.gap`, `record.health` …):
    /// domain = this producer's domain, channel = this emitter's (none for `""`).
    pub fn system(&self, kind: &str, payload: Value) -> Result<U64, EmitError> {
        let ts = self.clock.ts(TsSource::HostReadComplete);
        let rec = self.record(
            &self.producer.domain,
            kind.to_owned(),
            ts,
            None,
            Origin::Device,
            payload,
        );
        self.push(Lane::Ctl, rec)
    }
}

impl Emitter {
    /// Report a screen-only loss (`display.gap`, A13) on the control lane: records of this
    /// producer in `range` were journaled but not delivered to the gateway (e.g. no data-lane
    /// credit, cause `ipc_backpressure`). Domain/channel = the affected stream (contract 02).
    pub fn display_gap(
        &self,
        range: ProducerRange,
        count: GapCount,
        cause: &str,
        bytes: Option<u64>,
    ) -> Result<U64, EmitError> {
        self.display_gap_with(Some(range), count, cause, bytes, Value::Null)
    }

    /// display.gap의 범위·상세를 지정한다. 영향받은 emitter의 domain/channel을 유지한다.
    pub fn display_gap_with(
        &self,
        range: Option<ProducerRange>,
        count: GapCount,
        cause: &str,
        bytes: Option<u64>,
        detail: Value,
    ) -> Result<U64, EmitError> {
        let gap = GapData {
            seq_range: None,
            producer_range: range,
            count,
            cause: cause.to_owned(),
            bytes: bytes.map(U64),
            detail,
        };
        let payload = serde_json::to_value(&gap).unwrap_or(Value::Null);
        let ts = self.clock.ts(TsSource::HostReadComplete);
        let rec = self.record(
            &self.producer.domain,
            kinds::DISPLAY_GAP.to_owned(),
            ts,
            None,
            Origin::Device,
            payload,
        );
        self.push(Lane::Ctl, rec)
    }

    /// 원본 기록 손실을 영향받은 스트림의 record.gap으로 발행한다.
    pub fn record_gap(
        &self,
        range: Option<ProducerRange>,
        count: GapCount,
        cause: &str,
        bytes: Option<u64>,
        detail: Value,
    ) -> Result<U64, EmitError> {
        let gap = GapData {
            seq_range: None,
            producer_range: range,
            count,
            cause: cause.into(),
            bytes: bytes.map(U64),
            detail,
        };
        self.system(
            kinds::RECORD_GAP,
            serde_json::to_value(gap).expect("GapData 직렬화"),
        )
    }

    /// Producer id of this emitter.
    pub fn producer_id(&self) -> &str {
        &self.producer.producer_id
    }

    /// Producer epoch of this emitter.
    pub fn producer_epoch(&self) -> U64 {
        self.producer.producer_epoch
    }
}

/// Bounded in-memory sink for tests and simulations. Full → [`EmitError::Full`], never blocks
/// for long (one short mutex section per push).
#[derive(Debug)]
pub struct MemorySink {
    cap: usize,
    items: Mutex<VecDeque<(Lane, SourceRecord)>>,
}

impl MemorySink {
    /// Sink holding at most `cap` records.
    pub fn new(cap: usize) -> Self {
        MemorySink {
            cap,
            items: Mutex::new(VecDeque::with_capacity(cap.min(4096))),
        }
    }

    /// Remove and return everything pushed so far.
    pub fn drain(&self) -> Vec<(Lane, SourceRecord)> {
        match self.items.lock() {
            Ok(mut q) => q.drain(..).collect(),
            Err(p) => p.into_inner().drain(..).collect(),
        }
    }
}

impl EmitSink for MemorySink {
    fn try_push(&self, lane: Lane, record: SourceRecord) -> Result<(), EmitError> {
        let mut q = self.items.try_lock().map_err(|_| EmitError::Full)?;
        if q.len() >= self.cap {
            return Err(EmitError::Full);
        }
        q.push_back((lane, record));
        Ok(())
    }
}
