//! DoIP adapter (feature `doip`): [`DoipTransport`] over a `loom-uds-doip` target and
//! [`DoipConnector`], which owns everything DoIP-specific about a target's lifecycle (client
//! configuration, tester/target logical addresses, the client connection and its link events).

use loom_uds_doip::{AckWait, ClientConfig, DoipClient, DoipError, DoipTarget, LinkEvent};
use tokio::sync::mpsc;
use tokio::time::Instant;

use crate::link::{ConnectError, Connector, Link, LinkEvents, LinkSignal};
use crate::transport::{BoxFuture, Incoming, RecvError, Transport, TransportError};

/// DoIP adapter. The P2 timer starts at the diagnostic ACK.
#[derive(Debug)]
pub struct DoipTransport {
    target: DoipTarget,
    ack: Option<AckWait>,
}

impl DoipTransport {
    pub fn new(target: DoipTarget) -> Self {
        DoipTransport { target, ack: None }
    }

    pub fn target(&self) -> &DoipTarget {
        &self.target
    }
}

fn map(e: DoipError) -> TransportError {
    match e {
        DoipError::Closed => TransportError::Closed,
        DoipError::TxFull => TransportError::TxFull,
        DoipError::TxTimeout => TransportError::TxTimeout,
        DoipError::Nack(code) => TransportError::Nack(code),
        DoipError::AckTimeout => TransportError::AckTimeout,
        DoipError::InvalidPdu(_) => TransportError::InvalidPdu,
        DoipError::AckPending(_) => TransportError::Busy,
        other => TransportError::Other(other.to_string()),
    }
}

impl Transport for DoipTransport {
    fn generation(&self) -> u64 {
        self.target.generation()
    }

    fn submit<'a>(
        &'a mut self,
        pdu: &'a [u8],
        until: Instant,
    ) -> BoxFuture<'a, Result<Instant, TransportError>> {
        Box::pin(async move {
            self.ack = None;
            let wait = self.target.submit(pdu, until).await.map_err(map)?;
            let at = wait.handed_at();
            self.ack = Some(wait);
            Ok(at)
        })
    }

    fn confirm<'a>(&'a mut self, until: Instant) -> BoxFuture<'a, Result<Instant, TransportError>> {
        Box::pin(async move {
            let Some(wait) = self.ack.take() else {
                return Err(TransportError::Other("confirm without submit".into()));
            };
            wait.wait(until).await.map_err(map)
        })
    }

    fn recv<'a>(&'a mut self, until: Instant) -> BoxFuture<'a, Result<Incoming, RecvError>> {
        Box::pin(async move {
            match self.target.recv_until(until).await {
                Ok(i) => Ok(Incoming::Pdu {
                    data: i.data,
                    at: i.at,
                }),
                Err(loom_uds_doip::RecvError::Timeout) => Err(RecvError::Timeout),
                Err(loom_uds_doip::RecvError::Closed) => Err(RecvError::Closed),
            }
        })
    }

    fn try_recv(&mut self) -> Option<Incoming> {
        self.target.try_recv().map(|i| Incoming::Pdu {
            data: i.data,
            at: i.at,
        })
    }
}

/// Opens a DoIP client connection (TCP + routing activation) and one target on it.
#[derive(Debug, Clone)]
pub struct DoipConnector {
    cfg: ClientConfig,
    target_la: u16,
}

impl DoipConnector {
    /// `cfg.generation` is overwritten on every [`Connector::connect`].
    pub fn new(cfg: ClientConfig, target_la: u16) -> Self {
        DoipConnector { cfg, target_la }
    }

    pub fn config(&self) -> &ClientConfig {
        &self.cfg
    }

    pub fn target_la(&self) -> u16 {
        self.target_la
    }
}

/// DoIP link events → [`LinkSignal`]s (activation/NACK/unrouted notices are not link signals).
#[derive(Debug)]
struct DoipLinkEvents(mpsc::Receiver<LinkEvent>);

impl LinkEvents for DoipLinkEvents {
    fn next(&mut self) -> BoxFuture<'_, Option<LinkSignal>> {
        Box::pin(async move {
            loop {
                match self.0.recv().await? {
                    LinkEvent::Closed { reason } | LinkEvent::Error { reason } => {
                        return Some(LinkSignal::Down { reason });
                    }
                    LinkEvent::RxOverflow { dropped_total, .. } => {
                        return Some(LinkSignal::RxOverflow { dropped_total });
                    }
                    _ => {}
                }
            }
        })
    }
}

impl Connector for DoipConnector {
    fn endpoint(&self) -> String {
        format!("DoIP 0x{:04X}", self.target_la)
    }

    fn connect(&self, generation: u64) -> BoxFuture<'_, Result<Link, ConnectError>> {
        Box::pin(async move {
            let mut cfg = self.cfg.clone();
            cfg.generation = generation;
            let addr = cfg.entity;
            let (client, link_rx) = DoipClient::connect(cfg)
                .await
                .map_err(|e| ConnectError(format!("DoIP connect {addr}: {e}")))?;
            // On error `client` is dropped here: socket closed, reader/writer tasks aborted.
            let target = client
                .open_target(self.target_la)
                .map_err(|e| ConnectError(format!("DoIP target 0x{:04X}: {e}", self.target_la)))?;
            Ok(Link {
                transport: Box::new(DoipTransport::new(target)),
                events: Box::new(DoipLinkEvents(link_rx)),
                resources: Box::new(client),
            })
        })
    }
}
