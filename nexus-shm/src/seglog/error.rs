use std::fmt;

use crate::error::ShmError;

#[derive(Debug)]
#[non_exhaustive]
pub enum SegmentedLogError {
    RecordTooLarge {
        max: usize,
    },
    StandbyNotReady,
    ConfigMismatch {
        field: &'static str,
        expected: u64,
        found: u64,
    },
    SessionInUse {
        session_id: u32,
    },
    SessionNotFound {
        session_id: u32,
    },
    Shm(ShmError),
    Io(std::io::Error),
}

impl fmt::Display for SegmentedLogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RecordTooLarge { max } => {
                write!(f, "payload exceeds segment capacity ({max} bytes max)")
            }
            Self::StandbyNotReady => {
                write!(f, "conductor has not finished cleaning the standby segment")
            }
            Self::ConfigMismatch {
                field,
                expected,
                found,
            } => {
                write!(
                    f,
                    "manifest {field} mismatch: expected {expected}, found {found}"
                )
            }
            Self::SessionInUse { session_id } => {
                write!(f, "session {session_id} is already open")
            }
            Self::SessionNotFound { session_id } => {
                write!(f, "no manifest found for session {session_id}")
            }
            Self::Shm(e) => write!(f, "{e}"),
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for SegmentedLogError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Shm(e) => Some(e),
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<ShmError> for SegmentedLogError {
    fn from(e: ShmError) -> Self {
        Self::Shm(e)
    }
}

impl From<std::io::Error> for SegmentedLogError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
