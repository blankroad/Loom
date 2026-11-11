//! UDS risk table as data (plan §10.3) and the byte classifier behind the `classify` hook.
//!
//! One table drives the hook, the registry risk table, `help --json` and UI labels. Rules are
//! matched in order; the first match wins. Under F1 every class runs automatically; the class is
//! still shown and recorded. Only services Loom does not implement are blocked
//! (`immutable_deny`: 0x2A, 0x86). Risk is decided from the bytes actually sent, never from the
//! environment (B4) and never lowered by a diagnostic DB.

use loom_domain_api::{RiskRow, RiskTable};
use loom_proto::event::Risk;
use loom_proto::policy::BlockClass;
use loom_uds_codec::{SPRMIB, service};
use serde::Serialize;

/// Generic request method (CLI `loom uds raw`, UI console line).
pub const METHOD_REQUEST: &str = "uds.request";

/// Which requests of a SID a rule covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Selector {
    /// Every request of the SID.
    Any,
    /// Sub-function (bit 7 cleared) is one of these.
    Subs(&'static [u8]),
    /// Sub-function is odd / even (SecurityAccess seed / key).
    Odd,
    Even,
    /// DID in bytes 1–2 within the inclusive range.
    DidRange(u16, u16),
    /// Routine id in bytes 2–3.
    Rid(u16),
    /// InputOutputControl parameter in byte 3.
    IoParam(u8),
}

impl Selector {
    fn matches(self, req: &[u8]) -> bool {
        let sub = req.get(1).map(|b| b & !SPRMIB);
        let u16_at = |i: usize| req.get(i..i + 2).map(|b| u16::from_be_bytes([b[0], b[1]]));
        match self {
            Selector::Any => true,
            Selector::Subs(list) => sub.is_some_and(|s| list.contains(&s)),
            Selector::Odd => sub.is_some_and(|s| s % 2 == 1),
            Selector::Even => sub.is_some_and(|s| s % 2 == 0),
            Selector::DidRange(lo, hi) => u16_at(1).is_some_and(|d| (lo..=hi).contains(&d)),
            Selector::Rid(rid) => u16_at(2) == Some(rid),
            Selector::IoParam(p) => req.get(3) == Some(&p),
        }
    }

    /// Text form used in the registry risk table and help.
    pub fn describe(self) -> String {
        let hexes = |l: &[u8]| l.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(",");
        match self {
            Selector::Any => "any".into(),
            Selector::Subs(l) => format!("sub={}", hexes(l)),
            Selector::Odd => "sub=odd".into(),
            Selector::Even => "sub=even".into(),
            Selector::DidRange(lo, hi) => format!("did={lo:04X}..{hi:04X}"),
            Selector::Rid(r) => format!("rid={r:04X}"),
            Selector::IoParam(p) => format!("io_param={p:02X}"),
        }
    }
}

/// One row of the UDS risk table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RiskRule {
    pub sid: u8,
    pub selector: Selector,
    pub risk: Risk,
    /// Typed RPC that sends this SID (besides [`METHOD_REQUEST`]).
    pub method: Option<&'static str>,
    pub block: Option<BlockClass>,
    pub reason: &'static str,
}

const fn rule(sid: u8, selector: Selector, risk: Risk, reason: &'static str) -> RiskRule {
    RiskRule {
        sid,
        selector,
        risk,
        method: None,
        block: None,
        reason,
    }
}

const fn typed(mut r: RiskRule, method: &'static str) -> RiskRule {
    r.method = Some(method);
    r
}

const fn immutable(mut r: RiskRule) -> RiskRule {
    r.block = Some(BlockClass::ImmutableDeny);
    r
}

use Risk::{R0, R1, R2, R3, R4};
use Selector::{Any, DidRange, Even, IoParam, Odd, Rid, Subs};

