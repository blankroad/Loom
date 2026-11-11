//! Core RPCs used by the `loom` CLI (P1 C3): `core.events.query`, `core.help`, `core.ui.focus`,
//! `core.meta`, `core.shutdown`.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use loom_proto::cursor::{Cursor, TimeUnit};
use loom_proto::error::ErrorCode;
use loom_proto::event::kinds;
use loom_proto::num::U64;
use loom_proto::rpc::{INVALID_PARAMS, Role, RpcError, SubFilter};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Caller, Daemon, client_key};
use crate::hub::{RangeQuery, lock};
use crate::journal::{JournalRead, ReadLimits};
use crate::rpcutil::{app_error, app_error_detail, params, rpc_error};

/// `ui.focus_request` rate per client.
pub const FOCUS_EVERY: Duration = Duration::from_secs(2);
/// Default rows of `core.events.query`.
pub const EVENTS_DEFAULT: u32 = 100;
/// Hard rows (contract 03 §3.1).
pub const EVENTS_MAX: u32 = 1000;
/// Event JSON bytes per result (leaves room for the envelope under 64 KiB).
pub const EVENTS_BYTES: usize = 60 * 1024;

#[derive(Debug, Deserialize)]
struct EventsQuery {
    #[serde(default)]
    since: Option<String>,
    #[serde(default)]
    until: Option<String>,
    #[serde(default)]
    filter: SubFilter,
    #[serde(default)]
    max: Option<u32>,
    /// Do not advance the client's `@last` cursor.
    #[serde(default)]
    peek: bool,
}

fn rel(amount: u64, unit: TimeUnit) -> Duration {
    match unit {
        TimeUnit::Millis => Duration::from_millis(amount),
        TimeUnit::Secs => Duration::from_secs(amount),
        TimeUnit::Mins => Duration::from_secs(amount.saturating_mul(60)),
        TimeUnit::Hours => Duration::from_secs(amount.saturating_mul(3600)),
    }
}

fn parse_cursor(s: &str) -> Result<Cursor, RpcError> {
    s.parse::<Cursor>()
        .map_err(|e| rpc_error(INVALID_PARAMS, format!("bad cursor {s:?}: {e}")))
}

fn unsupported(c: &Cursor) -> RpcError {
    app_error_detail(
        ErrorCode::NotImplemented,
        &format!("cursor {c} is not supported by the M1 ring query (use @last, now, seq:N, mark:X or -5s)"),
        Value::Null,
    )
}

impl Daemon {
    pub(super) async fn mark_seq(&self, label: &str) -> Result<u64, RpcError> {
        let r = self
            .hub
            .range(RangeQuery {
                filter: SubFilter {
                    kinds: vec![kinds::CORE_MARK.into()],
                    ..SubFilter::default()
                },
                max: usize::MAX,
                newest: true,
                ..RangeQuery::default()
            })
            .await
            .map_err(|_| app_error(ErrorCode::Daemon, "hub stopped"))?;
        r.events
            .iter()
            .rev()
            .find(|p| p.rec.payload["label"] == label)
            .map(|p| p.rec.seq.get())
            .ok_or_else(|| {
                app_error(
                    ErrorCode::CursorGap,
                    &format!("mark {label:?} is not in the live ring"),
                )
            })
    }

