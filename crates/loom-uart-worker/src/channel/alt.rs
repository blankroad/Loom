//! Alternate-screen detector for the input arbiter (UART plan §3.7 rule 3: no automatic input
//! return while a full-screen program runs).
//!
//! A tiny byte state machine that only recognises private-mode `CSI ? Pn ; … h|l` with mode 47,
//! 1047 or 1049. State survives chunk boundaries; the parameter buffer is fixed-size.

/// Longest CSI parameter string inspected; longer sequences are ignored (§3.13 bounded state).
pub(crate) const ALT_PARAM_BYTES: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum St {
    Ground,
    Esc,
    Csi,
}

/// Tracks `?47` / `?1047` / `?1049` set/reset in RX bytes.
#[derive(Debug, Clone)]
pub(crate) struct AltScreen {
    st: St,
    private: bool,
    first: bool,
    params: [u8; ALT_PARAM_BYTES],
    len: usize,
    overflow: bool,
    on: bool,
}

impl AltScreen {
    pub(crate) fn new() -> Self {
        Self {
            st: St::Ground,
            private: false,
            first: false,
            params: [0; ALT_PARAM_BYTES],
            len: 0,
            overflow: false,
            on: false,
        }
    }

    /// Current state.
    pub(crate) fn on(&self) -> bool {
        self.on
    }

    /// Forget a half-parsed sequence (gap / generation boundary). The screen state is kept.
    pub(crate) fn reset_parser(&mut self) {
        self.st = St::Ground;
    }

    /// Feed RX bytes; `Some(state)` when an alternate-screen set/reset was seen in them.
    pub(crate) fn feed(&mut self, bytes: &[u8]) -> Option<bool> {
        let mut seen = false;
        for &b in bytes {
            match self.st {
                St::Ground => {
                    if b == 0x1b {
                        self.st = St::Esc;
                    }
                }
                St::Esc => {
                    self.st = match b {
                        b'[' => {
                            self.private = false;
                            self.first = true;
                            self.len = 0;
                            self.overflow = false;
                            St::Csi
                        }
                        0x1b => St::Esc,
                        _ => St::Ground,
                    };
                }
                St::Csi => {
                    let first = std::mem::replace(&mut self.first, false);
                    match b {
                        b'?' if first => self.private = true,
                        b'0'..=b'9' | b';' | b':' => {
                            if self.len < ALT_PARAM_BYTES {
                                self.params[self.len] = b;
                                self.len += 1;
                            } else {
                                self.overflow = true;
                            }
                        }
                        0x40..=0x7e => {
                            if self.private
                                && !self.overflow
                                && (b == b'h' || b == b'l')
                                && self.has_alt_mode()
                            {
                                self.on = b == b'h';
                                seen = true;
                            }
                            self.st = St::Ground;
                        }
                        0x1b => self.st = St::Esc,
                        0x18 | 0x1a => self.st = St::Ground,
                        // Intermediates and C0 controls inside a CSI: keep parsing.
                        _ => {}
                    }
                }
            }
        }
        seen.then_some(self.on)
    }

    fn has_alt_mode(&self) -> bool {
        self.params[..self.len]
            .split(|&c| c == b';')
            .any(|p| matches!(p, b"47" | b"1047" | b"1049"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_modes_across_chunks() {
        let mut a = AltScreen::new();
        assert_eq!(a.feed(b"hello \x1b[?10"), None);
        assert_eq!(a.feed(b"49h vim"), Some(true));
        assert!(a.on());
        assert_eq!(a.feed(b"\x1b[?1;47l"), Some(false));
        assert_eq!(a.feed(b"\x1b[?25l\x1b[1049h"), None, "non-private or other modes");
        assert_eq!(a.feed(b"\x1b[?1047h"), Some(true));
    }

    #[test]
    fn overlong_params_are_ignored() {
        let mut a = AltScreen::new();
        let mut s = b"\x1b[?".to_vec();
        s.extend(std::iter::repeat_n(b'1', 40));
        s.extend(b";1049h");
        assert_eq!(a.feed(&s), None);
        assert_eq!(a.feed(b"\x1b[?1049h"), Some(true));
    }
}
