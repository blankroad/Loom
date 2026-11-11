//! Incremental UTF-8 validator for the render sanitizer.
//!
//! Holds at most 3 bytes of an incomplete sequence between `feed` calls (plan §7.5).
//! Validity rules follow RFC 3629 / Unicode Table 3-7 (no overlongs, no surrogates,
//! nothing above U+10FFFF), so a split anywhere yields the same result.

/// Result of pushing one byte (≥ 0x80, or any byte while a sequence is pending).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Step {
    /// More continuation bytes are needed.
    NeedMore,
    /// A complete, valid scalar value: its code point and its encoded bytes.
    Char { cp: u32, bytes: [u8; 4], len: u8 },
    /// The pending sequence (or this lead byte) is invalid: emit one replacement.
    /// When `reprocess` is set the pushed byte was not consumed and must be handled
    /// again from the ground state (it may be ASCII, ESC or a new lead byte).
    Invalid { reprocess: bool },
}

/// Incremental decoder state (fixed size).
#[derive(Debug, Default, Clone, Copy)]
pub(super) struct Utf8 {
    buf: [u8; 4],
    len: u8,
    need: u8,
}

impl Utf8 {
    /// True while bytes of an incomplete sequence are held.
    pub(super) fn pending(&self) -> bool {
        self.len > 0
    }

    /// Number of held bytes (≤ 3).
    pub(super) fn held(&self) -> usize {
        usize::from(self.len)
    }

    /// Drop any held bytes (gap / generation reset).
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    /// Push one byte. With nothing pending, `b` must be ≥ 0x80.
    pub(super) fn push(&mut self, b: u8) -> Step {
        if self.len == 0 {
            let need = match b {
                0xC2..=0xDF => 1,
                0xE0..=0xEF => 2,
                0xF0..=0xF4 => 3,
                // 0x80..=0xC1 (stray continuation / overlong lead), 0xF5..=0xFF.
                _ => return Step::Invalid { reprocess: false },
            };
            self.buf[0] = b;
            self.len = 1;
            self.need = need;
            return Step::NeedMore;
        }
        let lead = self.buf[0];
        let ok = if self.len == 1 {
            match lead {
                0xE0 => (0xA0..=0xBF).contains(&b),
                0xED => (0x80..=0x9F).contains(&b),
                0xF0 => (0x90..=0xBF).contains(&b),
                0xF4 => (0x80..=0x8F).contains(&b),
                _ => (0x80..=0xBF).contains(&b),
            }
        } else {
            (0x80..=0xBF).contains(&b)
        };
        if !ok {
            self.reset();
            return Step::Invalid { reprocess: true };
        }
        self.buf[usize::from(self.len)] = b;
        self.len += 1;
        if self.len < self.need + 1 {
            return Step::NeedMore;
        }
        let len = self.len;
        let bytes = self.buf;
        let cp = match len {
            2 => (u32::from(bytes[0] & 0x1F) << 6) | u32::from(bytes[1] & 0x3F),
            3 => {
                (u32::from(bytes[0] & 0x0F) << 12)
                    | (u32::from(bytes[1] & 0x3F) << 6)
                    | u32::from(bytes[2] & 0x3F)
            }
            _ => {
                (u32::from(bytes[0] & 0x07) << 18)
                    | (u32::from(bytes[1] & 0x3F) << 12)
                    | (u32::from(bytes[2] & 0x3F) << 6)
                    | u32::from(bytes[3] & 0x3F)
            }
        };
        self.reset();
        Step::Char { cp, bytes, len }
    }
}
