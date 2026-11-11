//! Gateway piece: what `loomd` loads at start (compile-time registration, C3).
//!
//! Capture control and real sends go to the channel's worker. Reads (`query`, `show`, `stats`,
//! `follow`, `wait_for`, `export`) run here on the journal with `loom-eth-decode` and
//! `loom-eth-filter`, so the agent, the UI and TEST see one result for the same packets.

pub mod common;
pub mod evidence;
pub mod export;
pub mod handlers;
pub mod hooks;

use std::sync::Arc;
use std::time::Duration;

use loom_domain_api::*;
use loom_proto::event::Risk;
use loom_proto::policy::WriteClass;
use schemars::{Schema, json_schema};
use serde_json::{Value, json};

use self::common::{CONTRACT_MAX_ROWS, ChannelBook};
use self::evidence::{EthEvidence, EthReducer, EthSummarizer};
use self::handlers::*;
use self::hooks::EthHooks;
use crate::model::*;
use crate::{DOMAIN, MAX_CHANNELS, WORKER_KIND};

/// Default command wait (worker calls, scans).
pub const DEFAULT_WAIT: Duration = Duration::from_secs(30);
/// Worker soft memory (capture ring + decode); pressure shrinks screen mirrors.
pub const WORKER_MEM_SOFT: u64 = 256 << 20;
/// Worker hard memory.
pub const WORKER_MEM_HARD: u64 = 512 << 20;
/// Worker data-lane credit window.
pub const WORKER_CREDIT: u64 = 8 << 20;
/// Bytes a viewer panel keeps (rows + trees).
pub const PANEL_MEM_BYTES: u64 = 64 << 20;

/// The domain module.
#[derive(Debug, Default)]
pub struct Eth;

fn spec(
    method: &str,
    write: bool,
    handler: Arc<dyn Handler>,
    params: Schema,
    max_wait: Duration,
) -> CommandSpec {
    CommandSpec {
        method: method.into(),
        write_class: write.then_some(WriteClass::Request),
        params,
        result: json_schema!({"type": "object"}),
        roles: if write { RoleSet::WRITE } else { RoleSet::READ },
        handler,
        bounds: Bounds {
            max_wait,
            default_rows: QUERY_DEFAULT_ROWS,
            max_rows: CONTRACT_MAX_ROWS,
        },
        default_action: None, // F1: automatic; the user decides in loom.toml
    }
}

fn ptr(path: &str, ty: ValueType, unit: Option<&str>) -> PointerSpec {
    PointerSpec {
        path: path.into(),
        unit: unit.map(str::to_owned),
        ty,
    }
}

fn en(v: &[&str]) -> ValueType {
    ValueType::Enum(v.iter().map(|s| (*s).to_owned()).collect())
}

fn kind(k: &str, required: &[&str], pointers: Vec<PointerSpec>) -> EventKindSpec {
    EventKindSpec {
        kind: k.into(),
        payload_schema: json_schema!({"type": "object", "required": required}),
        pointers,
        version: 1,
    }
}

fn range_schema() -> Value {
    json!({"type": "object", "properties": {"from": {"type": "string"}, "to": {"type": "string"}}})
}

