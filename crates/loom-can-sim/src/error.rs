//! Errors.

use std::fmt;

use crate::record::Origin;

/// A node config is not valid. `path` points at the field, e.g. `node[0].on[1].data`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    pub path: String,
    pub message: String,
}

impl ConfigError {
    pub(crate) fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            write!(f, "sim node config: {}", self.message)
        } else {
            write!(f, "sim node config {}: {}", self.path, self.message)
        }
    }
}

impl std::error::Error for ConfigError {}

/// Sim bus / node operation refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimError {
    TooManyBuses {
        limit: usize,
    },
    TooManyEndpoints {
        bus: String,
        limit: usize,
    },
    TooManyNodes {
        limit: usize,
    },
    UnknownEndpoint,
    UnknownNode,
    UnknownBus {
        name: String,
    },
    /// STOP latch is set for this origin; the frame was not sent.
    Latched {
        origin: Origin,
    },
    Config(ConfigError),
}

impl fmt::Display for SimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SimError::TooManyBuses { limit } => write!(f, "more than {limit} sim buses"),
            SimError::TooManyEndpoints { bus, limit } => {
                write!(f, "sim bus {bus}: more than {limit} endpoints")
            }
            SimError::TooManyNodes { limit } => write!(f, "more than {limit} sim nodes"),
            SimError::UnknownEndpoint => write!(f, "unknown or detached endpoint"),
            SimError::UnknownNode => write!(f, "unknown or removed sim node"),
            SimError::UnknownBus { name } => write!(f, "no sim bus named {name}"),
            SimError::Latched { origin } => write!(f, "TX latched (STOP) for {origin:?}"),
            SimError::Config(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for SimError {}

impl From<ConfigError> for SimError {
    fn from(e: ConfigError) -> Self {
        SimError::Config(e)
    }
}
