//! InputArbiter rules: plan §3.7 table rows, M1 acceptance (e)/(f2), Ctrl-C,
//! hold, caps and event payloads (task p0-opus B).

use loom_console::arbiter::{
    AGENT_PENDING_MAX, AckState, AgentAdmit, ArbiterConfig, ArbiterEvent, INTERRUPT_MAX_IN_FLIGHT,
    InputArbiter, LOCK_TTL_MAX_MS, Owner, TickAction, USER_HELD_MAX_BYTES, USER_HELD_MAX_ITEMS, YieldResult,
    reason,
};
use loom_proto::error::ErrorCode;
use serde_json::json;

const SHELL: ArbiterConfig = ArbiterConfig {
    has_prompt: true,
    line_kill_ctrl_u: true,
};
const NO_PROMPT: ArbiterConfig = ArbiterConfig {
    has_prompt: false,
    line_kill_ctrl_u: false,
};

fn ready_ids(actions: &[TickAction]) -> Vec<&str> {
    actions
        .iter()
        .filter_map(|a| match a {
            TickAction::AgentReady { request_id, .. } => Some(request_id.as_str()),
            TickAction::AgentRejected { .. } => None,
        })
        .collect()
}

fn rejected(actions: &[TickAction]) -> Vec<(&str, ErrorCode, serde_json::Value)> {
    actions
        .iter()
        .filter_map(|a| match a {
            TickAction::AgentRejected {
                request_id,
                code,
                detail,
            } => Some((request_id.as_str(), *code, detail.clone())),
            TickAction::AgentReady { .. } => None,
        })
        .collect()
}

fn acks(arb: &mut InputArbiter) -> Vec<(u64, AckState, Option<&'static str>)> {
    arb.take_events()
        .into_iter()
        .filter_map(|e| match e {
            ArbiterEvent::InputAck {
                input_seq,
                state,
                reason,
            } => Some((input_seq, state, reason)),
            _ => None,
        })
        .collect()
}

// ---- plan §3.7 table ------------------------------------------------------

#[test]
fn row1_agent_send_while_user_idle_goes_now() {
    let mut a = InputArbiter::new(SHELL);
    assert_eq!(a.agent_send("claude", "q1", "ls⏎", 10_000, 0), AgentAdmit::Now);
    assert_eq!(
        a.owner(),
        &Owner::Agent {
            client: "claude".into(),
            request_id: "q1".into()
        }
    );
    assert!(a.agent_done("q1", 5).is_empty());
    assert_eq!(a.owner(), &Owner::None);
}

#[test]
fn row2_user_typing_defers_agent_until_line_done_and_prompt() {
    let mut a = InputArbiter::new(SHELL);
    let d = a.user_input("tab1", 1, b"ifconfig eth", 0);
    assert_eq!((d.send.as_slice(), d.ack), (&b"ifconfig eth"[..], AckState::Sent));
    assert_eq!(
        a.agent_send("claude", "q1", "ip -br addr⏎", 10_000, 100),
        AgentAdmit::Deferred { until_ms: 10_100 }
    );
    assert!(a.tick(1_000).is_empty());
    a.user_input("tab1", 2, b"0\r", 5_000);
    assert!(a.tick(5_100).is_empty(), "no prompt yet");
    let act = a.prompt_seen(5_200);
    assert_eq!(
        act,
        vec![TickAction::AgentReady {
            request_id: "q1".into(),
            deferred_ms: 5_100
        }]
    );
}

#[test]
fn row3_unfinished_line_past_defer_timeout_is_user_active() {
    let mut a = InputArbiter::new(SHELL);
    a.user_input("tab1", 1, b"ifconfig eth", 0);
    a.agent_send("claude", "q1", "ip⏎", 10_000, 0);
    assert!(a.tick(9_999).is_empty());
    let act = a.tick(10_000);
    let r = rejected(&act);
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].1, ErrorCode::UserActive);
    assert_eq!(r[0].2, json!({"reason": "dirty_line", "user_line_len": 12}));
}

