use core::marker::PhantomData;
use std::time::Instant;

use nexus_fix_codec::{
    FixDictionary, FixHeader, find_tag, parse_fix_bool, parse_fix_seqnum, parse_fix_uint,
};

use crate::{Out, SessionState, State};

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
}

impl core::fmt::Display for SessionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::MissingMsgType => write!(f, "tag 35 (MsgType) missing"),
            Self::MissingMsgSeqNum => write!(f, "tag 34 (MsgSeqNum) missing"),
            Self::MissingField { tag } => write!(f, "required tag {tag} missing"),
            Self::MalformedField { tag } => write!(f, "tag {tag} malformed"),
        }
    }
}

impl core::error::Error for SessionError {}

/// Dictionary-powered FIX session router.
///
/// Wraps [`SessionState`] with a codec layer: decodes the header via
/// `D::Header::decode`, validates CompIDs, routes by raw `MsgType` bytes, and
/// dispatches to the appropriate typed [`SessionState`] handler.
///
/// Admin messages (`A`, `5`, `0`, `1`, `2`, `4`, `3`) are consumed internally.
/// App messages are returned as `(Out, Some(header))` so the caller can decode
/// the full message body.
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
    /// [`SessionState`] handler. Returns the [`Out`] from the handler plus
    /// the decoded header for app messages; `None` for admin messages (consumed
    /// internally).
    pub fn on_message<'buf>(
        &mut self,
        buf: &'buf [u8],
        now: Instant,
    ) -> Result<(Out, Option<D::Header<'buf>>), SessionError> {
        let header = D::Header::decode(buf);

        // CompID validation — mismatch triggers a disconnect via SessionState.
        let sender_ok = header
            .sender_comp_id()
            .is_some_and(|v| v.as_bytes() == self.config.target.as_bytes());
        let target_ok = header
            .target_comp_id()
            .is_some_and(|v| v.as_bytes() == self.config.sender.as_bytes());
        if !sender_ok || !target_ok {
            return Ok((self.state.on_comp_id_mismatch(now), None));
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
                // Acceptor replies with its own Logon; initiator already sent one.
                let send_reply = self.state.state() != State::LogonSent;
                Ok((
                    self.state
                        .on_logon(seq, heart_bt_int, reset, send_reply, now),
                    None,
                ))
            }
            Some(b"5") => Ok((self.state.on_logout(seq, poss_dup, now), None)),
            Some(b"0") => Ok((self.state.on_heartbeat(seq, poss_dup, now), None)),
            Some(b"1") => {
                let test_req_id =
                    find_tag(buf, 0, 112).map_or_else(|| b"".as_ref(), |s| s.slice(buf));
                Ok((
                    self.state.on_test_request(seq, poss_dup, test_req_id, now),
                    None,
                ))
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
                Ok((
                    self.state.on_resend_request(seq, poss_dup, begin, end, now),
                    None,
                ))
            }
            Some(b"4") => {
                let new_seq = find_tag(buf, 0, 36).ok_or(SessionError::MissingField { tag: 36 })?;
                let new_seq = parse_fix_seqnum(new_seq.slice(buf))
                    .map_err(|_| SessionError::MalformedField { tag: 36 })?
                    as u32;
                let gap_fill = find_tag(buf, 0, 123)
                    .and_then(|s| parse_fix_bool(s.slice(buf)).ok())
                    .unwrap_or(false);
                Ok((
                    self.state.on_sequence_reset(seq, new_seq, gap_fill, now),
                    None,
                ))
            }
            Some(b"3") => {
                let ref_seq = find_tag(buf, 0, 45).ok_or(SessionError::MissingField { tag: 45 })?;
                let ref_seq = parse_fix_seqnum(ref_seq.slice(buf))
                    .map_err(|_| SessionError::MalformedField { tag: 45 })?
                    as u32;
                Ok((self.state.on_reject(seq, poss_dup, ref_seq, now), None))
            }
            Some(_) => {
                let out = self.state.on_app(seq, poss_dup, now);
                Ok((out, Some(header)))
            }
            None => Err(SessionError::MissingMsgType),
        }
    }
}
