mod event;
mod ring;

use std::time::{Duration, Instant};

use nexus_fix_codec::{
    FieldReader, FieldSpan, FieldWriter, checksum, encode_field, encode_fix_seqnum,
    encode_fix_uint, format_checksum, parse_fix_uint,
};

pub use event::{DisconnectReason, Event, State};
use ring::Ring;

use crate::timestamp::{UTC_TIMESTAMP_LEN, format_utc_timestamp};

const TAG_BEGIN_SEQ_NO: u32 = 7;
const TAG_BEGIN_STRING: u32 = 8;
const TAG_BODY_LENGTH: u32 = 9;
const TAG_CHECK_SUM: u32 = 10;
const TAG_END_SEQ_NO: u32 = 16;
const TAG_MSG_SEQ_NUM: u32 = 34;
const TAG_MSG_TYPE: u32 = 35;
const TAG_NEW_SEQ_NO: u32 = 36;
const TAG_POSS_DUP_FLAG: u32 = 43;
const TAG_REF_SEQ_NUM: u32 = 45;
const TAG_SENDER_COMP_ID: u32 = 49;
const TAG_SENDING_TIME: u32 = 52;
const TAG_TARGET_COMP_ID: u32 = 56;
const TAG_ENCRYPT_METHOD: u32 = 98;
const TAG_HEART_BT_INT: u32 = 108;
const TAG_TEST_REQ_ID: u32 = 112;
const TAG_GAP_FILL_FLAG: u32 = 123;
const TAG_RESET_SEQ_NUM_FLAG: u32 = 141;

/// Maximum echoed TestReqID length; longer IDs are truncated.
const TEST_REQ_ID_CAP: usize = 64;

const EVENT_CAP: usize = 16;
const PENDING_CAP: usize = 16;

/// Static session parameters. CompIDs are borrowed from the caller.
#[derive(Debug, Clone, Copy)]
pub struct SessionConfig<'a> {
    /// BeginString(8) for the session, e.g. `b"FIX.4.4"`.
    pub begin_string: &'a [u8],
    /// Our SenderCompID(49).
    pub sender_comp_id: &'a [u8],
    /// Counterparty's CompID, our TargetCompID(56).
    pub target_comp_id: &'a [u8],
    /// Heartbeat interval, HeartBtInt(108).
    pub heart_bt_int: Duration,
}

#[derive(Clone, Copy)]
enum Pending {
    Logon,
    Logout,
    Heartbeat {
        id: [u8; TEST_REQ_ID_CAP],
        id_len: u8,
    },
    TestRequest {
        id: u64,
    },
    ResendRequest {
        begin: u32,
    },
    SequenceReset {
        seq: u32,
        new_seq: u32,
    },
}

const fn msg_type_of(p: &Pending) -> &'static [u8] {
    match p {
        Pending::Heartbeat { .. } => b"0",
        Pending::TestRequest { .. } => b"1",
        Pending::ResendRequest { .. } => b"2",
        Pending::SequenceReset { .. } => b"4",
        Pending::Logout => b"5",
        Pending::Logon => b"A",
    }
}

struct Fields {
    msg_type: FieldSpan,
    seq_num: FieldSpan,
    sender: FieldSpan,
    target: FieldSpan,
    poss_dup: FieldSpan,
    begin_seq_no: FieldSpan,
    end_seq_no: FieldSpan,
    new_seq_no: FieldSpan,
    ref_seq_num: FieldSpan,
    heart_bt_int: FieldSpan,
    test_req_id: FieldSpan,
    gap_fill: FieldSpan,
    reset_seq_num: FieldSpan,
}