#[test]
fn row4_hold_rejects_new_and_pending_agent_sends() {
    let mut a = InputArbiter::new(SHELL);
    a.user_input("tab1", 1, b"x", 0);
    assert!(matches!(
        a.agent_send("claude", "q1", "", 10_000, 0),
        AgentAdmit::Deferred { .. }
    ));
    let act = a.set_hold(true, 10);
    assert_eq!(rejected(&act), vec![("q1", ErrorCode::HeldByUser, json!({}))]);
    assert!(matches!(
        a.agent_send("claude", "q2", "", 10_000, 20),
        AgentAdmit::Reject {
            code: ErrorCode::HeldByUser,
            ..
        }
    ));
    // User keys still flow during hold.
    assert_eq!(a.user_input("tab1", 2, b"\r", 30).ack, AckState::Sent);
}

#[test]
fn row5_agent_lock_holds_user_keys_until_takeover() {
    let mut a = InputArbiter::new(SHELL);
    a.lock("claude", 120_000, "u-boot env 수정", 0)
        .expect("lock free");
    let d = a.user_input("tab1", 7, b"printenv", 100);
    assert_eq!((d.ack, d.reason), (AckState::Held, Some(reason::AGENT_LOCK)));
    assert!(d.send.is_empty());
    // Lock holder keeps sending.
    assert_eq!(a.agent_send("claude", "q1", "", 10_000, 200), AgentAdmit::Now);
    a.agent_done("q1", 300);
    a.take_events();
    let t = a.takeover("tab1", 1_000);
    assert_eq!(t.send, b"printenv");
    assert_eq!(acks(&mut a), vec![(7, AckState::Sent, None)]);
    assert!(a.lock_info().is_none());
    assert!(matches!(
        a.agent_send("claude", "q2", "", 10_000, 1_100),
        AgentAdmit::Reject {
            code: ErrorCode::LockBroken,
            ..
        }
    ));
}

#[test]
fn row6_ctrl_c_during_agent_transfer_preempts_it() {
    let mut a = InputArbiter::new(SHELL);
    assert_eq!(
        a.agent_send("claude", "big", "file 16 KiB", 10_000, 0),
        AgentAdmit::Now
    );
    let d = a.user_input("tab1", 1, b"\x03", 50);
    assert_eq!(d.send, b"\x03");
    assert!(d.interrupt);
    assert_eq!(d.preempt_agent.as_deref(), Some("big"));
    assert_eq!(a.owner(), &Owner::User("tab1".into()));
    // The late agent_done for the preempted request must not steal the lease.
    a.agent_done("big", 60);
    assert_eq!(a.owner(), &Owner::User("tab1".into()));
}

// Row 7 (R3 send waits for approval) is the core approval pipeline, not the arbiter.

// ---- M1 acceptance --------------------------------------------------------

#[test]
fn m1_e_unfinished_line_plus_idle_never_lets_agent_bytes_in() {
    let mut a = InputArbiter::new(SHELL);
    a.user_input("tab1", 1, b"ifconfig eth", 0);
    assert!(matches!(
        a.agent_send("claude", "q1", "ip -br addr⏎", 10_000, 4_000),
        AgentAdmit::Deferred { .. }
    ));
    let mut t = 4_000;
    let mut all = Vec::new();
    while t <= 14_000 {
        all.extend(a.tick(t));
        t += 250;
    }
    assert!(ready_ids(&all).is_empty(), "agent bytes would mix into the line");
    assert_eq!(rejected(&all)[0].1, ErrorCode::UserActive);
}

