//! Registration, hooks and mirror through the CORE test kit.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use loom_proto::event::Risk;
use loom_testkit::{HookHarness, OwnedWriteRequest, RegistryProbe};
use loom_uart::Uart;
use loom_uart::commands::{VERBS, available, help_json};
use serde_json::json;

#[test]
fn registry_probe_passes() {
    let (reg, report) = RegistryProbe::assert_ok(&Uart::new());
    assert_eq!(report.commands.len(), VERBS.len());
    assert!(report.write_commands.contains(&"uart.send".to_string()));
    assert_eq!(reg.workers()[0].kind, "uart");
    assert!(report.warnings.is_empty(), "{report}");
}

#[test]
fn hooks_classify_fast() {
    let (reg, _) = RegistryProbe::assert_ok(&Uart::new());
    let h = HookHarness::new(reg.hooks_ref().cloned().unwrap());
    let req = |text: &str| {
        OwnedWriteRequest::new("uart.send", "dut").args(json!({"channel": "dut", "data": hex::encode(text)}))
    };
    for (text, risk) in [
        ("ls\r", Risk::R1),
        ("reboot\r", Risk::R2),
        ("saveenv\r", Risk::R3),
        ("sf erase 0 100000\r", Risk::R4),
    ] {
        let c = h.classify(&req(text));
        assert_eq!(c.value.risk, risk, "{text}");
        assert!(!c.slow, "{text}: {:?}", c.elapsed);
    }
    let sig = OwnedWriteRequest::new("uart.signal", "dut").args(json!({"channel": "dut", "kind": "break"}));
    assert_eq!(h.classify(&sig).value.risk, Risk::R2);
    assert!(h.presend_check(&req("ls\r"), None).value.is_ok());
}

#[test]
fn help_lists_m1_verbs_as_available() {
    let h = help_json();
    let verbs = h["verbs"].as_array().unwrap();
    assert_eq!(verbs.len(), VERBS.len());
    let send = verbs.iter().find(|v| v["method"] == "uart.send").unwrap();
    assert_eq!(send["available"], true);
    let sig = verbs.iter().find(|v| v["method"] == "uart.signal").unwrap();
    assert_eq!(sig["available"], false);
    assert!(VERBS.iter().filter(|v| available(v)).count() >= 9);
}
