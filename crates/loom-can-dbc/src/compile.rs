//! can-dbc AST → compiled IR. All lookups are built once here (hash maps), so compiling
//! is linear in the file size.

use std::collections::{HashMap, HashSet};
use std::mem::size_of;

use can_dbc::{
    AttributeDefinition, AttributeValue, AttributeValueType, Comment, Dbc, ExtendedMultiplex, MessageId,
    MultiplexIndicator, NumericValue, SignalExtendedValueType, ValDescription, ValueDescription, ValueType,
};
use loom_can_frame::CanId;

use crate::bits::{BitLayout, ByteOrder};
use crate::encode::int_to_bits;
use crate::error::{LimitKind, LoadError, WarningKind, Warnings};
use crate::load::{Header, check_limit};
use crate::model::{Database, InitialSource, Message, MuxCondition, Signal, SignalRef, ValueKind};
use crate::options::{Limits, TextEncoding};

/// Above this many signals in one message the pairwise overlap check is skipped (C2).
const OVERLAP_CHECK_MAX_SIGNALS: usize = 4096;
/// Attribute names hinting at E2E protection kept per message.
const MAX_E2E_ATTRS: usize = 8;

/// Effect of a dropped statement on one message.
pub(crate) struct Block {
    pub(crate) line: u32,
    pub(crate) reason: String,
    /// Also block decode (layout unknown), not only encode.
    pub(crate) decode: bool,
}

/// A `BO_` block that could not be parsed; kept as a blocked message so lookups explain why.
pub(crate) struct Placeholder {
    pub(crate) raw_id: u64,
    pub(crate) name: String,
    pub(crate) line: u32,
}

pub(crate) struct CompileInput<'a> {
    pub(crate) dbc: Dbc,
    pub(crate) headers: Vec<Header>,
    pub(crate) blocks: HashMap<MessageId, Vec<Block>>,
    pub(crate) placeholders: Vec<Placeholder>,
    pub(crate) limits: &'a Limits,
    pub(crate) sha256: [u8; 32],
    pub(crate) encoding: TextEncoding,
    pub(crate) encoding_guessed: bool,
    pub(crate) warnings: &'a mut Warnings,
}

pub(crate) fn compile(input: CompileInput<'_>) -> Result<Database, LoadError> {
    let CompileInput {
        dbc,
        headers,
        mut blocks,
        placeholders,
        limits,
        sha256,
        encoding,
        encoding_guessed,
        warnings,
    } = input;

    check_limit(
        LimitKind::Messages,
        dbc.messages.len() + placeholders.len(),
        limits.max_messages,
    )?;
    let total_signals: usize = dbc.messages.iter().map(|m| m.signals.len()).sum();
    check_limit(LimitKind::Signals, total_signals, limits.max_signals)?;

    let lk = Lookups::build(&dbc);
    let aligned = headers.len() == dbc.messages.len();
    let mut messages = Vec::with_capacity(dbc.messages.len() + placeholders.len());
    for (i, m) in dbc.messages.iter().enumerate() {
        let header = if aligned { headers.get(i) } else { None };
        let extra = blocks.remove(&m.id).unwrap_or_default();
        if let Some(msg) = compile_message(m, header, &lk, extra, limits, warnings)? {
            messages.push(msg);
        }
    }
    for p in placeholders {
        if let Some(msg) = placeholder_message(p, limits, warnings)? {
            messages.push(msg);
        }
    }

    let mut nodes = Vec::with_capacity(dbc.nodes.len());
    for n in &dbc.nodes {
        check_str(&n.0, limits)?;
        nodes.push(n.0.clone());
    }

    let mut db = Database {
        messages,
        by_id: HashMap::new(),
        ambiguous_ids: Vec::new(),
        by_name: HashMap::new(),
        signals_by_name: HashMap::new(),
        nodes,
        sha256,
        encoding,
        encoding_guessed,
        approx_bytes: 0,
    };
    build_indexes(&mut db, warnings);
    db.approx_bytes = estimate_bytes(&db);
    check_limit(LimitKind::IrBytes, db.approx_bytes, limits.max_ir_bytes)?;
    Ok(db)
}

