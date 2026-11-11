//! Domains compiled into `loomd` (C3: compile-time registration, no dylib). Each domain is behind a
//! cargo feature; `echo` (the sample domain) is on by default.
//!
//! A domain provides, per worker kind it registers (`WorkerSpec`):
//! * an **in-process constructor** (`inproc`) — used when `[workers] mode = "inproc"`;
//! * its worker piece for **`loomd worker <kind>`** ([`worker_main`]) — used in IPC mode (default),
//!   unless the domain ships a separate executable (`program`, e.g. `loom-eth-worker`).

use std::path::PathBuf;
use std::sync::Arc;

use loom_domain_api::{DOMAIN_API_VERSION, DomainModule, RegisterError, Registry, WorkerSpec};
use loom_worker_sdk::{Clock, EmitSink};

use crate::worker_host::WorkerHost;

/// In-process worker constructor: `(sink, clock, instance id) → host`.
pub type InprocFn = fn(Arc<dyn EmitSink>, Arc<dyn Clock>, &str) -> Arc<dyn WorkerHost>;

/// One worker kind a domain needs.
pub struct WorkerDecl {
    /// Spec registered by the domain.
    pub spec: WorkerSpec,
    /// In-process constructor.
    pub inproc: InprocFn,
    /// Separate worker executable; `None` = `loomd worker <kind>`.
    pub program: Option<PathBuf>,
}

impl std::fmt::Debug for WorkerDecl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkerDecl")
            .field("spec", &self.spec)
            .field("program", &self.program)
            .finish_non_exhaustive()
    }
}

/// A registered domain and the workers it declares.
pub struct Loaded {
    /// Registration.
    pub registry: Registry,
    /// Worker kinds.
    pub workers: Vec<WorkerDecl>,
}

impl std::fmt::Debug for Loaded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Loaded")
            .field("registry", &self.registry)
            .field("workers", &self.workers)
            .finish()
    }
}

/// Register a domain module (API version checked).
pub fn register<M: DomainModule>(m: &M) -> Result<Registry, RegisterError> {
    if M::API_VERSION != DOMAIN_API_VERSION {
        return Err(RegisterError::Invalid {
            what: "api_version",
            reason: format!(
                "domain {} built for API {}, loomd has {}",
                M::NAME,
                M::API_VERSION,
                DOMAIN_API_VERSION
            ),
        });
    }
    let mut r = if M::NAME == "test" {
        Registry::for_core("test")?
    } else {
        Registry::new(M::NAME)?
    };
    m.register(&mut r)?;
    Ok(r)
}

/// Pair each registered `WorkerSpec` with its constructor (by kind).
fn decls(registry: &Registry, table: &[(&str, InprocFn, Option<PathBuf>)]) -> Vec<WorkerDecl> {
    registry
        .workers()
        .iter()
        .filter_map(|spec| {
            let (_, f, p) = table.iter().find(|(k, ..)| *k == spec.kind)?;
            Some(WorkerDecl {
                spec: spec.clone(),
                inproc: *f,
                program: p.clone(),
            })
        })
        .collect()
}

/// Register `m` and pair its workers with constructors; log and skip it on failure.
#[allow(dead_code)]
fn load<M: DomainModule>(out: &mut Vec<Loaded>, m: &M, table: &[(&str, InprocFn, Option<PathBuf>)]) {
    match register(m) {
        Ok(registry) => {
            let workers = decls(&registry, table);
            for spec in registry.workers() {
                if !workers.iter().any(|d| d.spec.kind == spec.kind) {
                    tracing::warn!(
                        "domain {}: worker kind {} has no constructor in loomd",
                        M::NAME,
                        spec.kind
                    );
                }
            }
            out.push(Loaded { registry, workers });
        }
        Err(e) => tracing::error!("domain {} disabled: {e}", M::NAME),
    }
}

/// In-process constructor for any `Worker`.
macro_rules! inproc {
    ($name:ident, $domain:expr, $make:expr) => {
        fn $name(sink: Arc<dyn EmitSink>, clock: Arc<dyn Clock>, instance: &str) -> Arc<dyn WorkerHost> {
            crate::worker_host::InProcessHost::spawn($make, $domain, instance, sink, clock)
        }
    };
}

/// All built-in domains (cargo features). A domain that fails to register is skipped (logged).
pub fn builtin() -> Vec<Loaded> {
    #[allow(unused_mut)]
    let mut out = Vec::new();
    #[cfg(feature = "echo")]
    {
        inproc!(echo, loom_echo::DOMAIN, loom_echo::worker::EchoWorker::default());
        load(
            &mut out,
            &loom_echo::gateway::Echo,
            &[(loom_echo::WORKER_KIND, echo, None)],
        );
    }
    #[cfg(feature = "uart")]
    {
        inproc!(
            uart,
            loom_uart::DOMAIN,
            loom_uart_worker::worker::UartWorker::new()
        );
        load(
            &mut out,
            &loom_uart::Uart::new(),
            &[(loom_uart::WORKER_KIND, uart, None)],
        );
    }
    #[cfg(feature = "can")]
    {
        inproc!(can_sim, loom_can::DOMAIN, loom_can::worker::CanSimWorker::new());
        load(
            &mut out,
            &loom_can::gateway::Can,
            &[(loom_can::WORKER_KIND_SIM, can_sim, None)],
        );
    }
    #[cfg(feature = "uds")]
    load(&mut out, &loom_uds::UdsModule, &[]);
    #[cfg(feature = "eth")]
    {
        inproc!(eth, loom_eth::DOMAIN, loom_eth::worker::EthWorker::new());
        load(
            &mut out,
            &loom_eth::gateway::Eth,
            &[(loom_eth::WORKER_KIND, eth, None)],
        );
    }
    #[cfg(feature = "psu")]
    {
        inproc!(psu, loom_psu::DOMAIN, loom_psu::worker::PsuWorker::default());
        load(
            &mut out,
            &loom_psu::gateway::Psu::default(),
            &[(loom_psu::WORKER_KIND, psu, None)],
        );
    }
    #[cfg(feature = "test")]
    load(&mut out, &loom_test::TestModule::new(), &[]);
    out
}

/// `loomd worker <kind> …`: run the worker piece of a built-in domain (never returns for a known
/// kind; the SDK reads `--instance --producer-epoch --sock-dir …` itself). `false` = unknown kind.
pub fn worker_main(kind: &str) -> bool {
    match kind {
        #[cfg(feature = "echo")]
        loom_echo::WORKER_KIND => loom_worker_sdk::run_worker(loom_echo::worker::EchoWorker::default()),
        #[cfg(feature = "uart")]
        loom_uart::WORKER_KIND => loom_worker_sdk::run_worker(loom_uart_worker::worker::UartWorker::new()),
        #[cfg(feature = "can")]
        loom_can::WORKER_KIND_SIM => loom_worker_sdk::run_worker(loom_can::worker::CanSimWorker::new()),
        #[cfg(feature = "eth")]
        loom_eth::WORKER_KIND => loom_worker_sdk::run_worker(loom_eth::worker::EthWorker::new()),
        #[cfg(feature = "psu")]
        loom_psu::WORKER_KIND => loom_worker_sdk::run_worker(loom_psu::worker::PsuWorker::default()),
        _ => false,
    }
}