    /// `core.events.query{since?, until?, filter?, max?, peek?}` — bounded read of the hub ring.
    /// Without `since` the newest `max` events are returned. `@last` = after the events this client
    /// was last given (client-global, `#pid` ignored).
    pub(super) async fn events_query(&self, c: &Caller, p: Option<Value>) -> Result<Value, RpcError> {
        let q: EventsQuery = params(p)?;
        let max = q.max.unwrap_or(EVENTS_DEFAULT).clamp(1, EVENTS_MAX) as usize;
        let me = client_key(&c.client_id).to_owned();
        let hw_now = self.hw();
        let mut rq = RangeQuery {
            filter: q.filter.clone(),
            max,
            newest: true,
            ..RangeQuery::default()
        };
        let mut other_epoch: Option<(u64, u64)> = None;
        if let Some(s) = &q.since {
            let cur = parse_cursor(s)?;
            rq.newest = false;
            match &cur {
                Cursor::Last => match lock(&self.event_cursors).get(&me).copied() {
                    Some(seq) => rq.after = Some(seq),
                    None => rq.newest = true, // first call: the newest events
                },
                Cursor::Now => rq.after = Some(hw_now),
                Cursor::Seq { epoch, seq } => {
                    if let Some(e) = epoch.filter(|e| *e != self.hub.epoch) {
                        other_epoch = Some((e, *seq)); // an earlier daemon run: its recording
                    }
                    rq.after = Some(seq.saturating_sub(1));
                }
                Cursor::Mark(label) => rq.after = Some(self.mark_seq(label).await?.saturating_sub(1)),
                Cursor::Relative { amount, unit } => {
                    rq.since_at = Instant::now().checked_sub(rel(*amount, *unit));
                }
                other => return Err(unsupported(other)),
            }
        }
        if let Some(s) = &q.until {
            let cur = parse_cursor(s)?;
            match &cur {
                Cursor::Now | Cursor::Last => rq.until = Some(hw_now),
                Cursor::Seq { seq, .. } => rq.until = Some(*seq),
                Cursor::Mark(label) => rq.until = Some(self.mark_seq(label).await?),
                Cursor::Relative { amount, unit } => {
                    rq.until_at = Instant::now().checked_sub(rel(*amount, *unit));
                }
                other => return Err(unsupported(other)),
            }
        }
        if let Some((e, from)) = other_epoch {
            let to = match &q.until {
                Some(u) => match parse_cursor(u)? {
                    Cursor::Seq { seq, .. } => seq,
                    other => return Err(unsupported(&other)),
                },
                None => u64::MAX,
            };
            let jr = self.journal_read(e, from, to, &q.filter, max).await?;
            return Ok(journal_result(e, Vec::new(), jr, None));
        }
        let after = rq.after;
        let until = rq.until;
        let newest = rq.newest;
        let r = self
            .hub
            .range(rq)
            .await
            .map_err(|_| app_error(ErrorCode::Daemon, "hub stopped"))?;
        let mut events = Vec::with_capacity(r.events.len());
        let mut bytes = 0;
        // Newest-first reads (no `since`) are complete by definition: older events are not "more".
        let mut truncated = r.more && !r.events.is_empty() && !newest;
        let mut source = "ring";
        // Start older than the ring: read the gap from the recording (mapping → segments).
        if let Some(a) = after
            && a < r.hw
            && r.oldest.is_none_or(|o| a + 1 < o)
        {
            let ring_from = r.oldest.unwrap_or(r.hw + 1);
            let to = until.unwrap_or(r.hw).min(ring_from.saturating_sub(1));
            let jr = self
                .journal_read(self.hub.epoch, a + 1, to, &q.filter, max)
                .await?;
            if jr.next_seq.is_some() {
                // capped inside the journal part: the ring part comes on the next page
                return Ok(self.finish_journal(c, &q, jr));
            }
            for e in &jr.events {
                let v = serde_json::to_value(e).unwrap_or(Value::Null);
                bytes += v.to_string().len();
                events.push(v);
            }
            source = if events.is_empty() { "ring" } else { "journal+ring" };
        }
        for e in &r.events {
            if events.len() >= max {
                truncated = true;
                break;
            }
            if bytes + e.json.len() > EVENTS_BYTES && !events.is_empty() {
                truncated = true;
                break;
            }
            bytes += e.json.len();
            events.push(serde_json::from_str::<Value>(&e.json).unwrap_or(Value::Null));
        }
        let last = events
            .last()
            .and_then(|e| e["seq"].as_str())
            .and_then(|s| s.parse::<u64>().ok());
        let first = events
            .first()
            .and_then(|e| e["seq"].as_str())
            .and_then(|s| s.parse::<u64>().ok());
        // `@last` advances to what was returned (or to the high-water when nothing more matched).
        let reached = if truncated { last } else { last.max(Some(r.hw)) };
        if !q.peek
            && let Some(seq) = reached
        {
            lock(&self.event_cursors).insert(me, seq);
        }
        let next = if truncated {
            last.map(|l| {
                Cursor::Seq {
                    epoch: Some(self.hub.epoch),
                    seq: l + 1,
                }
                .to_string()
            })
        } else {
            None
        };
        Ok(json!({
            "events": events,
            "count": events.len(),
            "truncated": truncated,
            "next_cursor": next,
            "range": first.zip(last).map(|(a, b)| json!({"from": U64(a), "to": U64(b)})),
            "stream_seq_hw": U64(r.hw),
            "stream_epoch": U64(self.hub.epoch),
            "source": source,
        }))
    }