/// The table (plan §10.3, F1). First match wins.
pub const RULES: &[RiskRule] = &[
    typed(
        rule(
            0x10,
            Subs(&[0x01, 0x03]),
            R1,
            "DiagnosticSessionControl default/extended",
        ),
        "uds.session",
    ),
    typed(
        rule(
            0x10,
            Any,
            R3,
            "DiagnosticSessionControl programming/safety/OEM session",
        ),
        "uds.session",
    ),
    rule(0x11, Subs(&[0x01, 0x02, 0x03]), R2, "ECUReset hard/keyOffOn/soft"),
    rule(0x11, Any, R3, "ECUReset rapid power shutdown or other"),
    rule(0x14, Any, R2, "ClearDiagnosticInformation"),
    typed(rule(0x19, Any, R0, "ReadDTCInformation"), "uds.dtc.list"),
    typed(
        rule(
            0x22,
            DidRange(0xF200, 0xF3FF),
            R0,
            "ReadDataByIdentifier dynamic DID (memory read path, audited)",
        ),
        "uds.read_did",
    ),
    typed(rule(0x22, Any, R0, "ReadDataByIdentifier"), "uds.read_did"),
    rule(0x23, Any, R1, "ReadMemoryByAddress (may expose memory, audited)"),
    rule(0x24, Any, R0, "ReadScalingDataByIdentifier"),
    rule(0x27, Odd, R1, "SecurityAccess requestSeed"),
    rule(0x27, Even, R3, "SecurityAccess sendKey (no automatic retry)"),
    rule(0x28, Subs(&[0x00]), R1, "CommunicationControl enableRxAndTx"),
    rule(0x28, Any, R2, "CommunicationControl disable"),
    rule(0x29, Any, R3, "Authentication"),
    immutable(rule(
        0x2A,
        Any,
        R1,
        "ReadDataByPeriodicIdentifier: unsolicited responses not implemented",
    )),
    rule(0x2C, Any, R1, "DynamicallyDefineDataIdentifier"),
    typed(
        rule(
            0x2E,
            DidRange(0xF180, 0xF19E),
            R3,
            "WriteDataByIdentifier ISO identification DID",
        ),
        "uds.write_did",
    ),
    typed(rule(0x2E, Any, R2, "WriteDataByIdentifier"), "uds.write_did"),
    rule(0x2F, IoParam(0x00), R1, "InputOutputControl returnControlToECU"),
    rule(0x2F, Any, R3, "InputOutputControl (actuator)"),
    rule(0x31, Rid(0xFF00), R4, "RoutineControl eraseMemory"),
    rule(
        0x31,
        Rid(0xFF01),
        R1,
        "RoutineControl checkProgrammingDependencies",
    ),
    rule(0x31, Any, R3, "RoutineControl unregistered routine"),
    rule(0x34, Any, R4, "RequestDownload"),
    rule(0x35, Any, R4, "RequestUpload"),
    rule(0x36, Any, R4, "TransferData"),
    rule(0x37, Any, R4, "RequestTransferExit"),
    rule(0x38, Any, R4, "RequestFileTransfer"),
    rule(0x3D, Any, R4, "WriteMemoryByAddress"),
    rule(0x3E, Any, R0, "TesterPresent"),
    rule(0x83, Subs(&[0x01, 0x03]), R0, "AccessTimingParameter read"),
    rule(0x83, Any, R2, "AccessTimingParameter set"),
    rule(
        0x84,
        Any,
        R3,
        "SecuredDataTransmission (content not classifiable)",
    ),
    rule(0x85, Subs(&[0x01]), R1, "ControlDTCSetting on"),
    rule(0x85, Any, R2, "ControlDTCSetting off"),
    immutable(rule(
        0x86,
        Any,
        R2,
        "ResponseOnEvent: unsolicited responses not implemented",
    )),
    rule(0x87, Any, R3, "LinkControl"),
];

/// SIDs that keep their class when sent functionally.
const FUNCTIONAL_NO_RAISE: [u8; 3] = [0x3E, 0x22, 0x19];

/// Result of classifying one request PDU.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UdsClassification {
    pub risk: Risk,
    pub reason: String,
    pub block: Option<BlockClass>,
    pub sid: Option<u8>,
    pub service: Option<&'static str>,
    /// Sub-function with bit 7 cleared, for sub-function services.
    pub subfunction: Option<u8>,
    pub suppress_pos: bool,
}

fn raise(r: Risk) -> Risk {
    match r {
        R0 => R1,
        R1 => R2,
        R2 => R3,
        R3 | R4 => R4,
    }
}

/// Classify the bytes that will actually be sent. Never panics; O(table) with no allocation
/// beyond the reason string.
pub fn classify_pdu(req: &[u8], functional: bool) -> UdsClassification {
    let Some(&sid) = req.first() else {
        return UdsClassification {
            risk: R4,
            reason: "empty request".into(),
            block: None,
            sid: None,
            service: None,
            subfunction: None,
            suppress_pos: false,
        };
    };
    let info = service(sid);
    let has_sub = info.is_some_and(|i| i.has_subfunction);
    let subfunction = if has_sub {
        req.get(1).map(|b| b & !SPRMIB)
    } else {
        None
    };
    let suppress_pos = has_sub && req.get(1).is_some_and(|b| b & SPRMIB != 0);

    let (mut risk, reason, block) = match RULES.iter().find(|r| r.sid == sid && r.selector.matches(req)) {
        Some(r) => (r.risk, r.reason.to_string(), r.block),
        None if (0xBA..=0xBE).contains(&sid) => (R3, "system supplier specific service".to_string(), None),
        None if sid >= 0x40 => (R4, format!("0x{sid:02X} is not a request SID"), None),
        None => (R4, format!("reserved/undefined SID 0x{sid:02X}"), None),
    };
    let mut reason = format!("UDS 0x{sid:02X} {reason}");
    if functional && !FUNCTIONAL_NO_RAISE.contains(&sid) {
        risk = raise(risk);
        reason.push_str(" (functional: raised one class)");
    } else if functional {
        reason.push_str(" (functional)");
    }
    UdsClassification {
        risk,
        reason,
        block,
        sid: Some(sid),
        service: info.map(|i| i.name),
        subfunction,
        suppress_pos,
    }
}

/// Registry risk table generated from [`RULES`]: one row per rule for [`METHOD_REQUEST`] and one
/// for the typed method when the rule has one.
pub fn risk_table() -> RiskTable {
    let mut rows = Vec::with_capacity(RULES.len() * 2 + 2);
    for r in RULES {
        let selector = format!("sid={:02X} {}", r.sid, r.selector.describe());
        for method in std::iter::once(METHOD_REQUEST).chain(r.method) {
            rows.push(RiskRow {
                method: method.into(),
                selector: Some(selector.clone()),
                risk: r.risk,
                reason: r.reason.into(),
            });
        }
    }
    rows.push(RiskRow {
        method: METHOD_REQUEST.into(),
        selector: Some("sid=BA..BE".into()),
        risk: R3,
        reason: "system supplier specific service".into(),
    });
    rows.push(RiskRow {
        method: METHOD_REQUEST.into(),
        selector: Some("sid=other".into()),
        risk: R4,
        reason: "reserved/undefined SID or response SID".into(),
    });
    RiskTable {
        rows,
        default_blocked: Vec::new(),
        immutable_deny: RULES
            .iter()
            .filter(|r| r.block == Some(BlockClass::ImmutableDeny))
            .map(|r| format!("sid={:02X}", r.sid))
            .collect(),
    }
}
