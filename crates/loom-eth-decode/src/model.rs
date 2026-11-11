//! Decode result model: summary row, layer tree, warnings, and the field-value iterator.

use serde::Serialize;

use crate::fields::{FieldDef, field_def};
use crate::value::{FieldType, FieldValue};

/// Bytes of the frame copied into [`Decoded`]. Larger frames: the rest is not decoded
/// (warning `frame_too_large`); the journal keeps the original (decoder never sees more).
pub const MAX_FRAME_BYTES: usize = 262_144;
/// Layers per packet. Over: further layers are dropped with warning `limit_layers`.
pub const MAX_LAYERS: usize = 64;
/// Fields per packet (all layers). Over: further fields are dropped with warning `limit_fields`.
pub const MAX_FIELDS: usize = 4096;
/// Warnings per packet. Over: further warnings are dropped silently (count kept in the last one).
pub const MAX_WARNINGS: usize = 16;
/// Extra byte buffers (reassembled PDUs) per packet. Over: PDU decoded without buffer positions.
pub const MAX_BUFFERS: usize = 64;
/// Summary `info` length in bytes. Over: truncated at a char boundary with `…`.
pub const MAX_INFO_LEN: usize = 512;
/// Tree depth of fields inside a layer.
pub const MAX_FIELD_DEPTH: u8 = 8;

/// Direction seen by the capturing NIC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Dir {
    Rx,
    Tx,
}

impl Dir {
    pub fn as_str(self) -> &'static str {
        match self {
            Dir::Rx => "rx",
            Dir::Tx => "tx",
        }
    }
}

/// Attributed origin of a captured packet that Loom itself sent (plan §3.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    Agent,
    User,
    Script,
}

impl Origin {
    pub fn as_str(self) -> &'static str {
        match self {
            Origin::Agent => "agent",
            Origin::User => "user",
            Origin::Script => "script",
        }
    }
}

/// Per-packet metadata supplied by the caller (capture worker / pcapng reader).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct FrameMeta {
    /// Loom source sequence number.
    pub seq: u64,
    /// Capture timestamp, ns since epoch (or any monotonic base; only differences matter).
    pub ts_ns: u64,
    pub dir: Option<Dir>,
    pub attributed_origin: Option<Origin>,
    pub request_id: Option<String>,
    /// Original length on the wire if the capture was truncated by snaplen.
    pub orig_len: Option<u32>,
}

/// Where a layer or field lives. `buf` 0 = the frame, n ≥ 1 = reassembled buffer n.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pos {
    pub buf: u8,
    pub off: u32,
    pub len: u32,
}

impl Pos {
    pub(crate) fn frame(off: usize, len: usize) -> Option<Pos> {
        Some(Pos {
            buf: 0,
            off: u32::try_from(off).ok()?,
            len: u32::try_from(len).ok()?,
        })
    }
    pub(crate) fn at(buf: u8, off: usize, len: usize) -> Option<Pos> {
        Some(Pos {
            buf,
            off: u32::try_from(off).ok()?,
            len: u32::try_from(len).ok()?,
        })
    }
}

/// Base location used by sub-decoders to compute field positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Loc {
    pub buf: u8,
    pub off: usize,
}

impl Loc {
    pub(crate) fn pos(self, rel: usize, len: usize) -> Option<Pos> {
        Pos::at(self.buf, self.off.checked_add(rel)?, len)
    }
}

/// One field node. `depth` 0 = direct child of the layer; the tree is the flat order + depth.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: &'static str,
    pub value: FieldValue,
    /// Human text (value string), e.g. `0x8001 (Diagnostic message)`.
    pub show: Option<String>,
    pub pos: Option<Pos>,
    pub depth: u8,
}

/// One protocol layer (`eth`, `ip`, `tcp`, `doip` …). A packet may hold several of the same name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layer {
    pub name: &'static str,
    pub title: String,
    pub pos: Option<Pos>,
    pub fields: Vec<Field>,
}

/// tshark-like one-line summary.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Summary {
    pub proto: String,
    pub src: String,
    pub dst: String,
    pub len: u32,
    pub info: String,
}

/// Non-fatal decode problem. `code` is a stable machine string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DecodeWarning {
    pub code: &'static str,
    pub layer: &'static str,
    pub detail: String,
}

/// Decode result of one packet.
#[derive(Debug, Clone)]
pub struct Decoded {
    pub summary: Summary,
    pub layers: Vec<Layer>,
    pub warnings: Vec<DecodeWarning>,
    pub meta: FrameMeta,
    /// 0 = frame bytes (≤ MAX_FRAME_BYTES), then reassembled PDU buffers.
    buffers: Vec<Vec<u8>>,
    field_count: usize,
}

impl Decoded {
    pub(crate) fn new(meta: &FrameMeta, frame: &[u8]) -> Decoded {
        let keep = frame.get(..MAX_FRAME_BYTES).unwrap_or(frame);
        let mut d = Decoded {
            summary: Summary::default(),
            layers: Vec::new(),
            warnings: Vec::new(),
            meta: meta.clone(),
            buffers: vec![keep.to_vec()],
            field_count: 0,
        };
        if keep.len() < frame.len() {
            d.warn(
                "frame_too_large",
                "frame",
                format!("{} bytes, decoded first {}", frame.len(), keep.len()),
            );
        }
        d
    }

