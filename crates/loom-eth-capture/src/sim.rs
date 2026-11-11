use crate::*;
use etherparse::PacketBuilder;
use std::{
    collections::{HashMap, VecDeque},
    mem::size_of,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{Arc, Mutex},
    time::{Instant, SystemTime, UNIX_EPOCH},
};
/// Synthetic ingress byte ceiling (reserved queue/map storage + owned payloads); reject newest.
pub const MAX_SIM_BYTES: usize = 4 * 1024 * 1024;
/// Synthetic ingress count ceiling; reject newest before advancing TCP sequence state.
pub const MAX_SIM_FRAMES: usize = 4096;
/// TCP state table ceiling; new connections are rejected; closed connections can be removed.
pub const MAX_SIM_CONNECTIONS: usize = 1024;
/// IPv4 payload ceiling before transport headers; reject oversized synthetic datagrams/segments.
pub const MAX_SIM_IP_BYTES: usize = 65_535;
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct Endpoint {
    pub mac: [u8; 6],
    pub ip: [u8; 4],
    pub port: u16,
}
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct Connection {
    pub client: Endpoint,
    pub server: Endpoint,
}
/// Address-family independent synthetic TCP connection. Existing IPv4 Connection remains supported.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct SocketConnection {
    pub client: SocketAddr,
    pub server: SocketAddr,
    pub client_mac: [u8; 6],
    pub server_mac: [u8; 6],
}
impl From<Connection> for SocketConnection {
    fn from(c: Connection) -> Self {
        Self {
            client: SocketAddr::new(IpAddr::V4(Ipv4Addr::from(c.client.ip)), c.client.port),
            server: SocketAddr::new(IpAddr::V4(Ipv4Addr::from(c.server.ip)), c.server.port),
            client_mac: c.client.mac,
            server_mac: c.server.mac,
        }
    }
}
#[derive(Debug, Clone, Copy, Default)]
pub struct TcpFlags {
    pub syn: bool,
    pub fin: bool,
}
#[derive(Debug, Clone, Copy)]
struct TcpState {
    seq: [u32; 2],
    syn: [bool; 2],
    fin: [bool; 2],
}
impl Default for TcpState {
    fn default() -> Self {
        Self {
            seq: [1000, 2000],
            syn: [false; 2],
            fin: [false; 2],
        }
    }
}
#[derive(Debug)]
struct State {
    frames: VecDeque<Box<RawFrame>>,
    connections: HashMap<SocketConnection, TcpState>,
    bytes: usize,
    recv: u64,
    stop: StopSignal,
}
#[derive(Debug)]
struct Shared {
    state: Mutex<State>,
    interface: Interface,
    origin: Instant,
    epoch_ns: u128,
}
#[derive(Debug, Clone)]
pub struct SimTapHandle(Arc<Shared>);
#[derive(Debug)]
pub struct SimTapSource {
    handle: SimTapHandle,
}
impl Default for SimTapSource {
    fn default() -> Self {
        Self::new()
    }
}
impl SimTapSource {
    pub fn new() -> Self {
        let frames = VecDeque::with_capacity(MAX_SIM_FRAMES);
        let connections = HashMap::with_capacity(MAX_SIM_CONNECTIONS);
        // HashMap reserves implementation-dependent buckets; charge a conservative 2x
        // capacity bound including key/value/control overhead, not just live entries.
        let base = frames.capacity() * size_of::<Box<RawFrame>>()
            + connections.capacity()
                * 2
                * (size_of::<SocketConnection>() + size_of::<TcpState>() + size_of::<usize>());
        Self {
            handle: SimTapHandle(Arc::new(Shared {
                interface: Interface {
                    linktype: 1,
                    snaplen: 65_549,
                    name: "loom-sim-tap".into(),
                    tsresol: TimestampResolution::NANO,
                },
                origin: Instant::now(),
                epoch_ns: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(0, |d| d.as_nanos()),
                state: Mutex::new(State {
                    frames,
                    connections,
                    bytes: base,
                    recv: 0,
                    stop: StopSignal::default(),
                }),
            })),
        }
    }
    pub fn handle(&self) -> SimTapHandle {
        self.handle.clone()
    }
    pub fn push_tcp(&self, conn: Connection, dir: Direction, payload: &[u8]) -> Result<()> {
        self.handle.push_tcp(conn, dir, payload)
    }
    pub fn push_tcp_with_flags(
        &self,
        conn: Connection,
        dir: Direction,
        payload: &[u8],
        flags: TcpFlags,
    ) -> Result<()> {
        self.handle.push_tcp_with_flags(conn, dir, payload, flags)
    }
    pub fn push_udp(&self, src: Endpoint, dst: Endpoint, payload: &[u8]) -> Result<()> {
        self.handle.push_udp(src, dst, payload)
    }
}
impl SimTapHandle {
    fn raw(&self, data: Vec<u8>) -> RawFrame {
        RawFrame {
            ts_ns: self.0.epoch_ns + self.0.origin.elapsed().as_nanos(),
            caplen: data.len() as u32,
            origlen: data.len() as u32,
            data,
            interface_id: 0,
            section_index: 0,
            interface: self.0.interface.clone(),
            comments: Vec::new(),
            meta: None,
        }
    }
    fn enqueue(&self, s: &mut State, data: Vec<u8>) -> Result<()> {
        let raw = self.raw(data);
        let bytes = size_of::<RawFrame>() + raw.heap_bytes();
        if s.frames.len() >= MAX_SIM_FRAMES || bytes > MAX_SIM_BYTES.saturating_sub(s.bytes) {
            return Err(CaptureError::Limit("sim ingress queue"));
        }
        s.bytes += bytes;
        s.frames.push_back(Box::new(raw));
        s.stop.wake();
        Ok(())
    }
    pub fn push_tcp(&self, conn: Connection, dir: Direction, payload: &[u8]) -> Result<()> {
        self.push_tcp_with_flags(conn, dir, payload, TcpFlags::default())
    }
    pub fn push_tcp_with_flags(
        &self,
        conn: Connection,
        dir: Direction,
        payload: &[u8],
        flags: TcpFlags,
    ) -> Result<()> {
        self.push_tcp_socket(conn.into(), dir, payload, flags)
    }
    /// Enqueue one bounded IPv4/IPv6 TCP frame; the caller owns connection lifecycle.
    pub fn push_tcp_socket(
        &self,
        conn: SocketConnection,
        dir: Direction,
        payload: &[u8],
        flags: TcpFlags,
    ) -> Result<()> {
        if conn.client.is_ipv4() != conn.server.is_ipv4() {
            return Err(CaptureError::Invalid("mixed IP families"));
        }
        if payload.len() > MAX_SIM_IP_BYTES - 40 {
            return Err(CaptureError::Limit("sim TCP payload"));
        }
        let mut s = self.0.state.lock().unwrap_or_else(|p| p.into_inner());
        if s.frames.len() >= MAX_SIM_FRAMES {
            return Err(CaptureError::Limit("sim ingress count"));
        }
        if !s.connections.contains_key(&conn) && s.connections.len() >= MAX_SIM_CONNECTIONS {
            return Err(CaptureError::Limit("sim TCP connections"));
        }
        let mut state = s.connections.get(&conn).copied().unwrap_or_default();
        let i = usize::from(dir == Direction::Rx);
        let other = 1 - i;
        if (state.fin[i] && (!payload.is_empty() || flags.syn || flags.fin))
            || (flags.syn && state.syn[i])
            || (flags.syn && flags.fin)
        {
            return Err(CaptureError::Invalid("sim TCP direction already SYN/FIN"));
        }
        let (src, dst, sm, dm) = if i == 0 {
            (conn.client, conn.server, conn.client_mac, conn.server_mac)
        } else {
            (conn.server, conn.client, conn.server_mac, conn.client_mac)
        };
        let b = PacketBuilder::ethernet2(sm, dm);
        let b = match (src.ip(), dst.ip()) {
            (IpAddr::V4(s), IpAddr::V4(d)) => b.ipv4(s.octets(), d.octets(), 64),
            (IpAddr::V6(s), IpAddr::V6(d)) => b.ipv6(s.octets(), d.octets(), 64),
            _ => return Err(CaptureError::Invalid("mixed IP families")),
        };
        let mut b = b.tcp(src.port(), dst.port(), state.seq[i], 65_535);
        if state.syn[other] || !flags.syn {
            b = b.ack(state.seq[other]);
        }
        if flags.syn {
            b = b.syn();
        }
        if flags.fin {
            b = b.fin();
        }
        if !payload.is_empty() {
            b = b.psh();
        }
        let mut data = Vec::with_capacity(b.size(payload.len()));
        b.write(&mut data, payload)
            .map_err(|e| CaptureError::Backend(e.to_string()))?;
        self.enqueue(&mut s, data)?;
        state.seq[i] =
            state.seq[i].wrapping_add(payload.len() as u32 + u32::from(flags.syn) + u32::from(flags.fin));
        state.syn[i] |= flags.syn;
        state.fin[i] |= flags.fin;
        // Retain FIN sequence state for the final pure ACK; retire after that ACK.
        if state.fin.iter().all(|b| *b) && !flags.fin && !flags.syn && payload.is_empty() {
            s.connections.remove(&conn);
        } else {
            s.connections.insert(conn, state);
        }
        Ok(())
    }
    pub fn push_udp(&self, src: Endpoint, dst: Endpoint, payload: &[u8]) -> Result<()> {
        if payload.len() > MAX_SIM_IP_BYTES - 28 {
            return Err(CaptureError::Limit("sim UDP payload"));
        }
        let mut s = self.0.state.lock().unwrap_or_else(|p| p.into_inner());
        if s.frames.len() >= MAX_SIM_FRAMES {
            return Err(CaptureError::Limit("sim ingress count"));
        }
        let b = PacketBuilder::ethernet2(src.mac, dst.mac)
            .ipv4(src.ip, dst.ip, 64)
            .udp(src.port, dst.port);
        let mut data = Vec::with_capacity(b.size(payload.len()));
        b.write(&mut data, payload)
            .map_err(|e| CaptureError::Backend(e.to_string()))?;
        self.enqueue(&mut s, data)
    }
    pub fn forget_connection(&self, conn: &Connection) -> bool {
        self.forget_socket_connection(&(*conn).into())
    }
    pub fn forget_socket_connection(&self, conn: &SocketConnection) -> bool {
        self.0
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .connections
            .remove(conn)
            .is_some()
    }
    pub fn queued_frames(&self) -> usize {
        self.0
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .frames
            .len()
    }
    pub fn byte_usage(&self) -> usize {
        self.0.state.lock().unwrap_or_else(|p| p.into_inner()).bytes
    }
    pub fn connection_count(&self) -> usize {
        self.0
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .connections
            .len()
    }
    fn pop(&self) -> Option<RawFrame> {
        let mut s = self.0.state.lock().unwrap_or_else(|p| p.into_inner());
        let raw = s.frames.pop_front()?;
        s.bytes -= size_of::<RawFrame>() + raw.heap_bytes();
        s.recv = s.recv.saturating_add(1);
        Some(*raw)
    }
}
impl CaptureSource for SimTapSource {
    fn interface(&self) -> Interface {
        self.handle.0.interface.clone()
    }
    fn timestamp_source(&self) -> TimestampSource {
        TimestampSource::Sim
    }
    fn set_stop_signal(&mut self, signal: StopSignal) {
        self.handle.0.state.lock().unwrap_or_else(|p| p.into_inner()).stop = signal;
    }
    fn next(&mut self, timeout: Duration) -> Result<Option<RawFrame>> {
        let stop = self
            .handle
            .0
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .stop
            .clone();
        if stop.is_stopped() {
            return Ok(None);
        }
        if let Some(raw) = self.handle.pop() {
            return Ok(Some(raw));
        }
        stop.wait(timeout);
        if stop.is_stopped() {
            return Ok(None);
        }
        Ok(self.handle.pop())
    }
    fn stats(&mut self) -> DropStats {
        DropStats {
            recv: self.handle.0.state.lock().unwrap_or_else(|p| p.into_inner()).recv,
            ..DropStats::ZERO
        }
    }
}