fn check_str(s: &str, limits: &Limits) -> Result<(), LoadError> {
    check_limit(LimitKind::StringBytes, s.len(), limits.max_string_bytes)
}

type SigKey<'a> = (MessageId, &'a str);

/// One-time indexes over the AST.
struct Lookups<'a> {
    values: HashMap<SigKey<'a>, &'a [ValDescription]>,
    value_types: HashMap<SigKey<'a>, SignalExtendedValueType>,
    ext_mux: HashMap<SigKey<'a>, &'a ExtendedMultiplex>,
    ext_mux_messages: HashSet<MessageId>,
    cycle: HashMap<MessageId, &'a AttributeValue>,
    vframe: HashMap<MessageId, &'a AttributeValue>,
    start: HashMap<SigKey<'a>, &'a AttributeValue>,
    defaults: HashMap<&'a str, &'a AttributeValue>,
    vframe_enum: Option<&'a [String]>,
    e2e: HashMap<MessageId, Vec<String>>,
    msg_comment: HashMap<MessageId, &'a str>,
    sig_comment: HashMap<SigKey<'a>, &'a str>,
}

fn e2e_hint(attr: &str) -> bool {
    let a = attr.to_ascii_lowercase();
    ["e2e", "crc", "checksum", "chksum", "dataid", "alivecounter"]
        .iter()
        .any(|p| a.contains(p))
}

impl<'a> Lookups<'a> {
    fn build(dbc: &'a Dbc) -> Self {
        let mut lk = Lookups {
            values: HashMap::new(),
            value_types: HashMap::new(),
            ext_mux: HashMap::new(),
            ext_mux_messages: HashSet::new(),
            cycle: HashMap::new(),
            vframe: HashMap::new(),
            start: HashMap::new(),
            defaults: HashMap::new(),
            vframe_enum: None,
            e2e: HashMap::new(),
            msg_comment: HashMap::new(),
            sig_comment: HashMap::new(),
        };
        for v in &dbc.value_descriptions {
            if let ValueDescription::Signal {
                message_id,
                name,
                value_descriptions,
            } = v
            {
                lk.values
                    .entry((*message_id, name.as_str()))
                    .or_insert(value_descriptions.as_slice());
            }
        }
        for t in &dbc.signal_extended_value_type_list {
            lk.value_types
                .entry((t.message_id, t.signal_name.as_str()))
                .or_insert(t.signal_extended_value_type);
        }
        for x in &dbc.extended_multiplex {
            lk.ext_mux_messages.insert(x.message_id);
            lk.ext_mux
                .entry((x.message_id, x.signal_name.as_str()))
                .or_insert(x);
        }
        for a in &dbc.attribute_values_message {
            match a.name.as_str() {
                "GenMsgCycleTime" => {
                    lk.cycle.entry(a.message_id).or_insert(&a.value);
                }
                "VFrameFormat" => {
                    lk.vframe.entry(a.message_id).or_insert(&a.value);
                }
                _ => {}
            }
            if e2e_hint(&a.name) {
                push_e2e(&mut lk.e2e, a.message_id, &a.name);
            }
        }
        for a in &dbc.attribute_values_signal {
            if a.name == "GenSigStartValue" {
                lk.start
                    .entry((a.message_id, a.signal_name.as_str()))
                    .or_insert(&a.value);
            }
            if e2e_hint(&a.name) {
                push_e2e(&mut lk.e2e, a.message_id, &a.name);
            }
        }
        for d in &dbc.attribute_defaults {
            lk.defaults.entry(d.name.as_str()).or_insert(&d.value);
        }
        for d in &dbc.attribute_definitions {
            if let AttributeDefinition::Message(name, AttributeValueType::Enum(labels)) = d
                && name == "VFrameFormat"
            {
                lk.vframe_enum = Some(labels.as_slice());
            }
        }
        for c in &dbc.comments {
            match c {
                Comment::Message { id, comment } => {
                    lk.msg_comment.entry(*id).or_insert(comment.as_str());
                }
                Comment::Signal {
                    message_id,
                    name,
                    comment,
                } => {
                    lk.sig_comment
                        .entry((*message_id, name.as_str()))
                        .or_insert(comment.as_str());
                }
                _ => {}
            }
        }
        lk
    }
}

