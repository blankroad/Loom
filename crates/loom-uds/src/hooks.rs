//! `WriteHooks` for UDS (contract 08 §8.2) and the pure dry-run.
//!
//! All synchronous, no I/O, well under 1 ms. Classification always uses the bytes that will be
//! sent: `req.bytes` when the core supplies them, otherwise the bytes encoded from the typed
//! params (same encoder the handlers use).

use std::collections::BTreeMap;
use std::sync::Arc;

use loom_domain_api::{Binding, Classification, Grant, PresendError, WriteHooks, WriteRequest};
use loom_proto::event::{Origin, PayloadBytes, Risk};
use loom_proto::policy::{BlockClass, PolicyAction};
use loom_uds_codec::fnv1a64;
use serde_json::{Value, json};

use crate::manager::PresendState;
use crate::payload::{DryRunResult, summarize};
use crate::risk::{UdsClassification, classify_pdu};
use crate::text::{encode_typed, parse_text};

/// Is the request functionally addressed? (`params.functional == true`.)
pub fn is_functional(args: &Value) -> bool {
    args.get("functional").and_then(Value::as_bool).unwrap_or(false)
}

/// Bytes the request will send, or why they cannot be built.
pub fn request_bytes(req: &WriteRequest<'_>) -> Result<Vec<u8>, String> {
    match req.bytes {
        Some(b) => Ok(b.to_vec()),
        None => encode_typed(req.method, req.args),
    }
}

/// Classification of a request whose bytes could not be built (the handler rejects it with
/// `E_USAGE`; the class only matters for display).
fn unencodable(why: &str) -> Classification {
    Classification {
        risk: Risk::R4,
        reason: format!("UDS request cannot be encoded: {why}"),
        block: None,
    }
}

/// UDS write hooks. P0: stateless. Quarantine/lease/current-value checks join `presend_check`
/// when the TargetSession registry is wired in (P1).
#[derive(Debug, Default)]
pub struct UdsHooks {
    presend: Option<Arc<PresendState>>,
}

impl UdsHooks {
    /// Hooks that see the live target state (lease, generation, quarantine).
    pub fn with_presend(presend: Arc<PresendState>) -> Self {
        UdsHooks {
            presend: Some(presend),
        }
    }
}

impl WriteHooks for UdsHooks {
    fn classify(&self, req: &WriteRequest<'_>) -> Classification {
        match request_bytes(req) {
            Ok(bytes) => {
                let c = classify_pdu(&bytes, is_functional(req.args));
                Classification {
                    risk: c.risk,
                    reason: c.reason,
                    block: c.block,
                }
            }
            Err(why) => unencodable(&why),
        }
    }

    fn approval_binding(&self, req: &WriteRequest<'_>) -> Binding {
        let bytes = request_bytes(req).unwrap_or_default();
        let c = classify_pdu(&bytes, is_functional(req.args));
        let mut digest_fields = BTreeMap::new();
        digest_fields.insert(
            "target".into(),
            json!(req.target.target.as_deref().or(req.target.channel.as_deref())),
        );
        digest_fields.insert("generation".into(), json!(req.target.generation));
        digest_fields.insert(
            "request_fnv1a64".into(),
            json!(format!("{:016x}", fnv1a64(&bytes))),
        );
        digest_fields.insert("functional".into(), json!(is_functional(req.args)));
        Binding {
            digest_fields,
            display: json!({
                "service": c.service,
                "req_hex": hex::encode(&bytes),
                "risk": c.risk,
                "reason": c.reason,
                "functional": is_functional(req.args),
            }),
        }
    }

    fn presend_check(&self, req: &WriteRequest<'_>, _grant: Option<&Grant>) -> Result<(), PresendError> {
        if req.target.generation == Some(0) {
            return Err(PresendError::EpochChanged(
                "target link was never connected".into(),
            ));
        }
        let Some(state) = &self.presend else { return Ok(()) };
        let name = req
            .args
            .get("target")
            .and_then(Value::as_str)
            .or(req.target.channel.as_deref());
        let Some(snap) = name.and_then(|n| state.get(n)) else {
            return Ok(());
        };
        if let Some(g) = req.target.generation
            && g != snap.generation
        {
            return Err(PresendError::EpochChanged(format!(
                "generation {g} → {}",
                snap.generation
            )));
        }
        let bytes = request_bytes(req).unwrap_or_default();
        let risk = classify_pdu(&bytes, is_functional(req.args)).risk;
        if snap.user_lease && req.origin != Origin::User && risk >= Risk::R1 {
            return Err(PresendError::Locked("target held by the user".into()));
        }
        if let Some(sid) = bytes.first()
            && snap.quarantined.contains(sid)
        {
            return Err(PresendError::Quarantine(format!(
                "SID 0x{sid:02X} may still answer late"
            )));
        }
        Ok(())
    }
}

fn action_for(c: &UdsClassification) -> PolicyAction {
    // F1: every class runs automatically unless the service is blocked.
    if c.block.is_some() {
        PolicyAction::Deny
    } else {
        PolicyAction::Auto
    }
}

/// `uds.request{text|bytes_hex, target, dry_run:true}` (ui-uds answer #1). Pure: no event,
/// row, audit or journal entry. `policy_action` here is the F1 domain default; the core policy
/// (user `loom.toml`) may be stricter when the gateway wires it in.
pub fn dry_run(target: &str, args: &Value) -> DryRunResult {
    let parsed = match args.get("bytes_hex").and_then(Value::as_str) {
        Some(_) => {
            encode_typed("uds.request", args).map_err(|m| crate::text::ParseError { at: 0, message: m })
        }
        None => parse_text(args.get("text").and_then(Value::as_str).unwrap_or("")),
    };
    match parsed {
        Ok(bytes) => {
            let c = classify_pdu(&bytes, is_functional(args));
            let policy_action = action_for(&c);
            DryRunResult {
                dry_run: true,
                target: target.into(),
                bytes: Some(PayloadBytes::hex(&bytes)),
                sid: c.sid,
                service: c.service.map(Into::into),
                subfunction: c.subfunction,
                did_name: did_name(&bytes),
                summary: Some(summarize(&bytes, &c)),
                suppress_pos: c.suppress_pos,
                risk: Some(c.risk),
                reason: c.reason,
                policy_action,
                block: c.block,
                parse_error: None,
            }
        }
        Err(e) => DryRunResult {
            dry_run: true,
            target: target.into(),
            bytes: None,
            sid: None,
            service: None,
            subfunction: None,
            did_name: None,
            summary: None,
            suppress_pos: false,
            risk: None,
            reason: "parse error".into(),
            policy_action: PolicyAction::Deny,
            block: None::<BlockClass>,
            parse_error: Some(e),
        },
    }
}

/// Name of the first DID of a 0x22/0x2E request, from the built-in ISO table.
/// (The per-target diagnostic DB joins in P1-b.)
fn did_name(bytes: &[u8]) -> Option<String> {
    match bytes {
        [0x22 | 0x2E, hi, lo, ..] => {
            loom_uds_codec::did::standard_did(u16::from_be_bytes([*hi, *lo])).map(|d| d.name)
        }
        _ => None,
    }
}
