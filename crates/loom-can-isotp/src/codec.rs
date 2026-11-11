use crate::{Error, IsoTpConfig, MAX_PDU};
use loom_can_frame::{CanFrame, fd_padded_len};

/// ISO-TP flow-control status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowStatus {
    Continue,
    Wait,
    Overflow,
}

/// Borrowed PCI view; SF padding is excluded, CF padding is trimmed by reassembly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pci<'a> {
    Single(&'a [u8]),
    First {
        length: usize,
        data: &'a [u8],
    },
    Consecutive {
        sequence: u8,
        data: &'a [u8],
    },
    FlowControl {
        status: FlowStatus,
        block_size: u8,
        st_min: u8,
    },
}

/// STmin in nanoseconds; reserved byte values use the standard 127ms fallback.
pub const fn st_min_ns(value: u8) -> u64 {
    match value {
        0..=0x7F => value as u64 * 1_000_000,
        0xF1..=0xF9 => (value as u64 - 0xF0) * 100_000,
        _ => 127_000_000,
    }
}

fn frame(cfg: &IsoTpConfig, bytes: &[u8]) -> Result<CanFrame, Error> {
    let mut data = [0xCC; 64];
    if bytes.len() > cfg.tx_dl as usize {
        return Err(Error::InvalidLength);
    }
    data[..bytes.len()].copy_from_slice(bytes);
    let len = if let Some(pad) = cfg.padding {
        data[bytes.len()..].fill(pad);
        cfg.tx_dl as usize
    } else if cfg.can_fd {
        fd_padded_len(bytes.len()).ok_or(Error::InvalidLength)?
    } else {
        bytes.len()
    };
    if cfg.can_fd {
        CanFrame::fd(cfg.tx_id, &data[..len], cfg.bitrate_switch, false)
    } else {
        CanFrame::classic(cfg.tx_id, &data[..len])
    }
    .map_err(|_| Error::InvalidLength)
}

/// Encode a single frame. Actual CAN_DL>8 always uses the FD escape header,
/// including a short PDU padded to TX_DL>8. FD length rounding uses CC when padding=None.
pub fn encode_sf(cfg: &IsoTpConfig, pdu: &[u8]) -> Result<CanFrame, Error> {
    cfg.validate()?;
    if pdu.is_empty() || pdu.len() > cfg.single_frame_capacity() {
        return Err(Error::InvalidLength);
    }
    if pdu.len() > cfg.max_pdu {
        return Err(Error::PduTooLarge {
            len: pdu.len(),
            max: cfg.max_pdu,
        });
    }
    let escape = cfg.can_fd && (pdu.len() > 7 || (cfg.padding.is_some() && cfg.tx_dl > 8));
    let offset = if escape { 2 } else { 1 };
    let mut data = [0; 64];
    if escape {
        data[1] = pdu.len() as u8;
    } else {
        data[0] = pdu.len() as u8;
    }
    data[offset..offset + pdu.len()].copy_from_slice(pdu);
    frame(cfg, &data[..offset + pdu.len()])
}

/// Encode a 12-bit FF and the prefix of the PDU that fits TX_DL.
pub fn encode_ff(cfg: &IsoTpConfig, pdu: &[u8]) -> Result<CanFrame, Error> {
    cfg.validate()?;
    if pdu.len() > cfg.max_pdu {
        return Err(Error::PduTooLarge {
            len: pdu.len(),
            max: cfg.max_pdu,
        });
    }
    if pdu.len() <= cfg.single_frame_capacity() {
        return Err(Error::InvalidLength);
    }
    let mut data = [0; 64];
    data[0] = 0x10 | ((pdu.len() >> 8) as u8);
    data[1] = pdu.len() as u8;
    let count = cfg.tx_dl as usize - 2;
    data[2..2 + count].copy_from_slice(&pdu[..count]);
    frame(cfg, &data[..2 + count])
}

/// Encode one CF. The caller owns sequence progression and remaining length.
pub fn encode_cf(cfg: &IsoTpConfig, sequence: u8, payload: &[u8]) -> Result<CanFrame, Error> {
    cfg.validate()?;
    if sequence > 15 || payload.is_empty() || payload.len() >= cfg.tx_dl as usize {
        return Err(Error::InvalidLength);
    }
    let mut data = [0; 64];
    data[0] = 0x20 | sequence;
    data[1..1 + payload.len()].copy_from_slice(payload);
    frame(cfg, &data[..1 + payload.len()])
}

/// Encode CTS, WAIT or OVFLW; reserved STmin bytes remain visible on the wire.
pub fn encode_fc(
    cfg: &IsoTpConfig,
    status: FlowStatus,
    block_size: u8,
    st_min: u8,
) -> Result<CanFrame, Error> {
    cfg.validate()?;
    frame(
        cfg,
        &[
            0x30 | match status {
                FlowStatus::Continue => 0,
                FlowStatus::Wait => 1,
                FlowStatus::Overflow => 2,
            },
            block_size,
            st_min,
        ],
    )
}

/// Validate PCI and lengths without allocating. Identifier/mode filtering belongs to Engine.
pub fn decode(frame: &CanFrame) -> Result<Pci<'_>, Error> {
    let data = frame.data();
    if frame.is_remote() || data.is_empty() {
        return Err(Error::InvalidLength);
    }
    match data[0] >> 4 {
        0 => {
            let nibble = (data[0] & 15) as usize;
            let (length, offset) = if frame.is_fd() && data.len() > 8 {
                if nibble != 0 || data.len() < 2 {
                    return Err(Error::InvalidLength);
                }
                (data[1] as usize, 2)
            } else {
                if nibble == 0 {
                    return Err(Error::InvalidLength);
                }
                (nibble, 1)
            };
            if length == 0 || length > 62 || (!frame.is_fd() && length > 7) || offset + length > data.len() {
                return Err(Error::InvalidLength);
            }
            Ok(Pci::Single(&data[offset..offset + length]))
        }
        1 => {
            if data.len() < 8 {
                return Err(Error::InvalidLength);
            }
            let length = (((data[0] & 15) as usize) << 8) | data[1] as usize;
            if length == 0 {
                let extended = u32::from_be_bytes([data[2], data[3], data[4], data[5]]) as usize;
                return if extended > MAX_PDU {
                    Err(Error::PduTooLarge {
                        len: extended,
                        max: MAX_PDU,
                    })
                } else {
                    Err(Error::InvalidLength)
                };
            }
            let sf_capacity = if frame.is_fd() && data.len() > 8 {
                data.len() - 2
            } else {
                7
            };
            if length <= sf_capacity {
                return Err(Error::InvalidLength);
            }
            Ok(Pci::First {
                length,
                data: &data[2..],
            })
        }
        2 => {
            if data.len() < 2 {
                return Err(Error::InvalidLength);
            }
            Ok(Pci::Consecutive {
                sequence: data[0] & 15,
                data: &data[1..],
            })
        }
        3 => {
            if data.len() < 3 {
                return Err(Error::InvalidLength);
            }
            let status = match data[0] & 15 {
                0 => FlowStatus::Continue,
                1 => FlowStatus::Wait,
                2 => FlowStatus::Overflow,
                _ => return Err(Error::InvalidFlowStatus),
            };
            Ok(Pci::FlowControl {
                status,
                block_size: data[1],
                st_min: data[2],
            })
        }
        _ => Err(Error::InvalidPci),
    }
}
