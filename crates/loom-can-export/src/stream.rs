use crate::*;
use autors_blf::objects::{
    AppId, BlfObject, CanError, CanFdFlags, CanFdMessage, CanFlags, CanMessage, Header, ObjectFlags, Time,
};
use autors_blf::{BlfReader, BlfReaderOptions, BlfWriter};
use loom_can_frame::{FrameKind, TimestampSource};
use serde_json::{Value, json};
use std::io::{Read, Seek, SeekFrom, Write};
use std::time::{Duration, UNIX_EPOCH};

/// The verification reader always uses plan limits, never upstream permissive defaults.
pub fn bounded_blf_reader<R: Read>(input: R) -> autors_blf::Result<BlfReader<R>> {
    BlfReader::with_options(
        input,
        BlfReaderOptions {
            max_header_size: MAX_HEADER_BYTES,
            max_container_size: MAX_CONTAINER_BYTES,
            max_object_size: MAX_OBJECT_BYTES,
        },
    )
}
#[derive(Debug)]
struct Line<const N: usize> {
    bytes: Vec<u8>,
}
impl<const N: usize> Line<N> {
    fn new() -> Self {
        Self {
            bytes: Vec::with_capacity(N),
        }
    }
}
impl<const N: usize> Write for Line<N> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > N - self.bytes.len() {
            return Err(io::Error::other("bounded export row exceeded"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
enum Output<W> {
    Blf(BlfWriter<W>),
    Asc(W),
}

/// One-pass writer with bounded row staging and a streaming BLF container.
/// An error poisons the stream: it cannot subsequently be finished as complete.
pub struct Exporter<W: Write + Seek, S: Write> {
    output: Output<W>,
    sidecar: S,
    options: Options,
    format: Format,
    header_start: u64,
    line: Line<MAX_SIDECAR_LINE>,
    asc_line: Line<MAX_ASC_LINE>,
    summary: Summary,
    poisoned: bool,
}
impl<W: Write + Seek, S: Write> fmt::Debug for Exporter<W, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Exporter")
            .field("format", &self.format)
            .field("summary", &self.summary)
            .field("poisoned", &self.poisoned)
            .finish_non_exhaustive()
    }
}
pub(crate) fn time(wall: u64) -> Result<Time, Error> {
    UNIX_EPOCH
        .checked_add(Duration::from_nanos(wall))
        .and_then(Time::from_system_time)
        .ok_or(Error::TimestampOverflow)
}
fn date(wall: Option<u64>) -> Result<String, Error> {
    let t = time(wall.unwrap_or(0))?;
    let weekday = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]
        .get(usize::from(t.day_of_week))
        .ok_or(Error::Invalid("weekday"))?;
    let month = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ]
    .get(usize::from(t.month) - 1)
    .ok_or(Error::Invalid("month"))?;
    Ok(format!(
        "{weekday} {month} {:02} {:02}:{:02}:{:02}.{:03} {:04}",
        t.day, t.hour, t.minute, t.second, t.milliseconds, t.year
    ))
}
fn direction(dir: Direction) -> &'static str {
    match dir {
        Direction::Rx => "Rx",
        Direction::Tx => "Tx",
    }
}
fn source(meta: &Metadata) -> Value {
    meta.source.as_ref().map_or(Value::Null,|s|json!({"producer_id":s.producer_id,"producer_epoch":s.producer_epoch.to_string(),"source_seq":s.source_seq.to_string()}))
}
fn device(ts: Option<DeviceTimestamp>) -> Value {
    ts.map_or(Value::Null,|ts|json!({"value_ns":ts.value_ns.to_string(),"clock_id":ts.clock_id,"source":match ts.source {TimestampSource::Hardware=>"hardware",TimestampSource::KernelSoftware=>"kernel_sw",TimestampSource::Host=>"host"}}))
}