fn push_e2e(map: &mut HashMap<MessageId, Vec<String>>, id: MessageId, name: &str) {
    let v = map.entry(id).or_default();
    if v.len() < MAX_E2E_ATTRS && !v.iter().any(|n| n == name) {
        v.push(name.to_string());
    }
}

fn attr_number(v: &AttributeValue) -> Option<f64> {
    match v {
        AttributeValue::Uint(u) => Some(*u as f64),
        AttributeValue::Int(i) => Some(*i as f64),
        AttributeValue::Double(d) => Some(*d),
        AttributeValue::String(_) => None,
    }
}

fn numeric(v: NumericValue) -> f64 {
    match v {
        NumericValue::Uint(u) => u as f64,
        NumericValue::Int(i) => i as f64,
        NumericValue::Double(d) => d,
    }
}

/// First reason wins; later ones are only warnings.
#[derive(Default)]
struct Blocking {
    encode: Option<String>,
    decode: Option<String>,
}

impl Blocking {
    fn add(&mut self, reason: String, decode: bool) {
        if decode && self.decode.is_none() {
            self.decode = Some(reason.clone());
        }
        if self.encode.is_none() {
            self.encode = Some(reason);
        }
    }
}

fn message_id_to_can(id: MessageId, raw_text: Option<u64>) -> Result<CanId, String> {
    match id {
        MessageId::Extended(e) => CanId::extended(e).map_err(|e| e.to_string()),
        MessageId::Standard(s) => {
            // can-dbc keeps only 16 bits of a standard id; use the text when we have it.
            let raw = raw_text.unwrap_or(s as u64);
            u32::try_from(raw)
                .map_err(|_| format!("id {raw} too large"))
                .and_then(|r| CanId::standard(r).map_err(|e| e.to_string()))
        }
    }
}

