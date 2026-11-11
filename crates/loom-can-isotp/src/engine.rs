use crate::{
    Error, FaultPlan, FlowStatus, IsoTpConfig, MAX_CONTROLS, MAX_DATA_FRAMES, MAX_OUTPUTS, MAX_TRANSFERS,
    Pci, QueueLimits, decode, encode_cf, encode_fc, encode_ff, encode_sf, st_min_ns,
};
use loom_can_frame::CanFrame;
use std::collections::VecDeque;

/// Transfer identity, fenced by the endpoint's abort generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransferId {
    pub generation: u64,
    pub sequence: u64,
}
/// Opaque identity for one data or FC frame, never reused within an endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameHandle {
    pub generation: u64,
    pub sequence: u64,
}
/// A requested frame; the caller must enforce the earliest submission time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameRequest {
    pub handle: FrameHandle,
    pub transfer_id: Option<TransferId>,
    pub frame: CanFrame,
    pub not_before_ns: u64,
}
/// Transport progress only; ECU response validation belongs to UDS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxState {
    DriverAccepted,
    BusConfirmed,
}
/// Bounded engine outputs. A transfer can emit DriverAccepted then BusConfirmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    TxDone {
        transfer_id: TransferId,
        tx_state: TxState,
    },
    TxError {
        transfer_id: TransferId,
        error: Error,
        residual_tx_possible: usize,
    },
    RxStart {
        generation: u64,
        expected_length: usize,
    },
    RxComplete {
        generation: u64,
        pdu: Vec<u8>,
    },
    RxError {
        generation: u64,
        error: Error,
    },
}
/// Saturating counters, fixed-size regardless of traffic duration.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    pub ignored_flow_controls: u64,
    pub ignored_consecutive_frames: u64,
    pub ignored_generations: u64,
    pub ignored_callbacks: u64,
    pub ignored_identifiers: u64,
    pub output_full: u64,
}
/// Abort also accounts for work already removed from the engine output queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AbortReport {
    pub generation: u64,
    pub aborted_tx: bool,
    pub aborted_rx: bool,
    pub residual_tx_possible: usize,
    pub cancelled_frames: usize,
    pub discarded_events: usize,
}

#[derive(Debug)]
struct Tracked {
    handle: FrameHandle,
    submitted: bool,
    confirmed: bool,
    cancelled: bool,
    earliest: u64,
}
#[derive(Debug)]
struct Transfer {
    id: TransferId,
    frames: Vec<Tracked>,
    sealed: bool,
    failed: bool,
    accepted_event: bool,
    confirmed_event: bool,
}
#[derive(Debug, Clone, Copy)]
enum Next {
    Finish,
    WaitFc,
    Continue,
}
#[derive(Debug)]
struct Tx {
    id: TransferId,
    pdu: Vec<u8>,
    offset: usize,
    sn: u8,
    cf_number: u16,
    block_left: Option<u8>,
    separation: u64,
    last_cf_submit: Option<u64>,
    waits: u8,
    awaiting: Option<(FrameHandle, Next, bool)>,
    fc_deadline: Option<u64>,
    ready: Option<u64>,
}
#[derive(Debug)]
struct Rx {
    token: u64,
    pdu: Vec<u8>,
    length: usize,
    dl: usize,
    sn: u8,
    block_count: u8,
    waits_left: u8,
    deadline: u64,
    awaiting_cts: bool,
    flow: Option<(u64, u8)>,
}
#[derive(Debug)]
struct Control {
    tracked: Tracked,
    token: u64,
    status: FlowStatus,
}

