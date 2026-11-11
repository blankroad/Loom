//! JSON shapes.
//!
//! - `Layer` / `Decoded` serialize as a tree: `{name, title, off?, len?, fields:[{name, value, show?, off?, len?, children?}]}`
//!   (`off`/`len` only for positions inside the frame, for hex highlighting).
//! - [`CompactLayers`] serializes as plan §4 `--json` `layers`: `{"eth":{"src":…}, "doip":[{…},{…}]}` —
//!   short field names, repeated layers or fields become arrays.
//!
//! Value encoding: hex-base integers → `"0x0E80"` (uppercase, width by type), `U64` → decimal string
//! (JS safety, same rule as CORE U64), other integers → JSON number, bytes → lowercase hex string,
//! addresses → text.

use serde::ser::{SerializeMap, SerializeSeq, SerializeStruct};
use serde::{Serialize, Serializer};

use crate::fields::field_def;
use crate::model::{Decoded, Field, Layer};
use crate::value::{Base, FieldType, FieldValue, fmt_hex, fmt_mac};

/// Field value with its registry type applied.
#[derive(Debug)]
pub struct ValueJson<'a> {
    pub name: &'a str,
    pub value: &'a FieldValue,
}

impl Serialize for ValueJson<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let def = field_def(self.name);
        match self.value {
            FieldValue::Uint(v) => {
                let (ty, base) = def.map(|d| (d.ty, d.base)).unwrap_or((FieldType::U64, Base::Dec));
                if base == Base::Hex {
                    let w = match ty {
                        FieldType::U8 => 2,
                        FieldType::U16 => 4,
                        FieldType::U32 => 8,
                        _ => 16,
                    };
                    s.serialize_str(&format!("0x{v:0w$X}"))
                } else if ty == FieldType::U64 {
                    s.serialize_str(&v.to_string())
                } else {
                    s.serialize_u64(*v)
                }
            }
            FieldValue::Bool(b) => s.serialize_bool(*b),
            FieldValue::Bytes(b) => s.serialize_str(&fmt_hex(b)),
            FieldValue::Str(t) => s.serialize_str(t),
            FieldValue::Ipv4(a) => s.collect_str(a),
            FieldValue::Ipv6(a) => s.collect_str(a),
            FieldValue::Mac(m) => s.serialize_str(&fmt_mac(m)),
        }
    }
}

/// Indices of direct children of `fields[idx]`.
fn children(fields: &[Field], idx: usize) -> Vec<usize> {
    let Some(parent) = fields.get(idx) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut child_depth: Option<u8> = None;
    for (j, f) in fields.iter().enumerate().skip(idx + 1) {
        if f.depth <= parent.depth {
            break;
        }
        let cd = *child_depth.get_or_insert(f.depth);
        if f.depth <= cd {
            child_depth = Some(f.depth);
            out.push(j);
        }
    }
    out
}

fn top_level(fields: &[Field]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut level: Option<u8> = None;
    for (j, f) in fields.iter().enumerate() {
        let l = *level.get_or_insert(f.depth);
        if f.depth <= l {
            level = Some(f.depth);
            out.push(j);
        }
    }
    out
}

struct Node<'a> {
    fields: &'a [Field],
    idx: usize,
}

impl Serialize for Node<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let Some(f) = self.fields.get(self.idx) else {
            return s.serialize_none();
        };
        let kids = children(self.fields, self.idx);
        let frame_pos = f.pos.filter(|p| p.buf == 0);
        let n = 2
            + usize::from(f.show.is_some())
            + 2 * usize::from(frame_pos.is_some())
            + usize::from(!kids.is_empty());
        let mut st = s.serialize_struct("Field", n)?;
        st.serialize_field("name", f.name)?;
        st.serialize_field(
            "value",
            &ValueJson {
                name: f.name,
                value: &f.value,
            },
        )?;
        if let Some(show) = &f.show {
            st.serialize_field("show", show)?;
        }
        if let Some(p) = frame_pos {
            st.serialize_field("off", &p.off)?;
            st.serialize_field("len", &p.len)?;
        }
        if !kids.is_empty() {
            st.serialize_field(
                "children",
                &NodeList {
                    fields: self.fields,
                    idx: kids,
                },
            )?;
        }
        st.end()
    }
}

struct NodeList<'a> {
    fields: &'a [Field],
    idx: Vec<usize>,
}

impl Serialize for NodeList<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut seq = s.serialize_seq(Some(self.idx.len()))?;
        for &i in &self.idx {
            seq.serialize_element(&Node {
                fields: self.fields,
                idx: i,
            })?;
        }
        seq.end()
    }
}

impl Serialize for Layer {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let frame_pos = self.pos.filter(|p| p.buf == 0);
        let mut st = s.serialize_struct("Layer", 3 + 2 * usize::from(frame_pos.is_some()))?;
        st.serialize_field("name", self.name)?;
        st.serialize_field("title", &self.title)?;
        if let Some(p) = frame_pos {
            st.serialize_field("off", &p.off)?;
            st.serialize_field("len", &p.len)?;
        }
        st.serialize_field(
            "fields",
            &NodeList {
                fields: &self.fields,
                idx: top_level(&self.fields),
            },
        )?;
        st.end()
    }
}

impl Serialize for Decoded {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut st = s.serialize_struct("Decoded", 4)?;
        st.serialize_field("summary", &self.summary)?;
        st.serialize_field("layers", &self.layers)?;
        st.serialize_field("warnings", &self.warnings)?;
        st.serialize_field("meta", &self.meta)?;
        st.end()
    }
}

/// Compact `layers` map (plan §4 `--json`). Construct with `CompactLayers(&decoded.layers)`.
#[derive(Debug)]
pub struct CompactLayers<'a>(pub &'a [Layer]);

fn short<'a>(layer: &str, field: &'a str) -> &'a str {
    field
        .strip_prefix(layer)
        .and_then(|r| r.strip_prefix('.'))
        .unwrap_or(field)
}

struct CompactLayer<'a>(&'a Layer);

impl Serialize for CompactLayer<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut groups: Vec<(&str, Vec<&Field>)> = Vec::new();
        for f in &self.0.fields {
            let k = short(self.0.name, f.name);
            match groups.iter_mut().find(|(g, _)| *g == k) {
                Some((_, v)) => v.push(f),
                None => groups.push((k, vec![f])),
            }
        }
        let mut m = s.serialize_map(Some(groups.len()))?;
        for (k, fs) in &groups {
            match fs.as_slice() {
                [one] => m.serialize_entry(
                    k,
                    &ValueJson {
                        name: one.name,
                        value: &one.value,
                    },
                )?,
                many => {
                    let vals: Vec<ValueJson<'_>> = many
                        .iter()
                        .map(|f| ValueJson {
                            name: f.name,
                            value: &f.value,
                        })
                        .collect();
                    m.serialize_entry(k, &vals)?
                }
            }
        }
        m.end()
    }
}

impl Serialize for CompactLayers<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut groups: Vec<(&str, Vec<&Layer>)> = Vec::new();
        for l in self.0 {
            match groups.iter_mut().find(|(g, _)| *g == l.name) {
                Some((_, v)) => v.push(l),
                None => groups.push((l.name, vec![l])),
            }
        }
        let mut m = s.serialize_map(Some(groups.len()))?;
        for (k, ls) in &groups {
            match ls.as_slice() {
                [one] => m.serialize_entry(k, &CompactLayer(one))?,
                many => {
                    let v: Vec<CompactLayer<'_>> = many.iter().map(|l| CompactLayer(l)).collect();
                    m.serialize_entry(k, &v)?
                }
            }
        }
        m.end()
    }
}
