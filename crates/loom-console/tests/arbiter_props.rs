//! InputArbiter invariant under random event sequences (task p0-opus B):
//! an agent send is never admitted while the user's line on the device is
//! unfinished, never while hold is on, never two at once, and user bytes sent during
//! an agent transfer always preempt it.
//!
//! The model tracks the device line only from the bytes the arbiter tells the
//! caller to transmit, independently of the arbiter's internal state.

use loom_console::arbiter::{AgentAdmit, ArbiterConfig, InputArbiter, InputDecision, TickAction};
use proptest::prelude::*;

#[derive(Debug, Clone)]
enum Op {
    User(u8, Vec<u8>),
    AgentSend,
    AgentDone,
    Tick(u64),
    Prompt,
    ViewerLeft(u8),
    Clear(u8),
    Yield(u8),
    Takeover(u8),
    Hold(bool),
    Lock(bool),
    Unlock,
    Alt(bool),
    InterruptWritten,
}

fn op() -> impl Strategy<Value = Op> {
    let key = prop::sample::select(vec![b'a', b'b', b'\r', b'\n', 0x03, 0x15, 0x08]);
    prop_oneof![
        8 => (0u8..3, prop::collection::vec(key, 1..6)).prop_map(|(v, b)| Op::User(v, b)),
        5 => Just(Op::AgentSend),
        3 => Just(Op::AgentDone),
        6 => (1u64..3_000).prop_map(Op::Tick),
        3 => Just(Op::Prompt),
        1 => (0u8..3).prop_map(Op::ViewerLeft),
        1 => (0u8..3).prop_map(Op::Clear),
        2 => (0u8..3).prop_map(Op::Yield),
        1 => (0u8..3).prop_map(Op::Takeover),
        1 => any::<bool>().prop_map(Op::Hold),
        1 => any::<bool>().prop_map(Op::Lock),
        1 => Just(Op::Unlock),
        1 => any::<bool>().prop_map(Op::Alt),
        1 => Just(Op::InterruptWritten),
    ]
}

struct Model {
    cfg: ArbiterConfig,
    line_len: u32,
    transmitting: Option<String>,
    hold: bool,
    next_id: u64,
}

impl Model {
    fn absorb(&mut self, bytes: &[u8]) {
        for &b in bytes {
            match b {
                b'\r' | b'\n' | 0x03 => self.line_len = 0,
                0x15 if self.cfg.line_kill_ctrl_u => self.line_len = 0,
                0x15 => {}
                0x08 | 0x7F => self.line_len = self.line_len.saturating_sub(1),
                _ => self.line_len += 1,
            }
        }
    }

    fn admitted(&mut self, id: &str) -> Result<(), TestCaseError> {
        prop_assert_eq!(self.line_len, 0, "agent {} admitted into an unfinished line", id);
        prop_assert!(!self.hold, "agent {} admitted during hold", id);
        prop_assert!(
            self.transmitting.is_none(),
            "agent {} admitted while {:?} transmits",
            id,
            self.transmitting
        );
        self.transmitting = Some(id.to_owned());
        Ok(())
    }

    fn user_sent(&mut self, d: &InputDecision) -> Result<(), TestCaseError> {
        // Taking the lease (bytes or an explicit takeover) must stop the agent.
        if let Some(p) = &d.preempt_agent {
            prop_assert_eq!(self.transmitting.as_deref(), Some(p.as_str()));
            self.transmitting = None;
        }
        if !d.send.is_empty() {
            prop_assert!(
                self.transmitting.is_none(),
                "user bytes during agent transfer without preempt"
            );
            self.absorb(&d.send);
        }
        Ok(())
    }

    fn actions(&mut self, acts: &[TickAction]) -> Result<(), TestCaseError> {
        for a in acts {
            if let TickAction::AgentReady { request_id, .. } = a {
                self.admitted(request_id)?;
            }
        }
        Ok(())
    }
}

fn run(cfg: ArbiterConfig, ops: &[Op]) -> Result<(), TestCaseError> {
    let mut a = InputArbiter::new(cfg);
    let mut m = Model {
        cfg,
        line_len: 0,
        transmitting: None,
        hold: false,
        next_id: 0,
    };
    let mut now = 0u64;
    let mut seq = 0u64;
    let viewer = |v: u8| format!("tab{v}");
    for op in ops {
        now += 1;
        match op {
            Op::User(v, bytes) => {
                seq += 1;
                let d = a.user_input(&viewer(*v), seq, bytes, now);
                m.user_sent(&d)?;
            }
            Op::AgentSend => {
                m.next_id += 1;
                let id = format!("q{}", m.next_id);
                if a.agent_send("claude", &id, "cmd", 2_000, now) == AgentAdmit::Now {
                    m.admitted(&id)?;
                }
            }
            Op::AgentDone => {
                let id = m.transmitting.take().unwrap_or_else(|| "stale".to_owned());
                let acts = a.agent_done(&id, now);
                m.actions(&acts)?;
            }
            Op::Tick(dt) => {
                now += dt;
                let acts = a.tick(now);
                m.actions(&acts)?;
            }
            Op::Prompt => {
                let acts = a.prompt_seen(now);
                m.actions(&acts)?;
            }
            Op::ViewerLeft(v) => {
                let acts = a.viewer_left(&viewer(*v), now);
                m.actions(&acts)?;
            }
            Op::Clear(v) => {
                let (send, acts) = a.clear_input(&viewer(*v), now);
                if !send.is_empty() {
                    prop_assert!(m.transmitting.is_none(), "^U into an agent transfer");
                }
                // The user asserts the line is clean (^U on verified profiles).
                m.line_len = 0;
                m.actions(&acts)?;
            }
            Op::Yield(v) => {
                let (r, acts) = a.yield_input(&viewer(*v), now);
                if let loom_console::arbiter::YieldResult::Cleared { send } = &r {
                    prop_assert!(m.transmitting.is_none());
                    m.absorb(send);
                }
                m.actions(&acts)?;
            }
            Op::Takeover(v) => {
                let d = a.takeover(&viewer(*v), now);
                m.user_sent(&d)?;
            }
            Op::Hold(on) => {
                m.hold = *on;
                let acts = a.set_hold(*on, now);
                m.actions(&acts)?;
            }
            Op::Lock(other) => {
                let who = if *other { "codex" } else { "claude" };
                let _ = a.lock(who, 5_000, "seq", now);
            }
            Op::Unlock => {
                if let Ok(acts) = a.unlock("claude", now) {
                    m.actions(&acts)?;
                }
            }
            Op::Alt(on) => a.set_alt_screen(*on),
            Op::InterruptWritten => a.interrupt_written(1),
        }
        prop_assert!(a.held_bytes() <= loom_console::arbiter::USER_HELD_MAX_BYTES);
        prop_assert!(a.pending_agent_sends() <= loom_console::arbiter::AGENT_PENDING_MAX);
        a.take_events();
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1024))]

    #[test]
    fn no_agent_bytes_into_unfinished_user_line_verified(ops in prop::collection::vec(op(), 1..120)) {
        run(ArbiterConfig { has_prompt: true, line_kill_ctrl_u: true }, &ops)?;
    }

    #[test]
    fn no_agent_bytes_into_unfinished_user_line_generic(ops in prop::collection::vec(op(), 1..120)) {
        run(ArbiterConfig { has_prompt: false, line_kill_ctrl_u: false }, &ops)?;
    }
}
