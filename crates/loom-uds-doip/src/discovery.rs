//! UDP vehicle discovery: vehicle identification request + response/announcement collection,
//! bounded in count and time.

use std::net::SocketAddr;
use std::time::Duration;

use tokio::net::UdpSocket;
use tokio::time::{Instant, timeout_at};

use crate::client::DoipError;
use crate::frame::{MAX_DOIP_FRAME, Message, VERSION_DEFAULT_VIR, VehicleAnnouncement, decode_frame};

/// Most entities one discovery returns; extra responses are ignored.
pub const MAX_DISCOVERY_RESULTS: usize = 64;
/// Longest discovery window; longer requested timeouts are clamped.
pub const MAX_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(10);
/// Most datagrams inspected per discovery (bounds work under a datagram flood).
pub const MAX_DISCOVERY_DATAGRAMS: usize = 1024;

/// Which entities should answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VehicleIdFilter {
    All,
    Eid([u8; 6]),
    Vin([u8; 17]),
}

/// Discovery parameters.
#[derive(Debug, Clone)]
pub struct DiscoveryRequest {
    /// Broadcast (e.g. `255.255.255.255:13400`) or a unicast entity address.
    pub dest: SocketAddr,
    /// Local bind address (port 0 for an ephemeral port).
    pub bind: SocketAddr,
    pub filter: VehicleIdFilter,
    pub timeout: Duration,
    pub max_results: usize,
}

/// One answering entity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovered {
    pub from: SocketAddr,
    pub announcement: VehicleAnnouncement,
}

/// Send a vehicle identification request and collect responses until the timeout or
/// `max_results` (both clamped by the `MAX_*` limits). Duplicates (same address + LA) are merged.
pub async fn discover(req: &DiscoveryRequest) -> Result<Vec<Discovered>, DoipError> {
    let socket = UdpSocket::bind(req.bind).await?;
    socket.set_broadcast(true)?;
    let msg = match req.filter {
        VehicleIdFilter::All => Message::VehicleIdRequest,
        VehicleIdFilter::Eid(eid) => Message::VehicleIdRequestEid { eid },
        VehicleIdFilter::Vin(vin) => Message::VehicleIdRequestVin { vin },
    };
    let frame = msg.encode_with_version(VERSION_DEFAULT_VIR)?;
    socket.send_to(&frame, req.dest).await?;
    let until = Instant::now() + req.timeout.min(MAX_DISCOVERY_TIMEOUT);
    collect_announcements(&socket, until, req.max_results).await
}

/// Collect vehicle announcements / identification responses arriving on `socket` until `until`
/// or `max_results` (clamped to [`MAX_DISCOVERY_RESULTS`]). Invalid datagrams are skipped.
pub async fn collect_announcements(
    socket: &UdpSocket,
    until: Instant,
    max_results: usize,
) -> Result<Vec<Discovered>, DoipError> {
    let max = max_results.clamp(1, MAX_DISCOVERY_RESULTS);
    let mut found: Vec<Discovered> = Vec::with_capacity(max.min(8));
    let mut buf = vec![0u8; MAX_DOIP_FRAME];
    for _ in 0..MAX_DISCOVERY_DATAGRAMS {
        if found.len() >= max {
            break;
        }
        let (n, from) = match timeout_at(until, socket.recv_from(&mut buf)).await {
            Err(_) => break,
            Ok(r) => r?,
        };
        let Some(datagram) = buf.get(..n) else {
            continue;
        };
        if let Ok(Some((_, Message::VehicleAnnouncement(a), _))) = decode_frame(datagram) {
            let dup = found
                .iter()
                .any(|d| d.from == from && d.announcement.logical_address == a.logical_address);
            if !dup {
                found.push(Discovered {
                    from,
                    announcement: a,
                });
            }
        }
    }
    Ok(found)
}
