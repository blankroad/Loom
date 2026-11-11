//! `arbiter` — InputArbiter: input mediation for one channel (UART plan §3.7 1–7,
//! §5.3, §5.8, §3.13 TX row).
//!
//! A pure state machine: no I/O, threads or clock. Time is injected as `now_ms`.
//! The worker feeds it user key chunks (from viewer connections), agent send
//! requests, RX hints (prompt seen, alternate screen) and periodic [`InputArbiter::tick`]
//! calls; it answers what to transmit now, what to hold, and which deferred agent
//! sends may go or must fail. State changes are reported as [`ArbiterEvent`]s with
//! the `uart.*` payloads the UI panel reads (`ui/src/panels/uart/uartState.ts`).
//!
//! Rules (plan §3.7):
//! - The user always wins: a user key takes the input lease at once; an agent send
//!   in flight is preempted ([`InputDecision::preempt_agent`]).
//! - Nothing is mixed into an unfinished user line: agent sends wait (deferred) while
//!   the user owns the lease or the line is dirty, then fail with `E_USER_ACTIVE`.
//! - The lease returns automatically only when the last user byte ended the line
//!   (CR/LF/^C, or ^U on a `line_kill` profile) **and** the prompt came back (or, with
//!   no profile prompt, 1.5 s passed) **and** the device is not in the alternate screen.
//! - `input_dirty` is channel state: it survives the viewer leaving.
//! - One writer viewer at a time; another viewer's keys are held until it takes over
//!   (within 5 s) or they are discarded with a reason. Ctrl-C always passes.
//! - Every queue is bounded; overflow is reported, never silent (C2).

use std::collections::VecDeque;

use loom_proto::error::ErrorCode;
use serde::Serialize;
use serde_json::{Value, json};

/// Held user bytes across all viewers (plan §3.13 TX row "user 4 KiB").
pub const USER_HELD_MAX_BYTES: usize = 4096;
/// Held user input chunks across all viewers (plan §3.13 TX row "64항목").
pub const USER_HELD_MAX_ITEMS: usize = 64;
/// Ctrl-C reservation: interrupts handed to the TX actor and not yet written
/// (plan §3.13 TX row "Ctrl-C 별도 64 B·64항목").
pub const INTERRUPT_MAX_IN_FLIGHT: usize = 64;
/// Deferred agent sends per channel (plan §3.13 TX row "agent 64요청").
pub const AGENT_PENDING_MAX: usize = 64;
/// Agent lock TTL ceiling (plan §3.7-6 "상한 10분").
pub const LOCK_TTL_MAX_MS: u64 = 600_000;
/// Default agent lock TTL when 0 is passed (plan §4.7 `lock --ttl 120s`).
pub const LOCK_TTL_DEFAULT_MS: u64 = 120_000;
/// Window to take over held keys before they are discarded (plan §3.7-6/7).
pub const TAKEOVER_WINDOW_MS: u64 = 5_000;
/// Quiet time that returns the lease when the profile has no prompt (plan §3.7-3).
pub const QUIET_RETURN_MS: u64 = 1_500;
/// Longest viewer / client / request id or label kept (longer ones are cut).
pub const MAX_ID_BYTES: usize = 128;
/// Longest `uart.agent_activity.text` preview in characters (UI contract, ≤ 80).
pub const ACTIVITY_TEXT_MAX_CHARS: usize = 80;
/// Undrained event backlog; call [`InputArbiter::take_events`] after every call.
/// On overflow the oldest events are dropped and counted in `events_dropped`.
pub const EVENT_BACKLOG_MAX: usize = 512;

/// Ctrl-C.
const ETX: u8 = 0x03;
/// Ctrl-U (line kill on verified profiles).
const NAK: u8 = 0x15;

/// Hold reasons / ack reasons (stable wire strings).
pub mod reason {
    /// Another viewer owns the input lease (writer).
    pub const OTHER_WRITER: &str = "other_writer";
    /// An agent lock is active.
    pub const AGENT_LOCK: &str = "agent_lock";
    /// A bounded input queue is full: held keys (4 KiB · 64) or the Ctrl-C lane (64).
    pub const QUEUE_FULL: &str = "queue_full";
    /// Held keys were not taken over within the window.
    pub const TAKEOVER_TIMEOUT: &str = "takeover_timeout";
    /// The viewer that typed the held keys disconnected.
    pub const VIEWER_LEFT: &str = "viewer_left";
    /// Agent send failed because the user left a half-typed line.
    pub const DIRTY_LINE: &str = "dirty_line";
    /// Agent send failed because the user kept the lease (typing).
    pub const USER_TYPING: &str = "user_typing";
    /// Agent send failed because another agent send was in flight.
    pub const AGENT_BUSY: &str = "agent_busy";
}

