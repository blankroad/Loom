use crate::*;
pub(crate) const SHB: u32 = 0x0a0d0d0a;
pub(crate) const IDB: u32 = 1;
pub(crate) const EPB: u32 = 6;
pub(crate) const ISB: u32 = 5;
#[derive(Debug, Clone, Copy)]
pub(crate) enum Endian {
    Le,
    Be,
}
impl Endian {
    pub fn u16(self, s: &[u8]) -> Result<u16> {
        let a = s
            .get(..2)
            .ok_or_else(|| Error::invalid("short u16"))?
            .try_into()
            .map_err(|_| Error::invalid("u16"))?;
        Ok(match self {
            Self::Le => u16::from_le_bytes(a),
            Self::Be => u16::from_be_bytes(a),
        })
    }
    pub fn u32(self, s: &[u8]) -> Result<u32> {
        let a = s
            .get(..4)
            .ok_or_else(|| Error::invalid("short u32"))?
            .try_into()
            .map_err(|_| Error::invalid("u32"))?;
        Ok(match self {
            Self::Le => u32::from_le_bytes(a),
            Self::Be => u32::from_be_bytes(a),
        })
    }
    pub fn u64(self, s: &[u8]) -> Result<u64> {
        let a = s
            .get(..8)
            .ok_or_else(|| Error::invalid("short u64"))?
            .try_into()
            .map_err(|_| Error::invalid("u64"))?;
        Ok(match self {
            Self::Le => u64::from_le_bytes(a),
            Self::Be => u64::from_be_bytes(a),
        })
    }
}
pub(crate) fn padded(n: usize) -> usize {
    (n + 3) & !3
}
pub(crate) fn options(
    mut bytes: &[u8],
    e: Endian,
    mut visit: impl FnMut(u16, &[u8]) -> Result<()>,
) -> Result<()> {
    let mut count = 0;
    while !bytes.is_empty() {
        if bytes.len() < 4 {
            return Err(Error::invalid("short option header"));
        }
        count += 1;
        if count > MAX_OPTIONS {
            return Err(Error::limit("options"));
        }
        let code = e.u16(bytes)?;
        let n = e.u16(&bytes[2..])? as usize;
        bytes = &bytes[4..];
        if code == 0 {
            if n != 0 || bytes.iter().any(|b| *b != 0) {
                return Err(Error::invalid("end option"));
            }
            return Ok(());
        }
        let data = bytes
            .get(..n)
            .ok_or_else(|| Error::invalid("short option data"))?;
        if padded(n) > bytes.len() {
            return Err(Error::invalid("short option padding"));
        }
        visit(code, data)?;
        bytes = &bytes[padded(n)..];
    }
    Ok(())
}
pub(crate) fn text(s: &[u8], max: usize) -> Result<String> {
    if s.len() > max {
        return Err(Error::limit("text bytes"));
    }
    std::str::from_utf8(s)
        .map(str::to_owned)
        .map_err(|_| Error::invalid("invalid UTF-8"))
}
pub(crate) fn parse_epb(
    body: &[u8],
    e: Endian,
    resolution: TimestampResolution,
    section: u64,
) -> Result<Packet> {
    if body.len() < 20 {
        return Err(Error::invalid("short EPB"));
    }
    let caplen = e.u32(&body[12..])?;
    let origlen = e.u32(&body[16..])?;
    if caplen > origlen {
        return Err(Error::invalid("caplen exceeds origlen"));
    }
    let n = caplen as usize;
    if n > MAX_PACKET_BYTES {
        return Err(Error::limit("packet bytes"));
    }
    if padded(n) > body.len() - 20 {
        return Err(Error::invalid("short EPB packet"));
    }
    let ticks = (u64::from(e.u32(&body[4..])?) << 32) | u64::from(e.u32(&body[8..])?);
    let mut packet = Packet {
        interface_id: e.u32(body)?,
        section_index: section,
        ts_ns: resolution.to_ns(ticks),
        caplen,
        origlen,
        data: body[20..20 + n].to_vec(),
        comments: Vec::new(),
        meta: None,
    };
    let mut count = 0;
    options(&body[20 + padded(n)..], e, |code, data| {
        if code == 1 {
            count += 1;
            if count > MAX_COMMENTS {
                return Err(Error::limit("comments"));
            }
            let s = text(data, MAX_COMMENT_BYTES)?;
            if let Some(meta) = LoomMeta::from_comment(&s) {
                if packet.meta.is_some() {
                    return Err(Error::invalid("duplicate Loom metadata"));
                }
                packet.meta = Some(meta);
            } else {
                packet.comments.push(s);
            }
        }
        Ok(())
    })?;
    Ok(packet)
}
pub(crate) fn append_option(out: &mut Vec<u8>, code: u16, data: &[u8]) -> Result<()> {
    let n = u16::try_from(data.len()).map_err(|_| Error::limit("option bytes"))?;
    if out.len() + 4 + padded(data.len()) > MAX_BLOCK_BYTES - 12 {
        return Err(Error::limit("block bytes"));
    }
    out.extend(code.to_le_bytes());
    out.extend(n.to_le_bytes());
    out.extend(data);
    out.resize(out.len() + padded(data.len()) - data.len(), 0);
    Ok(())
}
pub(crate) fn block_bytes(kind: u32, body: &[u8]) -> Result<Vec<u8>> {
    let n = body
        .len()
        .checked_add(12)
        .filter(|n| *n <= MAX_BLOCK_BYTES && n % 4 == 0)
        .ok_or_else(|| Error::limit("block bytes/alignment"))?;
    let mut out = Vec::with_capacity(n);
    out.extend(kind.to_le_bytes());
    out.extend((n as u32).to_le_bytes());
    out.extend(body);
    out.extend((n as u32).to_le_bytes());
    Ok(out)
}
pub(crate) fn epb_bytes(p: &Packet, resolution: TimestampResolution) -> Result<Vec<u8>> {
    p.validate()?;
    let ticks = resolution.ticks(p.ts_ns)?;
    let mut n = 20 + padded(p.data.len()) + 4;
    for c in &p.comments {
        n += 4 + padded(c.len());
    }
    let meta = p.meta.as_ref().map(LoomMeta::to_comment).transpose()?;
    if let Some(m) = &meta {
        n += 4 + padded(m.len());
    }
    if n + 12 > MAX_BLOCK_BYTES {
        return Err(Error::limit("EPB block bytes"));
    }
    let mut body = Vec::with_capacity(n);
    for v in [
        p.interface_id,
        (ticks >> 32) as u32,
        ticks as u32,
        p.caplen,
        p.origlen,
    ] {
        body.extend(v.to_le_bytes());
    }
    body.extend(&p.data);
    body.resize(20 + padded(p.data.len()), 0);
    for c in &p.comments {
        append_option(&mut body, 1, c.as_bytes())?;
    }
    if let Some(m) = &meta {
        append_option(&mut body, 1, m.as_bytes())?;
    }
    append_option(&mut body, 0, &[])?;
    block_bytes(EPB, &body)
}