    /// Bounded journal read off the async workers. Out of retention → `E_CURSOR_GAP`, scan budget
    /// exhausted → `E_SCAN_LIMIT` with `resume_from` (contract 06 §6.5).
    pub(super) async fn journal_read(
        &self,
        epoch: u64,
        from: u64,
        to: u64,
        filter: &SubFilter,
        max: usize,
    ) -> Result<JournalRead, RpcError> {
        self.journal_read_bounded(epoch, from, to, filter, max, self.cfg.scan_bytes)
            .await
    }

    pub(super) async fn journal_read_bounded(
        &self,
        epoch: u64,
        from: u64,
        to: u64,
        filter: &SubFilter,
        max: usize,
        byte_limit: u64,
    ) -> Result<JournalRead, RpcError> {
        let j = self.journal.clone();
        let f = filter.clone();
        let lim = ReadLimits {
            max,
            result_bytes: EVENTS_BYTES,
            scan_bytes: self.cfg.scan_bytes.min(byte_limit),
            time: Duration::from_secs(2),
        };
        let jr = tokio::task::spawn_blocking(move || j.read_range(epoch, from, to, &f, lim))
            .await
            .map_err(|_| app_error(ErrorCode::Internal, "journal read panicked"))?
            .map_err(|e| app_error(ErrorCode::Internal, &format!("journal read: {e}")))?;
        if jr.out_of_retention {
            return Err(app_error_detail(
                ErrorCode::CursorGap,
                "this range is not in the recording (deleted by retention, or not recorded)",
                json!({"stream_epoch": U64(epoch), "from": U64(from)}),
            ));
        }
        if jr.scan_limited {
            let resume = jr.next_seq.map(|s| {
                Cursor::Seq {
                    epoch: Some(epoch),
                    seq: s,
                }
                .to_string()
            });
            return Err(app_error_detail(
                ErrorCode::ScanLimit,
                "scan budget reached — continue from resume_from",
                json!({"resume_from": resume, "partial": jr.events, "scanned_bytes": U64(jr.scanned_bytes)}),
            ));
        }
        Ok(jr)
    }

    fn finish_journal(&self, c: &Caller, q: &EventsQuery, jr: JournalRead) -> Value {
        if !q.peek
            && let Some(n) = jr.next_seq
        {
            lock(&self.event_cursors).insert(client_key(&c.client_id).to_owned(), n.saturating_sub(1));
        }
        journal_result(self.hub.epoch, Vec::new(), jr, None)
    }

    /// `core.help{domain?}` — canonical machine help (C8): core commands + Registry `HelpSpec` and
    /// command list of each domain.
    pub(super) fn help(&self, p: Option<Value>) -> Result<Value, RpcError> {
        let v: Value = params(p)?;
        let only = v["domain"].as_str();
        if let Some(d) = only
            && !self.domains.contains_key(d)
        {
            return Err(app_error(ErrorCode::Usage, &format!("unknown domain {d:?}")));
        }
        let domains: Vec<Value> = self
            .domains
            .iter()
            .filter(|(n, _)| only.is_none_or(|o| o == n.as_str()))
            .map(|(n, d)| {
                let mut cmds: Vec<Value> = d
                    .commands
                    .values()
                    .map(|c| {
                        json!({
                            "method": c.method,
                            "write_class": c.write_class,
                            "success_terminal": d.terminals.get(&c.method),
                            "roles": c.roles,
                            "max_wait_ms": u64::try_from(c.bounds.max_wait.as_millis()).unwrap_or(u64::MAX),
                            "default_rows": c.bounds.default_rows,
                            "max_rows": c.bounds.max_rows,
                            "params": c.params,
                        })
                    })
                    .collect();
                if !d.pools.is_empty() {
                    cmds.push(json!({"method": format!("{n}.input"), "write_class": "interactive", "roles": {"agent": false, "script": false, "viewer": "operate"}, "note": "viewer-only interactive input (core-provided)"}));
                }
                cmds.sort_by(|a, b| a["method"].as_str().cmp(&b["method"].as_str()));
                json!({"name": n, "help": d.help, "commands": cmds})
            })
            .collect();
        let core = if only.is_some() { Value::Null } else { core_help() };
        Ok(
            json!({"version": env!("CARGO_PKG_VERSION"), "proto": super::PROTO, "core": core, "domains": domains}),
        )
    }