fn event_kinds() -> Vec<EventKindSpec> {
    let org = || en(&["agent", "user", "script"]);
    vec![
        kind(
            KIND_PKT,
            &[
                "ch", "linktype", "len", "caplen", "proto", "src", "dst", "info", "bytes",
            ],
            vec![
                ptr("/payload/proto", ValueType::String, None),
                ptr("/payload/info", ValueType::String, None),
                ptr("/payload/len", ValueType::U32, Some("B")),
                ptr("/payload/caplen", ValueType::U32, Some("B")),
                ptr("/payload/src", ValueType::String, None),
                ptr("/payload/dst", ValueType::String, None),
                ptr("/payload/dir", en(&["rx", "tx"]), None),
                ptr("/payload/stream", ValueType::U32, None),
                ptr("/payload/attributed_origin", org(), None),
                ptr("/payload/link/request_id", ValueType::String, None),
                ptr("/payload/link/domain", ValueType::String, None),
            ],
        ),
        kind(
            KIND_BATCH,
            &["ch", "total", "shown"],
            vec![
                ptr("/payload/total", ValueType::U64String, None),
                ptr("/payload/shown", ValueType::U64String, None),
                ptr("/payload/by_proto", ValueType::Object, None),
            ],
        ),
        kind(
            KIND_TX,
            &["ch", "write_state", "kind", "len"],
            vec![
                ptr(
                    "/payload/write_state",
                    en(&["requested", "accepted", "sent", "failed"]),
                    None,
                ),
                ptr("/payload/kind", en(&["raw", "udp", "tcp"]), None),
                ptr("/payload/to", ValueType::String, None),
                ptr("/payload/len", ValueType::U32, Some("B")),
                ptr("/payload/hex_preview", ValueType::Hex, None),
            ],
        ),
        kind(
            KIND_LINK,
            &["eth_ref", "link"],
            vec![
                ptr("/payload/eth_ref", ValueType::String, None),
                ptr("/payload/link/request_id", ValueType::String, None),
                ptr("/payload/link/role", ValueType::String, None),
            ],
        ),
        kind(
            KIND_CAPTURE_STATE,
            &["ch", "state", "backend", "iface"],
            vec![
                ptr(
                    "/payload/state",
                    en(&[
                        "starting",
                        "live",
                        "stopped",
                        "error",
                        "permission_denied",
                        "replay_done",
                    ]),
                    None,
                ),
                ptr("/payload/backend", en(&["live", "replay", "sim"]), None),
                ptr("/payload/iface", ValueType::String, None),
                ptr("/payload/error", ValueType::String, None),
            ],
        ),
        kind(
            KIND_STATS,
            &["ch", "pps", "bps"],
            vec![
                ptr("/payload/pps", ValueType::U64String, Some("1/s")),
                ptr("/payload/bps", ValueType::U64String, Some("B/s")),
                ptr("/payload/kernel_drop", ValueType::U64String, None),
                ptr("/payload/if_drop", ValueType::U64String, None),
                ptr("/payload/decode_lag_ms", ValueType::U64String, Some("ms")),
            ],
        ),
    ]
}

fn risk_table() -> RiskTable {
    let row = |m: &str, sel: Option<&str>, risk: Risk, reason: &str| RiskRow {
        method: m.into(),
        selector: sel.map(str::to_owned),
        risk,
        reason: reason.into(),
    };
    RiskTable {
        rows: vec![
            row("eth.capture.start", None, Risk::R0, "observation only; audited"),
            row("eth.capture.stop", None, Risk::R0, "observation only; audited"),
            row(
                "eth.send",
                Some("local"),
                Risk::R1,
                "sim, replay or loopback: stays on this host",
            ),
            row("eth.send", Some("nic"), Risk::R2, "frame on a real NIC"),
            row(
                "eth.send",
                Some("broadcast"),
                Risk::R3,
                "broadcast/multicast reaches every node",
            ),
            row(
                "eth.send",
                Some("doip_uds"),
                Risk::R4,
                "raw DoIP 0x8001: class of the inner UDS service (UDS plan §10.3)",
            ),
        ],
        ..RiskTable::default()
    }
}

/// UI panel manifests (UI `PanelManifest` format; filters are declarative JSON).
pub fn panel_manifests() -> Vec<Value> {
    let packets = json!({
        "id": "eth.packets", "domain": DOMAIN, "title": "Packets", "version": 1, "milestone": "M1",
        "topics": [
            {"key": "pkts", "filter": {"domains": [DOMAIN], "kinds": [KIND_PKT, KIND_BATCH, KIND_TX, KIND_LINK]},
             "encoding": "json", "snapshot": true, "maxBatchMs": 50, "mirrorRows": 5000},
            {"key": "state", "filter": {"domains": [DOMAIN], "kinds": [KIND_CAPTURE_STATE, KIND_STATS]},
             "encoding": "json", "snapshot": true, "coalesceBy": "/channel"},
            {"key": "gaps", "filter": {"kinds": ["source.gap", "record.gap"]}, "encoding": "json", "snapshot": false}
        ],
        "queries": ["eth.query", "eth.show", "eth.filter.check", "eth.capture.status", "eth.ifaces"],
        "actions": [
            {"id": "capture.start", "rpc": "eth.capture.start", "class": "request", "right": "operate", "confirm": "none", "label": "Start capture"},
            {"id": "capture.stop", "rpc": "eth.capture.stop", "class": "request", "right": "operate", "confirm": "none",
             "safetyDirection": "reduce", "label": "Stop capture"},
            {"id": "send", "rpc": "eth.send", "class": "request", "right": "operate", "confirm": "card", "label": "Send / resend"},
            {"id": "export", "rpc": "eth.export", "class": "request", "right": "watch", "confirm": "none", "label": "Export pcapng"}
        ],
        "memory": {"maxBytes": PANEL_MEM_BYTES},
        "render": {"maxHz": 30, "budgetMs": 8, "lagTargetMs": 200}
    });
    let stream = json!({
        "id": "eth.stream", "domain": DOMAIN, "title": "Follow stream", "version": 1, "milestone": "M2",
        "topics": [], "queries": ["eth.follow"], "actions": [],
        "memory": {"maxBytes": u64::from(FOLLOW_MAX_BYTES) * 4}, "render": {"maxHz": 4, "budgetMs": 8}
    });
    let stats = json!({
        "id": "eth.stats", "domain": DOMAIN, "title": "Capture statistics", "version": 1, "milestone": "M2",
        "topics": [{"key": "stats", "filter": {"domains": [DOMAIN], "kinds": [KIND_STATS]}, "encoding": "json",
                    "snapshot": true, "coalesceBy": "/channel"}],
        "queries": ["eth.stats"], "actions": [],
        "memory": {"maxBytes": 4 << 20}, "render": {"maxHz": 2, "budgetMs": 4}
    });
    vec![packets, stream, stats]
}

