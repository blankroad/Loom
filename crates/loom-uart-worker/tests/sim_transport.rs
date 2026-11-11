//! `sim://` path: the same scenarios through `SimTransport` (the path Windows CI runs).

mod common;

use common::{Session, bootshell_script};
use loom_uart_worker::sim::{ANSI_DONE_MARKER, QUERY_SEQUENCES, binary_frames};
use loom_uart_worker::transport::{self, OpenOpts, PortConfig, TransportKind};
use loom_uart_worker::tx::TxLane;

fn open(target: &str) -> Session {
    let t = transport::open(target, &PortConfig::default(), &OpenOpts::default()).expect("open sim target");
    assert_eq!(t.kind(), TransportKind::Sim);
    Session::new(t)
}

#[test]
fn sim_bootshell_login_uname_ping_ctrl_c() {
    let mut s = open("sim://bootshell?seed=3");
    bootshell_script(&mut s);
}

#[test]
fn sim_bootshell_line_editing_and_unknown_command() {
    let mut s = open("sim://bootshell?seed=4");
    s.expect(b"login: ");
    s.send(b"root\r");
    s.expect(b"Password: ");
    s.send(b"x\r");
    s.expect(b"root@sim:~# ");
    // User keys with a typo, backspace, kill-line and a final command.
    s.send_lane(TxLane::User, b"statsu\x7f\x7fus");
    s.send_lane(TxLane::User, b"\r");
    let out = s.expect(b"root@sim:~# ");
    assert!(String::from_utf8_lossy(&out).contains("temp=41.5 rpm=1200"));
    s.send(b"junk\x15frob\r");
    let out = s.expect(b"root@sim:~# ");
    assert!(String::from_utf8_lossy(&out).contains("sh: frob: not found"));
}

#[test]
fn sim_binary_all_bytes_both_directions() {
    let mut s = open("sim://binary?frames=3&seed=1");
    assert_eq!(s.read_exact(768), binary_frames(3));
    let all: Vec<u8> = (0..=255u8).collect();
    s.send(&all);
    assert_eq!(s.read_exact(256), all);
}

#[test]
fn sim_ansi_emits_every_query_unchanged() {
    let mut s = open("sim://ansi?seed=2");
    let out = s.expect(ANSI_DONE_MARKER.as_bytes());
    for q in QUERY_SEQUENCES {
        assert!(
            out.windows(q.bytes.len()).any(|w| w == q.bytes),
            "{} missing",
            q.name
        );
    }
    // A leaked terminal auto-reply would be visible to tests as an `rx:` report.
    s.send(b"\x1b[?62;22c");
    s.expect(b"rx: \\x1b[?62;22c\r\n");
}

#[test]
fn sim_same_seed_same_bytes() {
    let run = || {
        let mut s = open("sim://bootshell?seed=11");
        s.expect(b"login: ")
    };
    assert_eq!(run(), run());
}