    /// `core.ui.focus{panel, filter?, at?}` → `ui.focus_request` (the UI may bring that panel up).
    pub(super) async fn ui_focus(&self, c: &Caller, p: Option<Value>) -> Result<Value, RpcError> {
        let v: Value = params(p)?;
        let panel = v["panel"]
            .as_str()
            .ok_or_else(|| rpc_error(INVALID_PARAMS, "panel is required"))?;
        // UI Q19: at most one focus request per client every 2 s (the UI also limits).
        {
            let key = client_key(&c.client_id).to_owned();
            let mut last = lock(&self.focus_last);
            if let Some(t) = last.get(&key)
                && t.elapsed() < FOCUS_EVERY
            {
                let wait = FOCUS_EVERY.saturating_sub(t.elapsed());
                return Err(app_error_detail(
                    ErrorCode::RateLimit,
                    "one ui focus request per 2 s",
                    json!({"retry_after_ms": u64::try_from(wait.as_millis()).unwrap_or(2000)}),
                ));
            }
            last.insert(key, Instant::now());
        }
        let payload = json!({
            "panel": panel,
            "props": v.get("props").or_else(|| v.get("filter")),
            "at": v.get("at"),
            "reason": v.get("reason"),
            "actor": c.client_id,
        });
        let mut s = self.source(c, "ui", None, kinds::UI_FOCUS_REQUEST, payload);
        s.actor_id = Some(c.client_id.clone());
        let rec = self.hub.publish(vec![s]).await.unwrap_or_default();
        Ok(json!({"seq": rec.first().map(|r| r.rec.seq), "viewers": self.viewers()}))
    }

    /// `core.meta` — envelope meta, warnings and `others_since_last` for a core command result
    /// (the CLI wraps plain core results into an envelope with these). Advances the cursor.
    pub(super) fn meta_rpc(&self, c: &Caller) -> Value {
        let (meta, warnings) = self.meta();
        let others = self.others_since_last(c);
        json!({"meta": meta, "warnings": warnings, "others_since_last": others})
    }

    /// `core.shutdown` (agent/script token only).
    pub(super) fn shutdown_rpc(&self, c: &Caller) -> Result<Value, RpcError> {
        if c.role == Role::Viewer {
            return Err(app_error(
                ErrorCode::Denied,
                "only the CLI (agent token) can stop the daemon",
            ));
        }
        tracing::info!(by = %c.client_id, "shutdown requested");
        self.shutdown.notify_waiters();
        self.shutdown.notify_one();
        Ok(json!({"shutting_down": true}))
    }
}

fn journal_result(epoch: u64, mut events: Vec<Value>, jr: JournalRead, hw: Option<u64>) -> Value {
    events.extend(
        jr.events
            .iter()
            .map(|e| serde_json::to_value(e).unwrap_or(Value::Null)),
    );
    let seq = |e: &Value| e["seq"].as_str().and_then(|s| s.parse::<u64>().ok());
    let (first, last) = (events.first().and_then(seq), events.last().and_then(seq));
    json!({
        "events": events,
        "count": events.len(),
        "truncated": jr.next_seq.is_some(),
        "next_cursor": jr.next_seq.map(|s| Cursor::Seq { epoch: Some(epoch), seq: s }.to_string()),
        "range": first.zip(last).map(|(a, b)| json!({"from": U64(a), "to": U64(b)})),
        "stream_seq_hw": hw.map(U64),
        "stream_epoch": U64(epoch),
        "source": "journal",
    })
}

fn core_help() -> Value {
    let rows: BTreeMap<&str, &str> = BTreeMap::from([
        ("loom status", "daemon, workers, latch, recording, viewers"),
        (
            "loom events [--since C] [--until C] [--domain D] [--channel X] [--kind K] [--origin O] [--max N] [--follow --timeout S]",
            "bounded read of recent events (≤ 1000 rows / 64 KiB)",
        ),
        (
            "loom mark <label> [--note T]",
            "put a marker on the shared timeline",
        ),
        (
            "loom stop-all [--reason T]",
            "STOP: latch every write (only a user can release)",
        ),
        (
            "loom ui [open <panel>]",
            "ask the user to open / focus the Loom UI",
        ),
        ("loom daemon start|stop|status|logs", "daemon lifecycle"),
        ("loom help --json | loom <domain> help --json", "this help"),
    ]);
    json!(
        rows.iter()
            .map(|(k, v)| json!({"cmd": k, "explain": v}))
            .collect::<Vec<_>>()
    )
}