fn compile_message(
    m: &can_dbc::Message,
    header: Option<&Header>,
    lk: &Lookups<'_>,
    extra_blocks: Vec<Block>,
    limits: &Limits,
    warnings: &mut Warnings,
) -> Result<Option<Message>, LoadError> {
    let line = header.map(|h| h.line);
    let raw_text = header.and_then(|h| h.raw_id);
    let pseudo_bits = raw_text.is_some_and(|r| r & 0x8000_0000 != 0 && r & 0x6000_0000 != 0);
    if m.name == "VECTOR__INDEPENDENT_SIG_MSG" || pseudo_bits {
        warnings.push(
            line,
            WarningKind::MessageSkipped,
            format!(
                "{}: pseudo message (signals without a frame), not a CAN frame",
                m.name
            ),
        );
        return Ok(None);
    }
    let id = match message_id_to_can(m.id, raw_text) {
        Ok(id) => id,
        Err(reason) => {
            warnings.push(
                line,
                WarningKind::MessageSkipped,
                format!("{}: invalid id ({reason})", m.name),
            );
            return Ok(None);
        }
    };
    check_str(&m.name, limits)?;
    let mut blocking = Blocking::default();
    for b in extra_blocks {
        warnings.push(
            Some(b.line),
            WarningKind::MessageBlocked,
            format!("{}: {}", m.name, b.reason),
        );
        blocking.add(b.reason, b.decode);
    }

    let size = if m.size > 64 {
        blocking.add(format!("length {} above 64 bytes", m.size), true);
        64u8
    } else {
        m.size as u8
    };

    let vframe_label: Option<String> = lk
        .vframe
        .get(&m.id)
        .copied()
        .or_else(|| lk.defaults.get("VFrameFormat").copied())
        .and_then(|v| match v {
            AttributeValue::String(s) => Some(s.clone()),
            other => {
                let idx = attr_number(other)? as usize;
                lk.vframe_enum.and_then(|labels| labels.get(idx)).cloned()
            }
        });
    let fd_attr = vframe_label
        .as_deref()
        .is_some_and(|l| l.to_ascii_lowercase().contains("fd"));
    if size > 8 && !fd_attr && vframe_label.is_some() {
        warnings.push(
            line,
            WarningKind::AttributeIgnored,
            format!(
                "{}: length {size} needs CAN FD but VFrameFormat is {:?}; treated as CAN FD",
                m.name,
                vframe_label.as_deref().unwrap_or("")
            ),
        );
    }
    let fd = fd_attr || size > 8;

    let cycle_time_ms = lk
        .cycle
        .get(&m.id)
        .copied()
        .or_else(|| lk.defaults.get("GenMsgCycleTime").copied())
        .and_then(attr_number)
        .filter(|v| v.is_finite() && *v >= 1.0 && *v <= u32::MAX as f64)
        .map(|v| v.round() as u32);

    let transmitter = match &m.transmitter {
        Some(t) => {
            check_str(t, limits)?;
            Some(t.clone())
        }
        None => None,
    };
    let comment = match lk.msg_comment.get(&m.id) {
        Some(c) => {
            check_str(c, limits)?;
            Some((*c).to_string())
        }
        None => None,
    };

    // Signals.
    let mut signals = Vec::with_capacity(m.signals.len());
    let mut indicators = Vec::with_capacity(m.signals.len());
    let mut signal_index: HashMap<Box<str>, u32> = HashMap::with_capacity(m.signals.len());
    for s in &m.signals {
        check_str(&s.name, limits)?;
        check_str(&s.unit, limits)?;
        if signal_index.contains_key(s.name.as_str()) {
            blocking.add(format!("signal name {} used twice", s.name), true);
            continue;
        }
        let order = match s.byte_order {
            can_dbc::ByteOrder::LittleEndian => ByteOrder::Intel,
            can_dbc::ByteOrder::BigEndian => ByteOrder::Motorola,
        };
        let Some(layout) = BitLayout::new(order, s.start_bit, s.size) else {
            blocking.add(
                format!(
                    "signal {} has an invalid bit layout ({}|{})",
                    s.name, s.start_bit, s.size
                ),
                true,
            );
            continue;
        };
        let kind = match lk.value_types.get(&(m.id, s.name.as_str())) {
            Some(SignalExtendedValueType::IEEEfloat32Bit) => ValueKind::Float32,
            Some(SignalExtendedValueType::IEEEdouble64bit) => ValueKind::Float64,
            _ => match s.value_type {
                ValueType::Signed => ValueKind::Signed,
                ValueType::Unsigned => ValueKind::Unsigned,
            },
        };
        let float_bits = match kind {
            ValueKind::Float32 => Some(32),
            ValueKind::Float64 => Some(64),
            _ => None,
        };
        if let Some(bits) = float_bits
            && s.size != bits
        {
            blocking.add(
                format!("float signal {} must be {bits} bits, is {}", s.name, s.size),
                true,
            );
        }
        if layout.required_len() > size as usize {
            blocking.add(
                format!(
                    "signal {} needs {} bytes, message has {size}",
                    s.name,
                    layout.required_len()
                ),
                false,
            );
        }
        if !s.factor.is_finite() || !s.offset.is_finite() {
            blocking.add(format!("signal {} has a non-finite factor/offset", s.name), true);
        } else if s.factor == 0.0 {
            blocking.add(format!("signal {} has factor 0", s.name), false);
        }

        let mut value_table: Vec<(i64, String)> = Vec::new();
        if let Some(vals) = lk.values.get(&(m.id, s.name.as_str())) {
            value_table.reserve(vals.len());
            for v in vals.iter() {
                check_str(&v.description, limits)?;
                value_table.push((v.id, v.description.clone()));
            }
            value_table.sort_by_key(|(v, _)| *v);
            value_table.dedup_by_key(|(v, _)| *v);
        }

        let (initial_bits, initial_source) = match lk
            .start
            .get(&(m.id, s.name.as_str()))
            .map(|v| (*v, InitialSource::Signal))
            .or_else(|| {
                lk.defaults
                    .get("GenSigStartValue")
                    .map(|v| (*v, InitialSource::Default))
            }) {
            Some((v, src)) => match initial_to_bits(kind, &layout, v) {
                Ok(bits) => (Some(bits), Some(src)),
                Err(why) => {
                    warnings.push(
                        line,
                        WarningKind::AttributeIgnored,
                        format!("{}.{}: GenSigStartValue ignored ({why})", m.name, s.name),
                    );
                    (None, None)
                }
            },
            None => (None, None),
        };

        let mut receivers = Vec::with_capacity(s.receivers.len());
        for r in &s.receivers {
            check_str(r, limits)?;
            receivers.push(r.clone());
        }
        let comment = match lk.sig_comment.get(&(m.id, s.name.as_str())) {
            Some(c) => {
                check_str(c, limits)?;
                Some((*c).to_string())
            }
            None => None,
        };

        signal_index.insert(s.name.as_str().into(), signals.len() as u32);
        indicators.push(s.multiplexer_indicator);
        signals.push(Signal {
            name: s.name.clone(),
            layout,
            kind,
            factor: s.factor,
            offset: s.offset,
            min: numeric(s.min),
            max: numeric(s.max),
            unit: s.unit.clone(),
            value_table,
            initial_bits,
            initial_source,
            is_multiplexor: matches!(
                s.multiplexer_indicator,
                MultiplexIndicator::Multiplexor | MultiplexIndicator::MultiplexorAndMultiplexedSignal(_)
            ),
            condition: None,
            receivers,
            comment,
        });
    }

    resolve_mux(m, &mut signals, &indicators, &signal_index, lk, &mut blocking);
    let depth = mux_depths(&signals, limits, &mut blocking, &m.name)?;
    if blocking.encode.is_none() {
        check_overlaps(&signals, &mut blocking, warnings, line, &m.name);
    }
    let mut order: Vec<u32> = (0..signals.len() as u32).collect();
    order.sort_by_key(|&i| depth[i as usize]);

    let mut e2e_attributes = lk.e2e.get(&m.id).cloned().unwrap_or_default();
    e2e_attributes.shrink_to_fit();
    if let Some(reason) = &blocking.encode {
        warnings.push(
            line,
            WarningKind::MessageBlocked,
            format!(
                "{}: {} blocked: {reason}",
                m.name,
                if blocking.decode.is_some() {
                    "encode and decode"
                } else {
                    "encode"
                }
            ),
        );
    }

    Ok(Some(Message {
        id,
        name: m.name.clone(),
        size,
        fd,
        transmitter,
        cycle_time_ms,
        comment,
        signals,
        signal_index,
        order,
        encode_blocked: blocking.encode,
        decode_blocked: blocking.decode,
        e2e_attributes,
        line,
    }))
}

