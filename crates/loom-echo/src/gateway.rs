//! Gateway piece: what `loomd` loads at start (compile-time registration, C3).

use std::sync::Arc;
use std::time::Duration;

use loom_domain_api::*;
use loom_proto::cursor::Cursor;
use loom_proto::event::{PublishedRecord, Risk, WriteState};
use loom_proto::policy::WriteClass;
use loom_proto::rpc::SubFilter;
use schemars::json_schema;
use serde_json::{Value, json};

use crate::{DOMAIN, KIND_DATA, KIND_WRITE, WORKER_KIND};

/// The domain module.
#[derive(Debug, Default)]
pub struct Echo;

/// `echo.send`: forward to the worker. The core pipeline (classify → policy → audit → presend)
/// has already run when this is called.
#[derive(Debug)]
struct Send;
impl Handler for Send {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            let rid = ctx.request_id().unwrap_or("echo-send").to_owned();
            ctx.worker(WORKER_KIND)?.write(&rid, params, ctx.deadline()).await
        })
    }
}

/// `echo.read`: bounded journal read (never unbounded, D5).
#[derive(Debug)]
struct Read;
impl Handler for Read {
    fn call<'a>(&'a self, ctx: &'a mut CallCtx, params: Value) -> BoxFuture<'a, Result<Value, DomainError>> {
        Box::pin(async move {
            let tail = params
                .get("tail")
                .and_then(Value::as_u64)
                .unwrap_or(100)
                .min(1000);
            let q = JournalQuery {
                from: Cursor::Last,
                until: None,
                filter: SubFilter {
                    kinds: vec![KIND_DATA.into()],
                    ..SubFilter::default()
                },
                request_id: None,
                max_events: u32::try_from(tail).unwrap_or(1000),
            };
            let r = ctx.journal().scan(&q, 64 * 1024).await?;
            let texts: Vec<Value> = r.events.iter().map(|e| e.payload["text"].clone()).collect();
            Ok(json!({"lines": texts, "complete": r.complete}))
        })
    }
}

/// Write hooks. DOMAIN: classify the bytes you actually send; keep it synchronous and < 1 ms.
#[derive(Debug)]
pub struct EchoHooks;
impl WriteHooks for EchoHooks {
    fn classify(&self, req: &WriteRequest<'_>) -> Classification {
        Classification {
            risk: Risk::R1,
            reason: format!("{} loopback write", req.method),
            block: None,
        }
    }
    fn presend_check(&self, req: &WriteRequest<'_>, _grant: Option<&Grant>) -> Result<(), PresendError> {
        // DOMAIN: re-check link generation, locks, quarantine … right before sending.
        match req.target.generation {
            Some(0) => Err(PresendError::EpochChanged("loopback not open".into())),
            _ => Ok(()),
        }
    }
}

/// Snapshot provider: last lines per channel for late-joining viewers.
#[derive(Debug, Default)]
pub struct LastLines(Vec<String>);
impl Reducer for LastLines {
    fn kinds(&self) -> &[&'static str] {
        &[KIND_DATA]
    }
    fn apply(&mut self, ev: &PublishedRecord) {
        if let Some(t) = ev.payload["text"].as_str() {
            if self.0.len() == 100 {
                self.0.remove(0);
            }
            self.0.push(t.to_owned());
        }
    }
    fn snapshot(&self, _filter: &SubFilter) -> Vec<Value> {
        vec![json!({"lines": self.0})]
    }
}

fn spec(method: &str, write: bool, handler: Arc<dyn Handler>) -> CommandSpec {
    CommandSpec {
        method: method.into(),
        write_class: write.then_some(WriteClass::Request),
        params: json_schema!({"type": "object"}),
        result: json_schema!({"type": "object"}),
        roles: if write { RoleSet::WRITE } else { RoleSet::READ },
        handler,
        bounds: Bounds {
            max_wait: Duration::from_secs(10),
            default_rows: 100,
            max_rows: 1000,
        },
        default_action: None, // F1: automatic; the user decides in loom.toml
    }
}

impl DomainModule for Echo {
    const NAME: &'static str = DOMAIN;
    const API_VERSION: u32 = DOMAIN_API_VERSION;

    fn register(&self, reg: &mut Registry) -> Result<(), RegisterError> {
        let text_ptr = PointerSpec {
            path: "/payload/text".into(),
            unit: None,
            ty: ValueType::String,
        };
        reg.command(spec("echo.send", true, Arc::new(Send)))?
            .command(spec("echo.read", false, Arc::new(Read)))?
            // DOMAIN: declare where a write is "done" (UART-like = sent, bus with ACK = confirmed).
            .success_terminal("echo.send", WriteState::Sent)?
            .event_kind(EventKindSpec {
                kind: KIND_DATA.into(),
                payload_schema: json_schema!({"type": "object", "properties": {"text": {"type": "string"}}}),
                pointers: vec![text_ptr],
                version: 1,
            })?
            .event_kind(EventKindSpec {
                kind: crate::KIND_LEASE_END.into(),
                payload_schema: json_schema!({"type": "object", "properties": {"lease_id": {"type": "string"}, "cause": {"type": "string"}}}),
                pointers: vec![PointerSpec {
                    path: "/payload/cause".into(),
                    unit: None,
                    ty: ValueType::Enum(vec!["conn_closed".into(), "released".into()]),
                }],
                version: 1,
            })?
            .event_kind(EventKindSpec {
                kind: KIND_WRITE.into(),
                payload_schema: json_schema!({"type": "object", "required": ["write_state"]}),
                pointers: vec![PointerSpec {
                    path: "/payload/write_state".into(),
                    unit: None,
                    ty: ValueType::Enum(vec![
                        "requested".into(),
                        "accepted".into(),
                        "sent".into(),
                        "failed".into(),
                    ]),
                }],
                version: 1,
            })?
            .risk_table(RiskTable {
                rows: vec![RiskRow {
                    method: "echo.send".into(),
                    selector: None,
                    risk: Risk::R1,
                    reason: "loopback".into(),
                }],
                ..RiskTable::default()
            })?
            .hooks(Arc::new(EchoHooks))?
            .worker(WorkerSpec {
                kind: WORKER_KIND.into(),
                instances: Instances::Shared,
                budgets: Budgets {
                    mem_soft: 16 << 20,
                    mem_hard: 32 << 20,
                    credit: 1 << 20,
                },
                reopen_after_crash: Reopen::None,
                arch: Arch::Native,
                platforms: vec![Platform::Windows, Platform::Linux, Platform::Macos],
            })?
            .reducer(Box::new(LastLines::default()))?
            .capability("loopback")?
            .help(HelpSpec {
                summary: "Simulated loopback device: sent text comes back as echo.data.".into(),
                examples: vec![
                    HelpExample {
                        cmd: "loom echo send hello".into(),
                        explain: "send text (R1)".into(),
                    },
                    HelpExample {
                        cmd: "loom echo read --tail 20".into(),
                        explain: "last 20 lines".into(),
                    },
                ],
                notes: vec!["Use only `loom echo …`; never open the device directly.".into()],
            })?;
        Ok(())
    }
}