#[test]
fn m1_f2_tab_closed_with_dirty_line_then_new_tab_clears() {
    let mut a = InputArbiter::new(SHELL);
    a.user_input("tab1", 1, b"rm -rf /tm", 0);
    a.viewer_left("tab1", 100);
    assert_eq!(a.owner(), &Owner::None);
    assert!(a.input_dirty(), "dirty survives the viewer");
    assert!(matches!(
        a.agent_send("claude", "q1", "ls⏎", 10_000, 200),
        AgentAdmit::Deferred { .. }
    ));
    assert!(a.tick(3_000).is_empty());
    let (send, act) = a.clear_input("tab2", 4_000);
    assert_eq!(send, vec![0x15], "verified profile sends ^U");
    assert_eq!(ready_ids(&act), vec!["q1"]);
}

#[test]
fn m1_f2_two_viewers_typing_do_not_mix_and_takeover_sends() {
    let mut a = InputArbiter::new(SHELL);
    let first = a.user_input("tab1", 1, b"ab", 0);
    assert_eq!(first.send, b"ab");
    let second = a.user_input("tab2", 2, b"cd", 10);
    assert!(second.send.is_empty());
    assert_eq!(
        (second.ack, second.reason),
        (AckState::Held, Some(reason::OTHER_WRITER))
    );
    // tab1 keeps typing normally; tab2's bytes never appear in its stream.
    assert_eq!(a.user_input("tab1", 3, b"c\r", 20).send, b"c\r");
    let t = a.takeover("tab2", 1_000);
    assert_eq!(t.send, b"cd");
    assert_eq!(a.owner(), &Owner::User("tab2".into()));
}

#[test]
fn m1_f2_held_keys_are_discarded_after_5_s() {
    let mut a = InputArbiter::new(SHELL);
    a.user_input("tab1", 1, b"ab", 0);
    a.user_input("tab2", 2, b"zz", 100);
    a.take_events();
    a.tick(5_099);
    assert!(acks(&mut a).is_empty());
    a.tick(5_100);
    assert_eq!(
        acks(&mut a),
        vec![(2, AckState::Discarded, Some(reason::TAKEOVER_TIMEOUT))]
    );
    assert_eq!(a.held_bytes(), 0);
    assert!(a.takeover("tab2", 5_200).send.is_empty());
}

// ---- Ctrl-C ---------------------------------------------------------------

#[test]
fn ctrl_c_breaks_agent_lock_and_keeps_own_held_keys_first() {
    let mut a = InputArbiter::new(SHELL);
    a.lock("claude", 60_000, "seq", 0).expect("lock");
    a.user_input("tab1", 1, b"ab", 10);
    let d = a.user_input("tab1", 2, b"\x03", 20);
    assert_eq!(d.send, b"ab\x03");
    assert!(d.interrupt);
    assert!(a.lock_info().is_none());
    assert!(!a.input_dirty());
    assert!(matches!(
        a.agent_send("claude", "q", "", 1_000, 30),
        AgentAdmit::Reject {
            code: ErrorCode::LockBroken,
            ..
        }
    ));
}

#[test]
fn ctrl_c_passes_from_any_viewer() {
    let mut a = InputArbiter::new(SHELL);
    a.user_input("tab1", 1, b"top", 0);
    let d = a.user_input("tab2", 2, b"\x03", 10);
    assert_eq!(d.send, b"\x03");
    assert_eq!(d.ack, AckState::Sent);
    assert_eq!(a.owner(), &Owner::User("tab2".into()));
}

// ---- lease return ----------------------------------------------------------

#[test]
fn no_prompt_profile_returns_after_quiet_time() {
    let mut a = InputArbiter::new(NO_PROMPT);
    a.user_input("tab1", 1, b"ls\r", 0);
    a.tick(1_499);
    assert_eq!(a.owner(), &Owner::User("tab1".into()));
    a.tick(1_500);
    assert_eq!(a.owner(), &Owner::None);
}

#[test]
fn alternate_screen_blocks_automatic_return() {
    let mut a = InputArbiter::new(SHELL);
    a.set_alt_screen(true);
    a.user_input("tab1", 1, b":q\r", 0);
    a.prompt_seen(100);
    assert_eq!(a.owner(), &Owner::User("tab1".into()));
    a.set_alt_screen(false);
    a.prompt_seen(200);
    assert_eq!(a.owner(), &Owner::None);
}