fn initial_to_bits(kind: ValueKind, layout: &BitLayout, v: &AttributeValue) -> Result<u64, String> {
    match kind {
        ValueKind::Float32 => {
            let f = attr_number(v).ok_or("not a number")? as f32;
            if f.is_finite() {
                Ok(f.to_bits() as u64)
            } else {
                Err("does not fit f32".into())
            }
        }
        ValueKind::Float64 => Ok(attr_number(v).ok_or("not a number")?.to_bits()),
        ValueKind::Unsigned | ValueKind::Signed => {
            let raw: i128 = match v {
                AttributeValue::Uint(u) => *u as i128,
                AttributeValue::Int(i) => *i as i128,
                AttributeValue::Double(d) if d.is_finite() && d.fract() == 0.0 => *d as i128,
                AttributeValue::Double(d) => return Err(format!("{d} is not an integer")),
                AttributeValue::String(_) => return Err("string value".into()),
            };
            int_to_bits(kind, layout.size(), raw)
                .ok_or_else(|| format!("{raw} does not fit {} bits", layout.size()))
        }
    }
}

fn resolve_mux(
    m: &can_dbc::Message,
    signals: &mut [Signal],
    indicators: &[MultiplexIndicator],
    index: &HashMap<Box<str>, u32>,
    lk: &Lookups<'_>,
    blocking: &mut Blocking,
) {
    let has_ext = lk.ext_mux_messages.contains(&m.id);
    let simple: Vec<u32> = indicators
        .iter()
        .enumerate()
        .filter(|(_, ind)| matches!(ind, MultiplexIndicator::Multiplexor))
        .map(|(i, _)| i as u32)
        .collect();

    for i in 0..signals.len() {
        let name = signals[i].name.clone();
        let condition = if let Some(x) = lk.ext_mux.get(&(m.id, name.as_str())) {
            let Some(&mi) = index.get(x.multiplexor_signal_name.as_str()) else {
                blocking.add(
                    format!(
                        "SG_MUL_VAL_ for {name} names unknown multiplexor {}",
                        x.multiplexor_signal_name
                    ),
                    true,
                );
                continue;
            };
            if mi as usize == i || !signals[mi as usize].is_multiplexor {
                blocking.add(
                    format!(
                        "SG_MUL_VAL_ for {name}: {} is not a multiplexor (M / mxM)",
                        x.multiplexor_signal_name
                    ),
                    true,
                );
                continue;
            }
            let ranges: Vec<(u64, u64)> = x.mappings.iter().map(|r| (r.min_value, r.max_value)).collect();
            if ranges.is_empty() || ranges.iter().any(|(lo, hi)| lo > hi) {
                blocking.add(
                    format!("SG_MUL_VAL_ for {name} has an empty or reversed range"),
                    true,
                );
                continue;
            }
            Some(MuxCondition {
                multiplexor: mi,
                ranges,
            })
        } else {
            match indicators[i] {
                MultiplexIndicator::MultiplexedSignal(v)
                | MultiplexIndicator::MultiplexorAndMultiplexedSignal(v) => {
                    if has_ext {
                        blocking.add(
                            format!(
                                "multiplexed signal {name} has no SG_MUL_VAL_ in an extended-mux message"
                            ),
                            true,
                        );
                        continue;
                    }
                    match simple.as_slice() {
                        [mi] if *mi as usize != i => Some(MuxCondition {
                            multiplexor: *mi,
                            ranges: vec![(v, v)],
                        }),
                        _ => {
                            blocking.add(format!("no unique multiplexor for {name}"), true);
                            continue;
                        }
                    }
                }
                _ => None,
            }
        };
        if let Some(c) = &condition {
            let mux = &signals[c.multiplexor as usize];
            if matches!(mux.kind, ValueKind::Float32 | ValueKind::Float64) {
                blocking.add(format!("multiplexor {} is a float", mux.name), true);
                continue;
            }
        }
        signals[i].condition = condition;
    }
}