fn scan(msg: &[u8]) -> Fields {
    let mut f = Fields {
        msg_type: FieldSpan::EMPTY,
        seq_num: FieldSpan::EMPTY,
        sender: FieldSpan::EMPTY,
        target: FieldSpan::EMPTY,
        poss_dup: FieldSpan::EMPTY,
        begin_seq_no: FieldSpan::EMPTY,
        end_seq_no: FieldSpan::EMPTY,
        new_seq_no: FieldSpan::EMPTY,
        ref_seq_num: FieldSpan::EMPTY,
        heart_bt_int: FieldSpan::EMPTY,
        test_req_id: FieldSpan::EMPTY,
        gap_fill: FieldSpan::EMPTY,
        reset_seq_num: FieldSpan::EMPTY,
    };
    let mut r = FieldReader::new(msg, 0);
    while let Some(field) = r.next_field() {
        match field.tag {
            TAG_MSG_TYPE => f.msg_type = field.value,
            TAG_MSG_SEQ_NUM => f.seq_num = field.value,
            TAG_SENDER_COMP_ID => f.sender = field.value,
            TAG_TARGET_COMP_ID => f.target = field.value,
            TAG_POSS_DUP_FLAG => f.poss_dup = field.value,
            TAG_BEGIN_SEQ_NO => f.begin_seq_no = field.value,
            TAG_END_SEQ_NO => f.end_seq_no = field.value,
            TAG_NEW_SEQ_NO => f.new_seq_no = field.value,
            TAG_REF_SEQ_NUM => f.ref_seq_num = field.value,
            TAG_HEART_BT_INT => f.heart_bt_int = field.value,
            TAG_TEST_REQ_ID => f.test_req_id = field.value,
            TAG_GAP_FILL_FLAG => f.gap_fill = field.value,
            TAG_RESET_SEQ_NUM_FLAG => f.reset_seq_num = field.value,
            TAG_CHECK_SUM => break,
            _ => {}
        }
    }
    f
}

/// Sans-IO FIX session state machine.
///
/// The caller owns the transport, the clock, and the encode buffer. Feed
/// framed inbound messages to [`handle_message`](Self::handle_message),
/// drive timers with [`handle_timeout`](Self::handle_timeout), drain
/// [`Event`]s with [`poll_event`](Self::poll_event), and flush queued
/// admin messages with [`encode_pending`](Self::encode_pending). Never
/// allocates after construction.
pub struct Session<'a> {
    cfg: SessionConfig<'a>,
    state: State,
    hb: Duration,
    next_outbound: u32,
    next_inbound: u32,
    gap_high: u32,
    last_sent: Option<Instant>,
    last_received: Option<Instant>,
    test_request_sent: Option<Instant>,
    state_entered: Option<Instant>,
    test_req_counter: u64,
    heartbeat_queued: bool,
    events: Ring<Event, EVENT_CAP>,
    pending: Ring<Pending, PENDING_CAP>,
}

impl<'a> Session<'a> {
    /// Creates a disconnected session with sequence numbers at 1.
    #[must_use]
    pub const fn new(cfg: SessionConfig<'a>) -> Self {
        Self {
            hb: cfg.heart_bt_int,
            cfg,
            state: State::Disconnected,
            next_outbound: 1,
            next_inbound: 1,
            gap_high: 0,
            last_sent: None,
            last_received: None,
            test_request_sent: None,
            state_entered: None,
            test_req_counter: 0,
            heartbeat_queued: false,
            events: Ring::new(),
            pending: Ring::new(),
        }
    }

    /// Current lifecycle state.
    #[must_use]
    pub const fn state(&self) -> State {
        self.state
    }

    /// Next expected inbound MsgSeqNum(34).
    #[must_use]
    pub const fn next_inbound_seq(&self) -> u32 {
        self.next_inbound
    }

    /// Next outbound MsgSeqNum(34).
    #[must_use]
    pub const fn next_outbound_seq(&self) -> u32 {
        self.next_outbound
    }

    /// Initiates the session: queues a Logon and awaits the reply.
    pub fn connect(&mut self, now: Instant) {
        if self.state != State::Disconnected {
            return;
        }
        self.pending.push(Pending::Logon);
        self.state = State::LogonSent;
        self.state_entered = Some(now);
        self.last_received = Some(now);
    }

    /// Initiates a clean logout: queues a Logout and awaits the confirm.
    pub fn logout(&mut self, now: Instant) {
        if !matches!(self.state, State::Active | State::Resending) {
            return;
        }
        self.pending.push(Pending::Logout);
        self.state = State::LogoutPending;
        self.state_entered = Some(now);
    }

