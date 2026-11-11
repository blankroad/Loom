//! Worker-local periodic adapter. One scheduler/budget across all logical channels.
//! Owner metadata and completion events are drained together; no stopped-job archive.

use super::*;
use crate::{
    KIND_JOB_STATE, RPC_PERIODIC_RENEW, RPC_PERIODIC_START, RPC_PERIODIC_STOP, RPC_PERIODIC_TAKEOVER,
    RPC_PERIODIC_UPDATE,
};
use loom_can_periodic::{Error as ScheduleError, Event, JobId, JobSnapshot, JobSpec, Patch, Scheduler};

const MS: u64 = 1_000_000;
const MAX_META_BYTES: usize = 128;

#[derive(Debug, Clone)]
pub(super) struct PeriodicTag {
    pub job: String,
    pub revision: u64,
}
#[derive(Debug, Clone)]
struct Meta {
    channel: String,
    frame: CanFrame,
    owner: String,
    origin: Origin,
    client_id: Option<String>,
    request_id: String,
    revision: u64,
    reason: Option<&'static str>,
}
#[derive(Debug, Default)]
pub(super) struct Periodic {
    pub scheduler: Scheduler,
    meta: BTreeMap<JobId, Meta>,
    pub failures: u64,
    inhibited: bool,
}
impl Periodic {
    pub(super) fn inhibited(&self) -> bool {
        self.inhibited
    }
    pub(super) fn owner_for(&self, channel: &str, id: CanId) -> Option<Value> {
        self.meta
            .iter()
            .find(|(_, m)| m.channel == channel && m.frame.id() == id)
            .map(|(job, m)| {
                json!({
            "job":job_text(*job),"owner":m.owner,"reason":"periodic_producer","channel":channel})
            })
    }
    pub(super) fn count_channel(&self, ch: &str) -> usize {
        self.meta.values().filter(|m| m.channel == ch).count()
    }
}
pub(super) fn is_write(method: &str) -> bool {
    matches!(
        method,
        RPC_PERIODIC_START
            | RPC_PERIODIC_UPDATE
            | RPC_PERIODIC_STOP
            | RPC_PERIODIC_RENEW
            | RPC_PERIODIC_TAKEOVER
    )
}
fn job_text(id: JobId) -> String {
    format!("p{}", id.0)
}
fn schedule_error(error: ScheduleError) -> WorkerError {
    let code = match &error {
        ScheduleError::Latched => ErrorCode::Stopped,
        ScheduleError::Conflict { .. } | ScheduleError::RevisionConflict { .. } => ErrorCode::Locked,
        ScheduleError::UnknownJob | ScheduleError::StaleDispatch => ErrorCode::NotFound,
        ScheduleError::ResourceExhausted | ScheduleError::OutputFull => ErrorCode::WorkerBusy,
        ScheduleError::LeaseCapExceeded | ScheduleError::NoLease | ScheduleError::Expired => ErrorCode::Lease,
        _ => ErrorCode::Usage,
    };
    let detail = match &error {
        ScheduleError::RevisionConflict { current } => {
            json!({"current_rev":current,"reason":"revision_conflict","code":"E_CONFLICT"})
        }
        ScheduleError::Conflict { owner } => json!({"owner":owner,"reason":"periodic_producer"}),
        _ => Value::Null,
    };
    WorkerError {
        code,
        message: error.to_string(),
        detail,
    }
}
fn periodic_origin(origin: Origin) -> Result<loom_can_periodic::Origin, WorkerError> {
    match origin {
        Origin::Agent => Ok(loom_can_periodic::Origin::Agent),
        Origin::User => Ok(loom_can_periodic::Origin::User),
        Origin::Script => Ok(loom_can_periodic::Origin::Script),
        Origin::Device => Err(err(
            ErrorCode::Denied,
            "device cannot own a periodic client job".into(),
        )),
    }
}
fn integer(value: &Value, key: &str, default: Option<u64>) -> Result<u64, WorkerError> {
    match value.get(key) {
        None | Some(Value::Null) => default.ok_or_else(|| err(ErrorCode::Usage, format!("missing {key}"))),
        Some(Value::Number(n)) => n
            .as_u64()
            .ok_or_else(|| err(ErrorCode::Usage, format!("{key} must be unsigned"))),
        Some(Value::String(s)) if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) => s
            .parse()
            .map_err(|_| err(ErrorCode::Usage, format!("{key} overflows u64"))),
        _ => Err(err(ErrorCode::Usage, format!("{key} must be unsigned"))),
    }
}
fn optional_integer(value: &Value, key: &str) -> Result<Option<u64>, WorkerError> {
    if value.get(key).is_none_or(Value::is_null) {
        Ok(None)
    } else {
        integer(value, key, None).map(Some)
    }
}
fn millis(value: u64) -> Result<u64, WorkerError> {
    value
        .checked_mul(MS)
        .ok_or_else(|| err(ErrorCode::Usage, "milliseconds overflow nanoseconds".into()))
}
fn hex(value: &Value, key: &str) -> Result<Vec<u8>, WorkerError> {
    let text = value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| err(ErrorCode::Usage, format!("{key} must be hex")))?;
    if text.len() > 128 || !text.len().is_multiple_of(2) || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(err(
            ErrorCode::Usage,
            format!("{key}: at most64 bytes of contiguous hex required"),
        ));
    }
    (0..text.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&text[i..i + 2], 16).map_err(|_| err(ErrorCode::Usage, "invalid hex".into()))
        })
        .collect()
}
fn metadata_text(value: &str) -> Result<(), WorkerError> {
    if value.is_empty() || value.len() > MAX_META_BYTES {
        Err(err(
            ErrorCode::Usage,
            "owner/client/request id must be1..128 bytes".into(),
        ))
    } else {
        Ok(())
    }
}
fn resolve_start_frame(op: &Value) -> Result<CanFrame, WorkerError> {
    if op["frame"]["bytes"]["data"]
        .as_str()
        .is_some_and(|s| s.len() > 128)
    {
        return Err(err(ErrorCode::Usage, "frame payload exceeds64 bytes".into()));
    }
    match op.get("frame") {
        Some(Value::String(s)) if s.len() <= 160 => {
            loom_can_frame::parse_frame(s, false).map_err(|e| err(ErrorCode::Usage, e.to_string()))
        }
        Some(Value::Object(_)) => serde_json::from_value::<FramePayload>(op["frame"].clone())
            .map_err(|e| err(ErrorCode::Usage, e.to_string()))?
            .to_frame()
            .map_err(|e| err(ErrorCode::Usage, e.to_string())),
        _ => Err(err(
            ErrorCode::Usage,
            "frame must be FramePayload (or CAN text)".into(),
        )),
    }
}