/// Number of mux hops to the root for each signal. A cycle blocks the message;
/// a chain deeper than the limit is a load error.
fn mux_depths(
    signals: &[Signal],
    limits: &Limits,
    blocking: &mut Blocking,
    msg_name: &str,
) -> Result<Vec<usize>, LoadError> {
    let mut depth = vec![0usize; signals.len()];
    for (i, d) in depth.iter_mut().enumerate() {
        let mut cur = i;
        let mut steps = 0usize;
        while let Some(c) = &signals[cur].condition {
            steps += 1;
            cur = c.multiplexor as usize;
            if steps > limits.max_mux_depth {
                if has_cycle(signals, i) {
                    // Decode and encode are blocked; the order no longer matters.
                    blocking.add(format!("multiplexer cycle in {msg_name}"), true);
                    return Ok(vec![0; signals.len()]);
                }
                return Err(LoadError::LimitExceeded {
                    what: LimitKind::MuxDepth,
                    limit: limits.max_mux_depth as u64,
                    actual: steps as u64,
                });
            }
        }
        *d = steps;
    }
    Ok(depth)
}

fn has_cycle(signals: &[Signal], start: usize) -> bool {
    let mut seen = vec![false; signals.len()];
    let mut cur = start;
    loop {
        if seen[cur] {
            return true;
        }
        seen[cur] = true;
        match &signals[cur].condition {
            Some(c) => cur = c.multiplexor as usize,
            None => return false,
        }
    }
}