/// Per-channel profile facts the arbiter needs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct ArbiterConfig {
    /// The channel profile has a prompt regex (return on prompt, not on quiet time).
    pub has_prompt: bool,
    /// Verified `line_kill = "^U"` profile (linux-shell, uboot): ^U clears a line.
    pub line_kill_ctrl_u: bool,
}

/// Who holds the input lease.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Owner {
    /// Nobody.
    None,
    /// A user viewer (connection id).
    User(String),
    /// An agent send in flight.
    Agent {
        /// Client name of the agent.
        client: String,
        /// Request being transmitted.
        request_id: String,
    },
}

/// `uart.input_ack` state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AckState {
    /// Handed to the TX actor now.
    Sent,
    /// Held (another writer / agent lock); may still be sent by a takeover.
    Held,
    /// Not sent and never will be.
    Discarded,
}

impl AckState {
    fn as_str(self) -> &'static str {
        match self {
            AckState::Sent => "sent",
            AckState::Held => "held",
            AckState::Discarded => "discarded",
        }
    }
}

/// `uart.agent_activity` kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityKind {
    /// An agent send waits for the user.
    DeferredSend,
    /// An agent expect is waiting (emitted by the matcher side, not here).
    Expect,
    /// Nothing pending.
    Idle,
}

impl ActivityKind {
    fn as_str(self) -> &'static str {
        match self {
            ActivityKind::DeferredSend => "deferred_send",
            ActivityKind::Expect => "expect",
            ActivityKind::Idle => "idle",
        }
    }
}

/// State change for the event stream (`kind()` + `payload()`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArbiterEvent {
    /// `uart.input_owner {owner, viewer?, client?, dirty, hold}`. The gateway adds
    /// `viewer_label` (it knows the connection names).
    InputOwner {
        /// `"none"`, `"user"` or `"agent"`.
        owner: &'static str,
        /// Viewer connection id (`conn_id`) when the owner is a user.
        viewer: Option<String>,
        /// Agent client id when the owner is an agent.
        client: Option<String>,
        /// `input_dirty`.
        dirty: bool,
        /// Channel hold.
        hold: bool,
    },
    /// `uart.hold {on}`.
    Hold {
        /// Hold state.
        on: bool,
    },
    /// `uart.lock {on, reason?, ttl_ms?, actor?}`.
    Lock {
        /// Lock state.
        on: bool,
        /// Agent-supplied reason.
        reason: Option<String>,
        /// Remaining TTL at event time (ms); re-emitted when a call extends it.
        ttl_ms: Option<u64>,
        /// Agent client holding the lock.
        actor: Option<String>,
    },
    /// `uart.agent_activity {kind, text?, elapsed_ms?, timeout_ms?, request_id?, client?}`.
    AgentActivity {
        /// Activity kind.
        kind: ActivityKind,
        /// Preview of the waiting send (escaped text ≤ [`ACTIVITY_TEXT_MAX_CHARS`]
        /// chars, never user input).
        text: Option<String>,
        /// Time waited so far.
        elapsed_ms: Option<u64>,
        /// Defer timeout.
        timeout_ms: Option<u64>,
        /// Waiting request.
        request_id: Option<String>,
        /// Agent client of the waiting request.
        client: Option<String>,
    },
    /// `uart.input_ack {input_seq, state, reason?}`.
    InputAck {
        /// Caller-supplied sequence number of the user input (JSON: u64 decimal string, contract 02 §2.2).
        input_seq: u64,
        /// Outcome.
        state: AckState,
        /// Why held / discarded.
        reason: Option<&'static str>,
    },
}

impl ArbiterEvent {
    /// Event kind (`uart.input_owner`, …).
    pub fn kind(&self) -> &'static str {
        match self {
            ArbiterEvent::InputOwner { .. } => "uart.input_owner",
            ArbiterEvent::Hold { .. } => "uart.hold",
            ArbiterEvent::Lock { .. } => "uart.lock",
            ArbiterEvent::AgentActivity { .. } => "uart.agent_activity",
            ArbiterEvent::InputAck { .. } => "uart.input_ack",
        }
    }

    /// JSON payload (snake_case fields; optional fields omitted when absent).
    pub fn payload(&self) -> Value {
        let mut v = match self {
            ArbiterEvent::InputOwner {
                owner,
                viewer,
                client,
                dirty,
                hold,
            } => json!({
                "owner": owner, "viewer": viewer, "client": client,
                "dirty": dirty, "hold": hold,
            }),
            ArbiterEvent::Hold { on } => json!({ "on": on }),
            ArbiterEvent::Lock {
                on,
                reason,
                ttl_ms,
                actor,
            } => json!({"on": on, "reason": reason, "ttl_ms": ttl_ms, "actor": actor}),
            ArbiterEvent::AgentActivity {
                kind,
                text,
                elapsed_ms,
                timeout_ms,
                request_id,
                client,
            } => json!({
                "kind": kind.as_str(), "text": text,
                "elapsed_ms": elapsed_ms, "timeout_ms": timeout_ms,
                "request_id": request_id, "client": client,
            }),
            ArbiterEvent::InputAck {
                input_seq,
                state,
                reason,
            } => json!({"input_seq": input_seq.to_string(), "state": state.as_str(), "reason": reason}),
        };
        if let Value::Object(map) = &mut v {
            map.retain(|_, x| !x.is_null());
        }
        v
    }
}

