//! Replay helpers on top of the real hub ([`crate::hub`]): record construction for generated
//! events, a latch flag and connection ids. Subscription/fan-out logic is the gateway's own.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use loom_proto::event::{Dir, Env, Origin, SourceRecord, Ts, TsSource};
use loom_proto::num::U64;
use serde_json::Value;

use crate::hub::{Hub, HubBuilder, Published};

/// Replay hub wrapper.
#[derive(Debug)]
pub struct ReplayHub {
    /// The real hub.
    pub hub: Hub,
    next_src: AtomicU64,
    next_conn: AtomicU64,
    latch: AtomicBool,
}

impl ReplayHub {
    /// Start a hub with stream epoch 1.
    pub fn start() -> std::io::Result<Arc<Self>> {
        Ok(Arc::new(ReplayHub {
            hub: HubBuilder::new(1).start()?,
            next_src: AtomicU64::new(1),
            next_conn: AtomicU64::new(1),
            latch: AtomicBool::new(false),
        }))
    }

    /// Stream epoch.
    pub fn epoch(&self) -> u64 {
        self.hub.epoch
    }

    /// New connection id.
    pub fn next_conn_id(&self) -> u64 {
        self.next_conn.fetch_add(1, Ordering::Relaxed)
    }

    /// Timestamp now.
    pub fn ts(&self, source: TsSource) -> Ts {
        self.hub.clock.ts(source)
    }

    /// Latch flag.
    pub fn latch(&self) -> bool {
        self.latch.load(Ordering::Acquire)
    }

    /// Set the latch flag (the caller publishes `core.latch`).
    pub fn set_latch(&self, on: bool) {
        self.latch.store(on, Ordering::Release);
    }

    /// High-water stream seq.
    pub async fn hw(&self) -> u64 {
        self.hub.status().await.map_or(0, |s| s.hw)
    }

    /// Publish (fire and forget; ordering is kept by the hub channel).
    pub fn publish(&self, srcs: Vec<SourceRecord>) {
        drop(self.hub.publish(srcs));
    }

    /// Publish and wait until delivered to every connection queue.
    pub async fn publish_wait(&self, srcs: Vec<SourceRecord>) -> Vec<Arc<Published>> {
        self.hub.publish(srcs).await.unwrap_or_default()
    }

    /// Record from the `replay` producer.
    pub fn source(
        &self,
        domain: &str,
        channel: Option<&str>,
        kind: &str,
        dir: Option<Dir>,
        origin: Origin,
        payload: Value,
    ) -> SourceRecord {
        SourceRecord {
            schema_version: loom_proto::SCHEMA_VERSION,
            session_id: "replay".into(),
            domain: domain.into(),
            channel: channel.map(Into::into),
            kind: kind.into(),
            producer_id: "replay".into(),
            producer_epoch: U64(1),
            source_seq: U64(self.next_src.fetch_add(1, Ordering::Relaxed)),
            ts: self.ts(TsSource::Synthetic),
            dir,
            origin,
            client_id: None,
            actor_id: None,
            external_actor: None,
            original_origin: None,
            request_id: None,
            job_id: None,
            generation: None,
            purpose: Some("sim".into()),
            risk: None,
            test_session: None,
            test_step: None,
            env: Some(Env::Sim),
            payload,
            metrics: Vec::new(),
            quality: vec!["simulated".into()],
        }
    }

    /// Give a replayed fixture record this producer's identity and a fresh seq/timestamp.
    pub fn restamp(&self, mut s: SourceRecord) -> SourceRecord {
        s.producer_id = "replay".into();
        s.producer_epoch = U64(1);
        s.source_seq = U64(self.next_src.fetch_add(1, Ordering::Relaxed));
        s.ts = self.ts(s.ts.source);
        s
    }
}
