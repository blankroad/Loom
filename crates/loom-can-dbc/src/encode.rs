//! Signal values → frame (plan D17 base-value rule, fail closed).
//!
//! Rules:
//! - Base `Initial` (default): every active signal not given takes its DBC start value
//!   (`GenSigStartValue`, raw). A signal without one must be given (`MissingSignal`).
//!   Unused bits are 0.
//! - Base `Payload(bytes)` (`--base last`): start from the caller's bytes. Signals active in
//!   both the base and the result keep their base bits. Signals of a newly selected mux
//!   branch take start values (or must be given); bits of branches that became inactive are
//!   cleared.
//! - Physical → raw: `raw = round_half_away_from_zero((physical - offset) / factor)`.
//!   Float signals store `(physical - offset) / factor` without rounding.
//! - Range: a physical value outside DBC `[min|max]` is `OutOfRange` (tolerance
//!   1e-9 × max(1, |min|, |max|)); `[0|0]` means no range. `force_range` skips this
//!   check (the caller is responsible). Labels are not range checked (they are defined
//!   by the DBC, e.g. 0xFF = "SNA"). A raw value that does not fit the bit width is
//!   always an error.
//! - Mux: a given signal of an inactive branch is an error. A multiplexor not given is set
//!   automatically when the branch allows exactly one value.

use loom_can_frame::{CanFrame, fd_padded_len};

use crate::error::EncodeError;
use crate::model::{Message, Signal, ValueKind};

/// A value for one signal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SignalInput<'s> {
    /// Physical value (after factor/offset).
    Physical(f64),
    /// Raw integer (for float signals: the IEEE value before factor/offset).
    Raw(i128),
    /// `VAL_` label.
    Label(&'s str),
}

/// Where values of signals that are not given come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EncodeBase<'b> {
    /// DBC start values.
    #[default]
    Initial,
    /// A previous payload (exactly the DBC length), e.g. the last received frame.
    Payload(&'b [u8]),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncodeOptions {
    /// Skip the DBC min/max check (caller's responsibility, plan R4).
    pub force_range: bool,
    /// CAN FD bit rate switch for FD messages.
    pub brs: bool,
    /// CAN FD error state indicator for FD messages.
    pub esi: bool,
}

impl Default for EncodeOptions {
    fn default() -> Self {
        Self {
            force_range: false,
            brs: true,
            esi: false,
        }
    }
}

/// Where a signal's value in the encoded frame came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueSource {
    Given,
    Initial,
    Base,
    AutoMultiplexor,
}

/// Encoded frame plus a per-signal account (for dry-run / IG preview).
#[derive(Debug, Clone, PartialEq)]
pub struct Encoded {
    pub frame: CanFrame,
    /// Active signals in DBC order with the source of their value.
    pub sources: Vec<(u32, ValueSource)>,
    /// The DBC length was not a valid CAN FD length; zero bytes were appended.
    pub padded: bool,
}

/// Integer raw value → bits for an integer signal of `size` bits. `None` if it does not fit.
pub(crate) fn int_to_bits(kind: ValueKind, size: u8, raw: i128) -> Option<u64> {
    let size = size as u32;
    match kind {
        ValueKind::Unsigned => {
            let max = if size == 64 {
                u64::MAX as i128
            } else {
                (1i128 << size) - 1
            };
            (0..=max).contains(&raw).then_some(raw as u64)
        }
        ValueKind::Signed => {
            let min = -(1i128 << (size - 1));
            let max = (1i128 << (size - 1)) - 1;
            if !(min..=max).contains(&raw) {
                return None;
            }
            let mask = if size == 64 { u64::MAX } else { (1u64 << size) - 1 };
            Some((raw as i64 as u64) & mask)
        }
        ValueKind::Float32 | ValueKind::Float64 => None,
    }
}

fn check_range(sig: &Signal, value: f64) -> Result<(), EncodeError> {
    if let Some((lo, hi)) = sig.range() {
        let tol = 1e-9 * lo.abs().max(hi.abs()).max(1.0);
        if value < lo - tol || value > hi + tol {
            return Err(EncodeError::OutOfRange {
                signal: sig.name.clone(),
                value,
                min: lo,
                max: hi,
            });
        }
    }
    Ok(())
}

fn overflow(sig: &Signal, raw: i128) -> EncodeError {
    EncodeError::RawOverflow {
        signal: sig.name.clone(),
        raw,
        bits: sig.size(),
        signed: sig.kind == ValueKind::Signed,
    }
}