/// Answer to one user input chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputDecision {
    /// Bytes to transmit now, in order (may include previously held keys).
    pub send: Vec<u8>,
    /// `send` contains Ctrl-C: use the TX priority lane and report
    /// [`InputArbiter::interrupt_written`] when done.
    pub interrupt: bool,
    /// Ack for this input.
    pub ack: AckState,
    /// Why held / discarded.
    pub reason: Option<&'static str>,
    /// Agent request whose remaining chunks the TX actor must cancel at the next
    /// chunk boundary (user wins; already-written bytes stay `outcome_unknown`).
    pub preempt_agent: Option<String>,
}

impl InputDecision {
    fn new(ack: AckState, reason: Option<&'static str>) -> Self {
        Self {
            send: Vec::new(),
            interrupt: false,
            ack,
            reason,
            preempt_agent: None,
        }
    }
}

/// Admission of an agent send.
#[derive(Debug, Clone, PartialEq)]
pub enum AgentAdmit {
    /// Transmit now; the agent owns the lease until [`InputArbiter::agent_done`].
    Now,
    /// Wait; a later [`TickAction`] says go or fail. Fails at `until_ms` at the latest.
    Deferred {
        /// Defer deadline.
        until_ms: u64,
    },
    /// Refused now.
    Reject {
        /// Error code (`E_HELD_BY_USER`, `E_LOCKED`, `E_LOCK_BROKEN`, `E_RATE_LIMIT`).
        code: ErrorCode,
        /// `error.detail` (never contains user input).
        detail: Value,
    },
}

/// Outcome of a deferred agent send, produced by [`InputArbiter::tick`] and the
/// user-command methods.
#[derive(Debug, Clone, PartialEq)]
pub enum TickAction {
    /// The deferred send may go now; the agent owns the lease.
    AgentReady {
        /// Request id.
        request_id: String,
        /// Time it waited (`deferred_ms` in the send result).
        deferred_ms: u64,
    },
    /// The deferred send failed.
    AgentRejected {
        /// Request id.
        request_id: String,
        /// `E_USER_ACTIVE`, `E_HELD_BY_USER` or `E_LOCKED`.
        code: ErrorCode,
        /// `error.detail`, e.g. `{"reason":"dirty_line","user_line_len":11}`.
        detail: Value,
    },
}

/// Result of `Ctrl-] y` (give the lease back).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum YieldResult {
    /// Lease returned (line was clean).
    Released,
    /// Line is half typed: press again to clear it (sends ^U on verified profiles).
    ConfirmClear {
        /// Bytes in the unfinished line.
        line_len: u32,
    },
    /// Second press on a verified profile: transmit `send` (^U); line cleared, lease returned.
    Cleared {
        /// Bytes to transmit (`[0x15]`).
        send: Vec<u8>,
    },
    /// Second press on an unverified profile: the user must clear the line by hand.
    ManualClearNeeded {
        /// Bytes in the unfinished line.
        line_len: u32,
    },
}

#[derive(Debug, Clone)]
struct Held {
    viewer: String,
    input_seq: u64,
    bytes: Vec<u8>,
    at_ms: u64,
}

#[derive(Debug, Clone)]
struct Lock {
    client: String,
    reason: String,
    ttl_ms: u64,
    expires_ms: u64,
}

#[derive(Debug, Clone)]
struct Pending {
    client: String,
    request_id: String,
    label: String,
    since_ms: u64,
    deadline_ms: u64,
    timeout_ms: u64,
}

