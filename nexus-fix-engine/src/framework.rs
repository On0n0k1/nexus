use core::marker::PhantomData;
use std::time::Instant;

use nexus_fix_codec::{
    FixAdminMsg, FixDictionary, FixHeader, find_tag, parse_fix_bool, parse_fix_seqnum,
    parse_fix_uint,
};

use crate::{DisconnectReason, SessionState, State};

const COMP_ID_CAP: usize = 20;

#[derive(Clone, Copy, Debug)]
pub struct CompId {
    bytes: [u8; COMP_ID_CAP],
    len: u8,
}

impl CompId {
    pub fn new(s: &[u8]) -> Option<Self> {
        if s.len() > COMP_ID_CAP {
            return None;
        }
        let mut bytes = [0u8; COMP_ID_CAP];
        bytes[..s.len()].copy_from_slice(s);
        Some(Self {
            bytes,
            len: s.len() as u8,
        })
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }
}

/// Session configuration: CompID pair used for inbound header validation.
#[derive(Clone, Copy, Debug)]
pub struct SessionConfig {
    /// Our own SenderCompID — must match incoming TargetCompID (tag 56).
    pub sender: CompId,
    /// Counterparty SenderCompID — must match incoming SenderCompID (tag 49).
    pub target: CompId,
}

/// Error returned by [`Session::on_message`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionError {
    /// Tag 35 (MsgType) absent.
    MissingMsgType,
    /// Tag 34 (MsgSeqNum) absent.
    MissingMsgSeqNum,
    /// A required field for this message type is absent.
    MissingField { tag: u32 },
    /// A field is present but fails to parse.
    MalformedField { tag: u32 },
    /// Admin message decoder failed.
    MalformedMessage,
}

impl core::fmt::Display for SessionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::MissingMsgType => write!(f, "tag 35 (MsgType) missing"),
            Self::MissingMsgSeqNum => write!(f, "tag 34 (MsgSeqNum) missing"),
            Self::MissingField { tag } => write!(f, "required tag {tag} missing"),
            Self::MalformedField { tag } => write!(f, "tag {tag} malformed"),
            Self::MalformedMessage => write!(f, "admin message malformed"),
        }
    }
}

impl core::error::Error for SessionError {}