/// Domain help (`loom eth help --json` is generated from it).
pub fn help() -> HelpSpec {
    let ex = |cmd: &str, explain: &str| HelpExample {
        cmd: cmd.into(),
        explain: explain.into(),
    };
    HelpSpec {
        summary: "Ethernet capture and decode (tshark for the agent, Wireshark for the user): capture on a NIC, \
                  replay or sim, list/filter/inspect packets (DoIP, UDS over DoIP, SOME/IP), wait for packets \
                  as evidence, send frames."
            .into(),
        examples: vec![
            ex("loom eth ifaces", "capture interfaces"),
            ex("loom eth capture start en5 --ch dut --bpf 'tcp port 13400'", "start capture (BPF = capture filter)"),
            ex("loom eth trace -Y 'doip && uds.sid == 0x22' --since -10s", "packets of the last 10 s matching a display filter"),
            ex("loom eth show eth:dut@seq:1234 --hex", "layer tree + hex of one packet"),
            ex("loom eth wait-for -Y 'uds.sid == 0x62' --timeout 5s", "wait for a packet (exit 3 on timeout)"),
            ex("loom eth send udp --to 192.168.0.10:13400 0200FFFF... --dry-run", "preview decode and risk; nothing is sent"),
            ex("loom eth filter check 'tcp.port == 13400'", "check a display filter"),
        ],
        notes: vec![
            "-Y is a Wireshark display-filter subset (see filter check); --bpf is a libpcap capture filter. They are different languages.".into(),
            "`a != b` means `!(a == b)`: no value equals b, and true when the field is absent.".into(),
            format!("Outputs are capped: {QUERY_DEFAULT_ROWS} rows by default, {CONTRACT_MAX_ROWS} max, follow {} KiB; continue with --since <next_cursor>.", FOLLOW_MAX_BYTES / 1024),
            "Wait with `wait-for`, never sleep. Copy REFs (eth:<ch>@seq:<n>) from results as evidence.".into(),
            "Packets Loom sent itself (org column) are never evidence of device behaviour.".into(),
        ],
    }
}

impl DomainModule for Eth {
    const NAME: &'static str = DOMAIN;
    const API_VERSION: u32 = DOMAIN_API_VERSION;

