//! Unregistered OS PTYs through the real serial backend, without SIM registry or injection.
#![cfg(target_os = "macos")]

use std::os::fd::AsFd;
use std::time::{Duration, Instant};

use loom_proto::event::{Dir, Env, Origin, WriteState};
use loom_testkit::FakeWorker;
use loom_uart_worker::transport::{OpenOpts, PortConfig, SerialTransport, Transport, TransportKind};
use loom_uart_worker::worker::UartWorker;
use nix::fcntl::{FcntlArg, OFlag, fcntl};
use nix::pty::{OpenptyResult, openpty};
use nix::sys::termios::{SetArg, cfmakeraw, tcgetattr, tcsetattr};
use nix::unistd::{read, ttyname, write};
use serde_json::json;

fn external_pty() -> OpenptyResult {
    let pair = openpty(None, None).expect("OS PTY, never registered with Loom SIM");
    let mut term = tcgetattr(&pair.slave).expect("termios");
    cfmakeraw(&mut term);
    tcsetattr(&pair.slave, SetArg::TCSANOW, &term).expect("raw slave");
    fcntl(&pair.master, FcntlArg::F_SETFL(OFlag::O_NONBLOCK)).expect("nonblocking fixture");
    pair
}

#[derive(Debug)]
struct Alias(std::path::PathBuf);

impl Drop for Alias {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn external_pty_symlink_without_tty_filename_uses_descriptor_identity() {
    let pair = external_pty();
    let path = ttyname(&pair.slave).expect("slave name");
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("wall time")
        .as_nanos();
    let dir = Alias(std::env::temp_dir().join(format!("loom-pty-alias-{}-{unique}", std::process::id())));
    std::fs::create_dir(&dir.0).expect("alias directory");
    let alias = dir.0.join("external-console");
    std::os::unix::fs::symlink(path, &alias).expect("symlink to PTY");
    let mut transport = SerialTransport::open(
        alias.to_str().expect("alias path"),
        &PortConfig::default(),
        &OpenOpts::default(),
    )
    .expect("open external PTY by arbitrary symlink");
    assert_eq!(transport.kind(), TransportKind::Pty);
    write(&pair.master, b"ALIAS_RX").expect("fixture TX");
    let mut buf = [0u8; 8];
    let n = transport
        .read(&mut buf, Duration::from_secs(1))
        .expect("alias RX");
    assert_eq!(&buf[..n], b"ALIAS_RX");
}

#[test]
fn unregistered_external_pty_serial_io_and_exclusive_reopen() {
    let pair = external_pty();
    let path = ttyname(&pair.slave).expect("slave name");
    // No sim_pty option, registration, path prefix, or simulator transport.
    let mut transport = SerialTransport::open(
        path.to_str().expect("UTF-8 path"),
        &PortConfig::default(),
        &OpenOpts::default(),
    )
    .expect("open external PTY through serialport");
    assert_eq!(transport.kind(), TransportKind::Pty);
    assert_eq!(
        transport.config().baud,
        115_200,
        "requested configuration preserved"
    );
    let busy = SerialTransport::open(
        path.to_str().expect("path"),
        &PortConfig::default(),
        &OpenOpts::default(),
    )
    .expect_err("exclusive open");
    assert_eq!(busy.kind(), std::io::ErrorKind::ResourceBusy);

    assert_eq!(write(&pair.master, b"EXTERNAL_RX\n").expect("fixture TX"), 12);
    let mut received = Vec::new();
    let mut buf = [0u8; 64];
    let deadline = Instant::now() + Duration::from_secs(2);
    while received.len() < 12 && Instant::now() < deadline {
        let n = transport
            .read(&mut buf, Duration::from_millis(50))
            .expect("serial RX");
        received.extend_from_slice(&buf[..n]);
    }
    assert_eq!(received, b"EXTERNAL_RX\n");
    let result = transport.write(b"HOST_TX\r", deadline);
    assert_eq!(result.written, 8);
    assert_eq!(read(&pair.master, &mut buf).expect("fixture RX"), 8);
    assert_eq!(&buf[..8], b"HOST_TX\r");
    drop(transport);
    let mut reopened = SerialTransport::open(
        path.to_str().expect("path"),
        &PortConfig::default(),
        &OpenOpts::default(),
    )
    .expect("reopen after close");
    // Last master close must reach the real serial reader as EOF/HUP, not fabricated bytes.
    drop(pair.master);
    assert!(reopened.read(&mut buf, Duration::from_secs(1)).is_err());
}

#[test]
fn external_pty_worker_keeps_bench_provenance_device_rx_and_generation() {
    let pair = external_pty();
    let path = ttyname(&pair.slave).expect("slave name");
    let mut fw = FakeWorker::new(UartWorker::new());
    let op = json!({"method":"uart.open", "target":path, "alias":"external"});
    let opened = fw.write("ext-open", op.clone());
    assert_eq!(opened.state, WriteState::Sent, "{:?}", opened.error);
    assert_eq!(
        opened.detail["env"], "bench",
        "PTY identity is not SIM provenance"
    );
    assert_eq!(opened.detail["generation"], 1);
    let rows = fw.drain_published();
    let state = rows
        .iter()
        .find(|r| r.kind == "uart.state" && r.payload["state"] == "open")
        .expect("real transport open row");
    assert_eq!(state.payload["transport"], "pty");
    assert_eq!(state.env, Some(Env::Bench));

    let tx = fw.write(
        "ext-send",
        json!({"method":"uart.send", "channel":"external", "data":hex::encode(b"CLI_COMMAND"), "eol":"cr"}),
    );
    assert_eq!(tx.state, WriteState::Sent, "{:?}", tx.error);
    let mut buf = [0u8; 64];
    let n = read(pair.master.as_fd(), &mut buf).expect("independent fixture sees actual TX");
    assert_eq!(&buf[..n], b"CLI_COMMAND\r");
    write(&pair.master, b"EXTERNAL_REPLY source_seq=1\r\n").expect("fixture response");
    fw.call("uart.expect", json!({"channel":"external", "patterns":[{"kind":"literal","pattern":"EXTERNAL_REPLY source_seq=1"}], "timeout_ms":2000})).expect("worker device RX");
    let rows = fw.drain_published();
    assert!(rows.iter().any(|r| r.kind == "uart.data"
        && r.dir == Some(Dir::Rx)
        && r.origin == Origin::Device
        && r.env == Some(Env::Bench)));
    let closed = fw.write("ext-close", json!({"method":"uart.close","channel":"external"}));
    assert_eq!(closed.detail["threads_left"], 0);
    assert_eq!(closed.detail["refs_left"], 0);
    let reopened = fw.write("ext-reopen", op);
    assert_eq!(reopened.state, WriteState::Sent, "{:?}", reopened.error);
    assert_eq!(reopened.detail["generation"], 2);
    assert_eq!(reopened.detail["env"], "bench");
    fw.write(
        "ext-final-close",
        json!({"method":"uart.close","channel":"external"}),
    );
}
