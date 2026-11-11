use crate::*;
use loom_eth_pcapng::PcapNgReader;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
    time::Instant,
};
/// Maximum interval multiplier; reject nonfinite/negative/excessive factors.
pub const MAX_REPLAY_FACTOR: f64 = 1_000_000.0;
/// Metadata scan work bound per poll; yield instead of hiding long runs of unknown blocks.
pub const MAX_METADATA_BLOCKS_PER_POLL: usize = 32;
#[derive(Debug, Clone, Copy)]
pub enum ReplaySpeed {
    Max,
    Realtime { factor: f64 },
}
#[derive(Debug, Clone, Copy)]
pub struct ReplayOptions {
    pub speed: ReplaySpeed,
    pub repeat: bool,
}
impl Default for ReplayOptions {
    fn default() -> Self {
        Self {
            speed: ReplaySpeed::Max,
            repeat: false,
        }
    }
}
#[derive(Debug)]
pub struct ReplaySource<R: Read + Seek + Send + std::fmt::Debug> {
    reader: Option<PcapNgReader<R>>,
    pending: Option<RawFrame>,
    interface: Interface,
    options: ReplayOptions,
    first_ts: Option<u128>,
    clock: Option<Instant>,
    exhausted: bool,
    emitted_in_cycle: bool,
    recv: u64,
    stop: StopSignal,
}
impl ReplaySource<File> {
    pub fn open(path: impl AsRef<Path>, options: ReplayOptions) -> Result<Self> {
        Self::new(File::open(path)?, options)
    }
}
impl<R: Read + Seek + Send + std::fmt::Debug> ReplaySource<R> {
    pub fn new(reader: R, options: ReplayOptions) -> Result<Self> {
        if let ReplaySpeed::Realtime { factor } = options.speed
            && (!factor.is_finite() || factor <= 0.0 || factor > MAX_REPLAY_FACTOR)
        {
            return Err(CaptureError::Invalid("replay interval factor"));
        }
        let mut reader = PcapNgReader::new(reader);
        let (mut pending, mut exhausted) = read_frame(&mut reader, None)?;
        // Initialization may scan arbitrary unknown pre-IDB blocks without retaining them.
        while pending.is_none() && !exhausted && reader.interfaces().is_empty() {
            (pending, exhausted) = read_frame(&mut reader, None)?;
        }
        let interface = pending
            .as_ref()
            .map(|p| p.interface.clone())
            .or_else(|| reader.interfaces().first().cloned())
            .ok_or(CaptureError::Invalid("capture has no interface"))?;
        Ok(Self {
            reader: Some(reader),
            pending,
            interface,
            options,
            first_ts: None,
            clock: None,
            exhausted,
            emitted_in_cycle: false,
            recv: 0,
            stop: StopSignal::default(),
        })
    }
    fn advance(&mut self) -> Result<()> {
        let r = self
            .reader
            .as_mut()
            .ok_or(CaptureError::Invalid("replay reader missing"))?;
        let (pending, exhausted) = read_frame(r, Some(&self.stop))?;
        self.pending = pending;
        self.exhausted = exhausted;
        if exhausted && self.options.repeat && self.emitted_in_cycle {
            let mut input = self
                .reader
                .take()
                .ok_or(CaptureError::Invalid("replay reader missing"))?
                .into_inner();
            input.seek(SeekFrom::Start(0))?;
            let mut r = PcapNgReader::new(input);
            let (pending, exhausted) = read_frame(&mut r, Some(&self.stop))?;
            self.pending = pending;
            self.exhausted = exhausted;
            self.reader = Some(r);
            self.emitted_in_cycle = false;
            self.first_ts = None;
            self.clock = None;
        }
        Ok(())
    }
}
fn read_frame<R: Read>(
    reader: &mut PcapNgReader<R>,
    stop: Option<&StopSignal>,
) -> Result<(Option<RawFrame>, bool)> {
    for _ in 0..MAX_METADATA_BLOCKS_PER_POLL {
        if stop.is_some_and(StopSignal::is_stopped) {
            return Ok((None, false));
        }
        let Some(block) = reader.next_block()? else {
            return Ok((None, true));
        };
        if let loom_eth_pcapng::Block::Packet(p) = block {
            let interface = reader
                .interfaces()
                .get(p.interface_id as usize)
                .cloned()
                .ok_or(CaptureError::Invalid("replay interface"))?;
            return Ok((
                Some(RawFrame {
                    ts_ns: p.ts_ns,
                    caplen: p.caplen,
                    origlen: p.origlen,
                    data: p.data,
                    interface_id: p.interface_id,
                    section_index: p.section_index,
                    interface,
                    comments: p.comments,
                    meta: p.meta,
                }),
                false,
            ));
        }
    }
    Ok((None, false))
}
impl<R: Read + Seek + Send + std::fmt::Debug> CaptureSource for ReplaySource<R> {
    fn interface(&self) -> Interface {
        self.interface.clone()
    }
    fn timestamp_source(&self) -> TimestampSource {
        TimestampSource::Replay
    }
    fn is_exhausted(&self) -> bool {
        self.exhausted
    }
    fn set_stop_signal(&mut self, s: StopSignal) {
        self.stop = s;
    }
    fn next(&mut self, timeout: Duration) -> Result<Option<RawFrame>> {
        if self.stop.is_stopped() || self.exhausted {
            return Ok(None);
        }
        if self.pending.is_none() {
            self.advance()?;
        }
        let Some(frame) = &self.pending else {
            return Ok(None);
        };
        if let ReplaySpeed::Realtime { factor } = self.options.speed {
            let ts = *self.first_ts.get_or_insert(frame.ts_ns);
            let clock = *self.clock.get_or_insert_with(Instant::now);
            let due = Duration::try_from_secs_f64(frame.ts_ns.saturating_sub(ts) as f64 / 1e9 * factor)
                .map_err(|_| CaptureError::Invalid("replay interval overflow"))?;
            let delay = due.saturating_sub(clock.elapsed());
            if !delay.is_zero() {
                self.stop.wait(delay.min(timeout));
                if self.stop.is_stopped() || clock.elapsed() < due {
                    return Ok(None);
                }
            }
        }
        self.emitted_in_cycle = true;
        self.recv = self
            .recv
            .checked_add(1)
            .ok_or(CaptureError::Limit("replay recv counter"))?;
        // No unbounded read-ahead: only this packet and the reader's bounded block are owned.
        Ok(self.pending.take())
    }
    fn stats(&mut self) -> DropStats {
        DropStats {
            recv: self.recv,
            ..DropStats::ZERO
        }
    }
}