/// One full-duplex endpoint: at most one active PDU per direction, no IO or clock.
#[derive(Debug)]
pub struct Engine {
    cfg: IsoTpConfig,
    limits: QueueLimits,
    generation: u64,
    serial: u64,
    rx_serial: u64,
    now: u64,
    tx: Option<Tx>,
    rx: Option<Rx>,
    transfers: Vec<Transfer>,
    controls: Vec<Control>,
    frames: VecDeque<FrameRequest>,
    events: VecDeque<Event>,
    stats: Stats,
    retired_control_residual: usize,
}
impl Engine {
    /// Validate configuration and allocate bounded queue storage.
    pub fn new(cfg: IsoTpConfig) -> Result<Self, Error> {
        Self::with_limits(cfg, QueueLimits::default())
    }
    /// Two frame and four event slots are reserved for atomic ingress/timer handling.
    pub fn with_limits(cfg: IsoTpConfig, limits: QueueLimits) -> Result<Self, Error> {
        cfg.validate()?;
        if !(2..=MAX_OUTPUTS).contains(&limits.frames)
            || !(4..=MAX_OUTPUTS).contains(&limits.events)
            || !(1..=MAX_TRANSFERS).contains(&limits.transfers)
        {
            return Err(Error::InvalidConfig("queue limits"));
        }
        Ok(Self {
            cfg,
            limits,
            generation: 0,
            serial: 0,
            rx_serial: 0,
            now: 0,
            tx: None,
            rx: None,
            transfers: Vec::with_capacity(limits.transfers),
            controls: Vec::with_capacity(MAX_CONTROLS),
            frames: VecDeque::with_capacity(limits.frames),
            events: VecDeque::with_capacity(limits.events),
            stats: Stats::default(),
            retired_control_residual: 0,
        })
    }
    /// Immutable effective configuration.
    pub fn config(&self) -> &IsoTpConfig {
        &self.cfg
    }
    /// Stamp ingress in the worker before queuing it, and pass this generation back on delivery.
    pub fn generation(&self) -> u64 {
        self.generation
    }
    /// Fixed-size counters; no per-frame history map.
    pub fn stats(&self) -> Stats {
        self.stats
    }
    /// Whether another outgoing PDU would be Busy.
    pub fn is_sending(&self) -> bool {
        self.tx.is_some()
    }
    /// Whether a segmented incoming PDU is in progress.
    pub fn is_receiving(&self) -> bool {
        self.rx.is_some()
    }
    /// Remove the next frame request. Enforce not_before_ns even after removing it.
    pub fn pop_frame(&mut self) -> Option<FrameRequest> {
        self.frames.pop_front()
    }
    /// Remove the oldest event; draining promptly avoids explicit OutputFull errors.
    pub fn pop_event(&mut self) -> Option<Event> {
        self.events.pop_front()
    }
    /// Whether a removed request is still eligible for driver submission.
    /// Recheck on the single arbitration thread immediately before enqueueing TX;
    /// RX replacement can invalidate an FC without changing the endpoint generation.
    pub fn is_frame_pending(&self, handle: FrameHandle) -> bool {
        handle.generation == self.generation
            && (self.transfers.iter().any(|r| {
                r.frames
                    .iter()
                    .any(|f| f.handle == handle && !f.submitted && !f.cancelled)
            }) || self
                .controls
                .iter()
                .any(|c| c.tracked.handle == handle && !c.tracked.submitted))
    }
    /// Current frame/event queue occupancy.
    pub fn output_len(&self) -> (usize, usize) {
        (self.frames.len(), self.events.len())
    }