#[test]
fn prompt_before_line_end_does_not_return() {
    let mut a = InputArbiter::new(SHELL);
    a.user_input("tab1", 1, b"ech", 0);
    a.prompt_seen(10);
    assert_eq!(a.owner(), &Owner::User("tab1".into()));
}

#[test]
fn ctrl_u_ends_a_line_only_on_verified_profiles() {
    let mut v = InputArbiter::new(SHELL);
    v.user_input("t", 1, b"abc\x15", 0);
    assert!(!v.input_dirty());
    let mut u = InputArbiter::new(NO_PROMPT);
    u.user_input("t", 1, b"abc\x15", 0);
    assert!(u.input_dirty());
}

#[test]
fn yield_twice_clears_with_ctrl_u_only_on_verified_profile() {
    let mut a = InputArbiter::new(SHELL);
    a.user_input("tab1", 1, b"ifconfig eth", 0);
    a.agent_send("claude", "q1", "", 10_000, 0);
    let (r1, act1) = a.yield_input("tab1", 100);
    assert_eq!(r1, YieldResult::ConfirmClear { line_len: 12 });
    assert!(act1.is_empty());
    let (r2, act2) = a.yield_input("tab1", 200);
    assert_eq!(r2, YieldResult::Cleared { send: vec![0x15] });
    assert_eq!(ready_ids(&act2), vec!["q1"]);

    let mut b = InputArbiter::new(NO_PROMPT);
    b.user_input("tab1", 1, b"abc", 0);
    b.yield_input("tab1", 100);
    let (r, _) = b.yield_input("tab1", 200);
    assert_eq!(r, YieldResult::ManualClearNeeded { line_len: 3 });
    assert!(b.input_dirty());
}

#[test]
fn yield_with_clean_line_releases() {
    let mut a = InputArbiter::new(SHELL);
    a.user_input("tab1", 1, b"ls\r", 0);
    a.agent_send("claude", "q1", "", 10_000, 0);
    let (r, act) = a.yield_input("tab1", 50);
    assert_eq!(r, YieldResult::Released);
    assert_eq!(ready_ids(&act), vec!["q1"]);
}

// ---- lock -----------------------------------------------------------------

#[test]
fn lock_ttl_is_capped_extended_and_exclusive() {
    let mut a = InputArbiter::new(SHELL);
    a.lock("claude", 3_600_000, "long", 0).expect("lock");
    let (_, _, exp) = a.lock_info().expect("locked");
    assert_eq!(exp, LOCK_TTL_MAX_MS);
    assert_eq!(a.lock("codex", 1_000, "x", 10), Err(ErrorCode::Locked));
    assert!(matches!(
        a.agent_send("codex", "q", "", 1_000, 20),
        AgentAdmit::Reject {
            code: ErrorCode::Locked,
            ..
        }
    ));
    assert_eq!(a.agent_send("claude", "q1", "", 1_000, 500_000), AgentAdmit::Now);
    let (_, _, exp) = a.lock_info().expect("locked");
    assert_eq!(exp, 500_000 + LOCK_TTL_MAX_MS, "holder's call extends");
    a.agent_done("q1", 500_001);
    a.tick(500_000 + LOCK_TTL_MAX_MS);
    assert!(a.lock_info().is_none(), "expired");
}

// ---- caps (plan §3.13 TX row) ----------------------------------------------

#[test]
fn held_queue_item_cap_discards_with_reason() {
    let mut a = InputArbiter::new(SHELL);
    a.user_input("tab1", 0, b"a", 0);
    for i in 0..USER_HELD_MAX_ITEMS as u64 {
        assert_eq!(a.user_input("tab2", i + 1, b"k", 1).ack, AckState::Held);
    }
    let d = a.user_input("tab2", 999, b"k", 2);
    assert_eq!((d.ack, d.reason), (AckState::Discarded, Some(reason::QUEUE_FULL)));
}

