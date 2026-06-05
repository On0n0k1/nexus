//! Sans-IO FIX session layer.
//!
//! [`SessionState`] is a pure state machine: the caller owns the transport,
//! the clock, and the wire encoding. Each typed handler (e.g.
//! [`SessionState::on_logon`], [`SessionState::on_app`]) receives pre-decoded
//! fields and returns an [`Out`] containing any outbound admin messages and a
//! session event. The framework layer above encodes those messages and drives
//! the transport.

mod session;

pub use session::{AdminMsg, DisconnectReason, Event, Out, SessionState, State};
