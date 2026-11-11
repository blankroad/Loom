//! `SimHub`: sim buses (by name), endpoints, STOP latch and SimNodes.
//!
//! Single-threaded by design: the caller (the can-sim worker loop) calls [`SimHub::send`]
//! for client frames and [`SimHub::poll`] when time moves on. The hub never spawns threads
//! and never sleeps. [`SimHub::next_deadline_ns`] says when the next node event is due.
//!
//! Time model: frames carry the bus time at which they happened. Node replies and periodic
//! frames happen at their deadline even if `poll` runs later (discrete-event simulation),
//! so intervals are exact. Bus time never goes backwards: a frame is stamped
//! `max(time, last bus time)`, so `source_seq` order and time order agree.
//!
//! Overflow policy (C2):
//! - endpoint queue full → that endpoint misses the record; the bus counts a drop gap for it
//!   (first/last `source_seq`, count). Nobody else is slowed down.
//! - more than `max_pending_per_node` scheduled replies for a node → the new reply is not
//!   scheduled; `deadline_overflow` counts it (periodic frames always keep their one slot).
//! - more than `max_events_per_poll` due events → the rest wait for the next `poll`
//!   (`PollReport::more_pending`).
//! - bus/endpoint/node counts above the limits → `SimError`.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, VecDeque};

use loom_can_frame::{CanFrame, CanId, FD_MAX_LEN, TimedFrame};

use crate::clock::Clock;
use crate::config::{NodeConfig, ReplyData};
use crate::endpoint::{DEFAULT_QUEUE_FRAMES, Endpoint, LocalReceiver, local_endpoint};
use crate::error::SimError;
use crate::record::{BusId, EndpointId, NodeId, Origin, Sender, SimRecord};

/// Drop gaps kept per endpoint; older ones are only counted.
pub const MAX_GAPS_KEPT: usize = 32;

/// Size limits of a hub (plan §9.1: 16 endpoints per bus, 8,192-frame queues).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HubLimits {
    pub max_buses: usize,
    pub max_endpoints_per_bus: usize,
    /// Node slots (running, stopped or crashed).
    pub max_nodes: usize,
    pub max_pending_per_node: usize,
    pub max_events_per_poll: usize,
}

impl Default for HubLimits {
    fn default() -> Self {
        Self {
            max_buses: 16,
            max_endpoints_per_bus: 16,
            max_nodes: 16,
            max_pending_per_node: 1_024,
            max_events_per_poll: 65_536,
        }
    }
}

/// A run of consecutive records an endpoint did not get.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gap {
    pub first_seq: u64,
    pub last_seq: u64,
    pub count: u64,
}

/// Drops of one endpoint (basis for `display.gap` / `record.gap`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DropStats {
    pub total: u64,
    /// Most recent gaps, oldest first (at most [`MAX_GAPS_KEPT`]).
    pub gaps: Vec<Gap>,
    /// Older gaps no longer listed.
    pub gaps_evicted: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointStats {
    pub id: EndpointId,
    pub name: String,
    pub origin: Origin,
    pub delivered: u64,
    pub drops: DropStats,
}