impl<W: Write + Seek, S: Write> Exporter<W, S> {
    /// Writes bounded headers. No timestamp/date is inferred from the current host clock.
    pub fn new(mut output: W, sidecar: S, format: Format, options: Options) -> Result<Self, Error> {
        let header_start = output.stream_position()?;
        let output = match format {
            Format::Blf => {
                let mut header = Header::new(options.compressed, AppId(0), 0, 1, 0);
                header.start = time(options.anchor_wall_ns.unwrap_or(0))?;
                header.end = header.start;
                if header.encoded_size() > MAX_HEADER_BYTES {
                    return Err(Error::Invalid("header bound"));
                }
                let mut writer = BlfWriter::new(output, header)?;
                // Upstream has a growable Vec; half-threshold bounds its geometric growth.
                writer.set_container_size(CONTAINER_FLUSH_BYTES)?;
                Output::Blf(writer)
            }
            Format::Asc => {
                let date = date(options.anchor_wall_ns)?;
                writeln!(
                    output,
                    "date {date}\nbase hex timestamps absolute\ninternal events logged\n// UTC; unknown wall anchor uses epoch placeholder; consult .loom.jsonl\nBegin Triggerblock {date}\n 0.000000000 Start of measurement"
                )?;
                Output::Asc(output)
            }
        };
        let mut this = Self {
            output,
            sidecar,
            options,
            format,
            header_start,
            line: Line::new(),
            asc_line: Line::new(),
            summary: Summary::default(),
            poisoned: false,
        };
        let header = json!({"kind":"header","schema_version":1,"exporter":"loom-can-export/0.1.0","format":format,
            "anchor_mono_ns":options.anchor_mono_ns.to_string(),"anchor_wall_ns":options.anchor_wall_ns.map(|v|v.to_string()),
            "wall_anchor_known":options.anchor_wall_ns.is_some(),"file_time_offset_ns":options.anchor_wall_ns.unwrap_or(0).rem_euclid(1_000_000).to_string(),
            "commit_marker":"primary_file","channel_index":"zero_based_to_one_based_wire","bounds":{"header":MAX_HEADER_BYTES,"container":MAX_CONTAINER_BYTES,"object":MAX_OBJECT_BYTES,"writer":WRITER_BUFFER_BYTES,"flush":CONTAINER_FLUSH_BYTES,"sidecar_line":MAX_SIDECAR_LINE}});
        this.sidecar_row(&header)?;
        Ok(this)
    }
    /// A bounded current summary, including the last successfully staged provenance row.
    pub fn summary(&self) -> &Summary {
        &self.summary
    }
    fn encode_sidecar_row(&mut self, row: &Value) -> Result<(), Error> {
        self.line.bytes.clear();
        serde_json::to_writer(&mut self.line, row)?;
        self.line.write_all(b"\n")?;
        Ok(())
    }
    fn sidecar_row(&mut self, row: &Value) -> Result<(), Error> {
        self.encode_sidecar_row(row)?;
        self.sidecar.write_all(&self.line.bytes)?;
        self.summary.max_sidecar_line = self.summary.max_sidecar_line.max(self.line.bytes.len());
        Ok(())
    }
    /// Stages one record without retaining input history. Descending monotonic time is rejected.
    pub fn push(&mut self, record: &ExportRecord) -> Result<(), Error> {
        if self.poisoned {
            return Err(Error::Invalid("exporter is poisoned"));
        }
        let result = self.push_inner(record);
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }
    fn push_inner(&mut self, record: &ExportRecord) -> Result<(), Error> {
        record.validate()?;
        let stamp = record.time();
        if stamp < self.options.anchor_mono_ns || self.summary.last_mono_ns.is_some_and(|t| stamp < t) {
            return Err(Error::OutOfOrder);
        }
        if self.summary.records == u64::MAX || self.summary.objects == u32::MAX as u64 {
            return Err(Error::CountOverflow);
        }
        let delta = stamp - self.options.anchor_mono_ns;
        let file_time = delta
            .checked_add(self.options.anchor_wall_ns.unwrap_or(0) % 1_000_000)
            .ok_or(Error::TimestampOverflow)?;
        self.options
            .anchor_wall_ns
            .unwrap_or(0)
            .checked_add(delta)
            .ok_or(Error::TimestampOverflow)?;
        let meta = record.metadata();
        let is_object = !matches!(record, ExportRecord::Gap(_));
        let mut row = json!({"kind":match record{ExportRecord::Frame(_)=>"frame",ExportRecord::Error(_)=>"error",ExportRecord::Gap(_)=>"gap"},
            "record_index":self.summary.records.to_string(),"object_index":is_object.then(||self.summary.objects.to_string()),
            "ts_mono_ns":stamp.to_string(),"file_time_ns":file_time.to_string(),"time_quality":meta.time_quality,"request_id":meta.request_id,"source":source(meta)});
        if let Value::Object(fields) = &mut row {
            let extra = match record {
                ExportRecord::Frame(f) => {
                    json!({"channel_index":f.channel_index,"dir":direction(f.dir).to_lowercase(),"origin":f.origin,
                    "wall_ns":f.wall_ns.map(|v|v.to_string()),"device_ts":device(f.device_ts),"id":f.frame.id().raw(),"extended":f.frame.id().is_extended(),"fd":f.frame.is_fd()})
                }
                ExportRecord::Error(e) => {
                    json!({"channel_index":e.channel_index,"dir":direction(e.dir).to_lowercase(),"origin":e.origin,"wall_ns":e.wall_ns.map(|v|v.to_string()),"description":e.description})
                }
                ExportRecord::Gap(g) => {
                    json!({"scope":g.scope,"count":g.count.map(|v|v.to_string()),"reason":g.reason})
                }
            };
            if let Value::Object(extra) = extra {
                fields.extend(extra);
            }
        }
        // Validate serialized metadata length before staging a standard-format object.
        self.encode_sidecar_row(&row)?;
        match &mut self.output {
            Output::Blf(writer) => {
                if let Some(object) = blf_object(record, file_time)? {
                    writer.write_object(&object)?;
                }
            }
            Output::Asc(output) => {
                asc_record(&mut self.asc_line, record, file_time, self.summary.records)?;
                output.write_all(&self.asc_line.bytes)?;
                self.summary.max_asc_line = self.summary.max_asc_line.max(self.asc_line.bytes.len());
            }
        }
        self.sidecar.write_all(&self.line.bytes)?;
        self.summary.max_sidecar_line = self.summary.max_sidecar_line.max(self.line.bytes.len());
        self.summary.records += 1;
        if is_object {
            self.summary.objects += 1;
        }
        match record {
            ExportRecord::Frame(_) => self.summary.frames += 1,
            ExportRecord::Error(_) => self.summary.errors += 1,
            ExportRecord::Gap(_) => self.summary.gaps += 1,
        }
        self.summary.last_mono_ns = Some(stamp);
        if let Some(s) = &meta.source {
            if self.summary.first_source.is_none() {
                self.summary.first_source = Some(s.clone());
            }
            self.summary.last_source = Some(s.clone());
        }
        Ok(())
    }
    /// Flushes objects, patches BLF end time, completes the sidecar and returns streams.
    /// This does not publish filesystem paths; export_file performs fsync/rename.
    pub fn finish(mut self) -> Result<(W, S, Summary), Error> {
        if self.poisoned {
            return Err(Error::Invalid("exporter is poisoned"));
        }
        let mut output = match self.output {
            Output::Blf(writer) => {
                let mut output = writer.finish()?;
                let end = output.stream_position()?;
                let delta = self.summary.last_mono_ns.unwrap_or(self.options.anchor_mono_ns)
                    - self.options.anchor_mono_ns;
                let end_time = time(
                    self.options
                        .anchor_wall_ns
                        .unwrap_or(0)
                        .checked_add(delta)
                        .ok_or(Error::TimestampOverflow)?,
                )?;
                let mut bytes = [0u8; 16];
                for (chunk, value) in bytes.chunks_exact_mut(2).zip([
                    end_time.year,
                    end_time.month,
                    end_time.day_of_week,
                    end_time.day,
                    end_time.hour,
                    end_time.minute,
                    end_time.second,
                    end_time.milliseconds,
                ]) {
                    chunk.copy_from_slice(&value.to_le_bytes());
                }
                output.seek(SeekFrom::Start(
                    self.header_start
                        .checked_add(56)
                        .ok_or(Error::TimestampOverflow)?,
                ))?;
                output.write_all(&bytes)?;
                output.seek(SeekFrom::Start(end))?;
                output
            }
            Output::Asc(mut output) => {
                writeln!(output, "End TriggerBlock")?;
                output
            }
        };
        output.flush()?;
        let footer = json!({"kind":"complete","records":self.summary.records.to_string(),"objects":self.summary.objects.to_string(),"frames":self.summary.frames.to_string(),"errors":self.summary.errors.to_string(),"gaps":self.summary.gaps.to_string(),
            "last_mono_ns":self.summary.last_mono_ns.map(|v|v.to_string()),"first_source":source_ref(self.summary.first_source.as_ref()),"last_source":source_ref(self.summary.last_source.as_ref())});
        self.line.bytes.clear();
        serde_json::to_writer(&mut self.line, &footer)?;
        self.line.write_all(b"\n")?;
        self.sidecar.write_all(&self.line.bytes)?;
        self.sidecar.flush()?;
        self.summary.max_sidecar_line = self.summary.max_sidecar_line.max(self.line.bytes.len());
        Ok((output, self.sidecar, self.summary))
    }
}
pub(crate) fn source_ref(value: Option<&SourceRef>) -> Value {
    value.map_or(Value::Null,|s|json!({"producer_id":s.producer_id,"producer_epoch":s.producer_epoch.to_string(),"source_seq":s.source_seq.to_string()}))
}
fn blf_object(record: &ExportRecord, stamp: u64) -> Result<Option<BlfObject>, Error> {
    let flags = ObjectFlags::TIME_ONE_NANS;
    let object = match record {
        ExportRecord::Frame(f) => {
            let channel = f.channel_index + 1;
            let id = f.frame.id().raw()
                | if f.frame.id().is_extended() {
                    0x8000_0000
                } else {
                    0
                };
            let tx = f.dir == Direction::Tx;
            if let FrameKind::Fd { brs, esi } = f.frame.kind() {
                let mut object = CanFdMessage::new(flags, stamp, channel, id, f.frame.data(), tx, true, brs);
                if esi {
                    object.fd_flags = object.fd_flags | CanFdFlags::ESI;
                }
                BlfObject::CanFdMessage(object)
            } else {
                let mut object = CanMessage::new(flags, stamp, channel, id, f.frame.data(), tx);
                if f.frame.is_remote() {
                    object.flags = object.flags | CanFlags::RTR;
                    object.dlc = f.frame.dlc();
                }
                BlfObject::CanMessage(object)
            }
        }
        ExportRecord::Error(e) => {
            let mut object = CanError::new(flags, stamp);
            object.channel = e.channel_index + 1;
            BlfObject::CanError(object)
        }
        ExportRecord::Gap(_) => return Ok(None),
    };
    let size = match &object {
        BlfObject::CanMessage(o) => o.header.base.object_size,
        BlfObject::CanFdMessage(o) => o.header.base.object_size,
        BlfObject::CanError(o) => o.header.base.object_size,
        _ => 0,
    } as usize;
    if size == 0 || size > 128 || size > MAX_OBJECT_BYTES || size > CONTAINER_FLUSH_BYTES {
        return Err(Error::Invalid("generated object bound"));
    }
    Ok(Some(object))
}
fn asc_record(
    line: &mut Line<MAX_ASC_LINE>,
    record: &ExportRecord,
    stamp: u64,
    index: u64,
) -> Result<(), Error> {
    line.bytes.clear();
    if matches!(record, ExportRecord::Gap(_)) {
        writeln!(line, "// loom gap record_index={index}; see .loom.jsonl")?;
        return Ok(());
    }
    write!(line, " {}.{:09} ", stamp / 1_000_000_000, stamp % 1_000_000_000)?;
    match record {
        ExportRecord::Frame(f) => {
            let id = f.frame.id().raw();
            let ext = if f.frame.id().is_extended() { "x" } else { "" };
            let channel = f.channel_index + 1;
            if let FrameKind::Fd { brs, esi } = f.frame.kind() {
                write!(
                    line,
                    "CANFD {channel} {} {id:X}{ext} {} {} {:X} {}",
                    direction(f.dir),
                    u8::from(brs),
                    u8::from(esi),
                    f.frame.dlc(),
                    f.frame.len()
                )?;
                for byte in f.frame.data() {
                    write!(line, " {byte:02X}")?;
                }
                let flags = 0x1000 | if brs { 0x2000 } else { 0 } | if esi { 0x4000 } else { 0 };
                write!(line, " 0 0 {flags:X} 0 0 0 0 0")?;
            } else {
                write!(
                    line,
                    "{channel} {id:X}{ext} {} {} {:X}",
                    direction(f.dir),
                    if f.frame.is_remote() { "r" } else { "d" },
                    f.frame.dlc()
                )?;
                for byte in f.frame.data() {
                    write!(line, " {byte:02X}")?;
                }
            }
        }
        ExportRecord::Error(e) => write!(line, "{} ErrorFrame", e.channel_index + 1)?,
        ExportRecord::Gap(_) => {}
    }
    line.write_all(b"\n")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn bounded_lines_refuse_growth_past_capacity() {
        let mut line = Line::<MAX_SIDECAR_LINE>::new();
        line.write_all(&[0; MAX_SIDECAR_LINE]).unwrap();
        assert!(line.write_all(b"x").is_err());
        assert_eq!(line.bytes.len(), MAX_SIDECAR_LINE);
        assert_eq!(line.bytes.capacity(), MAX_SIDECAR_LINE);
        let mut asc = Line::<MAX_ASC_LINE>::new();
        asc.write_all(&[0; MAX_ASC_LINE]).unwrap();
        assert!(asc.write_all(b"x").is_err());
        assert_eq!(asc.bytes.capacity(), MAX_ASC_LINE);
    }
}
