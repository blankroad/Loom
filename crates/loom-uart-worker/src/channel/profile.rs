//! Channel profiles (UART plan §4.6 prompt defaults, §3.7 line kill) and line endings (§4.3).

use serde::{Deserialize, Serialize};

/// Line ending appended to text sends (plan §4.3: profile value, `cr` without a profile).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Eol {
    /// Nothing (`--hex`, `--file`, `--ctrl`).
    None,
    /// `\r` — what a terminal Enter sends (default).
    Cr,
    /// `\n`.
    Lf,
    /// `\r\n`.
    CrLf,
}

impl Eol {
    /// The bytes of this line ending.
    pub fn bytes(&self) -> &'static [u8] {
        match self {
            Eol::None => b"",
            Eol::Cr => b"\r",
            Eol::Lf => b"\n",
            Eol::CrLf => b"\r\n",
        }
    }

    /// Parse the CLI / RPC spelling: `none`, `cr`, `lf`, `crlf`.
    pub fn parse(s: &str) -> Option<Eol> {
        match s {
            "none" => Some(Eol::None),
            "cr" => Some(Eol::Cr),
            "lf" => Some(Eol::Lf),
            "crlf" => Some(Eol::CrLf),
            _ => None,
        }
    }

    /// The CLI / RPC spelling.
    pub fn as_str(&self) -> &'static str {
        match self {
            Eol::None => "none",
            Eol::Cr => "cr",
            Eol::Lf => "lf",
            Eol::CrLf => "crlf",
        }
    }
}

/// What the channel knows about the device's console.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    /// Profile name (`generic`, `linux-shell`, `uboot`).
    pub name: String,
    /// Prompt regex (Rust `regex` syntax), matched at the end of the received data (§4.5).
    pub prompt: Option<String>,
    /// Ctrl-U is a verified line kill on this console (§3.7 rule 3).
    pub line_kill_ctrl_u: bool,
    /// Default line ending for text sends.
    pub eol: Eol,
}

impl Profile {
    /// Built-in profile by name (plan §4.6): `generic` — prompt `[#$>] ?$`, no ^U;
    /// `linux-shell` — `[#$] $`, ^U; `uboot` — `=> $`, ^U. Unknown name → `None`.
    pub fn named(name: &str) -> Option<Profile> {
        let (prompt, kill) = match name {
            "generic" => (r"[#$>] ?$", false),
            "linux-shell" => (r"[#$] $", true),
            "uboot" => (r"=> $", true),
            _ => return None,
        };
        Some(Profile {
            name: name.to_owned(),
            prompt: Some(prompt.to_owned()),
            line_kill_ctrl_u: kill,
            eol: Eol::Cr,
        })
    }
}

impl Default for Profile {
    /// The `generic` profile.
    fn default() -> Self {
        Profile {
            name: "generic".to_owned(),
            prompt: Some(r"[#$>] ?$".to_owned()),
            line_kill_ctrl_u: false,
            eol: Eol::Cr,
        }
    }
}