    fn prepare(&mut self, now: u64, frames: usize, events: usize) -> Result<(), Error> {
        if now < self.now {
            return Err(Error::TimeWentBackwards);
        }
        if self.frames.len() + frames > self.limits.frames || self.events.len() + events > self.limits.events
        {
            self.stats.output_full = self.stats.output_full.saturating_add(1);
            return Err(Error::OutputFull);
        }
        self.now = now;
        Ok(())
    }
    fn handle(&mut self) -> Result<FrameHandle, Error> {
        self.serial = self.serial.checked_add(1).ok_or(Error::CounterExhausted)?;
        Ok(FrameHandle {
            generation: self.generation,
            sequence: self.serial,
        })
    }
    fn deadline(&self, ms: u32) -> u64 {
        self.now.saturating_add(ms as u64 * 1_000_000)
    }
    fn track_data(&mut self, id: TransferId, frame: CanFrame, now: u64) -> Result<FrameHandle, Error> {
        let handle = self.handle()?;
        let record = self
            .transfers
            .iter_mut()
            .find(|r| r.id == id)
            .ok_or(Error::InvalidHandle)?;
        if record.frames.len() == MAX_DATA_FRAMES {
            return Err(Error::ResourceExhausted);
        }
        record.frames.push(Tracked {
            handle,
            submitted: false,
            confirmed: false,
            cancelled: false,
            earliest: now,
        });
        self.frames.push_back(FrameRequest {
            handle,
            transfer_id: Some(id),
            frame,
            not_before_ns: now,
        });
        Ok(handle)
    }
    /// Admit one bounded PDU. A second active send fails Busy without replacing it.
    pub fn send(&mut self, pdu: &[u8], now: u64) -> Result<TransferId, Error> {
        if self.tx.is_some() {
            return Err(Error::Busy);
        }
        if pdu.len() > self.cfg.max_pdu {
            return Err(Error::PduTooLarge {
                len: pdu.len(),
                max: self.cfg.max_pdu,
            });
        }
        if pdu.is_empty() {
            return Err(Error::InvalidLength);
        }
        if self.transfers.len() == self.limits.transfers {
            return Err(Error::ResourceExhausted);
        }
        self.prepare(now, 1, 0)?;
        let single = pdu.len() <= self.cfg.single_frame_capacity();
        let frame = if single {
            encode_sf(&self.cfg, pdu)?
        } else {
            encode_ff(&self.cfg, pdu)?
        };
        let id = TransferId {
            generation: self.generation,
            sequence: self.handle()?.sequence,
        };
        self.transfers.push(Transfer {
            id,
            frames: Vec::with_capacity(MAX_DATA_FRAMES),
            sealed: false,
            failed: false,
            accepted_event: false,
            confirmed_event: false,
        });
        let handle = self.track_data(id, frame, now)?;
        self.tx = Some(Tx {
            id,
            pdu: pdu.to_vec(),
            offset: if single {
                pdu.len()
            } else {
                self.cfg.tx_dl as usize - 2
            },
            sn: 1,
            cf_number: 0,
            block_left: None,
            separation: 0,
            last_cf_submit: None,
            waits: 0,
            awaiting: Some((handle, if single { Next::Finish } else { Next::WaitFc }, false)),
            fc_deadline: None,
            ready: None,
        });
        Ok(id)
    }