impl Core {
    pub(super) fn periodic_write(&mut self, w: &Write) -> WriteOutcome {
        // Drain lifecycle facts and synchronize the scheduler with the real hub clock first.
        self.pump();
        let result = self.periodic_operation(w);
        match result {
            Ok(detail) => WriteOutcome {
                state: WriteState::Accepted,
                detail,
                error: None,
            },
            Err(error) => WriteOutcome {
                state: WriteState::Failed,
                detail: json!({"accepted":false}),
                error: Some(error),
            },
        }
    }
    fn periodic_operation(&mut self, w: &Write) -> Result<Value, WorkerError> {
        periodic_origin(w.origin)?;
        metadata_text(&w.request_id)?;
        if let Some(client) = &w.client_id {
            metadata_text(client)?;
        }
        let method = w.op["method"].as_str().unwrap_or("");
        let now = self.hub.now_ns();
        if method == RPC_PERIODIC_START {
            return self.periodic_start(w, now);
        }
        if method == RPC_PERIODIC_STOP && w.op.get("all_mine").and_then(Value::as_bool) == Some(true) {
            if w.op.get("job").is_some() {
                return Err(err(ErrorCode::Usage, "give job or all_mine, not both".into()));
            }
            let jobs: Vec<_> = self
                .periodic
                .meta
                .iter()
                .filter(|(_, m)| m.client_id == w.client_id && m.origin == w.origin)
                .map(|(id, _)| *id)
                .collect();
            for id in &jobs {
                self.periodic.scheduler.stop(*id).map_err(schedule_error)?;
            }
            self.periodic_events();
            return Ok(json!({"stopped":jobs.into_iter().map(job_text).collect::<Vec<_>>()}));
        }
        let id = self.periodic_id(&w.op)?;
        if method != RPC_PERIODIC_TAKEOVER {
            self.periodic_authorize(id, w)?;
        }
        match method {
            RPC_PERIODIC_UPDATE => {
                let expected = integer(&w.op, "expected_rev", None)?;
                let patch = &w.op["patch"];
                let base = self
                    .periodic
                    .meta
                    .get(&id)
                    .ok_or_else(|| err(ErrorCode::NotFound, "no job".into()))?
                    .frame;
                let (change, bytes) = if patch.get("data").is_some() {
                    if patch.get("mask").is_some() || patch.get("bytes").is_some() {
                        return Err(err(ErrorCode::Usage, "give data or mask/bytes".into()));
                    }
                    let bytes = hex(patch, "data")?;
                    (Patch::replace(&bytes).map_err(schedule_error)?, bytes)
                } else {
                    let mask = hex(patch, "mask")?;
                    let data = hex(patch, "bytes")?;
                    if mask.len() > base.len() || mask.len() != data.len() {
                        return Err(err(ErrorCode::Usage, "mask exceeds current payload".into()));
                    }
                    let mut bytes = base.data().to_vec();
                    for (i, (&mask, &value)) in mask.iter().zip(&data).enumerate() {
                        bytes[i] = (bytes[i] & !mask) | (value & mask);
                    }
                    (Patch::masked(0, &mask, &data).map_err(schedule_error)?, bytes)
                };
                let desired = if let loom_can_frame::FrameKind::Fd { brs, esi } = base.kind() {
                    CanFrame::fd(base.id(), &bytes, brs, esi)
                } else {
                    CanFrame::classic(base.id(), &bytes)
                }
                .map_err(|e| err(ErrorCode::Usage, e.to_string()))?;
                let revision = self
                    .periodic
                    .scheduler
                    .patch(id, expected, change)
                    .map_err(schedule_error)?;
                if let Some(meta) = self.periodic.meta.get_mut(&id) {
                    meta.revision = revision;
                    meta.frame = desired;
                }
                self.publish_periodic(id, "patched", None, Some(w))?;
                Ok(json!({"job":job_text(id),"revision":revision}))
            }
            RPC_PERIODIC_STOP => {
                self.periodic.scheduler.stop(id).map_err(schedule_error)?;
                self.periodic_events();
                Ok(json!({"job":job_text(id),"state":"stopped"}))
            }
            RPC_PERIODIC_RENEW => {
                let end = self.periodic.scheduler.renew(id, now).map_err(schedule_error)?;
                self.publish_periodic(id, "renewed", None, Some(w))?;
                Ok(json!({"job":job_text(id),"lease_left_ms":end.saturating_sub(now)/MS}))
            }
            RPC_PERIODIC_TAKEOVER => {
                if self.latched() {
                    return Err(err(ErrorCode::Stopped, "periodic TX is latched".into()));
                }
                if self.periodic.inhibited {
                    return Err(err(
                        ErrorCode::RecordingFailed,
                        "periodic publication fault; restart worker before new jobs".into(),
                    ));
                }
                let owner = w
                    .client_id
                    .clone()
                    .unwrap_or_else(|| format!("{:?}", w.origin).to_lowercase());
                let mut old = self
                    .periodic
                    .meta
                    .get(&id)
                    .cloned()
                    .ok_or_else(|| err(ErrorCode::NotFound, "job not found".into()))?;
                let new = self
                    .periodic
                    .scheduler
                    .takeover(id, owner.clone(), periodic_origin(w.origin)?)
                    .map_err(schedule_error)?;
                old.frame = self
                    .periodic
                    .scheduler
                    .job(new)
                    .map_err(schedule_error)?
                    .spec
                    .frame;
                self.periodic_events();
                self.periodic.meta.insert(
                    new,
                    Meta {
                        owner,
                        origin: w.origin,
                        client_id: w.client_id.clone(),
                        request_id: w.request_id.clone(),
                        revision: 0,
                        reason: None,
                        ..old
                    },
                );
                self.publish_periodic(new, "started", Some("takeover"), Some(w))?;
                Ok(json!({"job":job_text(new),"previous_job":job_text(id),"revision":0}))
            }
            _ => Err(err(ErrorCode::UnknownMethod, "unknown periodic write".into())),
        }
    }
    fn periodic_start(&mut self, w: &Write, now: u64) -> Result<Value, WorkerError> {
        if self.latched() {
            return Err(err(
                ErrorCode::Stopped,
                "STOP latched: all periodic starts refused".into(),
            ));
        }
        if self.periodic.inhibited {
            return Err(err(
                ErrorCode::RecordingFailed,
                "periodic publication fault; restart worker before new jobs".into(),
            ));
        }
        let channel = required(&w.op, "channel")?.to_owned();
        if !self.channels.contains_key(&channel) {
            return Err(err(ErrorCode::NotOpen, format!("channel {channel} is not open")));
        }
        let frame = resolve_start_frame(&w.op)?;
        if frame.is_remote() {
            return Err(err(
                ErrorCode::Usage,
                "periodic remote frames are unsupported".into(),
            ));
        }
        if let Some(owner) = self.reserved(&channel, frame.id()) {
            return Err(WorkerError {
                code: ErrorCode::Denied,
                message: "ISO-TP reserves tx_id".into(),
                detail: owner,
            });
        }
        if let Some(b) = self
            .bursts
            .iter()
            .find(|b| b.channel == channel && b.frame.id() == frame.id())
        {
            return Err(WorkerError {
                code: ErrorCode::Locked,
                message: "burst producer owns id".into(),
                detail: json!({"owner":b.req.client_id,"reason":"burst_producer"}),
            });
        }
        let period = integer(&w.op, "period_ms", None)?;
        if !(1..=60_000).contains(&period) {
            return Err(err(ErrorCode::Usage, "period_ms must be1..60000".into()));
        }
        let owner = match w.op.get("owner") {
            None | Some(Value::Null) => w
                .client_id
                .clone()
                .unwrap_or_else(|| format!("{:?}", w.origin).to_lowercase()),
            Some(Value::String(owner)) => owner.clone(),
            _ => return Err(err(ErrorCode::Usage, "owner must be text".into())),
        };
        metadata_text(&owner)?;
        let mut spec = JobSpec::new(
            &channel,
            frame,
            millis(period)?,
            periodic_origin(w.origin)?,
            &owner,
        );
        spec.offset_ns = millis(integer(&w.op, "offset_ms", Some(0))?)?;
        spec.count = optional_integer(&w.op, "count")?;
        spec.lease_ns = optional_integer(&w.op, "lease_ms")?.map(millis).transpose()?;
        spec.max_duration_ns = millis(integer(&w.op, "max_duration_ms", Some(3_600_000))?)?;
        let id = self
            .periodic
            .scheduler
            .start_at(spec, now)
            .map_err(schedule_error)?;
        self.periodic.meta.insert(
            id,
            Meta {
                channel,
                frame,
                owner,
                origin: w.origin,
                client_id: w.client_id.clone(),
                request_id: w.request_id.clone(),
                revision: 0,
                reason: None,
            },
        );
        self.publish_periodic(id, "started", None, Some(w))?;
        Ok(json!({"job":job_text(id),"revision":0}))
    }
    fn periodic_id(&self, args: &Value) -> Result<JobId, WorkerError> {
        let text = required(args, "job")?;
        let id = text
            .strip_prefix('p')
            .and_then(|n| n.parse::<u64>().ok())
            .map(JobId)
            .filter(|id| job_text(*id) == text && self.periodic.meta.contains_key(id));
        id.ok_or_else(|| err(ErrorCode::NotFound, format!("no periodic job {text}")))
    }
    fn periodic_authorize(&self, id: JobId, w: &Write) -> Result<(), WorkerError> {
        let meta = self
            .periodic
            .meta
            .get(&id)
            .ok_or_else(|| err(ErrorCode::NotFound, "no job".into()))?;
        if w.origin == Origin::User || (w.origin == meta.origin && w.client_id == meta.client_id) {
            Ok(())
        } else {
            Err(WorkerError {
                code: ErrorCode::Locked,
                message: "job belongs to another client; use explicit takeover".into(),
                detail: json!({"job":job_text(id),"owner":meta.owner}),
            })
        }
    }
    fn job_state(
        &mut self,
        id: JobId,
        meta: &Meta,
        state: &str,
        reason: Option<&str>,
        w: Option<&Write>,
        extra: Value,
    ) -> bool {
        let Some(ch) = self.channels.get(&meta.channel) else {
            return false;
        };
        let mut payload = json!({"job":job_text(id),"channel":meta.channel,"id":meta.frame.id().raw(),"ext":meta.frame.id().is_extended(),
            "state":state,"revision":meta.revision,"owner":meta.owner,"origin":meta.origin});
        if let Some(p) = payload.as_object_mut() {
            if let Some(reason) = reason {
                p.insert("reason".into(), json!(reason));
            }
            if let Value::Object(extra) = extra {
                p.extend(extra);
            }
        }
        let mut d = draft(
            KIND_JOB_STATE,
            ch.emitter.clock().ts(TsSource::Synthetic),
            None,
            w.map_or(meta.origin, |w| w.origin),
            Some(ch.generation),
            payload,
        );
        d.job_id = Some(job_text(id));
        d.purpose = Some("periodic".into());
        d.client_id = w.map_or_else(|| meta.client_id.clone(), |w| w.client_id.clone());
        d.request_id = Some(w.map_or_else(|| meta.request_id.clone(), |w| w.request_id.clone()));
        if ch.emitter.priority(d).is_ok() {
            true
        } else {
            self.emit_failed = self.emit_failed.saturating_add(1);
            false
        }
    }
    fn publish_periodic(
        &mut self,
        id: JobId,
        state: &str,
        reason: Option<&str>,
        w: Option<&Write>,
    ) -> Result<(), WorkerError> {
        let meta = self
            .periodic
            .meta
            .get(&id)
            .cloned()
            .ok_or_else(|| err(ErrorCode::Internal, "missing periodic metadata".into()))?;
        if self.job_state(id, &meta, state, reason, w, Value::Null) {
            return Ok(());
        }
        // A job must not continue invisibly after its lifecycle publication failed.
        self.periodic.inhibited = true;
        self.periodic.failures = self.periodic.failures.saturating_add(1);
        self.periodic_latch(true);
        Err(err(
            ErrorCode::RecordingFailed,
            "periodic lifecycle publication failed; periodic TX inhibited".into(),
        ))
    }
    pub(super) fn periodic_events(&mut self) {
        while let Some(event) = self.periodic.scheduler.pop_event() {
            match event {
                Event::Stopped {
                    job,
                    reason,
                    resumable,
                    stats,
                    ..
                } => {
                    let meta = if resumable {
                        self.periodic.meta.get(&job).cloned()
                    } else {
                        self.periodic.meta.remove(&job)
                    };
                    if let Some(meta) = meta {
                        let why = meta.reason.unwrap_or(match reason {
                            loom_can_periodic::StopReason::LeaseExpired => "lease_expired",
                            loom_can_periodic::StopReason::Completed => "completed",
                            loom_can_periodic::StopReason::MaxDuration => "max_duration",
                            loom_can_periodic::StopReason::Stopped => "stopped",
                            loom_can_periodic::StopReason::Latched => "latched",
                            loom_can_periodic::StopReason::Takeover => "takeover",
                            loom_can_periodic::StopReason::BusOff => "bus_off",
                            loom_can_periodic::StopReason::ErrorPassive => "error_passive",
                        });
                        if !self.job_state(job,&meta,if resumable {"paused"}else{"stopped"},Some(why),None,
                            json!({"sent":stats.sent,"missed":stats.missed,"failed":stats.failed,"lateness_p99_us":stats.p99_lateness_ns.map(|v|v.div_ceil(1000))})) {
                            self.periodic.inhibited=true;self.periodic.failures=self.periodic.failures.saturating_add(1);
                        }
                    }
                }
                Event::Resumed { job, .. } => {
                    if let Some(meta) = self.periodic.meta.get(&job).cloned() {
                        self.job_state(job, &meta, "started", Some("resumed"), None, Value::Null);
                    }
                }
                Event::SendFailed { .. } => {
                    self.periodic.failures = self.periodic.failures.saturating_add(1);
                }
            }
        }
    }
    pub(super) fn periodic_latch(&mut self, on: bool) {
        self.periodic_events();
        if self.periodic.scheduler.latch_at(on, self.hub.now_ns()).is_err() {
            self.periodic.failures = self.periodic.failures.saturating_add(1);
        }
        self.periodic_events();
    }
    pub(super) fn stop_periodic_channel(&mut self, channel: &str) {
        let ids: Vec<_> = self
            .periodic
            .meta
            .iter()
            .filter(|(_, m)| m.channel == channel)
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            if let Some(meta) = self.periodic.meta.get_mut(&id) {
                meta.reason = Some("channel_closed");
            }
            if self.periodic.scheduler.stop(id).is_err() {
                self.periodic.failures = self.periodic.failures.saturating_add(1);
            }
        }
        self.periodic_events();
    }
    pub(super) fn periodic_list(&self, args: &Value) -> Result<Value, WorkerError> {
        let channel = name_arg(args, "channel")?;
        let now = self.hub.now_ns();
        if channel.is_some_and(|c| !self.channels.contains_key(c)) {
            return Err(err(ErrorCode::NotOpen, "channel not open".into()));
        }
        let jobs: Vec<_> = self
            .periodic
            .meta
            .iter()
            .filter(|(_, m)| channel.is_none_or(|c| c == m.channel))
            .filter_map(|(id, meta)| {
                self.periodic
                    .scheduler
                    .job(*id)
                    .ok()
                    .map(|job| job_json(job, meta.frame, now))
            })
            .collect();
        Ok(json!({"jobs":jobs}))
    }
    pub(super) fn run_periodic(&mut self) -> usize {
        if self.latched() || self.periodic.inhibited {
            if !self.periodic.scheduler.is_latched() {
                self.periodic_latch(true);
            }
            return 0;
        }
        if self.periodic.scheduler.is_empty() {
            return 0;
        }
        if self.periodic.scheduler.on_timer(self.hub.now_ns()).is_err() {
            self.periodic_events();
            if self.periodic.scheduler.on_timer(self.hub.now_ns()).is_err() {
                self.periodic.inhibited = true;
                self.periodic.failures = self.periodic.failures.saturating_add(1);
                self.periodic_latch(true);
                return 0;
            }
        }
        let mut sent = 0;
        while let Some(due) = self.periodic.scheduler.pop_due() {
            if self.latched() || self.periodic.inhibited {
                self.periodic_latch(true);
                break;
            }
            let now = self.hub.now_ns();
            if !self.periodic.scheduler.is_due_current(&due, now) {
                continue;
            }
            if self.reserved(&due.channel, due.frame.id()).is_some() {
                if let Some(meta) = self.periodic.meta.get_mut(&due.job) {
                    meta.reason = Some("tx_reserved");
                }
                let _ = self.periodic.scheduler.stop(due.job);
                continue;
            }
            let Some(meta) = self.periodic.meta.get(&due.job).cloned() else {
                continue;
            };
            let Some(ch) = self.channels.get(&due.channel) else {
                let _ = self.periodic.scheduler.stop(due.job);
                continue;
            };
            let Some(slot) = sender_slot(meta.origin) else {
                let _ = self.periodic.scheduler.stop(due.job);
                continue;
            };
            let sender = ch.senders[slot];
            let bus = ch.bus.clone();
            // Final per-unit gate (including User periodic jobs), immediately before enqueue.
            if self.latched() || !self.periodic.scheduler.is_due_current(&due, self.hub.now_ns()) {
                self.periodic_latch(true);
                break;
            }
            match self.hub.send(sender, due.frame) {
                Ok(seq) => {
                    sent += 1;
                    for ch in self.channels.values_mut().filter(|ch| ch.bus == bus) {
                        ch.push_tag(
                            Tag {
                                seq,
                                request_id: Some(meta.request_id.clone()),
                                client_id: meta.client_id.clone(),
                                frame_index: None,
                                isotp: None,
                                periodic: Some(PeriodicTag {
                                    job: job_text(due.job),
                                    revision: due.revision,
                                }),
                                override_of: None,
                            },
                            self.limits.max_tags_per_channel,
                        );
                    }
                    // Retry only the callback on output pressure, never the physical send.
                    if self
                        .periodic
                        .scheduler
                        .on_dispatch_submitted(due.dispatch, self.hub.now_ns())
                        .is_err()
                    {
                        self.periodic_events();
                        if self
                            .periodic
                            .scheduler
                            .on_dispatch_submitted(due.dispatch, self.hub.now_ns())
                            .is_err()
                        {
                            self.periodic.inhibited = true;
                            self.periodic.failures = self.periodic.failures.saturating_add(1);
                        }
                    }
                }
                Err(_) => {
                    let _ = self
                        .periodic
                        .scheduler
                        .on_dispatch_failed(due.dispatch, self.hub.now_ns());
                    if let Some(meta) = self.periodic.meta.get_mut(&due.job) {
                        meta.reason = Some("send_failed");
                    }
                    let _ = self.periodic.scheduler.stop(due.job);
                }
            }
        }
        sent
    }
}
fn job_json(job: JobSnapshot, desired: CanFrame, now: u64) -> Value {
    json!({"job":job_text(job.id),"channel":job.spec.channel,"id":job.spec.frame.id().raw(),"ext":job.spec.frame.id().is_extended(),
        "fd":job.spec.frame.is_fd(),"frame":FramePayload::from_frame(&desired,None),"data":desired.data_hex().to_lowercase(),
        "effective_frame":FramePayload::from_frame(&job.spec.frame,None),"period_ms":job.spec.period_ns/MS,
        "owner":job.spec.owner,"origin":match job.spec.origin {loom_can_periodic::Origin::Agent=>"agent",loom_can_periodic::Origin::User=>"user",loom_can_periodic::Origin::Script=>"script"},
        "revision":job.revision,"applied_revision":job.effective_revision,"count_left":job.remaining,"lease_left_ms":job.lease_end_ns.map(|v|v.saturating_sub(now)/MS),
        "sent":job.stats.sent,"missed":job.stats.missed,"lateness_p99_us":job.stats.p99_lateness_ns.map(|v|v.div_ceil(1000)),"state":if job.paused.is_some(){"paused"}else{"running"}})
}