fn float_bits(sig: &Signal, x: f64) -> Result<u64, EncodeError> {
    let not = |detail: &str| EncodeError::NotRepresentable {
        signal: sig.name.clone(),
        detail: detail.into(),
    };
    match sig.kind {
        ValueKind::Float32 => {
            let f = x as f32;
            if f.is_finite() {
                Ok(f.to_bits() as u64)
            } else {
                Err(not("outside f32 range"))
            }
        }
        _ if x.is_finite() => Ok(x.to_bits()),
        _ => Err(not("not finite")),
    }
}

fn is_float(sig: &Signal) -> bool {
    matches!(sig.kind, ValueKind::Float32 | ValueKind::Float64)
}

fn input_to_bits(sig: &Signal, input: SignalInput<'_>, force_range: bool) -> Result<u64, EncodeError> {
    match input {
        SignalInput::Label(label) => {
            let raw = sig
                .raw_for_label(label)
                .ok_or_else(|| EncodeError::UnknownLabel {
                    signal: sig.name.clone(),
                    label: label.to_string(),
                })?;
            if is_float(sig) {
                float_bits(sig, raw as f64)
            } else {
                int_to_bits(sig.kind, sig.size(), raw as i128).ok_or_else(|| overflow(sig, raw as i128))
            }
        }
        SignalInput::Raw(raw) => {
            if !force_range {
                check_range(sig, raw as f64 * sig.factor + sig.offset)?;
            }
            if is_float(sig) {
                float_bits(sig, raw as f64)
            } else {
                int_to_bits(sig.kind, sig.size(), raw).ok_or_else(|| overflow(sig, raw))
            }
        }
        SignalInput::Physical(p) => {
            if !p.is_finite() {
                return Err(EncodeError::NotRepresentable {
                    signal: sig.name.clone(),
                    detail: "not finite".into(),
                });
            }
            if !force_range {
                check_range(sig, p)?;
            }
            let x = (p - sig.offset) / sig.factor;
            if is_float(sig) {
                return float_bits(sig, x);
            }
            let r = x.round();
            // i128 covers every 64-bit raw value; anything beyond is an overflow anyway.
            if !r.is_finite() || r.abs() > 1.7e38 {
                return Err(EncodeError::NotRepresentable {
                    signal: sig.name.clone(),
                    detail: format!("raw {x} out of range"),
                });
            }
            let raw = r as i128;
            int_to_bits(sig.kind, sig.size(), raw).ok_or_else(|| overflow(sig, raw))
        }
    }
}

