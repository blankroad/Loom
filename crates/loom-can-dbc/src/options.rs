//! Load options and limits.

/// Text encoding of the DBC file. There is no silent guessing: `Auto` marks the result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextEncoding {
    /// Strict UTF-8 (a leading BOM is skipped). Invalid bytes are a load error.
    #[default]
    Utf8,
    /// Windows-1252. Also use this for files described as Latin-1 / ISO-8859-1
    /// (the printable range is the same; WHATWG maps the ISO-8859-1 label to it).
    Windows1252,
    /// UTF-8 if valid, otherwise Windows-1252. `Database::encoding_guessed()` is true
    /// and a warning is added.
    Auto,
}

/// Hard limits applied while loading (C2). Exceeding one is a `LoadError::LimitExceeded`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_file_bytes: usize,
    /// Top-level statements (lines starting at column 0).
    pub max_statements: usize,
    pub max_messages: usize,
    pub max_signals: usize,
    /// Any kept string: names, units, comments, value labels.
    pub max_string_bytes: usize,
    /// Nesting of multiplexers (extended multiplexing).
    pub max_mux_depth: usize,
    /// Estimated heap size of the compiled database.
    pub max_ir_bytes: usize,
    /// Warnings kept in the list; more are only counted.
    pub max_warnings: usize,
    /// Statements can-dbc could not parse; more than this fails the load.
    pub max_dropped_statements: usize,
}

impl Default for Limits {
    /// 16 MiB file, 50k messages, 100k signals, 64 KiB strings, mux depth 16,
    /// compiled IR 48 MiB. Plan §7 said 500k signals / 32 MiB IR, but those do not
    /// fit together (~350 B of IR per signal, measured); lead decision 2026-11-11.
    fn default() -> Self {
        Self {
            max_file_bytes: 16 * 1024 * 1024,
            max_statements: 2_000_000,
            max_messages: 50_000,
            max_signals: 100_000,
            max_string_bytes: 64 * 1024,
            max_mux_depth: 16,
            max_ir_bytes: 48 * 1024 * 1024,
            max_warnings: 1_000,
            max_dropped_statements: 10_000,
        }
    }
}

/// Options for [`crate::Database::load`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LoadOptions {
    pub encoding: TextEncoding,
    pub limits: Limits,
}
