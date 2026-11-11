//! Bounds-checked big-endian reads. Every parser uses these instead of indexing (no panics).

pub(crate) fn u8_at(b: &[u8], off: usize) -> Option<u8> {
    b.get(off).copied()
}

pub(crate) fn be16(b: &[u8], off: usize) -> Option<u16> {
    let s = b.get(off..off.checked_add(2)?)?;
    Some(u16::from_be_bytes([s[0], s[1]]))
}

pub(crate) fn be24(b: &[u8], off: usize) -> Option<u32> {
    let s = b.get(off..off.checked_add(3)?)?;
    Some(u32::from_be_bytes([0, s[0], s[1], s[2]]))
}

pub(crate) fn be32(b: &[u8], off: usize) -> Option<u32> {
    let s = b.get(off..off.checked_add(4)?)?;
    Some(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

pub(crate) fn arr<const N: usize>(b: &[u8], off: usize) -> Option<[u8; N]> {
    b.get(off..off.checked_add(N)?)?.try_into().ok()
}

/// Space-separated uppercase hex of at most `max` bytes, `…` if longer.
pub(crate) fn hex_spaced(b: &[u8], max: usize) -> String {
    let mut s = String::new();
    for (i, x) in b.iter().take(max).enumerate() {
        if i > 0 {
            s.push(' ');
        }
        s.push_str(&format!("{x:02X}"));
    }
    if b.len() > max {
        s.push_str(" …");
    }
    s
}

/// Printable ASCII text (≥ 1 char, all printable) or None.
pub(crate) fn printable(b: &[u8]) -> Option<String> {
    if b.is_empty() || !b.iter().all(|c| (0x20..0x7f).contains(c)) {
        return None;
    }
    Some(b.iter().map(|&c| char::from(c)).collect())
}