/// Snapshot used to emit `uart.input_owner` only on change.
type OwnerSnapshot = (&'static str, Option<String>, Option<String>, bool, bool);

/// Input mediation state for one channel.
#[derive(Debug, Clone)]
pub struct InputArbiter {
    cfg: ArbiterConfig,
    owner: Owner,
    dirty: bool,
    line_len: u32,
    last_ends_line: bool,
    last_user_ms: u64,
    prompt_after_line: bool,
    alt_screen: bool,
    hold: bool,
    lock: Option<Lock>,
    lock_broken: Option<String>,
    held: VecDeque<Held>,
    held_bytes: usize,
    interrupts_in_flight: usize,
    pending: VecDeque<Pending>,
    yield_armed: Option<String>,
    shown_activity: Option<String>,
    last_owner_event: Option<OwnerSnapshot>,
    events: VecDeque<ArbiterEvent>,
    events_dropped: u64,
}

/// Activity preview: at most [`ACTIVITY_TEXT_MAX_CHARS`] chars, `…` when cut.
fn preview(s: &str) -> String {
    match s.char_indices().nth(ACTIVITY_TEXT_MAX_CHARS - 1) {
        Some((i, _)) if s[i..].chars().nth(1).is_some() => {
            let mut out = s[..i].to_owned();
            out.push('…');
            out
        }
        _ => s.to_owned(),
    }
}

fn clip(s: &str) -> String {
    if s.len() <= MAX_ID_BYTES {
        return s.to_owned();
    }
    let mut end = MAX_ID_BYTES;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_owned()
}

impl InputArbiter {
    /// New arbiter: nobody owns input, line clean, no hold, no lock.
    pub fn new(cfg: ArbiterConfig) -> Self {
        Self {
            cfg,
            owner: Owner::None,
            dirty: false,
            line_len: 0,
            last_ends_line: true,
            last_user_ms: 0,
            prompt_after_line: false,
            alt_screen: false,
            hold: false,
            lock: None,
            lock_broken: None,
            held: VecDeque::with_capacity(USER_HELD_MAX_ITEMS),
            held_bytes: 0,
            interrupts_in_flight: 0,
            pending: VecDeque::with_capacity(AGENT_PENDING_MAX),
            yield_armed: None,
            shown_activity: None,
            last_owner_event: None,
            events: VecDeque::new(),
            events_dropped: 0,
        }
    }

    /// Current lease owner.
    pub fn owner(&self) -> &Owner {
        &self.owner
    }

    /// `input_dirty`: the user left an unfinished line.
    pub fn input_dirty(&self) -> bool {
        self.dirty
    }

    /// Bytes in the unfinished user line (length only, never content).
    pub fn user_line_len(&self) -> u32 {
        self.line_len
    }

    /// Channel hold.
    pub fn hold_on(&self) -> bool {
        self.hold
    }

    /// Active agent lock: `(client, reason, expires_ms)`.
    pub fn lock_info(&self) -> Option<(&str, &str, u64)> {
        self.lock
            .as_ref()
            .map(|l| (l.client.as_str(), l.reason.as_str(), l.expires_ms))
    }

    /// Deferred agent sends waiting.
    pub fn pending_agent_sends(&self) -> usize {
        self.pending.len()
    }

    /// Held user bytes (all viewers).
    pub fn held_bytes(&self) -> usize {
        self.held_bytes
    }

    /// Events dropped because [`InputArbiter::take_events`] was not called.
    pub fn events_dropped(&self) -> u64 {
        self.events_dropped
    }

    /// Drain state-change events (oldest first).
    pub fn take_events(&mut self) -> Vec<ArbiterEvent> {
        self.events.drain(..).collect()
    }

    // ---------------------------------------------------------------- user side

    /// A user viewer typed `bytes` (`uart.input`, write class ③).
    pub fn user_input(&mut self, viewer: &str, input_seq: u64, bytes: &[u8], now_ms: u64) -> InputDecision {
        let viewer = clip(viewer);
        self.housekeeping(now_ms);
        self.yield_armed = None;
        let decision = if bytes.is_empty() {
            InputDecision::new(AckState::Sent, None)
        } else if bytes.contains(&ETX) {
            self.interrupt(&viewer, bytes, now_ms)
        } else {
            self.ordinary(&viewer, input_seq, bytes, now_ms)
        };
        self.ack(input_seq, decision.ack, decision.reason);
        self.emit_owner();
        decision
    }

    /// Ctrl-C path: never held, breaks an agent lock, keeps this viewer's own held
    /// keys in front of it.
    fn interrupt(&mut self, viewer: &str, bytes: &[u8], now_ms: u64) -> InputDecision {
        if self.interrupts_in_flight >= INTERRUPT_MAX_IN_FLIGHT {
            return InputDecision::new(AckState::Discarded, Some(reason::QUEUE_FULL));
        }
        self.interrupts_in_flight += 1;
        self.break_lock();
        let mut d = InputDecision::new(AckState::Sent, None);
        d.interrupt = true;
        d.preempt_agent = self.take_lease(viewer);
        d.send = self.drain_held_of(viewer);
        d.send.extend_from_slice(bytes);
        self.absorb_user(&d.send, now_ms);
        d
    }

    fn ordinary(&mut self, viewer: &str, input_seq: u64, bytes: &[u8], now_ms: u64) -> InputDecision {
        let blocked = match &self.owner {
            Owner::User(w) if w != viewer => Some(reason::OTHER_WRITER),
            _ if self.lock.is_some() => Some(reason::AGENT_LOCK),
            _ => None,
        };
        if let Some(why) = blocked {
            if self.held.len() >= USER_HELD_MAX_ITEMS || self.held_bytes + bytes.len() > USER_HELD_MAX_BYTES {
                return InputDecision::new(AckState::Discarded, Some(reason::QUEUE_FULL));
            }
            self.held_bytes += bytes.len();
            self.held.push_back(Held {
                viewer: viewer.to_owned(),
                input_seq,
                bytes: bytes.to_vec(),
                at_ms: now_ms,
            });
            return InputDecision::new(AckState::Held, Some(why));
        }
        let mut d = InputDecision::new(AckState::Sent, None);
        d.preempt_agent = self.take_lease(viewer);
        d.send = bytes.to_vec();
        self.absorb_user(&d.send, now_ms);
        d
    }

    /// Take over (`[입력 가져오기]` / `Ctrl-] o`): this viewer becomes the writer,
    /// its held keys are sent, an agent lock is broken (`E_LOCK_BROKEN` next call).
    pub fn takeover(&mut self, viewer: &str, now_ms: u64) -> InputDecision {
        let viewer = clip(viewer);
        self.housekeeping(now_ms);
        self.yield_armed = None;
        self.break_lock();
        let mut d = InputDecision::new(AckState::Sent, None);
        d.preempt_agent = self.take_lease(&viewer);
        d.send = self.drain_held_of(&viewer);
        if !d.send.is_empty() {
            self.absorb_user(&d.send, now_ms);
        }
        self.emit_owner();
        d
    }

    /// `Ctrl-] y`: give the lease back. A half-typed line needs a second press,
    /// which sends ^U only on a verified `line_kill` profile (plan §3.7-3).
    pub fn yield_input(&mut self, viewer: &str, now_ms: u64) -> (YieldResult, Vec<TickAction>) {
        let viewer = clip(viewer);
        self.housekeeping(now_ms);
        let result = if !self.dirty {
            self.yield_armed = None;
            if self.owner == Owner::User(viewer.clone()) {
                self.owner = Owner::None;
            }
            YieldResult::Released
        } else if self.yield_armed.as_deref() == Some(viewer.as_str()) {
            self.yield_armed = None;
            if self.cfg.line_kill_ctrl_u {
                self.clear_line();
                if matches!(self.owner, Owner::User(_)) {
                    self.owner = Owner::None;
                }
                YieldResult::Cleared { send: vec![NAK] }
            } else {
                YieldResult::ManualClearNeeded {
                    line_len: self.line_len,
                }
            }
        } else {
            self.yield_armed = Some(viewer);
            YieldResult::ConfirmClear {
                line_len: self.line_len,
            }
        };
        let actions = self.admit_and_expire(now_ms);
        self.emit_owner();
        (result, actions)
    }

    /// `Ctrl-] c` / `[⌫ 줄 지우기]` (input clear): on a verified profile returns `[^U]`
    /// to transmit; on others the user asserts the line is clean. Clears
    /// `input_dirty` and returns the lease.
    pub fn clear_input(&mut self, _viewer: &str, now_ms: u64) -> (Vec<u8>, Vec<TickAction>) {
        self.housekeeping(now_ms);
        self.yield_armed = None;
        let send = if self.dirty && self.cfg.line_kill_ctrl_u {
            vec![NAK]
        } else {
            Vec::new()
        };
        self.clear_line();
        if matches!(self.owner, Owner::User(_)) {
            self.owner = Owner::None;
        }
        let actions = self.admit_and_expire(now_ms);
        self.emit_owner();
        (send, actions)
    }

    /// The viewer connection closed: its lease ends, its held keys are discarded,
    /// `input_dirty` stays (plan §3.7-3).
    pub fn viewer_left(&mut self, viewer: &str, now_ms: u64) -> Vec<TickAction> {
        let viewer = clip(viewer);
        self.housekeeping(now_ms);
        self.discard_held(|h| h.viewer == viewer, reason::VIEWER_LEFT);
        if self.owner == Owner::User(viewer.clone()) {
            self.owner = Owner::None;
        }
        if self.yield_armed.as_deref() == Some(viewer.as_str()) {
            self.yield_armed = None;
        }
        let actions = self.admit_and_expire(now_ms);
        self.emit_owner();
        actions
    }

    /// Channel hold on/off (`Ctrl-] h`, viewer only). Turning it on rejects every
    /// deferred agent send with `E_HELD_BY_USER` at once.
    pub fn set_hold(&mut self, on: bool, now_ms: u64) -> Vec<TickAction> {
        self.housekeeping(now_ms);
        let mut actions = Vec::new();
        if self.hold != on {
            self.hold = on;
            self.push_event(ArbiterEvent::Hold { on });
        }
        if on {
            while let Some(p) = self.pending.pop_front() {
                actions.push(TickAction::AgentRejected {
                    request_id: p.request_id,
                    code: ErrorCode::HeldByUser,
                    detail: json!({}),
                });
            }
        }
        actions.extend(self.admit_and_expire(now_ms));
        self.emit_owner();
        actions
    }

    /// The TX actor wrote `n` interrupts handed out with `interrupt = true`.
    pub fn interrupt_written(&mut self, n: usize) {
        self.interrupts_in_flight = self.interrupts_in_flight.saturating_sub(n);
    }

    // ---------------------------------------------------------------- RX hints

    /// The profile prompt reappeared at the end of RX (caller runs the matcher).
    pub fn prompt_seen(&mut self, now_ms: u64) -> Vec<TickAction> {
        if self.last_ends_line {
            self.prompt_after_line = true;
        }
        self.tick(now_ms)
    }

    /// Alternate screen state from the RX side (sanitizer / VT tracker).
    pub fn set_alt_screen(&mut self, on: bool) {
        self.alt_screen = on;
    }

    // ---------------------------------------------------------------- agent side

    /// An agent asks to transmit request `request_id` (`label` = escaped summary
    /// for the panel, never user input). `defer_timeout_ms` = `--defer-timeout`.
    pub fn agent_send(
        &mut self,
        client: &str,
        request_id: &str,
        label: &str,
        defer_timeout_ms: u64,
        now_ms: u64,
    ) -> AgentAdmit {
        let client = clip(client);
        self.housekeeping(now_ms);
        let request_id = clip(request_id);
        let label = preview(label);
        let admit = self.agent_send_inner(client, request_id, label, defer_timeout_ms, now_ms);
        self.emit_owner();
        admit
    }

    fn agent_send_inner(
        &mut self,
        client: String,
        request_id: String,
        label: String,
        defer_timeout_ms: u64,
        now_ms: u64,
    ) -> AgentAdmit {
        if self.lock_broken.as_deref() == Some(client.as_str()) {
            self.lock_broken = None;
            return AgentAdmit::Reject {
                code: ErrorCode::LockBroken,
                detail: json!({}),
            };
        }
        if self.hold {
            return AgentAdmit::Reject {
                code: ErrorCode::HeldByUser,
                detail: json!({}),
            };
        }
        if let Some(lock) = &mut self.lock {
            if lock.client != client {
                return AgentAdmit::Reject {
                    code: ErrorCode::Locked,
                    detail: json!({
                        "reason": lock.reason,
                        "remaining_ms": lock.expires_ms.saturating_sub(now_ms),
                    }),
                };
            }
            lock.expires_ms = now_ms + lock.ttl_ms;
            let ev = ArbiterEvent::Lock {
                on: true,
                reason: Some(lock.reason.clone()),
                ttl_ms: Some(lock.ttl_ms),
                actor: Some(lock.client.clone()),
            };
            self.push_event(ev);
        }
        if self.pending.is_empty() && self.can_admit() {
            self.owner = Owner::Agent { client, request_id };
            return AgentAdmit::Now;
        }
        if self.pending.len() >= AGENT_PENDING_MAX {
            return AgentAdmit::Reject {
                code: ErrorCode::RateLimit,
                detail: json!({ "pending_max": AGENT_PENDING_MAX }),
            };
        }
        let until_ms = now_ms + defer_timeout_ms;
        self.pending.push_back(Pending {
            client,
            request_id,
            label,
            since_ms: now_ms,
            deadline_ms: until_ms,
            timeout_ms: defer_timeout_ms,
        });
        self.show_activity(now_ms);
        AgentAdmit::Deferred { until_ms }
    }

    /// The agent send `request_id` finished (sent, failed or cancelled): release
    /// the lease and admit the next deferred send if possible.
    pub fn agent_done(&mut self, request_id: &str, now_ms: u64) -> Vec<TickAction> {
        if matches!(&self.owner, Owner::Agent { request_id: r, .. } if r == request_id) {
            self.owner = Owner::None;
        }
        self.tick(now_ms)
    }

    /// Agent lock for a multi-step sequence (`loom uart lock --ttl`). TTL is capped
    /// at 10 minutes; each agent send by the holder extends it.
    pub fn lock(
        &mut self,
        client: &str,
        ttl_ms: u64,
        lock_reason: &str,
        now_ms: u64,
    ) -> Result<(), ErrorCode> {
        let client = clip(client);
        self.housekeeping(now_ms);
        if self.hold {
            return Err(ErrorCode::HeldByUser);
        }
        if let Some(l) = &self.lock
            && l.client != client
        {
            return Err(ErrorCode::Locked);
        }
        let ttl_ms = if ttl_ms == 0 {
            LOCK_TTL_DEFAULT_MS
        } else {
            ttl_ms.min(LOCK_TTL_MAX_MS)
        };
        if self.lock_broken.as_deref() == Some(client.as_str()) {
            self.lock_broken = None;
        }
        let reason = clip(lock_reason);
        self.lock = Some(Lock {
            client: client.clone(),
            reason: reason.clone(),
            ttl_ms,
            expires_ms: now_ms + ttl_ms,
        });
        self.push_event(ArbiterEvent::Lock {
            on: true,
            reason: Some(reason),
            ttl_ms: Some(ttl_ms),
            actor: Some(client),
        });
        Ok(())
    }

    /// Release the agent lock held by `client`.
    pub fn unlock(&mut self, client: &str, now_ms: u64) -> Result<Vec<TickAction>, ErrorCode> {
        let client = clip(client);
        match &self.lock {
            Some(l) if l.client == client => {
                self.lock = None;
                self.push_event(ArbiterEvent::Lock {
                    on: false,
                    reason: None,
                    ttl_ms: None,
                    actor: Some(client),
                });
                Ok(self.tick(now_ms))
            }
            _ => Err(ErrorCode::Locked),
        }
    }

    /// Periodic step: lock expiry, held-key timeout, automatic lease return,
    /// deferred-send admission and deadlines. Call every ≤ 100 ms and after RX.
    pub fn tick(&mut self, now_ms: u64) -> Vec<TickAction> {
        self.housekeeping(now_ms);
        let actions = self.admit_and_expire(now_ms);
        self.emit_owner();
        actions
    }

    // ---------------------------------------------------------------- internals

    fn can_admit(&self) -> bool {
        self.owner == Owner::None && !self.dirty && !self.hold
    }

    /// Lock expiry, held-key timeout, automatic return.
    fn housekeeping(&mut self, now_ms: u64) {
        if let Some(l) = &self.lock
            && now_ms >= l.expires_ms
        {
            let actor = l.client.clone();
            self.lock = None;
            self.push_event(ArbiterEvent::Lock {
                on: false,
                reason: None,
                ttl_ms: None,
                actor: Some(actor),
            });
        }
        self.discard_held(
            |h| now_ms.saturating_sub(h.at_ms) >= TAKEOVER_WINDOW_MS,
            reason::TAKEOVER_TIMEOUT,
        );
        if let Owner::User(_) = self.owner {
            let returned_by = if self.cfg.has_prompt {
                self.prompt_after_line
            } else {
                now_ms.saturating_sub(self.last_user_ms) >= QUIET_RETURN_MS
            };
            if self.last_ends_line && !self.dirty && !self.alt_screen && returned_by {
                self.owner = Owner::None;
            }
        }
    }

    fn admit_and_expire(&mut self, now_ms: u64) -> Vec<TickAction> {
        let mut actions = Vec::new();
        let front_may_go = self.pending.front().is_some_and(|front| {
            self.can_admit() && self.lock.as_ref().is_none_or(|l| l.client == front.client)
        });
        if front_may_go && let Some(p) = self.pending.pop_front() {
            actions.push(TickAction::AgentReady {
                request_id: p.request_id.clone(),
                deferred_ms: now_ms.saturating_sub(p.since_ms),
            });
            self.owner = Owner::Agent {
                client: p.client,
                request_id: p.request_id,
            };
        }
        let mut kept = VecDeque::with_capacity(self.pending.len());
        while let Some(p) = self.pending.pop_front() {
            if now_ms < p.deadline_ms {
                kept.push_back(p);
                continue;
            }
            let (code, detail) = match &self.owner {
                Owner::Agent { .. } => (ErrorCode::Locked, json!({ "reason": reason::AGENT_BUSY })),
                Owner::User(_) if !self.dirty => (
                    ErrorCode::UserActive,
                    json!({"reason": reason::USER_TYPING, "user_line_len": self.line_len}),
                ),
                _ if self.dirty => (
                    ErrorCode::UserActive,
                    json!({"reason": reason::DIRTY_LINE, "user_line_len": self.line_len}),
                ),
                _ => match &self.lock {
                    Some(l) if l.client != p.client => (
                        ErrorCode::Locked,
                        json!({"reason": l.reason, "remaining_ms": l.expires_ms.saturating_sub(now_ms)}),
                    ),
                    _ => (ErrorCode::Locked, json!({ "reason": reason::AGENT_BUSY })),
                },
            };
            actions.push(TickAction::AgentRejected {
                request_id: p.request_id,
                code,
                detail,
            });
        }
        self.pending = kept;
        self.show_activity(now_ms);
        actions
    }

    /// Emit `uart.agent_activity` when the head of the deferred queue changes.
    fn show_activity(&mut self, now_ms: u64) {
        let head = self.pending.front().map(|p| p.request_id.clone());
        if head == self.shown_activity {
            return;
        }
        let ev = match self.pending.front() {
            Some(p) => ArbiterEvent::AgentActivity {
                kind: ActivityKind::DeferredSend,
                text: Some(p.label.clone()),
                elapsed_ms: Some(now_ms.saturating_sub(p.since_ms)),
                timeout_ms: Some(p.timeout_ms),
                request_id: Some(p.request_id.clone()),
                client: Some(p.client.clone()),
            },
            None => ArbiterEvent::AgentActivity {
                kind: ActivityKind::Idle,
                text: None,
                elapsed_ms: None,
                timeout_ms: None,
                request_id: None,
                client: None,
            },
        };
        self.shown_activity = head;
        self.push_event(ev);
    }

    /// User takes the lease; returns the agent request to preempt, if any.
    fn take_lease(&mut self, viewer: &str) -> Option<String> {
        let preempt = match &self.owner {
            Owner::Agent { request_id, .. } => Some(request_id.clone()),
            _ => None,
        };
        self.owner = Owner::User(viewer.to_owned());
        preempt
    }

    fn break_lock(&mut self) {
        if let Some(l) = self.lock.take() {
            self.push_event(ArbiterEvent::Lock {
                on: false,
                reason: None,
                ttl_ms: None,
                actor: Some(l.client.clone()),
            });
            self.lock_broken = Some(l.client);
        }
    }

    /// Remove and concatenate this viewer's held keys (acked as sent).
    fn drain_held_of(&mut self, viewer: &str) -> Vec<u8> {
        let mut out = Vec::new();
        let mut rest = VecDeque::with_capacity(self.held.len());
        let mut sent = Vec::new();
        while let Some(h) = self.held.pop_front() {
            if h.viewer == viewer {
                self.held_bytes -= h.bytes.len();
                out.extend_from_slice(&h.bytes);
                sent.push(h.input_seq);
            } else {
                rest.push_back(h);
            }
        }
        self.held = rest;
        for seq in sent {
            self.ack(seq, AckState::Sent, None);
        }
        out
    }

    fn discard_held(&mut self, mut pred: impl FnMut(&Held) -> bool, why: &'static str) {
        let mut rest = VecDeque::with_capacity(self.held.len());
        let mut gone = Vec::new();
        while let Some(h) = self.held.pop_front() {
            if pred(&h) {
                self.held_bytes -= h.bytes.len();
                gone.push(h.input_seq);
            } else {
                rest.push_back(h);
            }
        }
        self.held = rest;
        for seq in gone {
            self.ack(seq, AckState::Discarded, Some(why));
        }
    }

    /// Track the user line from transmitted user bytes.
    fn absorb_user(&mut self, bytes: &[u8], now_ms: u64) {
        for &b in bytes {
            match b {
                b'\r' | b'\n' | ETX => self.end_line(),
                NAK if self.cfg.line_kill_ctrl_u => self.end_line(),
                NAK => self.last_ends_line = false,
                0x08 | 0x7F => {
                    self.line_len = self.line_len.saturating_sub(1);
                    self.dirty = self.line_len > 0;
                    self.last_ends_line = false;
                }
                _ => {
                    self.line_len = self.line_len.saturating_add(1);
                    self.dirty = true;
                    self.last_ends_line = false;
                }
            }
        }
        self.prompt_after_line = false;
        self.last_user_ms = now_ms;
    }

    fn end_line(&mut self) {
        self.line_len = 0;
        self.dirty = false;
        self.last_ends_line = true;
    }

    fn clear_line(&mut self) {
        self.end_line();
    }

    fn ack(&mut self, input_seq: u64, state: AckState, why: Option<&'static str>) {
        self.push_event(ArbiterEvent::InputAck {
            input_seq,
            state,
            reason: why,
        });
    }

    fn emit_owner(&mut self) {
        let (owner, viewer, client) = match &self.owner {
            Owner::None => ("none", None, None),
            Owner::User(v) => ("user", Some(v.clone()), None),
            Owner::Agent { client, .. } => ("agent", None, Some(client.clone())),
        };
        let snap: OwnerSnapshot = (owner, viewer, client, self.dirty, self.hold);
        if self.last_owner_event.as_ref() == Some(&snap) {
            return;
        }
        self.last_owner_event = Some(snap.clone());
        let (owner, viewer, client, dirty, hold) = snap;
        self.push_event(ArbiterEvent::InputOwner {
            owner,
            viewer,
            client,
            dirty,
            hold,
        });
    }

    fn push_event(&mut self, ev: ArbiterEvent) {
        if self.events.len() >= EVENT_BACKLOG_MAX {
            self.events.pop_front();
            self.events_dropped = self.events_dropped.saturating_add(1);
        }
        self.events.push_back(ev);
    }
}