impl Message {
    /// Encodes a frame. See the module docs for the rules. Errors leave nothing half-built.
    pub fn encode(
        &self,
        values: &[(&str, SignalInput<'_>)],
        base: EncodeBase<'_>,
        options: &EncodeOptions,
    ) -> Result<Encoded, EncodeError> {
        if let Some(reason) = &self.encode_blocked {
            return Err(EncodeError::Blocked {
                reason: reason.clone(),
            });
        }
        let n = self.signals.len();
        let size = self.size();
        let mut given: Vec<Option<(u64, ValueSource)>> = vec![None; n];

        for (name, input) in values {
            let (i, sig) = self
                .signal_by_name(name)
                .ok_or_else(|| EncodeError::UnknownSignal {
                    name: name.to_string(),
                })?;
            if given[i as usize].is_some() {
                return Err(EncodeError::DuplicateSignal {
                    name: name.to_string(),
                });
            }
            given[i as usize] = Some((
                input_to_bits(sig, *input, options.force_range)?,
                ValueSource::Given,
            ));
        }

        self.require_multiplexors(&mut given)?;

        let mut data = [0u8; 64];
        let base_bits: Option<Vec<Option<u64>>> = match base {
            EncodeBase::Initial => None,
            EncodeBase::Payload(bytes) => {
                if bytes.len() != size {
                    return Err(EncodeError::BaseLengthMismatch {
                        expected: size,
                        actual: bytes.len(),
                    });
                }
                data[..size].copy_from_slice(bytes);
                Some(self.active_bits(bytes))
            }
        };

        // Final values in mux order (multiplexors before their branches).
        let mut fin: Vec<Option<u64>> = vec![None; n];
        let mut src: Vec<Option<ValueSource>> = vec![None; n];
        for &i in &self.order {
            let i = i as usize;
            let sig = &self.signals[i];
            let active = match &sig.condition {
                None => true,
                Some(c) => fin[c.multiplexor as usize].is_some_and(|v| c.contains(v)),
            };
            if !active {
                if given[i].is_some_and(|(_, s)| s == ValueSource::Given) {
                    let c = sig.condition.as_ref().expect("inactive implies condition");
                    return Err(self.inactive_error(i, c.multiplexor as usize, &fin));
                }
                continue;
            }
            let (bits, source) = match (given[i], &base_bits) {
                (Some(v), _) => v,
                (None, Some(b)) if b[i].is_some() => (b[i].unwrap_or_default(), ValueSource::Base),
                _ => match sig.initial_bits {
                    Some(b) => (b, ValueSource::Initial),
                    None => {
                        return Err(EncodeError::MissingSignal {
                            name: sig.name.clone(),
                        });
                    }
                },
            };
            fin[i] = Some(bits);
            src[i] = Some(source);
        }

        let payload = &mut data[..size];
        if let Some(b) = &base_bits {
            for (i, sig) in self.signals.iter().enumerate() {
                if b[i].is_some() && fin[i].is_none() {
                    sig.layout().clear(payload);
                }
            }
        }
        for (sig, bits) in self.signals.iter().zip(&fin) {
            if let Some(bits) = bits {
                sig.layout().insert(payload, *bits);
            }
        }

        let (frame, padded) = if self.fd {
            let len = fd_padded_len(size).expect("DBC size is at most 64");
            (
                CanFrame::fd(self.id, &data[..len], options.brs, options.esi),
                len != size,
            )
        } else {
            (CanFrame::classic(self.id, &data[..size]), false)
        };
        let frame = frame.map_err(EncodeError::Frame)?;
        let sources = src
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.map(|s| (i as u32, s)))
            .collect();
        Ok(Encoded {
            frame,
            sources,
            padded,
        })
    }

    /// Makes sure every given signal's branch is selected, setting multiplexors that are
    /// not given when the branch allows one value. Walks up nested multiplexors.
    fn require_multiplexors(&self, given: &mut [Option<(u64, ValueSource)>]) -> Result<(), EncodeError> {
        let n = self.signals.len();
        let mut queued = vec![false; n];
        let mut work: Vec<usize> = Vec::with_capacity(n);
        for (i, g) in given.iter().enumerate() {
            if g.is_some() {
                queued[i] = true;
                work.push(i);
            }
        }
        while let Some(i) = work.pop() {
            let Some(c) = &self.signals[i].condition else {
                continue;
            };
            let m = c.multiplexor as usize;
            let mux = &self.signals[m];
            match given[m] {
                Some((v, _)) if c.contains(v) => {}
                Some((v, ValueSource::AutoMultiplexor)) => {
                    return Err(EncodeError::ConflictingMultiplexor {
                        multiplexor: mux.name.clone(),
                        first: v,
                        second: c.single_value().unwrap_or(c.ranges[0].0),
                    });
                }
                Some((v, _)) => {
                    return Err(EncodeError::InactiveMuxSignal {
                        signal: self.signals[i].name.clone(),
                        multiplexor: mux.name.clone(),
                        multiplexor_value: Some(v),
                    });
                }
                None => {
                    let v = c
                        .single_value()
                        .ok_or_else(|| EncodeError::AmbiguousMultiplexor {
                            signal: self.signals[i].name.clone(),
                            multiplexor: mux.name.clone(),
                        })?;
                    if v > mux.layout().mask() {
                        return Err(overflow(mux, v as i128));
                    }
                    given[m] = Some((v, ValueSource::AutoMultiplexor));
                }
            }
            if !queued[m] {
                queued[m] = true;
                work.push(m);
            }
        }
        Ok(())
    }

    /// Raw bits of the signals that are active in `data` (full-length payload).
    fn active_bits(&self, data: &[u8]) -> Vec<Option<u64>> {
        let mut out: Vec<Option<u64>> = vec![None; self.signals.len()];
        for &i in &self.order {
            let sig = &self.signals[i as usize];
            let active = match &sig.condition {
                None => true,
                Some(c) => out[c.multiplexor as usize].is_some_and(|v| c.contains(v)),
            };
            if active {
                out[i as usize] = sig.layout().extract(data);
            }
        }
        out
    }

    fn inactive_error(&self, i: usize, m: usize, fin: &[Option<u64>]) -> EncodeError {
        EncodeError::InactiveMuxSignal {
            signal: self.signals[i].name.clone(),
            multiplexor: self.signals[m].name.clone(),
            multiplexor_value: fin[m],
        }
    }
}