/// Mux conditions from a signal up to the root.
fn chain(signals: &[Signal], i: usize) -> Vec<(u32, &[(u64, u64)])> {
    let mut out = Vec::new();
    let mut cur = i;
    while let Some(c) = &signals[cur].condition {
        out.push((c.multiplexor, c.ranges.as_slice()));
        cur = c.multiplexor as usize;
        if out.len() > signals.len() {
            break;
        }
    }
    out
}

fn disjoint(a: &[(u64, u64)], b: &[(u64, u64)]) -> bool {
    a.iter()
        .all(|&(alo, ahi)| b.iter().all(|&(blo, bhi)| ahi < blo || bhi < alo))
}

/// Two signals that can be active together must not share bits (encode would corrupt).
fn check_overlaps(
    signals: &[Signal],
    blocking: &mut Blocking,
    warnings: &mut Warnings,
    line: Option<u32>,
    msg_name: &str,
) {
    if signals.len() > OVERLAP_CHECK_MAX_SIGNALS {
        warnings.push(
            line,
            WarningKind::AttributeIgnored,
            format!(
                "{msg_name}: {} signals, overlap check skipped (limit {OVERLAP_CHECK_MAX_SIGNALS})",
                signals.len()
            ),
        );
        return;
    }
    let occ: Vec<[u64; 8]> = signals.iter().map(|s| s.layout().occupancy()).collect();
    let chains: Vec<_> = (0..signals.len()).map(|i| chain(signals, i)).collect();
    for i in 0..signals.len() {
        for j in i + 1..signals.len() {
            if !occ[i].iter().zip(&occ[j]).any(|(a, b)| a & b != 0) {
                continue;
            }
            let exclusive = chains[i]
                .iter()
                .any(|(mi, ri)| chains[j].iter().any(|(mj, rj)| mi == mj && disjoint(ri, rj)));
            if !exclusive {
                blocking.add(
                    format!("signals {} and {} overlap", signals[i].name, signals[j].name),
                    false,
                );
                return;
            }
        }
    }
}

fn placeholder_message(
    p: Placeholder,
    limits: &Limits,
    warnings: &mut Warnings,
) -> Result<Option<Message>, LoadError> {
    check_str(&p.name, limits)?;
    let id = if p.raw_id & 0x8000_0000 != 0 {
        if p.raw_id & 0x6000_0000 != 0 || p.raw_id > u32::MAX as u64 {
            return Ok(None);
        }
        CanId::extended((p.raw_id & 0x1FFF_FFFF) as u32).ok()
    } else {
        u32::try_from(p.raw_id).ok().and_then(|r| CanId::standard(r).ok())
    };
    let Some(id) = id else {
        warnings.push(
            Some(p.line),
            WarningKind::MessageSkipped,
            format!("{}: unparsable BO_ with invalid id {}", p.name, p.raw_id),
        );
        return Ok(None);
    };
    let reason = format!("BO_ block at line {} could not be parsed", p.line);
    Ok(Some(Message {
        id,
        name: p.name,
        size: 0,
        fd: false,
        transmitter: None,
        cycle_time_ms: None,
        comment: None,
        signals: Vec::new(),
        signal_index: HashMap::new(),
        order: Vec::new(),
        encode_blocked: Some(reason.clone()),
        decode_blocked: Some(reason),
        e2e_attributes: Vec::new(),
        line: Some(p.line),
    }))
}

