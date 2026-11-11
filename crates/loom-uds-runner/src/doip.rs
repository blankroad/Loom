//! [`Transport`] over a `loom-uds-doip` target.

use loom_uds_doip::{AckWait, DoipError, DoipTarget};
use tokio::time::Instant;

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