    fn register(&self, reg: &mut Registry) -> Result<(), RegisterError> {
        let book = ChannelBook::default();
        let obj = |props: Value, required: &[&str]| {
            Schema::try_from(json!({"type": "object", "properties": props, "required": required}))
                .unwrap_or_else(|_| json_schema!({"type": "object"}))
        };
        let ch = json!({"type": "string"});
        let commands = vec![
            spec(
                "eth.ifaces",
                false,
                Arc::new(Forward("eth.ifaces")),
                obj(json!({}), &[]),
                DEFAULT_WAIT,
            ),
            spec(
                "eth.capture.start",
                true,
                Arc::new(Forward("eth.capture.start")),
                obj(
                    json!({"iface": {"type": "string"}, "ch": ch, "backend": {"enum": ["live", "replay", "sim"]},
                           "bpf": {"type": "string"}, "snaplen": {"type": "integer"}, "promisc": {"type": "boolean"},
                           "buffer_mib": {"type": "integer"}, "replay": {"type": "object"}}),
                    &["iface"],
                ),
                DEFAULT_WAIT,
            ),
            spec(
                "eth.capture.stop",
                true,
                Arc::new(Forward("eth.capture.stop")),
                obj(json!({"ch": ch}), &[]),
                DEFAULT_WAIT,
            ),
            spec(
                "eth.capture.status",
                false,
                Arc::new(Status(book.clone())),
                obj(json!({"ch": ch}), &[]),
                DEFAULT_WAIT,
            ),
            spec(
                "eth.query",
                false,
                Arc::new(Query(book.clone())),
                obj(
                    json!({"ch": ch, "range": range_schema(), "filter": {"type": "string"}, "max": {"type": "integer"},
                           "detail": {"type": "boolean"}, "wait_ms": {"type": "integer"}}),
                    &[],
                ),
                FOLLOW_WAIT_MAX,
            ),
            spec(
                "eth.show",
                false,
                Arc::new(Show),
                obj(json!({"ref": {"type": "string"}}), &["ref"]),
                DEFAULT_WAIT,
            ),
            spec(
                "eth.wait_for",
                false,
                Arc::new(WaitFor(book.clone())),
                obj(
                    json!({"ch": ch, "filter": {"type": "string"}, "count": {"type": "object"}, "within_ms": {"type": "integer"},
                           "anchor": {"type": "string"}, "timeout_ms": {"type": "integer"}, "include_own": {"type": "boolean"}}),
                    &["filter"],
                ),
                WAIT_MAX,
            ),
            spec(
                "eth.stats",
                false,
                Arc::new(Stats(book.clone())),
                obj(
                    json!({"ch": ch, "by": {"enum": ["proto", "conv", "endpoint", "io"]}, "filter": {"type": "string"}, "range": range_schema()}),
                    &[],
                ),
                DEFAULT_WAIT,
            ),
            spec(
                "eth.follow",
                false,
                Arc::new(Follow(book.clone())),
                obj(
                    json!({"ch": ch, "proto": {"enum": ["tcp", "udp"]}, "stream": {"type": "integer"},
                           "max_bytes": {"type": "integer"}, "range": range_schema()}),
                    &["stream"],
                ),
                DEFAULT_WAIT,
            ),
            spec(
                "eth.send",
                true,
                Arc::new(Send(book.clone())),
                obj(
                    json!({"ch": ch, "kind": {"enum": ["raw", "udp", "tcp"]}, "hex": {"type": "string"}, "to": {"type": "string"},
                           "from_port": {"type": "integer"}, "keep_open": {"type": "boolean"}, "dry_run": {"type": "boolean"}}),
                    &["kind", "hex"],
                ),
                DEFAULT_WAIT,
            ),
            spec(
                "eth.export",
                false,
                Arc::new(Export(book.clone())),
                obj(
                    json!({"ch": ch, "format": {"enum": ["pcapng"]}, "filter": {"type": "string"}, "range": range_schema(), "path": {"type": "string"}}),
                    &[],
                ),
                Duration::from_secs(600),
            ),
            spec(
                "eth.filter.check",
                false,
                Arc::new(FilterCheck),
                obj(json!({"expr": {"type": "string"}}), &["expr"]),
                DEFAULT_WAIT,
            ),
        ];
        for c in commands {
            reg.command(c)?;
        }
        for k in event_kinds() {
            reg.event_kind(k)?;
        }
        reg.risk_table(risk_table())?
            .hooks(Arc::new(EthHooks { book: book.clone() }))?
            .worker(WorkerSpec {
                kind: WORKER_KIND.into(),
                instances: Instances::PerChannel {
                    max: u16::try_from(MAX_CHANNELS).unwrap_or(4),
                },
                budgets: Budgets {
                    mem_soft: WORKER_MEM_SOFT,
                    mem_hard: WORKER_MEM_HARD,
                    credit: WORKER_CREDIT,
                },
                reopen_after_crash: Reopen::None,
                arch: Arch::Native,
                platforms: vec![Platform::Windows, Platform::Linux, Platform::Macos],
            })?
            .reducer(Box::new(EthReducer::new(book)))?
            .evidence_verifier(Arc::new(EthEvidence))?
            .display_summarizer(Arc::new(EthSummarizer))?
            .capability("capture")?
            .capability("decode:doip,uds,someip")?
            .capability("filter:wireshark-subset")?;
        for p in panel_manifests() {
            reg.panel_manifest(p)?;
        }
        reg.help(help())?;
        Ok(())
    }
}