/// Counters for one bus. fps = difference of `frames_total` over time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BusStats {
    pub id: BusId,
    pub name: String,
    /// Last assigned `source_seq` (0 = none yet).
    pub last_seq: u64,
    pub last_time_ns: u64,
    pub frames_total: u64,
    /// Indexed like [`Origin::ALL`].
    pub frames_by_origin: [u64; 4],
    pub latched: Vec<Origin>,
    /// Sends refused by the latch, indexed like [`Origin::ALL`].
    pub latch_rejected: [u64; 4],
    pub endpoints: Vec<EndpointStats>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeState {
    Running,
    Stopped,
    /// Stopped by the crash fault (or [`SimHub::crash_node`]).
    Crashed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeInfo {
    pub id: NodeId,
    pub name: String,
    pub channel: String,
    pub state: NodeState,
    pub tx_frames: u64,
    /// Frames not sent because of the `drop_ids` fault.
    pub fault_dropped: u64,
    /// Replies not scheduled because the node's deadline queue was full.
    pub deadline_overflow: u64,
    /// Rule replies whose frame could not be built (e.g. FD-length copy into Classic).
    pub reply_errors: u64,
    pub pending: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PollReport {
    /// Node events handled (frames sent or dropped by fault).
    pub events: usize,
    /// Due events left for the next poll (`max_events_per_poll` reached).
    pub more_pending: bool,
}

struct Slot {
    generation: u32,
    name: String,
    origin: Origin,
    endpoint: Box<dyn Endpoint>,
    delivered: u64,
    drops: DropStats,
    in_gap: bool,
    gaps: VecDeque<Gap>,
}

struct Bus {
    name: String,
    slots: Vec<Option<Slot>>,
    next_seq: u64,
    last_time: u64,
    latched: [bool; 4],
    frames_by_origin: [u64; 4],
    latch_rejected: [u64; 4],
}

impl Bus {
    /// Assigns seq and time and offers the record to every endpoint in slot order.
    fn publish(
        &mut self,
        id: BusId,
        origin: Origin,
        sender: Sender,
        frame: CanFrame,
        time: u64,
    ) -> SimRecord {
        let time = time.max(self.last_time);
        self.last_time = time;
        let seq = self.next_seq;
        self.next_seq += 1;
        self.frames_by_origin[origin.index()] += 1;
        let record = SimRecord {
            bus: id,
            source_seq: seq,
            origin,
            sender,
            frame: TimedFrame {
                frame,
                dir: origin.direction(),
                ingest_mono_ns: time,
                device_ts: None,
            },
        };
        for slot in self.slots.iter_mut().flatten() {
            if slot.endpoint.offer(&record) {
                slot.delivered += 1;
                slot.in_gap = false;
                continue;
            }
            slot.drops.total += 1;
            match slot.gaps.back_mut() {
                Some(g) if slot.in_gap => {
                    g.last_seq = seq;
                    g.count += 1;
                }
                _ => {
                    if slot.gaps.len() == MAX_GAPS_KEPT {
                        slot.gaps.pop_front();
                        slot.drops.gaps_evicted += 1;
                    }
                    slot.gaps.push_back(Gap {
                        first_seq: seq,
                        last_seq: seq,
                        count: 1,
                    });
                }
            }
            slot.in_gap = true;
        }
        record
    }
}

/// A `follow` source id and the last payload seen with it (`None` = not yet).
type FollowSource = (CanId, Option<([u8; FD_MAX_LEN], u8)>);

struct NodeRt {
    generation: u32,
    cfg: NodeConfig,
    bus: u16,
    state: NodeState,
    pending: usize,
    tx_frames: u64,
    fault_dropped: u64,
    deadline_overflow: u64,
    reply_errors: u64,
    rng: u64,
    /// Ids used as `follow` sources, with the last payload seen (None = not yet).
    follow_sources: Vec<FollowSource>,
}

impl NodeRt {
    fn id(&self, slot: usize) -> NodeId {
        NodeId {
            slot: slot as u16,
            generation: self.generation,
        }
    }

    /// splitmix64: small, seeded, repeatable.
    fn next_random(&mut self) -> u64 {
        self.rng = self.rng.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.rng;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

#[derive(Clone, Copy)]
enum EventKind {
    Reply(CanFrame),
    Periodic(u16),
    Crash,
}

struct Event {
    due: u64,
    order: u64,
    node: u16,
    generation: u32,
    kind: EventKind,
}

impl PartialEq for Event {
    fn eq(&self, other: &Self) -> bool {
        self.due == other.due && self.order == other.order
    }
}
impl Eq for Event {}
impl PartialOrd for Event {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Event {
    /// Reversed so `BinaryHeap` pops the earliest (then first scheduled) event.
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .due
            .cmp(&self.due)
            .then_with(|| other.order.cmp(&self.order))
    }
}

/// Owner of all sim buses, endpoints and nodes of one can-sim worker.
pub struct SimHub {
    clock: Box<dyn Clock>,
    limits: HubLimits,
    buses: Vec<Bus>,
    bus_names: HashMap<String, u16>,
    nodes: Vec<Option<NodeRt>>,
    events: BinaryHeap<Event>,
    event_order: u64,
    next_generation: u32,
}

impl std::fmt::Debug for SimHub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SimHub")
            .field("buses", &self.buses.len())
            .field("nodes", &self.nodes.iter().filter(|n| n.is_some()).count())
            .field("pending_events", &self.events.len())
            .field("limits", &self.limits)
            .finish()
    }
}

impl SimHub {
    pub fn new(clock: impl Clock + 'static, limits: HubLimits) -> Self {
        Self {
            clock: Box::new(clock),
            limits,
            buses: Vec::new(),
            bus_names: HashMap::new(),
            nodes: Vec::new(),
            events: BinaryHeap::new(),
            event_order: 0,
            next_generation: 1,
        }
    }

    pub fn now_ns(&self) -> u64 {
        self.clock.now_ns()
    }

    pub fn limits(&self) -> &HubLimits {
        &self.limits
    }

    pub fn bus_id(&self, name: &str) -> Option<BusId> {
        self.bus_names.get(name).map(|&i| BusId(i))
    }

    fn bus_index(&self, name: &str) -> Result<u16, SimError> {
        self.bus_names.get(name).copied().ok_or(SimError::UnknownBus {
            name: name.to_string(),
        })
    }

    /// The bus with this name; created on first use. Same name = same bus.
    fn bus_or_create(&mut self, name: &str) -> Result<u16, SimError> {
        if let Some(&i) = self.bus_names.get(name) {
            return Ok(i);
        }
        if self.buses.len() >= self.limits.max_buses {
            return Err(SimError::TooManyBuses {
                limit: self.limits.max_buses,
            });
        }
        let i = self.buses.len() as u16;
        self.buses.push(Bus {
            name: name.to_string(),
            slots: Vec::new(),
            next_seq: 1,
            last_time: 0,
            latched: [false; 4],
            frames_by_origin: [0; 4],
            latch_rejected: [0; 4],
        });
        self.bus_names.insert(name.to_string(), i);
        Ok(i)
    }

    fn generation(&mut self) -> u32 {
        let g = self.next_generation;
        self.next_generation = self.next_generation.wrapping_add(1).max(1);
        g
    }

    /// Attaches an endpoint to bus `bus`. `origin` comes from the connection role
    /// (the endpoint cannot choose it per frame).
    pub fn attach(
        &mut self,
        bus: &str,
        name: &str,
        origin: Origin,
        endpoint: Box<dyn Endpoint>,
    ) -> Result<EndpointId, SimError> {
        let b = self.bus_or_create(bus)?;
        let generation = self.generation();
        let max = self.limits.max_endpoints_per_bus;
        let slots = &mut self.buses[b as usize].slots;
        let free = slots.iter().position(Option::is_none);
        let slot = match free {
            Some(i) => i,
            None if slots.len() < max => {
                slots.push(None);
                slots.len() - 1
            }
            None => {
                return Err(SimError::TooManyEndpoints {
                    bus: bus.to_string(),
                    limit: max,
                });
            }
        };
        slots[slot] = Some(Slot {
            generation,
            name: name.to_string(),
            origin,
            endpoint,
            delivered: 0,
            drops: DropStats::default(),
            in_gap: false,
            gaps: VecDeque::new(),
        });
        Ok(EndpointId {
            bus: b,
            slot: slot as u16,
            generation,
        })
    }

    /// Attaches an in-process endpoint with the default 8,192-record queue.
    pub fn attach_local(
        &mut self,
        bus: &str,
        name: &str,
        origin: Origin,
    ) -> Result<(EndpointId, LocalReceiver), SimError> {
        self.attach_local_with_capacity(bus, name, origin, DEFAULT_QUEUE_FRAMES)
    }

    pub fn attach_local_with_capacity(
        &mut self,
        bus: &str,
        name: &str,
        origin: Origin,
        capacity: usize,
    ) -> Result<(EndpointId, LocalReceiver), SimError> {
        let (ep, rx) = local_endpoint(capacity);
        let id = self.attach(bus, name, origin, Box::new(ep))?;
        Ok((id, rx))
    }

    fn slot(&self, id: EndpointId) -> Result<&Slot, SimError> {
        self.buses
            .get(id.bus as usize)
            .and_then(|b| b.slots.get(id.slot as usize))
            .and_then(Option::as_ref)
            .filter(|s| s.generation == id.generation)
            .ok_or(SimError::UnknownEndpoint)
    }

    pub fn detach(&mut self, id: EndpointId) -> Result<(), SimError> {
        self.slot(id)?;
        self.buses[id.bus as usize].slots[id.slot as usize] = None;
        Ok(())
    }

    /// Puts a client frame on the bus. Returns its `source_seq`. The sender also gets
    /// the record (Tx echo, same seq). Refused with `Latched` while STOP holds its origin.
    pub fn send(&mut self, from: EndpointId, frame: CanFrame) -> Result<u64, SimError> {
        let origin = self.slot(from)?.origin;
        let now = self.clock.now_ns();
        let bus = &mut self.buses[from.bus as usize];
        if bus.latched[origin.index()] {
            bus.latch_rejected[origin.index()] += 1;
            return Err(SimError::Latched { origin });
        }
        let record = bus.publish(BusId(from.bus), origin, Sender::Endpoint(from), frame, now);
        self.dispatch_to_nodes(&record);
        Ok(record.source_seq)
    }

    /// STOP (A3): refuse new frames from these origins until [`SimHub::release`].
    /// `Device` cannot be latched: SimNodes stand for ECUs, which keep running.
    pub fn latch(&mut self, bus: &str, origins: &[Origin]) -> Result<(), SimError> {
        let b = self.bus_index(bus)?;
        for o in origins {
            if *o != Origin::Device {
                self.buses[b as usize].latched[o.index()] = true;
            }
        }
        Ok(())
    }

    /// Clears the latch. Only an explicit call releases it.
    pub fn release(&mut self, bus: &str) -> Result<(), SimError> {
        let b = self.bus_index(bus)?;
        self.buses[b as usize].latched = [false; 4];
        Ok(())
    }

    /// Time of the earliest scheduled node event.
    pub fn next_deadline_ns(&self) -> Option<u64> {
        self.events.peek().map(|e| e.due)
    }

    /// Runs node events that are due at the current clock time.
    pub fn poll(&mut self) -> PollReport {
        let now = self.clock.now_ns();
        let mut report = PollReport::default();
        while let Some(ev) = self.events.peek() {
            if ev.due > now {
                break;
            }
            if report.events >= self.limits.max_events_per_poll {
                report.more_pending = true;
                break;
            }
            let ev = self.events.pop().expect("peeked");
            let slot = ev.node as usize;
            let Some(node) = self.nodes.get_mut(slot).and_then(Option::as_mut) else {
                continue;
            };
            if node.generation != ev.generation || node.state != NodeState::Running {
                continue;
            }
            node.pending -= 1;
            report.events += 1;
            match ev.kind {
                EventKind::Crash => self.halt_node(slot, NodeState::Crashed),
                EventKind::Reply(frame) => self.emit_from_node(slot, frame, ev.due),
                EventKind::Periodic(i) => {
                    let p = &node.cfg.periodic[i as usize];
                    let next = ev.due + p.period_ns;
                    let frame = periodic_frame(node, i as usize);
                    schedule(
                        &mut self.events,
                        &mut self.event_order,
                        node,
                        slot,
                        next,
                        EventKind::Periodic(i),
                        usize::MAX,
                    );
                    if let Some(frame) = frame {
                        self.emit_from_node(slot, frame, ev.due);
                    }
                }
            }
        }
        report
    }

    fn emit_from_node(&mut self, slot: usize, frame: CanFrame, time: u64) {
        let node = self.nodes[slot].as_mut().expect("caller checked");
        if node.cfg.faults.drop_ids.contains(&frame.id()) {
            node.fault_dropped += 1;
            return;
        }
        node.tx_frames += 1;
        let id = node.id(slot);
        let b = node.bus;
        let record = self.buses[b as usize].publish(BusId(b), Origin::Device, Sender::Node(id), frame, time);
        self.dispatch_to_nodes(&record);
    }

    /// Lets every running node on the record's bus (except its sender) react.
    fn dispatch_to_nodes(&mut self, rec: &SimRecord) {
        let max_pending = self.limits.max_pending_per_node;
        for slot in 0..self.nodes.len() {
            let Some(node) = self.nodes[slot].as_mut() else {
                continue;
            };
            if node.state != NodeState::Running
                || node.bus != rec.bus.0
                || rec.sender == Sender::Node(node.id(slot))
            {
                continue;
            }
            let frame = &rec.frame.frame;
            for (id, last) in node.follow_sources.iter_mut() {
                if *id == frame.id() {
                    let mut buf = [0u8; FD_MAX_LEN];
                    buf[..frame.data().len()].copy_from_slice(frame.data());
                    *last = Some((buf, frame.data().len() as u8));
                }
            }
            for r in 0..node.cfg.rules.len() {
                let rule = &node.cfg.rules[r];
                if rule.id != frame.id() || !matches(&rule.data, &rule.mask, frame.data()) {
                    continue;
                }
                let data = match &rule.reply {
                    ReplyData::Copy => frame.data(),
                    ReplyData::Fixed(b) => b.as_slice(),
                };
                let built = if rule.fd || frame.is_fd() {
                    CanFrame::fd(rule.reply_id, data, true, false)
                } else {
                    CanFrame::classic(rule.reply_id, data)
                };
                let Ok(reply) = built else {
                    node.reply_errors += 1;
                    continue;
                };
                let mut delay = rule.delay_ns + node.cfg.faults.extra_delay_ns;
                let jitter = node.cfg.faults.jitter_ns;
                if jitter > 0 {
                    delay += node.next_random() % (jitter + 1);
                }
                let due = rec.frame.ingest_mono_ns + delay;
                schedule(
                    &mut self.events,
                    &mut self.event_order,
                    node,
                    slot,
                    due,
                    EventKind::Reply(reply),
                    max_pending,
                );
            }
        }
    }

    /// Starts a node on its channel (bus created if needed). Periodic frames start at
    /// now + offset; the crash fault, if set, fires at now + crash_after.
    pub fn start_node(&mut self, cfg: NodeConfig) -> Result<NodeId, SimError> {
        cfg.validate()?;
        let slot = match self
            .nodes
            .iter()
            .position(|n| n.as_ref().is_none_or(|n| n.state != NodeState::Running))
        {
            Some(i) => i,
            None if self.nodes.len() < self.limits.max_nodes => {
                self.nodes.push(None);
                self.nodes.len() - 1
            }
            None => {
                return Err(SimError::TooManyNodes {
                    limit: self.limits.max_nodes,
                });
            }
        };
        let bus = self.bus_or_create(&cfg.channel)?;
        self.remove_events(slot);
        let generation = self.generation();
        let now = self.clock.now_ns();
        let mut follow_sources: Vec<FollowSource> = Vec::new();
        for f in cfg.periodic.iter().flat_map(|p| &p.follow) {
            if !follow_sources.iter().any(|(id, _)| *id == f.source_id) {
                follow_sources.push((f.source_id, None));
            }
        }
        let mut node = NodeRt {
            generation,
            rng: cfg.seed,
            cfg,
            bus,
            state: NodeState::Running,
            pending: 0,
            tx_frames: 0,
            fault_dropped: 0,
            deadline_overflow: 0,
            reply_errors: 0,
            follow_sources,
        };
        for i in 0..node.cfg.periodic.len() {
            let due = now + node.cfg.periodic[i].offset_ns;
            schedule(
                &mut self.events,
                &mut self.event_order,
                &mut node,
                slot,
                due,
                EventKind::Periodic(i as u16),
                usize::MAX,
            );
        }
        if let Some(c) = node.cfg.faults.crash_after_ns {
            schedule(
                &mut self.events,
                &mut self.event_order,
                &mut node,
                slot,
                now + c,
                EventKind::Crash,
                usize::MAX,
            );
        }
        let id = node.id(slot);
        self.nodes[slot] = Some(node);
        Ok(id)
    }

    fn node_mut(&mut self, id: NodeId) -> Result<&mut NodeRt, SimError> {
        self.nodes
            .get_mut(id.slot as usize)
            .and_then(Option::as_mut)
            .filter(|n| n.generation == id.generation)
            .ok_or(SimError::UnknownNode)
    }

    /// Stops a node; scheduled frames are discarded. It stays listed until reused.
    pub fn stop_node(&mut self, id: NodeId) -> Result<(), SimError> {
        self.node_mut(id)?;
        self.halt_node(id.slot as usize, NodeState::Stopped);
        Ok(())
    }

    /// Fault injection: the node dies now (same as the `crash_after` fault).
    pub fn crash_node(&mut self, id: NodeId) -> Result<(), SimError> {
        self.node_mut(id)?;
        self.halt_node(id.slot as usize, NodeState::Crashed);
        Ok(())
    }

    /// Removes a node from the list (stops it first).
    pub fn remove_node(&mut self, id: NodeId) -> Result<(), SimError> {
        self.node_mut(id)?;
        self.remove_events(id.slot as usize);
        self.nodes[id.slot as usize] = None;
        Ok(())
    }

    fn halt_node(&mut self, slot: usize, state: NodeState) {
        self.remove_events(slot);
        if let Some(n) = self.nodes[slot].as_mut() {
            n.state = state;
            n.pending = 0;
        }
    }

    fn remove_events(&mut self, slot: usize) {
        self.events.retain(|e| e.node as usize != slot);
    }

    pub fn nodes(&self) -> Vec<NodeInfo> {
        self.nodes
            .iter()
            .enumerate()
            .filter_map(|(slot, n)| {
                let n = n.as_ref()?;
                Some(NodeInfo {
                    id: n.id(slot),
                    name: n.cfg.name.clone(),
                    channel: n.cfg.channel.clone(),
                    state: n.state,
                    tx_frames: n.tx_frames,
                    fault_dropped: n.fault_dropped,
                    deadline_overflow: n.deadline_overflow,
                    reply_errors: n.reply_errors,
                    pending: n.pending,
                })
            })
            .collect()
    }

    pub fn bus_stats(&self, name: &str) -> Option<BusStats> {
        let i = *self.bus_names.get(name)?;
        let b = &self.buses[i as usize];
        Some(BusStats {
            id: BusId(i),
            name: b.name.clone(),
            last_seq: b.next_seq - 1,
            last_time_ns: b.last_time,
            frames_total: b.frames_by_origin.iter().sum(),
            frames_by_origin: b.frames_by_origin,
            latched: Origin::ALL
                .iter()
                .copied()
                .filter(|o| b.latched[o.index()])
                .collect(),
            latch_rejected: b.latch_rejected,
            endpoints: b
                .slots
                .iter()
                .enumerate()
                .filter_map(|(si, s)| {
                    let s = s.as_ref()?;
                    let mut drops = s.drops.clone();
                    drops.gaps = s.gaps.iter().copied().collect();
                    Some(EndpointStats {
                        id: EndpointId {
                            bus: i,
                            slot: si as u16,
                            generation: s.generation,
                        },
                        name: s.name.clone(),
                        origin: s.origin,
                        delivered: s.delivered,
                        drops,
                    })
                })
                .collect(),
        })
    }
}

fn matches(data: &[u8], mask: &[u8], payload: &[u8]) -> bool {
    payload.len() >= data.len()
        && data
            .iter()
            .zip(mask)
            .zip(payload)
            .all(|((d, m), p)| d & m == p & m)
}

fn periodic_frame(node: &mut NodeRt, i: usize) -> Option<CanFrame> {
    let p = &node.cfg.periodic[i];
    let mut buf = [0u8; FD_MAX_LEN];
    let len = p.data.len();
    buf[..len].copy_from_slice(&p.data);
    for f in &p.follow {
        if let Some((_, Some((src, src_len)))) = node.follow_sources.iter().find(|(id, _)| *id == f.source_id)
            && (f.source_byte as usize) < *src_len as usize
        {
            buf[f.byte as usize] = src[f.source_byte as usize];
        }
    }
    let built = if p.fd {
        CanFrame::fd(p.id, &buf[..len], true, false)
    } else {
        CanFrame::classic(p.id, &buf[..len])
    };
    // Validated at start; a failure here would be a bug, so count it instead of panicking.
    match built {
        Ok(f) => Some(f),
        Err(_) => {
            node.reply_errors += 1;
            None
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn schedule(
    events: &mut BinaryHeap<Event>,
    order: &mut u64,
    node: &mut NodeRt,
    slot: usize,
    due: u64,
    kind: EventKind,
    max_pending: usize,
) {
    if node.pending >= max_pending {
        node.deadline_overflow += 1;
        return;
    }
    node.pending += 1;
    *order += 1;
    events.push(Event {
        due,
        order: *order,
        node: slot as u16,
        generation: node.generation,
        kind,
    });
}
