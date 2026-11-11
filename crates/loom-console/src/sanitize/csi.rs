//! CSI classification for the render sanitizer (allowlist, plan §5.4 · §5.5 ①).
//!
//! Input is one complete control sequence `ESC [ … final` (≤ [`super::MAX_SEQ_BYTES`]).
//! A passing sequence is copied verbatim; a DECSET/DECRST/SM/RM mode list is
//! rewritten to its allowed modes; a TUI DECSTBM is clamped. Everything else is
//! dropped and reported through `on_block`.

use super::{BlockKind, Profile, SanitizeConfig};

/// DEC private modes that pass (cursor keys, origin, autowrap, blink, cursor
/// visibility, alternate screens, save cursor, bracketed paste).
const ALLOWED_DEC_MODES: &[u32] = &[1, 6, 7, 12, 25, 47, 1047, 1048, 1049, 2004];
/// Mouse tracking / encoding modes (passed only with `allow_mouse`).
const MOUSE_MODES: &[u32] = &[9, 1000, 1001, 1002, 1003, 1005, 1006, 1015, 1016];
/// Focus reporting: the terminal would send `CSI I` / `CSI O` to the device.
const FOCUS_MODE: u32 = 1004;
/// Alternate screen modes tracked for the InputArbiter (plan §3.7-3).
const ALT_SCREEN_MODES: &[u32] = &[47, 1047, 1049];
/// Bracketed paste mode tracked for the paste confirmation (plan §5.3).
const BRACKETED_PASTE: u32 = 2004;
/// ANSI (non-private) modes that pass: IRM insert mode only.
const ALLOWED_ANSI_MODES: &[u32] = &[4];
/// Most mode parameters kept from one DECSET when rewriting (fixed scratch, C2).
const MAX_MODES: usize = 16;

/// Terminal modes the sanitizer mirrors from passed sequences.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct Modes {
    pub(super) alt_screen: bool,
    pub(super) bracketed_paste: bool,
}

/// Classify one CSI sequence; write what passes to `out`.
pub(super) fn filter(
    seq: &[u8],
    cfg: &SanitizeConfig,
    modes: &mut Modes,
    out: &mut Vec<u8>,
    on_block: &mut dyn FnMut(BlockKind),
) {
    // seq = ESC '[' body… final
    let Some((&fin, head)) = seq.split_last() else {
        on_block(BlockKind::Csi);
        return;
    };
    let body = head.get(2..).unwrap_or(&[]);
    let (prefix, rest) = match body.first() {
        Some(&p @ (b'<' | b'=' | b'>' | b'?')) => (Some(p), &body[1..]),
        _ => (None, body),
    };
    // Parameter bytes 0x30..=0x3F, then intermediates 0x20..=0x2F.
    let split = rest
        .iter()
        .position(|b| (0x20..=0x2F).contains(b))
        .unwrap_or(rest.len());
    let (params, inter) = rest.split_at(split);
    let malformed = params.iter().any(|b| matches!(b, b'<' | b'=' | b'>' | b'?'))
        || inter.iter().any(|b| !(0x20..=0x2F).contains(b));
    if malformed {
        on_block(BlockKind::Csi);
        return;
    }

    match (prefix, inter, fin) {
        // Replies the terminal would type back into the device (§5.5 ①).
        (_, _, b'c' | b'n' | b'x') => on_block(BlockKind::Query),
        (_, b"$", b'p' | b'w' | b'u') => on_block(BlockKind::Query),
        (None, b"*", b'y') => on_block(BlockKind::Query),
        (Some(b'?'), b"", b'u' | b'm' | b'S') => on_block(BlockKind::Query),
        (Some(b'>'), b"", b'q') => on_block(BlockKind::Query),
        (None, b"", b't') => on_block(BlockKind::WindowOp),

        (None, b"", b'm') => out.extend_from_slice(seq),
        (
            None,
            b"",
            b'A' | b'B' | b'C' | b'D' | b'E' | b'F' | b'G' | b'H' | b'f' | b'd' | b'e' | b'`' | b'a' | b'J'
            | b'K' | b'@' | b'P' | b'L' | b'M' | b'S' | b'X' | b'g' | b'I' | b'Z',
        ) => out.extend_from_slice(seq),
        // SD with one parameter; the 5-parameter form is mouse highlight tracking.
        (None, b"", b'T') if count_params(params) <= 1 => out.extend_from_slice(seq),
        // SCOSC / SCORC without parameters (with parameters they are DECSLRM / kitty).
        (None, b"", b's' | b'u') if params.is_empty() => out.extend_from_slice(seq),
        (None, b"", b'r') => decstbm(seq, params, cfg, out, on_block),
        (None, b"", b'h' | b'l') => modes_filter(
            seq,
            None,
            params,
            fin,
            ALLOWED_ANSI_MODES,
            cfg,
            modes,
            out,
            on_block,
        ),
        (Some(b'?'), b"", b'h' | b'l') => modes_filter(
            seq,
            Some(b'?'),
            params,
            fin,
            ALLOWED_DEC_MODES,
            cfg,
            modes,
            out,
            on_block,
        ),
        (Some(b'?'), b"", b's' | b'r') => on_block(BlockKind::Mode),
        // DECSCUSR cursor style (vim uses it).
        (None, b" ", b'q') => out.extend_from_slice(seq),
        _ => on_block(BlockKind::Csi),
    }
}