    /// Deliver a received frame in the current generation (for synchronous callers).
    pub fn on_frame(&mut self, frame: &CanFrame, now: u64) -> Result<(), Error> {
        self.on_frame_in_generation(frame, self.generation, now)
    }
    /// Deliver stamped ingress. Old generations are ignored, even during a new send.
    /// CAN itself has no transfer generation; physically late, unstamped wire frames
    /// require a worker drain/quarantine boundary before reusing the same address.
    pub fn on_frame_in_generation(
        &mut self,
        frame: &CanFrame,
        generation: u64,
        now: u64,
    ) -> Result<(), Error> {
        if generation != self.generation {
            self.stats.ignored_generations = self.stats.ignored_generations.saturating_add(1);
            return Ok(());
        }
        if frame.id() != self.cfg.rx_id {
            self.stats.ignored_identifiers = self.stats.ignored_identifiers.saturating_add(1);
            return Ok(());
        }
        self.prepare(now, 1, 4)?;
        // Expiry is checked before ingress, including at the exact deadline.
        self.expire();
        if frame.is_fd() != self.cfg.can_fd {
            self.fail_rx(Error::InvalidLength);
            return Ok(());
        }
        let pci = match decode(frame) {
            Ok(p) => p,
            Err(error) => {
                // Expiry above may already have consumed two event slots.
                if self.events.len() == self.limits.events {
                    return Err(Error::OutputFull);
                }
                let pci_type = frame.data().first().map(|v| v >> 4);
                if pci_type == Some(3) && self.tx.is_some() {
                    self.fail_tx(error);
                } else if pci_type == Some(1) && matches!(error, Error::PduTooLarge { .. }) {
                    // 32-bit escape FF_DL above 4095: valid on the wire but larger than any
                    // buffer here. ISO 15765-2 answers FC OVFLW so the sender stops at once
                    // instead of waiting for N_Bs.
                    if self.rx.is_some() {
                        self.fail_rx(Error::UnexpectedPdu);
                    }
                    self.events.push_back(Event::RxError {
                        generation: self.generation,
                        error,
                    });
                    self.control(0, FlowStatus::Overflow)?;
                } else {
                    self.fail_rx(error);
                }
                return Ok(());
            }
        };
        match pci {
            Pci::Single(pdu) => {
                if self.rx.is_some() {
                    self.fail_rx(Error::UnexpectedPdu);
                }
                if pdu.len() > self.cfg.max_pdu {
                    self.fail_rx(Error::PduTooLarge {
                        len: pdu.len(),
                        max: self.cfg.max_pdu,
                    });
                } else {
                    self.events.push_back(Event::RxComplete {
                        generation: self.generation,
                        pdu: pdu.to_vec(),
                    });
                }
            }
            Pci::First { length, data } => {
                if self.rx.is_some() {
                    self.fail_rx(Error::UnexpectedPdu);
                }
                if length > self.cfg.max_pdu {
                    self.events.push_back(Event::RxError {
                        generation: self.generation,
                        error: Error::PduTooLarge {
                            len: length,
                            max: self.cfg.max_pdu,
                        },
                    });
                    self.control(0, FlowStatus::Overflow)?;
                } else {
                    self.rx_serial = self.rx_serial.checked_add(1).ok_or(Error::CounterExhausted)?;
                    let mut pdu = Vec::with_capacity(self.cfg.max_pdu);
                    pdu.extend_from_slice(data);
                    let due = self.deadline(self.cfg.faults.fc_delay_ms);
                    self.rx = Some(Rx {
                        token: self.rx_serial,
                        pdu,
                        length,
                        dl: frame.len(),
                        sn: 1,
                        block_count: 0,
                        waits_left: self.cfg.faults.fc_wait_count,
                        deadline: due.saturating_add(self.cfg.n_cr_ms as u64 * 1_000_000),
                        awaiting_cts: true,
                        flow: Some((due, self.cfg.faults.fc_wait_count)),
                    });
                    self.events.push_back(Event::RxStart {
                        generation: self.generation,
                        expected_length: length,
                    });
                    self.emit_flow()?;
                }
            }
            Pci::Consecutive { sequence, data } => self.receive_cf(sequence, data, frame.len())?,
            Pci::FlowControl {
                status,
                block_size,
                st_min,
            } => self.receive_fc(status, block_size, st_min)?,
        }
        Ok(())
    }
    fn receive_cf(&mut self, sn: u8, data: &[u8], dl: usize) -> Result<(), Error> {
        let Some(rx) = self.rx.as_ref() else {
            self.stats.ignored_consecutive_frames = self.stats.ignored_consecutive_frames.saturating_add(1);
            return Ok(());
        };
        let error = if rx.awaiting_cts {
            Some(Error::UnexpectedPdu)
        } else if sn != rx.sn {
            Some(Error::WrongSequence {
                expected: rx.sn,
                actual: sn,
            })
        } else if (data.len() < rx.length - rx.pdu.len() && dl != rx.dl) || dl > rx.dl {
            Some(Error::InvalidLength)
        } else {
            None
        };
        if let Some(e) = error {
            self.fail_rx(e);
            return Ok(());
        }
        // Peer progress proves the current block's FC is no longer cancellable pending TX.
        self.controls.retain(|c| c.token != rx.token);
        let rx = self.rx.as_mut().ok_or(Error::InvalidHandle)?;
        let count = data.len().min(rx.length - rx.pdu.len());
        rx.pdu.extend_from_slice(&data[..count]);
        rx.sn = (rx.sn + 1) & 15;
        rx.block_count = rx.block_count.saturating_add(1);
        rx.deadline = self.now.saturating_add(self.cfg.n_cr_ms as u64 * 1_000_000);
        if rx.pdu.len() == rx.length {
            let rx = self.rx.take().ok_or(Error::InvalidHandle)?;
            self.events.push_back(Event::RxComplete {
                generation: self.generation,
                pdu: rx.pdu,
            });
        } else if self.cfg.block_size != 0 && rx.block_count >= self.cfg.block_size {
            rx.block_count = 0;
            rx.awaiting_cts = true;
            rx.flow = Some((
                self.now
                    .saturating_add(self.cfg.faults.fc_delay_ms as u64 * 1_000_000),
                self.cfg.faults.fc_wait_count,
            ));
            self.emit_flow()?;
        }
        Ok(())
    }
    fn receive_fc(&mut self, status: FlowStatus, bs: u8, st: u8) -> Result<(), Error> {
        let Some(tx) = self.tx.as_mut().filter(|t| t.fc_deadline.is_some()) else {
            self.stats.ignored_flow_controls = self.stats.ignored_flow_controls.saturating_add(1);
            return Ok(());
        };
        match status {
            FlowStatus::Continue => {
                tx.waits = 0;
                tx.fc_deadline = None;
                tx.block_left = if bs == 0 { None } else { Some(bs) };
                tx.separation = st_min_ns(st);
                tx.ready = Some(
                    tx.last_cf_submit
                        .map_or(self.now, |last| self.now.max(last.saturating_add(tx.separation))),
                );
                self.emit_cf()?;
            }
            FlowStatus::Wait => {
                if tx.waits >= self.cfg.wft_max {
                    self.fail_tx(Error::WaitLimit);
                } else {
                    tx.waits += 1;
                    tx.fc_deadline = Some(self.now.saturating_add(self.cfg.n_bs_ms as u64 * 1_000_000));
                }
            }
            FlowStatus::Overflow => self.fail_tx(Error::FlowOverflow),
        }
        Ok(())
    }
    fn control(&mut self, token: u64, status: FlowStatus) -> Result<(), Error> {
        if self.controls.len() == MAX_CONTROLS {
            return Err(Error::ResourceExhausted);
        }
        let frame = encode_fc(&self.cfg, status, self.cfg.block_size, self.cfg.st_min)?;
        let handle = self.handle()?;
        self.controls.push(Control {
            tracked: Tracked {
                handle,
                submitted: false,
                confirmed: false,
                cancelled: false,
                earliest: self.now,
            },
            token,
            status,
        });
        self.frames.push_back(FrameRequest {
            handle,
            transfer_id: None,
            frame,
            not_before_ns: self.now,
        });
        Ok(())
    }
    fn emit_flow(&mut self) -> Result<(), Error> {
        let Some(rx) = self.rx.as_ref() else {
            return Ok(());
        };
        let Some((due, waits)) = rx.flow.filter(|(due, _)| *due <= self.now) else {
            return Ok(());
        };
        let _ = due;
        let token = rx.token;
        let status = if self.cfg.faults.fc_overflow {
            FlowStatus::Overflow
        } else if waits > 0 {
            FlowStatus::Wait
        } else {
            FlowStatus::Continue
        };
        self.control(token, status)?;
        let rx = self.rx.as_mut().ok_or(Error::InvalidHandle)?;
        rx.flow = None;
        rx.waits_left = waits.saturating_sub(1);
        if status == FlowStatus::Overflow {
            // Keep OVFLW in the output, but do not accept CF after it.
            self.rx = None;
            self.events.push_back(Event::RxError {
                generation: self.generation,
                error: Error::FlowOverflow,
            });
        }
        Ok(())
    }
    fn emit_cf(&mut self) -> Result<(), Error> {
        let Some(tx) = self
            .tx
            .as_mut()
            .filter(|t| t.ready.is_some_and(|at| at <= self.now))
        else {
            return Ok(());
        };
        let count = (self.cfg.tx_dl as usize - 1).min(tx.pdu.len() - tx.offset);
        let number = tx.cf_number + 1;
        let sequence = if self.cfg.faults.wrong_sn_cf_number == Some(number) {
            (tx.sn + 1) & 15
        } else {
            tx.sn
        };
        let frame = encode_cf(&self.cfg, sequence, &tx.pdu[tx.offset..tx.offset + count])?;
        tx.cf_number = number;
        tx.offset += count;
        tx.sn = (tx.sn + 1) & 15;
        tx.ready = None;
        if let Some(left) = tx.block_left.as_mut() {
            *left -= 1;
        }
        let next = if tx.offset == tx.pdu.len() {
            Next::Finish
        } else if tx.block_left == Some(0) {
            Next::WaitFc
        } else {
            Next::Continue
        };
        let id = tx.id;
        if self.cfg.faults.drop_cf_number == Some(number) {
            self.advance_tx(next, true);
            self.complete(id);
        } else {
            let handle = self.track_data(id, frame, self.now)?;
            self.tx.as_mut().ok_or(Error::InvalidHandle)?.awaiting = Some((handle, next, true));
        }
        Ok(())
    }
    fn advance_tx(&mut self, next: Next, is_cf: bool) {
        let Some(tx) = self.tx.as_mut() else {
            return;
        };
        tx.awaiting = None;
        if is_cf {
            tx.last_cf_submit = Some(self.now);
        }
        match next {
            Next::Finish => {
                let id = tx.id;
                if let Some(r) = self.transfers.iter_mut().find(|r| r.id == id) {
                    r.sealed = true;
                }
                self.tx = None;
            }
            Next::WaitFc => {
                tx.fc_deadline = Some(self.now.saturating_add(self.cfg.n_bs_ms as u64 * 1_000_000))
            }
            Next::Continue => tx.ready = Some(self.now.saturating_add(tx.separation)),
        }
    }
    fn complete(&mut self, id: TransferId) {
        let Some(r) = self.transfers.iter_mut().find(|r| r.id == id) else {
            return;
        };
        if !r.sealed || r.failed || r.frames.iter().any(|f| !f.submitted) {
            return;
        }
        if !r.accepted_event {
            r.accepted_event = true;
            self.events.push_back(Event::TxDone {
                transfer_id: id,
                tx_state: TxState::DriverAccepted,
            });
        }
        if r.frames.iter().all(|f| f.confirmed) && !r.confirmed_event {
            r.confirmed_event = true;
            self.events.push_back(Event::TxDone {
                transfer_id: id,
                tx_state: TxState::BusConfirmed,
            });
        }
        if r.confirmed_event {
            self.transfers.retain(|r| r.id != id);
        }
    }
    /// Submission at the most recently supplied clock instant. Prefer the explicit `_at` method.
    pub fn on_frame_submitted(&mut self, handle: FrameHandle) -> Result<(), Error> {
        self.on_frame_submitted_at(handle, self.now)
    }
    /// Report actual driver submission time; starts N_Bs or the next STmin interval.
    pub fn on_frame_submitted_at(&mut self, handle: FrameHandle, now: u64) -> Result<(), Error> {
        self.prepare(now, 0, 2)?;
        for record in &mut self.transfers {
            if let Some(f) = record
                .frames
                .iter_mut()
                .find(|f| f.handle == handle && !f.cancelled)
            {
                if now < f.earliest {
                    return Err(Error::SubmissionTooEarly);
                }
                if f.submitted {
                    return Ok(());
                }
                f.submitted = true;
                let id = record.id;
                if let Some((_, next, cf)) = self
                    .tx
                    .as_ref()
                    .and_then(|t| t.awaiting)
                    .filter(|(h, _, _)| *h == handle)
                {
                    self.advance_tx(next, cf);
                }
                self.complete(id);
                return Ok(());
            }
        }
        if let Some(i) = self.controls.iter().position(|c| c.tracked.handle == handle) {
            let token = self.controls[i].token;
            if self.controls[i].tracked.submitted {
                return Ok(());
            }
            if self.rx.as_ref().is_none_or(|rx| rx.token != token) {
                // Terminal FC (OVFLW for an oversized FF, or the OVFLW fault): no receive
                // state waits for it and no peer CF will retire it. Without bus confirms it
                // would stay forever and exhaust MAX_CONTROLS, so retire this one record now.
                // Only this handle: other OVFLW controls share token 0 and may still be queued.
                self.controls.remove(i);
                self.retired_control_residual = self.retired_control_residual.saturating_add(1);
                return Ok(());
            }
            let c = &mut self.controls[i];
            c.tracked.submitted = true;
            if let Some(rx) = self.rx.as_mut().filter(|rx| rx.token == token) {
                rx.deadline = now.saturating_add(self.cfg.n_cr_ms as u64 * 1_000_000);
                if c.status == FlowStatus::Continue {
                    rx.awaiting_cts = false;
                }
                if c.status == FlowStatus::Wait {
                    let left = rx.waits_left;
                    let due = now.saturating_add(self.cfg.faults.fc_delay_ms as u64 * 1_000_000);
                    rx.flow = Some((due, left));
                    rx.deadline = due.saturating_add(self.cfg.n_cr_ms as u64 * 1_000_000);
                }
            }
            return Ok(());
        }
        self.stats.ignored_callbacks = self.stats.ignored_callbacks.saturating_add(1);
        Ok(())
    }
    /// Optional bus TX completion. Never call this for a mere successful driver write.
    pub fn on_frame_confirmed(&mut self, handle: FrameHandle) -> Result<(), Error> {
        self.prepare(self.now, 0, 2)?;
        for r in &mut self.transfers {
            if let Some(f) = r.frames.iter_mut().find(|f| f.handle == handle && !f.cancelled) {
                if !f.submitted {
                    return Err(Error::ConfirmationBeforeSubmission);
                }
                f.confirmed = true;
                let id = r.id;
                self.complete(id);
                return Ok(());
            }
        }
        if let Some(i) = self.controls.iter().position(|c| c.tracked.handle == handle) {
            if !self.controls[i].tracked.submitted {
                return Err(Error::ConfirmationBeforeSubmission);
            }
            self.controls.remove(i);
            return Ok(());
        }
        self.stats.ignored_callbacks = self.stats.ignored_callbacks.saturating_add(1);
        Ok(())
    }
    /// Release a terminal transfer after DriverAccepted or failure, e.g. for a backend
    /// without TX confirmations. Returns unconfirmed submitted frames conservatively.
    /// Late callbacks after release are ignored; the caller owns confirmation timeout.
    pub fn release_transfer(&mut self, id: TransferId) -> Result<usize, Error> {
        if self.tx.as_ref().is_some_and(|t| t.id == id) {
            return Err(Error::Busy);
        }
        let i = self
            .transfers
            .iter()
            .position(|r| r.id == id)
            .ok_or(Error::InvalidHandle)?;
        let r = self.transfers.remove(i);
        Ok(r.frames.iter().filter(|f| f.submitted && !f.confirmed).count())
    }
    fn retire_controls(&mut self, token: u64) {
        let handles: Vec<_> = self
            .controls
            .iter()
            .filter(|c| c.token == token)
            .map(|c| c.tracked.handle)
            .collect();
        self.frames.retain(|f| !handles.contains(&f.handle));
        self.retired_control_residual = self.retired_control_residual.saturating_add(
            self.controls
                .iter()
                .filter(|c| c.token == token && c.tracked.submitted && !c.tracked.confirmed)
                .count(),
        );
        self.controls.retain(|c| c.token != token);
    }
    fn fail_rx(&mut self, error: Error) {
        if let Some(rx) = self.rx.take() {
            self.retire_controls(rx.token);
        }
        self.events.push_back(Event::RxError {
            generation: self.generation,
            error,
        });
    }
    fn fail_tx(&mut self, error: Error) {
        let Some(tx) = self.tx.take() else {
            return;
        };
        self.frames.retain(|f| f.transfer_id != Some(tx.id));
        if let Some(r) = self.transfers.iter_mut().find(|r| r.id == tx.id) {
            r.failed = true;
            r.sealed = true;
            for f in &mut r.frames {
                if !f.submitted {
                    f.cancelled = true;
                }
            }
            self.events.push_back(Event::TxError {
                transfer_id: tx.id,
                error,
                residual_tx_possible: r.frames.iter().filter(|f| f.submitted && !f.confirmed).count(),
            });
        }
    }
    fn expire(&mut self) {
        if self
            .tx
            .as_ref()
            .is_some_and(|t| t.fc_deadline.is_some_and(|at| self.now >= at))
        {
            self.fail_tx(Error::NBsTimeout);
        }
        if self.rx.as_ref().is_some_and(|r| self.now >= r.deadline) {
            self.fail_rx(Error::NCrTimeout);
        }
    }
    /// Earliest protocol wakeup. Driver submission itself has no engine-owned timeout.
    pub fn poll_at(&self) -> Option<u64> {
        let mut next = None;
        for at in [
            self.tx.as_ref().and_then(|t| t.ready),
            self.tx.as_ref().and_then(|t| t.fc_deadline),
            self.rx.as_ref().map(|r| r.deadline),
            self.rx.as_ref().and_then(|r| r.flow.map(|p| p.0)),
        ]
        .into_iter()
        .flatten()
        {
            next = Some(next.map_or(at, |old: u64| old.min(at)));
        }
        next
    }
    /// Advance timers; at most one CF and one FC are produced per call, even at STmin0.
    pub fn on_timer(&mut self, now: u64) -> Result<(), Error> {
        self.prepare(now, 2, 3)?;
        self.expire();
        // Neither emitter can complete a PDU after expire removed its state.
        self.emit_cf()?;
        self.emit_flow()?;
        Ok(())
    }
    /// Abort cannot be blocked by full output queues: it explicitly discards them,
    /// reports the counts, resets both directions and changes the generation.
    pub fn abort(&mut self, now: u64) -> Result<AbortReport, Error> {
        if now < self.now {
            return Err(Error::TimeWentBackwards);
        }
        let generation = self.generation.checked_add(1).ok_or(Error::CounterExhausted)?;
        let residual = self
            .transfers
            .iter()
            .flat_map(|r| &r.frames)
            .filter(|f| f.submitted && !f.confirmed)
            .count()
            .saturating_add(
                self.controls
                    .iter()
                    .filter(|c| c.tracked.submitted && !c.tracked.confirmed)
                    .count(),
            )
            .saturating_add(self.retired_control_residual);
        let cancelled = self
            .transfers
            .iter()
            .flat_map(|r| &r.frames)
            .filter(|f| !f.submitted && !f.cancelled)
            .count()
            .saturating_add(self.controls.iter().filter(|c| !c.tracked.submitted).count());
        let tx = self.tx.take();
        let rx = self.rx.take();
        let result = AbortReport {
            generation,
            aborted_tx: tx.is_some(),
            aborted_rx: rx.is_some(),
            residual_tx_possible: residual,
            cancelled_frames: cancelled,
            discarded_events: self.events.len(),
        };
        self.now = now;
        self.frames.clear();
        self.events.clear();
        self.transfers.clear();
        self.controls.clear();
        self.retired_control_residual = 0;
        if let Some(tx) = tx {
            self.events.push_back(Event::TxError {
                transfer_id: tx.id,
                error: Error::Aborted,
                residual_tx_possible: residual,
            });
        }
        if rx.is_some() {
            self.events.push_back(Event::RxError {
                generation: self.generation,
                error: Error::Aborted,
            });
        }
        self.generation = generation;
        Ok(result)
    }
    /// Current simulation plan; immutable for this endpoint, so active transfers cannot change underneath it.
    pub fn fault_plan(&self) -> FaultPlan {
        self.cfg.faults
    }
}