    /// Allocates the MsgSeqNum(34) for an outbound application message
    /// and counts it as outbound activity for the heartbeat timer.
    pub fn allocate_seq(&mut self, now: Instant) -> u32 {
        let seq = self.next_outbound;
        self.next_outbound += 1;
        self.last_sent = Some(now);
        seq
    }

    /// Processes one framed inbound message. The framer validates
    /// BodyLength and CheckSum before this point.
    pub fn handle_message(&mut self, msg: &[u8], now: Instant) {
        let f = scan(msg);
        if !f.msg_type.is_present() {
            return;
        }
        let mt = f.msg_type.slice(msg);
        if self.state == State::Disconnected && mt != b"A" {
            return;
        }
        self.last_received = Some(now);
        self.test_request_sent = None;

        if f.sender.slice(msg) != self.cfg.target_comp_id
            || f.target.slice(msg) != self.cfg.sender_comp_id
        {
            self.pending.push(Pending::Logout);
            self.disconnect(DisconnectReason::CompIdMismatch);
            return;
        }
        let Ok(seq) = parse_fix_uint(f.seq_num.slice(msg)) else {
            return;
        };

        match self.state {
            State::Disconnected => self.on_logon(seq, &f, msg, true),
            State::LogonSent => match mt {
                b"A" => self.on_logon(seq, &f, msg, false),
                b"5" => self.disconnect(DisconnectReason::Logout),
                _ => {
                    self.pending.push(Pending::Logout);
                    self.disconnect(DisconnectReason::ProtocolViolation);
                }
            },
            State::Active | State::Resending | State::LogoutPending => {
                self.on_session_message(mt, seq, &f, msg);
            }
        }
    }

    /// Fires due timers: heartbeat emission, TestRequest probing, and
    /// logon/logout/test-request timeouts. Call at or after
    /// [`next_timeout`](Self::next_timeout).
    pub fn handle_timeout(&mut self, now: Instant) {
        match self.state {
            State::Disconnected => {}
            State::LogonSent => {
                if let Some(t) = self.state_entered
                    && now.duration_since(t) >= self.hb
                {
                    self.disconnect(DisconnectReason::LogonTimeout);
                }
            }
            State::LogoutPending => {
                if let Some(t) = self.state_entered
                    && now.duration_since(t) >= self.hb
                {
                    self.disconnect(DisconnectReason::LogoutTimeout);
                }
            }
            State::Active | State::Resending => {
                if let Some(t) = self.test_request_sent {
                    if now.duration_since(t) >= self.hb {
                        self.pending.push(Pending::Logout);
                        self.disconnect(DisconnectReason::TestRequestTimeout);
                        return;
                    }
                } else if let Some(t) = self.last_received
                    && now.duration_since(t) >= self.inbound_grace()
                {
                    self.test_req_counter += 1;
                    self.pending.push(Pending::TestRequest {
                        id: self.test_req_counter,
                    });
                    self.test_request_sent = Some(now);
                }
                if !self.heartbeat_queued
                    && let Some(t) = self.last_sent
                    && now.duration_since(t) >= self.hb
                {
                    self.pending.push(Pending::Heartbeat {
                        id: [0; TEST_REQ_ID_CAP],
                        id_len: 0,
                    });
                    self.heartbeat_queued = true;
                }
            }
        }
    }

