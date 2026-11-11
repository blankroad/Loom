//! Priority and abort primitives (plan §3.5: abort > user > agent/script).

use std::sync::Arc;

use loom_proto::error::ErrorCode;
use loom_proto::event::Origin;
use tokio::sync::watch;

use crate::runner::TxnRequest;

/// Queue priority of a request. Higher runs first; the request in flight is never pre-empted
/// (the ECU-side state of a started service is preserved). Scripts use [`Priority::Agent`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Priority {
    /// Agent or script request (default).
    #[default]
    Agent,
    /// Human request from a viewer (console send, takeover).
    User,
    /// Abort / STOP follow-up traffic (e.g. safe-state recovery). Not cancelled by
    /// [`AbortScope::All`] or [`AbortScope::Origin`], only by [`AbortScope::Request`].
    Abort,
}

/// Why a request was aborted; selects the contract error code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AbortKind {
    /// User abort (`!` in the console, `uds.abort`) → `E_INTERRUPTED_BY_USER`.
    User,
    /// STOP latch → `E_STOPPED`.
    Stop,
}

impl AbortKind {
    pub fn error_code(self) -> ErrorCode {
        match self {
            AbortKind::User => ErrorCode::InterruptedByUser,
            AbortKind::Stop => ErrorCode::Stopped,
        }
    }
}

/// Which requests an abort hits.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AbortScope {
    /// Every waiting request and the one in flight, except [`Priority::Abort`] requests.
    All,
    /// Requests of this origin, except [`Priority::Abort`] requests.
    Origin(Origin),
    /// The request with this `request_id`, whatever its priority.
    Request(String),
}

impl AbortScope {
    pub fn matches(&self, req: &TxnRequest) -> bool {
        match self {
            AbortScope::All => req.priority != Priority::Abort,
            AbortScope::Origin(o) => req.priority != Priority::Abort && req.origin == *o,
            AbortScope::Request(id) => req.request_id.as_deref() == Some(id.as_str()),
        }
    }
}

/// One-shot abort signal for one execution (cheap to clone; the first `abort` wins).
#[derive(Debug, Clone)]
pub struct AbortSignal {
    tx: Arc<watch::Sender<Option<AbortKind>>>,
}

impl Default for AbortSignal {
    fn default() -> Self {
        AbortSignal::new()
    }
}

impl AbortSignal {
    pub fn new() -> Self {
        AbortSignal {
            tx: Arc::new(watch::Sender::new(None)),
        }
    }

    /// Fire the signal. Non-blocking; later calls keep the first kind.
    pub fn abort(&self, kind: AbortKind) {
        self.tx.send_if_modified(|v| {
            if v.is_none() {
                *v = Some(kind);
                true
            } else {
                false
            }
        });
    }

    /// The kind if fired.
    pub fn kind(&self) -> Option<AbortKind> {
        *self.tx.borrow()
    }

    /// Resolves once the signal fires.
    pub(crate) async fn fired(&self) -> AbortKind {
        let mut rx = self.tx.subscribe();
        loop {
            if let Some(k) = *rx.borrow_and_update() {
                return k;
            }
            if rx.changed().await.is_err() {
                // The sender lives in `self`, so this cannot happen; never resolve.
                std::future::pending::<()>().await;
            }
        }
    }
}