    /// The (possibly cut) frame bytes.
    pub fn frame(&self) -> &[u8] {
        self.buffers.first().map(Vec::as_slice).unwrap_or(&[])
    }

    /// Bytes at a position (empty if out of range).
    pub fn bytes_at(&self, pos: Pos) -> &[u8] {
        let Some(b) = self.buffers.get(usize::from(pos.buf)) else {
            return &[];
        };
        let start = pos.off as usize;
        let end = start.saturating_add(pos.len as usize);
        b.get(start..end).unwrap_or(&[])
    }

    /// Bytes of a layer (empty if the layer has no position).
    pub fn layer_bytes(&self, layer: &Layer) -> &[u8] {
        layer.pos.map(|p| self.bytes_at(p)).unwrap_or(&[])
    }

    /// True if a layer with this protocol name exists.
    pub fn has_protocol(&self, name: &str) -> bool {
        self.layers.iter().any(|l| l.name == name)
    }

    /// All values of a field (canonical name or alias). Unknown names yield nothing.
    /// Protocol names yield the layer bytes once per layer. Union fields yield member values.
    pub fn field_values<'a>(&'a self, name: &str) -> FieldValues<'a> {
        FieldValues {
            d: self,
            def: field_def(name),
            li: 0,
            fi: 0,
        }
    }

    pub(crate) fn push_layer(
        &mut self,
        name: &'static str,
        title: String,
        pos: Option<Pos>,
    ) -> Option<usize> {
        if self.layers.len() >= MAX_LAYERS {
            self.warn("limit_layers", name, format!("more than {MAX_LAYERS} layers"));
            return None;
        }
        self.layers.push(Layer {
            name,
            title,
            pos,
            fields: Vec::new(),
        });
        Some(self.layers.len() - 1)
    }

    pub(crate) fn add(
        &mut self,
        layer: Option<usize>,
        name: &'static str,
        value: FieldValue,
        pos: Option<Pos>,
    ) {
        self.add_full(layer, name, value, None, pos, 0);
    }

    pub(crate) fn add_show(
        &mut self,
        layer: Option<usize>,
        name: &'static str,
        value: FieldValue,
        show: String,
        pos: Option<Pos>,
    ) {
        self.add_full(layer, name, value, Some(show), pos, 0);
    }

    pub(crate) fn add_full(
        &mut self,
        layer: Option<usize>,
        name: &'static str,
        value: FieldValue,
        show: Option<String>,
        pos: Option<Pos>,
        depth: u8,
    ) {
        let Some(li) = layer else { return };
        if self.field_count >= MAX_FIELDS {
            if self.field_count == MAX_FIELDS {
                self.field_count += 1;
                self.warn("limit_fields", name, format!("more than {MAX_FIELDS} fields"));
            }
            return;
        }
        let Some(l) = self.layers.get_mut(li) else { return };
        l.fields.push(Field {
            name,
            value,
            show,
            pos,
            depth: depth.min(MAX_FIELD_DEPTH),
        });
        self.field_count += 1;
    }

    pub(crate) fn warn(&mut self, code: &'static str, layer: &'static str, detail: String) {
        if self.warnings.len() < MAX_WARNINGS {
            self.warnings.push(DecodeWarning { code, layer, detail });
        }
    }

    /// Adds a reassembled buffer; returns its index, or None when over MAX_BUFFERS.
    pub(crate) fn add_buffer(&mut self, bytes: Vec<u8>) -> Option<u8> {
        if self.buffers.len() >= MAX_BUFFERS {
            self.warn(
                "limit_buffers",
                "frame",
                format!("more than {MAX_BUFFERS} buffers"),
            );
            return None;
        }
        self.buffers.push(bytes);
        u8::try_from(self.buffers.len() - 1).ok()
    }

    pub(crate) fn set_info(&mut self, info: String) {
        self.summary.info = cap_info(info);
    }
}

pub(crate) fn cap_info(mut s: String) -> String {
    if s.len() > MAX_INFO_LEN {
        let mut cut = MAX_INFO_LEN.saturating_sub(3);
        while cut > 0 && !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
        s.push('…');
    }
    s
}

/// Iterator returned by [`Decoded::field_values`].
#[derive(Debug)]
pub struct FieldValues<'a> {
    d: &'a Decoded,
    def: Option<&'static FieldDef>,
    li: usize,
    fi: usize,
}

impl Iterator for FieldValues<'_> {
    type Item = FieldValue;

    fn next(&mut self) -> Option<FieldValue> {
        let def = self.def?;
        loop {
            let layer = self.d.layers.get(self.li)?;
            if def.ty == FieldType::Protocol {
                self.li += 1;
                if layer.name == def.name {
                    return Some(FieldValue::Bytes(self.d.layer_bytes(layer).to_vec()));
                }
                continue;
            }
            match layer.fields.get(self.fi) {
                Some(f) => {
                    self.fi += 1;
                    let hit = if def.union.is_empty() {
                        f.name == def.name
                    } else {
                        def.union.contains(&f.name)
                    };
                    if hit {
                        return Some(f.value.clone());
                    }
                }
                None => {
                    self.li += 1;
                    self.fi = 0;
                }
            }
        }
    }
}