fn build_indexes(db: &mut Database, warnings: &mut Warnings) {
    let mut ids: HashMap<CanId, Vec<u32>> = HashMap::with_capacity(db.messages.len());
    let mut names: HashMap<&str, Vec<u32>> = HashMap::with_capacity(db.messages.len());
    for (i, m) in db.messages.iter().enumerate() {
        ids.entry(m.id).or_default().push(i as u32);
        names.entry(m.name.as_str()).or_default().push(i as u32);
    }
    let mut by_name = HashMap::with_capacity(names.len());
    for (name, list) in names {
        if let [one] = list.as_slice() {
            by_name.insert(Box::<str>::from(name), *one);
        } else {
            warnings.push(
                db.messages[list[0] as usize].line,
                WarningKind::DuplicateMessageName,
                format!("{} messages named {name}; look them up by id", list.len()),
            );
        }
    }
    let mut by_id = HashMap::with_capacity(ids.len());
    let mut ambiguous = Vec::new();
    for (id, list) in ids {
        if let [one] = list.as_slice() {
            by_id.insert(id, *one);
            continue;
        }
        let names: Vec<&str> = list
            .iter()
            .map(|&i| db.messages[i as usize].name.as_str())
            .collect();
        let reason = format!("id {id} shared by {}", names.join(", "));
        warnings.push(
            db.messages[list[0] as usize].line,
            WarningKind::DuplicateMessageId,
            reason.clone(),
        );
        ambiguous.push(id);
        for &i in &list {
            let m = &mut db.messages[i as usize];
            m.encode_blocked.get_or_insert_with(|| reason.clone());
            m.decode_blocked.get_or_insert_with(|| reason.clone());
        }
    }
    ambiguous.sort();
    let mut signals_by_name: HashMap<Box<str>, Vec<SignalRef>> = HashMap::new();
    for (mi, m) in db.messages.iter().enumerate() {
        for (si, s) in m.signals.iter().enumerate() {
            signals_by_name
                .entry(s.name.as_str().into())
                .or_default()
                .push(SignalRef {
                    message: mi as u32,
                    signal: si as u32,
                });
        }
    }
    db.by_id = by_id;
    db.by_name = by_name;
    db.ambiguous_ids = ambiguous;
    db.signals_by_name = signals_by_name;
}

/// Rough heap estimate (strings, vectors, hash entries) for the IR limit.
fn estimate_bytes(db: &Database) -> usize {
    const ENTRY: usize = 48; // hash map entry + control overhead, rough
    let opt = |s: &Option<String>| s.as_ref().map_or(0, String::len);
    let mut total = db.messages.capacity() * size_of::<Message>();
    for m in &db.messages {
        total += m.name.len() + opt(&m.comment) + opt(&m.transmitter);
        total += opt(&m.encode_blocked) + opt(&m.decode_blocked);
        total += m.signals.capacity() * size_of::<Signal>();
        total += m.signal_index.len() * ENTRY + m.order.capacity() * 4;
        for s in &m.signals {
            total += 2 * s.name.len() + s.unit.len() + opt(&s.comment);
            total += s
                .value_table
                .iter()
                .map(|(_, l)| 8 + size_of::<String>() + l.len())
                .sum::<usize>();
            total += s
                .receivers
                .iter()
                .map(|r| size_of::<String>() + r.len())
                .sum::<usize>();
            total += s.condition.as_ref().map_or(0, |c| c.ranges.len() * 16);
        }
    }
    total += db.by_id.len() * ENTRY + db.by_name.len() * ENTRY;
    total += db
        .signals_by_name
        .iter()
        .map(|(k, v)| ENTRY + k.len() + v.capacity() * size_of::<SignalRef>())
        .sum::<usize>();
    total
}