#[test]
fn held_queue_byte_cap_discards_with_reason() {
    let mut a = InputArbiter::new(SHELL);
    a.user_input("tab1", 0, b"a", 0);
    let big = vec![b'x'; USER_HELD_MAX_BYTES];
    assert_eq!(a.user_input("tab2", 1, &big, 1).ack, AckState::Held);
    let d = a.user_input("tab2", 2, b"y", 2);
    assert_eq!(d.reason, Some(reason::QUEUE_FULL));
    assert_eq!(a.held_bytes(), USER_HELD_MAX_BYTES);
}

#[test]
fn agent_pending_cap_is_rate_limit() {
    let mut a = InputArbiter::new(SHELL);
    a.user_input("tab1", 0, b"typing", 0);
    for i in 0..AGENT_PENDING_MAX {
        assert!(matches!(
            a.agent_send("claude", &format!("q{i}"), "", 10_000, 1),
            AgentAdmit::Deferred { .. }
        ));
    }
    assert!(matches!(
        a.agent_send("claude", "over", "", 10_000, 2),
        AgentAdmit::Reject {
            code: ErrorCode::RateLimit,
            ..
        }
    ));
}

#[test]
fn interrupt_lane_cap_and_release() {
    let mut a = InputArbiter::new(SHELL);
    for i in 0..INTERRUPT_MAX_IN_FLIGHT as u64 {
        assert!(a.user_input("tab1", i, b"\x03", i).interrupt);
    }
    let d = a.user_input("tab1", 100, b"\x03", 100);
    assert_eq!((d.ack, d.reason), (AckState::Discarded, Some(reason::QUEUE_FULL)));
    a.interrupt_written(1);
    assert_eq!(a.user_input("tab1", 101, b"\x03", 101).ack, AckState::Sent);
}

// ---- events -----------------------------------------------------------------

#[test]
fn event_payloads_match_the_ui_contract() {
    let mut a = InputArbiter::new(SHELL);
    a.user_input("tab1", 5, b"ab", 0);
    a.agent_send("claude", "q1", "ls⏎", 10_000, 0);
    a.lock("codex", 30_000, "flash", 10).expect("lock");
    a.set_hold(true, 20);
    let evs = a.take_events();
    let rendered: Vec<(&str, serde_json::Value)> = evs.iter().map(|e| (e.kind(), e.payload())).collect();
    assert!(rendered.contains(&(
        "uart.input_owner",
        json!({"owner": "user", "viewer": "tab1", "dirty": true, "hold": false})
    )));
    assert!(rendered.contains(&("uart.input_ack", json!({"input_seq": "5", "state": "sent"}))));
    assert!(rendered.contains(&(
        "uart.agent_activity",
        json!({"kind": "deferred_send", "text": "ls⏎", "elapsed_ms": 0, "timeout_ms": 10_000,
               "request_id": "q1", "client": "claude"})
    )));
    assert!(rendered.contains(&(
        "uart.lock",
        json!({"on": true, "reason": "flash", "ttl_ms": 30_000, "actor": "codex"})
    )));
    assert!(rendered.contains(&("uart.hold", json!({"on": true}))));
    assert!(rendered.contains(&("uart.agent_activity", json!({"kind": "idle"}))));
    assert!(rendered.contains(&(
        "uart.input_owner",
        json!({"owner": "user", "viewer": "tab1", "dirty": true, "hold": true})
    )));
}

#[test]
fn owner_event_only_on_change() {
    let mut a = InputArbiter::new(SHELL);
    a.user_input("tab1", 1, b"a", 0);
    a.user_input("tab1", 2, b"b", 1);
    let owners = a
        .take_events()
        .into_iter()
        .filter(|e| e.kind() == "uart.input_owner")
        .count();
    assert_eq!(owners, 1);
}