/// Typed inbound message returned by [`Session::on_message`].
///
/// Admin messages carry the dictionary's zero-copy decoder for the message type
/// so callers can read any field — protocol-required or venue-specific — without
/// re-parsing. App messages surface the decoded header so the caller can route
/// by `MsgType` and decode the body independently.
pub enum Message<'buf, D: FixDictionary> {
    /// Counterparty initiated a Logon (acceptor role). Send your own Logon back.
    LogonRequest { msg: D::Logon<'buf> },
    /// Counterparty acknowledged our Logon (initiator role). Session is live.
    LogonAcknowledged { msg: D::Logon<'buf> },
    /// Counterparty initiated a Logout. Send a Logout acknowledgement.
    LogoutRequest { msg: D::Logout<'buf> },
    /// Counterparty acknowledged our Logout. Session is done.
    LogoutAcknowledged { msg: D::Logout<'buf> },
    /// Heartbeat (35=0). No reply required unless it carries a TestReqID.
    Heartbeat { msg: D::Heartbeat<'buf> },
    /// TestRequest (35=1). Echo the `TestReqID` in a Heartbeat reply.
    TestRequest { msg: D::TestRequest<'buf> },
    /// ResendRequest (35=2). Re-send or gap-fill the requested range.
    ResendRequest { msg: D::ResendRequest<'buf> },
    /// SequenceReset (35=4). State updated internally; inspect if needed.
    SequenceReset { msg: D::SequenceReset<'buf> },
    /// Reject (35=3). State updated internally; inspect if needed.
    Reject { msg: D::Reject<'buf> },
    /// Business message. Route by `header.raw_msg_type()` and decode the body.
    Application { header: D::Header<'buf> },
    /// Session disconnected (CompID mismatch, timeout, or protocol violation).
    Disconnected { reason: DisconnectReason },
}

/// Dictionary-powered FIX session router.
///
/// Wraps [`SessionState`] with a codec layer: decodes the header via
/// `D::Header::decode`, validates CompIDs, routes by raw `MsgType` bytes, and
/// dispatches to the appropriate [`SessionState`] handler. Returns a typed
/// [`Message`] so the caller owns both encoding and venue-specific logic.
pub struct Session<D: FixDictionary> {
    state: SessionState,
    config: SessionConfig,
    _dict: PhantomData<D>,
}

impl<D: FixDictionary> Session<D> {
    pub fn new(state: SessionState, config: SessionConfig) -> Self {
        Self {
            state,
            config,
            _dict: PhantomData,
        }
    }

    pub fn state(&self) -> &SessionState {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut SessionState {
        &mut self.state
    }

    /// Route an inbound message.
    ///
    /// Decodes the header, validates CompIDs, and dispatches to the matching
    /// [`SessionState`] handler. Returns a [`Message`] carrying either the
    /// decoded admin message or the application header.
    pub fn on_message<'buf>(
        &mut self,
        buf: &'buf [u8],
        now: Instant,
    ) -> Result<Message<'buf, D>, SessionError> {
        let header = D::Header::decode(buf);

        // CompID validation — mismatch → disconnect.
        let sender_ok = header
            .sender_comp_id()
            .is_some_and(|v| v.as_bytes() == self.config.target.as_bytes());
        let target_ok = header
            .target_comp_id()
            .is_some_and(|v| v.as_bytes() == self.config.sender.as_bytes());
        if !sender_ok || !target_ok {
            self.state.on_comp_id_mismatch(now);
            return Ok(Message::Disconnected {
                reason: DisconnectReason::CompIdMismatch,
            });
        }

        let seq = header
            .msg_seq_num()
            .ok_or(SessionError::MissingMsgSeqNum)?
            .checked()
            .map_err(|_| SessionError::MalformedField { tag: 34 })? as u32;

        let poss_dup = header
            .poss_dup_flag()
            .and_then(|v| v.checked().ok())
            .unwrap_or(false);

        match header.raw_msg_type().map(|v| v.as_bytes()) {
            Some(b"A") => {
                let hbi = find_tag(buf, 0, 108).ok_or(SessionError::MissingField { tag: 108 })?;
                let heart_bt_int = parse_fix_uint(hbi.slice(buf))
                    .map_err(|_| SessionError::MalformedField { tag: 108 })?;
                let reset = find_tag(buf, 0, 141)
                    .and_then(|s| parse_fix_bool(s.slice(buf)).ok())
                    .unwrap_or(false);
                let was_logon_sent = self.state.state() == State::LogonSent;
                let send_reply = !was_logon_sent;
                self.state
                    .on_logon(seq, heart_bt_int, reset, send_reply, now);
                let msg = D::Logon::decode(buf)
                    .map_err(|_| SessionError::MalformedMessage)?;
                Ok(if was_logon_sent {
                    Message::LogonAcknowledged { msg }
                } else {
                    Message::LogonRequest { msg }
                })
            }
            Some(b"5") => {
                let was_logout_pending = self.state.state() == State::LogoutPending;
                self.state.on_logout(seq, poss_dup, now);
                let msg = D::Logout::decode(buf)
                    .map_err(|_| SessionError::MalformedMessage)?;
                Ok(if was_logout_pending {
                    Message::LogoutAcknowledged { msg }
                } else {
                    Message::LogoutRequest { msg }
                })
            }
            Some(b"0") => {
                self.state.on_heartbeat(seq, poss_dup, now);
                Ok(Message::Heartbeat {
                    msg: D::Heartbeat::decode(buf)
                        .map_err(|_| SessionError::MalformedMessage)?,
                })
            }
            Some(b"1") => {
                let test_req_id =
                    find_tag(buf, 0, 112).map_or_else(|| b"".as_ref(), |s| s.slice(buf));
                self.state.on_test_request(seq, poss_dup, test_req_id, now);
                Ok(Message::TestRequest {
                    msg: D::TestRequest::decode(buf)
                        .map_err(|_| SessionError::MalformedMessage)?,
                })
            }
            Some(b"2") => {
                let begin = find_tag(buf, 0, 7).ok_or(SessionError::MissingField { tag: 7 })?;
                let begin = parse_fix_seqnum(begin.slice(buf))
                    .map_err(|_| SessionError::MalformedField { tag: 7 })?
                    as u32;
                let end = find_tag(buf, 0, 16).ok_or(SessionError::MissingField { tag: 16 })?;
                let end = parse_fix_seqnum(end.slice(buf))
                    .map_err(|_| SessionError::MalformedField { tag: 16 })?
                    as u32;
                self.state.on_resend_request(seq, poss_dup, begin, end, now);
                Ok(Message::ResendRequest {
                    msg: D::ResendRequest::decode(buf)
                        .map_err(|_| SessionError::MalformedMessage)?,
                })
            }
            Some(b"4") => {
                let new_seq = find_tag(buf, 0, 36).ok_or(SessionError::MissingField { tag: 36 })?;
                let new_seq = parse_fix_seqnum(new_seq.slice(buf))
                    .map_err(|_| SessionError::MalformedField { tag: 36 })?
                    as u32;
                let gap_fill = find_tag(buf, 0, 123)
                    .and_then(|s| parse_fix_bool(s.slice(buf)).ok())
                    .unwrap_or(false);
                self.state.on_sequence_reset(seq, new_seq, gap_fill, now);
                Ok(Message::SequenceReset {
                    msg: D::SequenceReset::decode(buf)
                        .map_err(|_| SessionError::MalformedMessage)?,
                })
            }
            Some(b"3") => {
                let ref_seq = find_tag(buf, 0, 45).ok_or(SessionError::MissingField { tag: 45 })?;
                let ref_seq = parse_fix_seqnum(ref_seq.slice(buf))
                    .map_err(|_| SessionError::MalformedField { tag: 45 })?
                    as u32;
                self.state.on_reject(seq, poss_dup, ref_seq, now);
                Ok(Message::Reject {
                    msg: D::Reject::decode(buf)
                        .map_err(|_| SessionError::MalformedMessage)?,
                })
            }
            Some(_) => {
                self.state.on_app(seq, poss_dup, now);
                Ok(Message::Application { header })
            }
            None => Err(SessionError::MissingMsgType),
        }
    }
}