/// Number of `;`-separated parameters (an empty body counts as zero).
fn count_params(params: &[u8]) -> usize {
    if params.is_empty() {
        0
    } else {
        params.iter().filter(|&&b| b == b';').count() + 1
    }
}

/// Parse one parameter: digits only (subparameters or junk → `None`).
fn parse_num(p: &[u8]) -> Option<u32> {
    if p.is_empty() || p.len() > 9 || !p.iter().all(u8::is_ascii_digit) {
        return None;
    }
    p.iter().try_fold(0u32, |acc, d| {
        acc.checked_mul(10)?.checked_add(u32::from(d - b'0'))
    })
}

/// DECSTBM: Web passes verbatim; TUI clamps the bottom margin to `rows − 1` so the
/// status line stays outside the scroll region (plan §5.15.2).
fn decstbm(
    seq: &[u8],
    params: &[u8],
    cfg: &SanitizeConfig,
    out: &mut Vec<u8>,
    on_block: &mut dyn FnMut(BlockKind),
) {
    if cfg.profile == Profile::Web {
        out.extend_from_slice(seq);
        return;
    }
    let rows = u32::from(cfg.rows.max(2));
    let mut it = params.split(|&b| b == b';');
    let top_raw = it.next().unwrap_or(&[]);
    let bottom_raw = it.next().unwrap_or(&[]);
    if it.next().is_some() {
        on_block(BlockKind::Csi);
        return;
    }
    let top = if top_raw.is_empty() {
        Some(1)
    } else {
        parse_num(top_raw)
    };
    let bottom = if bottom_raw.is_empty() {
        Some(rows)
    } else {
        parse_num(bottom_raw)
    };
    let (Some(top), Some(bottom)) = (top, bottom) else {
        on_block(BlockKind::Csi);
        return;
    };
    let top = top.max(1);
    let bottom = bottom.min(rows - 1);
    if top >= bottom {
        on_block(BlockKind::Csi);
        return;
    }
    out.extend_from_slice(b"\x1b[");
    push_num(out, top);
    out.push(b';');
    push_num(out, bottom);
    out.push(b'r');
}

/// Mode set/reset: keep allowed modes, drop and report the rest, rewrite if mixed.
#[allow(clippy::too_many_arguments)]
fn modes_filter(
    seq: &[u8],
    prefix: Option<u8>,
    params: &[u8],
    fin: u8,
    allowed: &[u32],
    cfg: &SanitizeConfig,
    modes: &mut Modes,
    out: &mut Vec<u8>,
    on_block: &mut dyn FnMut(BlockKind),
) {
    let set = fin == b'h';
    let mut kept = [0u32; MAX_MODES];
    let mut n_kept = 0usize;
    let mut n_total = 0usize;
    for raw in params.split(|&b| b == b';') {
        n_total += 1;
        let Some(m) = parse_num(raw) else {
            on_block(BlockKind::Mode);
            continue;
        };
        let pass = if prefix.is_some() && MOUSE_MODES.contains(&m) {
            if cfg.allow_mouse {
                true
            } else {
                on_block(BlockKind::Mouse);
                false
            }
        } else if prefix.is_some() && m == FOCUS_MODE {
            on_block(BlockKind::Focus);
            false
        } else if allowed.contains(&m) {
            true
        } else {
            on_block(BlockKind::Mode);
            false
        };
        if pass {
            if n_kept == MAX_MODES {
                on_block(BlockKind::Mode);
                continue;
            }
            kept[n_kept] = m;
            n_kept += 1;
            if prefix.is_some() {
                if ALT_SCREEN_MODES.contains(&m) {
                    modes.alt_screen = set;
                } else if m == BRACKETED_PASTE {
                    modes.bracketed_paste = set;
                }
            }
        }
    }
    if n_kept == 0 {
        return;
    }
    if n_kept == n_total {
        out.extend_from_slice(seq);
        return;
    }
    out.extend_from_slice(b"\x1b[");
    if let Some(p) = prefix {
        out.push(p);
    }
    for (i, m) in kept[..n_kept].iter().enumerate() {
        if i > 0 {
            out.push(b';');
        }
        push_num(out, *m);
    }
    out.push(fin);
}

/// Append a decimal number without allocating.
fn push_num(out: &mut Vec<u8>, mut n: u32) {
    let mut digits = [0u8; 10];
    let mut i = digits.len();
    loop {
        i -= 1;
        // n % 10 < 10, fits in u8.
        digits[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    out.extend_from_slice(&digits[i..]);
}
