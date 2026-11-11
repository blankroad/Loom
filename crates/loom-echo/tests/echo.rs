//! The sample domain through the test kit — the pattern domain teams copy.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use loom_cli_kit::{CliOf, DynDomainCli, run_cli_with};
use loom_echo::cli::EchoCli;
use loom_echo::gateway::Echo;
use loom_echo::worker::EchoWorker;
use loom_echo::{KIND_DATA, KIND_WRITE};
use loom_proto::event::{Risk, payload_bytes};
use loom_proto::worker::Lane;
use loom_testkit::{
    FakeWorker, HookHarness, OwnedWriteRequest, RegistryProbe, assert_no_gap_lies, assert_write_rows,
};
use serde_json::json;

#[test]
fn registry_probe_passes() {
    let (reg, report) = RegistryProbe::assert_ok(&Echo);
    assert_eq!(report.commands, ["echo.send", "echo.read"]);
    assert_eq!(report.write_commands, ["echo.send"]);
    assert!(report.warnings.is_empty(), "{report}");
    assert_eq!(reg.workers()[0].kind, "echo");
}

#[test]
fn hooks_are_fast_and_classify() {
    let (reg, _) = RegistryProbe::assert_ok(&Echo);
    let h = HookHarness::new(reg.hooks_ref().cloned().unwrap());
    let req = OwnedWriteRequest::new("echo.send", "loop").args(json!({"text": "hi"}));
    let c = h.classify(&req);
    assert_eq!(c.value.risk, Risk::R1);
    assert!(!c.slow, "{:?}", c.elapsed);
    assert!(h.presend_check(&req, None).value.is_ok());
    let mut closed = req.clone();
    closed.target.generation = Some(0);
    assert!(h.presend_check(&closed, None).value.is_err());
}

#[test]
fn send_comes_back_as_rx() {
    let mut fw = FakeWorker::new(EchoWorker::default());
    fw.clock().advance_ns(1_000);
    let out = fw.write("q-1", json!({"text": "hello"}));
    assert_eq!(out.state, loom_proto::event::WriteState::Sent);
    let lanes: Vec<Lane> = fw.drain().iter().map(|(l, _)| *l).collect();
    assert_eq!(lanes, [Lane::Ctl, Lane::Data]);

    fw.write("q-2", json!({"text": "world"}));
    let evs = fw.drain_published();
    let rows: Vec<_> = evs.iter().filter(|e| e.kind == KIND_WRITE).cloned().collect();
    assert_write_rows(&rows);
    let rx = evs.iter().find(|e| e.kind == KIND_DATA).unwrap();
    assert_eq!(payload_bytes(&rx.payload).unwrap().unwrap(), b"world");
    assert_no_gap_lies(&evs);
    assert_eq!(fw.call("echo.status", json!({})).unwrap()["sent"], json!(2));
}

#[test]
fn stop_latch_refuses_agent_writes() {
    let mut fw = FakeWorker::new(EchoWorker::default());
    fw.latch(true);
    let out = fw.write("q-3", json!({"text": "x"}));
    assert_eq!(out.state, loom_proto::event::WriteState::Failed);
    assert!(fw.drain().is_empty(), "nothing transmitted while latched");
    fw.input("lease-1", b"user key", true).unwrap();
    assert_eq!(fw.drain().len(), 1, "user input still flows (A3)");
}

#[test]
fn cli_fragment() {
    let cli = CliOf::<EchoCli>::new();
    let domains: [&dyn DynDomainCli; 1] = [&cli];
    let run = |args: &[&str]| {
        let a: Vec<String> = std::iter::once("loom")
            .chain(args.iter().copied())
            .map(String::from)
            .collect();
        let (mut o, mut e) = (Vec::new(), Vec::new());
        (
            run_cli_with(&a, &domains, &mut o, &mut e),
            String::from_utf8(o).unwrap(),
        )
    };
    let (code, out) = run(&["echo", "send", "hi", "--json"]);
    assert_eq!(code, 1, "P0: E_NOT_IMPLEMENTED after argument checks");
    assert!(out.contains("echo.send"));
    assert_eq!(run(&["echo", "read", "--tail", "5000"]).0, 2, "bounded tail");
    assert_eq!(run(&["echo", "read", "--since", "-5s", "--json"]).0, 1);
}
