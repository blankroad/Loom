//! `DomainModule` registration (contract 08 §8.1).

use std::sync::Arc;

use loom_domain_api::{
    Arch, Budgets, DOMAIN_API_VERSION, DomainModule, Instances, Owner, Platform, RegisterError, Registry,
    Reopen, RiskRow, RiskTable, WorkerSpec,
};
use loom_proto::event::{Risk, WriteState};

use crate::classify::BASE_RULES;
use crate::commands::{command_specs, help_spec};
use crate::hooks::UartHooks;
use crate::mirror::{Mirror, MirrorReducer};
use crate::{DOMAIN, MAX_PORTS, WORKER_KIND, kinds};

/// The UART domain module.
#[derive(Debug, Default)]
pub struct Uart {
    mirror: Mirror,
}

impl Uart {
    /// Module with a fresh channel mirror.
    pub fn new() -> Self {
        Uart::default()
    }

    /// The mirror shared by hooks and reducer (tests, `uart.status` fallbacks).
    pub fn mirror(&self) -> &Mirror {
        &self.mirror
    }
}

fn risk_table() -> RiskTable {
    let mut rows = vec![
        RiskRow {
            method: "uart.send".into(),
            selector: None,
            risk: Risk::R1,
            reason: "plain text / control characters".into(),
        },
        RiskRow {
            method: "uart.run".into(),
            selector: None,
            risk: Risk::R1,
            reason: "plain command line".into(),
        },
        RiskRow {
            method: "uart.input".into(),
            selector: None,
            risk: Risk::R1,
            reason: "user keystrokes (interactive)".into(),
        },
        RiskRow {
            method: "uart.input.clear".into(),
            selector: None,
            risk: Risk::R1,
            reason: "user line kill (^U on verified profiles)".into(),
        },
        RiskRow {
            method: "uart.open".into(),
            selector: None,
            risk: Risk::R1,
            reason: "open may toggle DTR/RTS".into(),
        },
        RiskRow {
            method: "uart.close".into(),
            selector: None,
            risk: Risk::R1,
            reason: "close may toggle DTR/RTS".into(),
        },
        RiskRow {
            method: "uart.signal".into(),
            selector: None,
            risk: Risk::R2,
            reason: "break / DTR / RTS".into(),
        },
    ];
    for r in BASE_RULES {
        for m in ["uart.send", "uart.run"] {
            rows.push(RiskRow {
                method: m.into(),
                selector: Some(r.id.into()),
                risk: r.risk,
                reason: format!("text rule {}", r.id),
            });
        }
    }
    // F1: nothing blocked by default; UART declares no immutable_deny (strings cannot identify
    // a service with certainty, plan §10.1).
    RiskTable {
        rows,
        ..RiskTable::default()
    }
}

impl DomainModule for Uart {
    const NAME: &'static str = DOMAIN;
    const API_VERSION: u32 = DOMAIN_API_VERSION;

    fn register(&self, reg: &mut Registry) -> Result<(), RegisterError> {
        crate::classify::warm();
        for c in command_specs() {
            reg.command(c)?;
        }
        for method in ["uart.open", "uart.close", "uart.send", "uart.run"] {
            reg.success_terminal(method, WriteState::Sent)?;
        }
        reg.input_events(Owner::Domain)?;
        for k in kinds::specs() {
            reg.event_kind(k)?;
        }
        reg.risk_table(risk_table())?
            .hooks(Arc::new(UartHooks::new(self.mirror.clone())))?
            .worker(WorkerSpec {
                kind: WORKER_KIND.into(),
                instances: Instances::PerChannel { max: MAX_PORTS },
                // plan §3.13: RSS target 48 MiB, hard 64 MiB; data-lane credit 1 MiB.
                budgets: Budgets {
                    mem_soft: 48 << 20,
                    mem_hard: 64 << 20,
                    credit: 1 << 20,
                },
                // Never reopen a port after a worker crash: DTR toggles can reset the board.
                reopen_after_crash: Reopen::None,
                arch: Arch::Native,
                platforms: vec![Platform::Windows, Platform::Linux, Platform::Macos],
            })?
            .reducer(Box::new(MirrorReducer(self.mirror.clone())))?
            .capability("sim")?
            .capability("pty")?
            .help(help_spec())?;
        Ok(())
    }
}