    /// Earliest instant at which [`handle_timeout`](Self::handle_timeout)
    /// has work to do.
    #[must_use]
    pub fn next_timeout(&self) -> Option<Instant> {
        match self.state {
            State::Disconnected => None,
            State::LogonSent | State::LogoutPending => self.state_entered.map(|t| t + self.hb),
            State::Active | State::Resending => {
                let outbound = self.last_sent.map(|t| t + self.hb);
                let inbound = self.test_request_sent.map_or_else(
                    || self.last_received.map(|t| t + self.inbound_grace()),
                    |t| Some(t + self.hb),
                );
                match (outbound, inbound) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    (a, b) => a.or(b),
                }
            }
        }
    }

    /// Drains the next session event.
    pub fn poll_event(&mut self) -> Option<Event> {
        self.events.pop()
    }

    /// `true` if admin messages are queued for
    /// [`encode_pending`](Self::encode_pending).
    #[must_use]
    pub const fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Encodes the next queued admin message into `buf`, returning its
    /// length. `buf` must hold the full message (128 bytes plus CompID
    /// lengths is always enough). `unix_nanos` stamps SendingTime(52).
    pub fn encode_pending(
        &mut self,
        buf: &mut [u8],
        now: Instant,
        unix_nanos: i128,
    ) -> Option<usize> {
        let p = self.pending.pop()?;
        if matches!(p, Pending::Heartbeat { .. }) {
            self.heartbeat_queued = false;
        }
        let n = self.encode_admin(&p, buf, unix_nanos);
        self.last_sent = Some(now);
        Some(n)
    }

    fn on_logon(&mut self, seq: u32, f: &Fields, msg: &[u8], reply: bool) {
        if f.reset_seq_num.slice(msg) == b"Y" {
            self.next_inbound = 1;
            if reply {
                self.next_outbound = 1;
            }
        }
        if let Ok(hb) = parse_fix_uint(f.heart_bt_int.slice(msg)) {
            self.hb = Duration::from_secs(u64::from(hb));
        }
        if reply {
            self.pending.push(Pending::Logon);
        }
        if seq < self.next_inbound {
            self.pending.push(Pending::Logout);
            self.disconnect(DisconnectReason::SeqNumTooLow);
            return;
        }
        self.events.push(Event::Established {
            heart_bt_int_s: self.hb.as_secs() as u32,
        });
        if seq > self.next_inbound {
            self.gap_high = seq;
            self.pending.push(Pending::ResendRequest {
                begin: self.next_inbound,
            });
            self.state = State::Resending;
        } else {
            self.next_inbound += 1;
            self.state = State::Active;
        }
    }

    fn on_session_message(&mut self, mt: &[u8], seq: u32, f: &Fields, msg: &[u8]) {
        if mt == b"4" && f.gap_fill.slice(msg) != b"Y" {
            // SequenceReset-Reset disregards MsgSeqNum entirely.
            if let Ok(new_seq) = parse_fix_uint(f.new_seq_no.slice(msg)) {
                self.next_inbound = new_seq;
                self.events.push(Event::SequenceReset { new_seq });
                self.check_resend_done();
            }
            return;
        }
        let poss_dup = f.poss_dup.slice(msg) == b"Y";
        if seq > self.next_inbound {
            if self.gap_high < seq {
                self.gap_high = seq;
            }
            if self.state != State::Resending {
                self.pending.push(Pending::ResendRequest {
                    begin: self.next_inbound,
                });
                if self.state == State::Active {
                    self.state = State::Resending;
                }
            }
            return;
        }
        if seq < self.next_inbound {
            if poss_dup {
                return;
            }
            self.pending.push(Pending::Logout);
            self.disconnect(DisconnectReason::SeqNumTooLow);
            return;
        }
        self.next_inbound += 1;
        match mt {
            b"0" | b"A" => {}
            b"1" => {
                let id = f.test_req_id.slice(msg);
                let mut echo = [0u8; TEST_REQ_ID_CAP];
                let id_len = id.len().min(TEST_REQ_ID_CAP);
                echo[..id_len].copy_from_slice(&id[..id_len]);
                self.pending.push(Pending::Heartbeat {
                    id: echo,
                    id_len: id_len as u8,
                });
            }
            b"2" => {
                let begin = parse_fix_uint(f.begin_seq_no.slice(msg)).unwrap_or(1);
                let end = parse_fix_uint(f.end_seq_no.slice(msg)).unwrap_or(0);
                self.pending.push(Pending::SequenceReset {
                    seq: begin,
                    new_seq: self.next_outbound,
                });
                self.events.push(Event::ResendRange { begin, end });
            }
            b"3" => {
                let ref_seq_num = parse_fix_uint(f.ref_seq_num.slice(msg)).unwrap_or(0);
                self.events.push(Event::RejectReceived { ref_seq_num });
            }
            b"4" => {
                if let Ok(new_seq) = parse_fix_uint(f.new_seq_no.slice(msg)) {
                    if new_seq > self.next_inbound {
                        self.next_inbound = new_seq;
                    }
                    self.events.push(Event::SequenceReset { new_seq });
                }
            }
            b"5" => {
                if self.state != State::LogoutPending {
                    self.pending.push(Pending::Logout);
                }
                self.disconnect(DisconnectReason::Logout);
                return;
            }
            _ => self.events.push(Event::App {
                seq_num: seq,
                poss_dup,
            }),
        }
        self.check_resend_done();
    }

    fn check_resend_done(&mut self) {
        if self.state == State::Resending && self.next_inbound > self.gap_high {
            self.state = State::Active;
        }
    }

    fn disconnect(&mut self, reason: DisconnectReason) {
        self.state = State::Disconnected;
        self.test_request_sent = None;
        self.state_entered = None;
        self.heartbeat_queued = false;
        self.events.push(Event::Disconnected { reason });
    }

    fn inbound_grace(&self) -> Duration {
        self.hb + self.hb / 5
    }

    fn encode_admin(&mut self, p: &Pending, buf: &mut [u8], unix_nanos: i128) -> usize {
        let reserve = self.cfg.begin_string.len() + 11;
        let (seq, poss_dup) = if let Pending::SequenceReset { seq, .. } = p {
            (*seq, true)
        } else {
            let s = self.next_outbound;
            self.next_outbound += 1;
            (s, false)
        };
        let mut ts = [0u8; UTC_TIMESTAMP_LEN];
        format_utc_timestamp(unix_nanos, &mut ts);
        let mut num = [0u8; 20];
        let body_end = {
            let mut w = FieldWriter::wrap_at(buf, reserve);
            w.field(TAG_MSG_TYPE, msg_type_of(p));
            w.field(TAG_SENDER_COMP_ID, self.cfg.sender_comp_id);
            w.field(TAG_TARGET_COMP_ID, self.cfg.target_comp_id);
            let n = encode_fix_uint(seq, &mut num);
            w.field(TAG_MSG_SEQ_NUM, &num[..n]);
            if poss_dup {
                w.field(TAG_POSS_DUP_FLAG, b"Y");
            }
            w.field(TAG_SENDING_TIME, &ts);
            match p {
                Pending::Logon => {
                    w.field(TAG_ENCRYPT_METHOD, b"0");
                    let n = encode_fix_uint(self.hb.as_secs() as u32, &mut num);
                    w.field(TAG_HEART_BT_INT, &num[..n]);
                }
                Pending::Logout => {}
                Pending::SequenceReset { new_seq, .. } => {
                    w.field(TAG_GAP_FILL_FLAG, b"Y");
                    let n = encode_fix_uint(*new_seq, &mut num);
                    w.field(TAG_NEW_SEQ_NO, &num[..n]);
                }
                Pending::Heartbeat { id, id_len } => {
                    if *id_len > 0 {
                        w.field(TAG_TEST_REQ_ID, &id[..usize::from(*id_len)]);
                    }
                }
                Pending::TestRequest { id } => {
                    let n = encode_fix_seqnum(*id, &mut num);
                    w.field(TAG_TEST_REQ_ID, &num[..n]);
                }
                Pending::ResendRequest { begin } => {
                    let n = encode_fix_uint(*begin, &mut num);
                    w.field(TAG_BEGIN_SEQ_NO, &num[..n]);
                    w.field(TAG_END_SEQ_NO, b"0");
                }
            }
            w.pos()
        };
        let body_len = body_end - reserve;
        let mut pos = encode_field(buf, 0, TAG_BEGIN_STRING, self.cfg.begin_string);
        let n = encode_fix_seqnum(body_len as u64, &mut num);
        pos = encode_field(buf, pos, TAG_BODY_LENGTH, &num[..n]);
        buf.copy_within(reserve..body_end, pos);
        let end = pos + body_len;
        let ck = format_checksum(checksum(&buf[..end]));
        encode_field(buf, end, TAG_CHECK_SUM, &ck)
    }
}
