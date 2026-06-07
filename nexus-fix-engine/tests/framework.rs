use std::time::{Duration, Instant};

use nexus_fix_codec::{FieldView, FixAdminMsg, FixDictionary, FixHeader, FixTimestamp, find_tag};
use nexus_fix_engine::{
    CompId, DisconnectReason, Message, Session, SessionConfig, SessionState, State,
};

// ── minimal mock dictionary ──────────────────────────────────────────────────

struct MockDict;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum MockMsgType {}

// Minimal find_tag-based admin decoder used for all 7 types in the mock.
struct AdminDecoder<'buf> {
    buf: &'buf [u8],
}

impl<'buf> FixAdminMsg<'buf> for AdminDecoder<'buf> {
    fn decode(buf: &'buf [u8]) -> Result<Self, nexus_fix_codec::DecodeError> {
        Ok(Self { buf })
    }
}

impl<'buf> AdminDecoder<'buf> {
    fn heart_bt_int(&self) -> Option<FieldView<'buf, u32>> {
        find_tag(self.buf, 0, 108).and_then(|s| FieldView::new(s, self.buf))
    }

    fn test_req_id(&self) -> Option<FieldView<'buf, &'buf nexus_fix_codec::AsciiTextStr>> {
        find_tag(self.buf, 0, 112).and_then(|s| FieldView::new(s, self.buf))
    }

    fn begin_seq_no(&self) -> Option<FieldView<'buf, u64>> {
        find_tag(self.buf, 0, 7).and_then(|s| FieldView::new(s, self.buf))
    }

    fn end_seq_no(&self) -> Option<FieldView<'buf, u64>> {
        find_tag(self.buf, 0, 16).and_then(|s| FieldView::new(s, self.buf))
    }

    fn new_seq_no(&self) -> Option<FieldView<'buf, u64>> {
        find_tag(self.buf, 0, 36).and_then(|s| FieldView::new(s, self.buf))
    }
}

impl FixDictionary for MockDict {
    type MsgType = MockMsgType;
    type Header<'buf> = MockHeader<'buf>;
    type Logon<'buf> = AdminDecoder<'buf>;
    type Logout<'buf> = AdminDecoder<'buf>;
    type Heartbeat<'buf> = AdminDecoder<'buf>;
    type TestRequest<'buf> = AdminDecoder<'buf>;
    type ResendRequest<'buf> = AdminDecoder<'buf>;
    type SequenceReset<'buf> = AdminDecoder<'buf>;
    type Reject<'buf> = AdminDecoder<'buf>;
    const BEGIN_STRING: &'static [u8] = b"FIX.4.4";
    fn is_admin(_: MockMsgType) -> bool {
        false
    }
}

struct MockHeader<'buf> {
    buf: &'buf [u8],
}

impl<'buf> FixHeader<'buf> for MockHeader<'buf> {
    fn decode(buf: &'buf [u8]) -> Self {
        Self { buf }
    }

    fn raw_msg_type(&self) -> Option<FieldView<'buf, &'buf [u8]>> {
        find_tag(self.buf, 0, 35).and_then(|s| FieldView::new(s, self.buf))
    }

    fn msg_seq_num(&self) -> Option<FieldView<'buf, u64>> {
        find_tag(self.buf, 0, 34).and_then(|s| FieldView::new(s, self.buf))
    }

    fn sender_comp_id(&self) -> Option<FieldView<'buf, &'buf nexus_fix_codec::AsciiTextStr>> {
        find_tag(self.buf, 0, 49).and_then(|s| FieldView::new(s, self.buf))
    }

    fn target_comp_id(&self) -> Option<FieldView<'buf, &'buf nexus_fix_codec::AsciiTextStr>> {
        find_tag(self.buf, 0, 56).and_then(|s| FieldView::new(s, self.buf))
    }

    fn poss_dup_flag(&self) -> Option<FieldView<'buf, bool>> {
        find_tag(self.buf, 0, 43).and_then(|s| FieldView::new(s, self.buf))
    }

    fn sending_time(&self) -> Option<FieldView<'buf, FixTimestamp>> {
        None
    }
}

// ── helpers ──────────────────────────────────────────────────────────────────

const HB: Duration = Duration::from_secs(30);

fn session() -> Session<MockDict> {
    let state = SessionState::new(HB);
    let config = SessionConfig {
        sender: CompId::new(b"SENDER").unwrap(),
        target: CompId::new(b"TARGET").unwrap(),
    };
    Session::new(state, config)
}

// 49=TARGET (their sender = our target), 56=SENDER (their target = our sender).
fn logon(seq: u32, hbi: u32) -> Vec<u8> {
    let mut v = Vec::new();
    for part in [
        format!("34={seq}\x01"),
        "35=A\x01".to_string(),
        "49=TARGET\x01".to_string(),
        "56=SENDER\x01".to_string(),
        format!("108={hbi}\x01"),
    ] {
        v.extend_from_slice(part.as_bytes());
    }
    v
}

fn logout(seq: u32) -> Vec<u8> {
    format!("34={seq}\x0135=5\x0149=TARGET\x0156=SENDER\x01").into_bytes()
}

fn heartbeat(seq: u32) -> Vec<u8> {
    format!("34={seq}\x0135=0\x0149=TARGET\x0156=SENDER\x01").into_bytes()
}

fn test_request(seq: u32, id: &str) -> Vec<u8> {
    format!("34={seq}\x0135=1\x0149=TARGET\x0156=SENDER\x01112={id}\x01").into_bytes()
}

fn resend_request(seq: u32, begin: u32, end: u32) -> Vec<u8> {
    format!("34={seq}\x017={begin}\x0116={end}\x0135=2\x0149=TARGET\x0156=SENDER\x01").into_bytes()
}

fn sequence_reset(seq: u32, new_seq: u32, gap_fill: bool) -> Vec<u8> {
    format!(
        "34={seq}\x0135=4\x0149=TARGET\x0156=SENDER\x0136={new_seq}\x01123={}\x01",
        if gap_fill { "Y" } else { "N" }
    )
    .into_bytes()
}

fn app_msg(seq: u32) -> Vec<u8> {
    format!("34={seq}\x0135=D\x0149=TARGET\x0156=SENDER\x01").into_bytes()
}

// ── tests ────────────────────────────────────────────────────────────────────

#[test]
fn acceptor_logon() {
    let mut s = session();
    let msg = logon(1, 30);
    let m = s.on_message(&msg, Instant::now()).unwrap();
    assert!(matches!(m, Message::LogonRequest { .. }));
    assert_eq!(s.state().state(), State::Active);
    if let Message::LogonRequest { msg } = m {
        assert_eq!(msg.heart_bt_int().and_then(|v| v.checked().ok()), Some(30));
    }
}

#[test]
fn initiator_logon_reply() {
    let mut s = session();
    let now = Instant::now();
    s.state_mut().connect(now);
    assert_eq!(s.state().state(), State::LogonSent);

    let msg = logon(1, 30);
    let m = s.on_message(&msg, now).unwrap();
    assert!(matches!(m, Message::LogonAcknowledged { .. }));
    assert_eq!(s.state().state(), State::Active);
}

#[test]
fn logout_round_trip() {
    let mut s = session();
    let now = Instant::now();
    s.on_message(&logon(1, 30), now).unwrap();

    let msg = logout(2);
    let m = s.on_message(&msg, now).unwrap();
    assert!(matches!(m, Message::LogoutRequest { .. }));
    assert_eq!(s.state().state(), State::Disconnected);
}

#[test]
fn heartbeat_is_handled() {
    let mut s = session();
    let now = Instant::now();
    s.on_message(&logon(1, 30), now).unwrap();

    let hb = heartbeat(2);
    let m = s.on_message(&hb, now).unwrap();
    assert!(matches!(m, Message::Heartbeat { .. }));
}

#[test]
fn test_request_surfaces_id() {
    let mut s = session();
    let now = Instant::now();
    s.on_message(&logon(1, 30), now).unwrap();

    let msg = test_request(2, "PROBE1");
    let m = s.on_message(&msg, now).unwrap();
    if let Message::TestRequest { msg } = m {
        let id = msg
            .test_req_id()
            .and_then(|v| v.checked().ok())
            .map(nexus_fix_codec::AsciiTextStr::as_bytes);
        assert_eq!(id, Some(b"PROBE1".as_ref()));
    } else {
        panic!("expected TestRequest");
    }
}

#[test]
fn resend_request_fields() {
    let mut s = session();
    let now = Instant::now();
    s.on_message(&logon(1, 30), now).unwrap();
    s.state_mut().allocate_seq(now); // seq 2
    s.state_mut().allocate_seq(now); // seq 3

    let msg = resend_request(2, 2, 3);
    let m = s.on_message(&msg, now).unwrap();
    if let Message::ResendRequest { msg } = m {
        assert_eq!(msg.begin_seq_no().and_then(|v| v.checked().ok()), Some(2));
        assert_eq!(msg.end_seq_no().and_then(|v| v.checked().ok()), Some(3));
    } else {
        panic!("expected ResendRequest");
    }
    assert_eq!(s.state().state(), State::Active);
}

#[test]
fn sequence_reset_gap_fill() {
    let mut s = session();
    let now = Instant::now();
    s.on_message(&logon(1, 30), now).unwrap();

    // trigger a gap
    s.on_message(&app_msg(5), now).unwrap();
    assert_eq!(s.state().state(), State::Resending);

    let msg = sequence_reset(2, 6, true);
    let m = s.on_message(&msg, now).unwrap();
    if let Message::SequenceReset { msg } = m {
        assert_eq!(msg.new_seq_no().and_then(|v| v.checked().ok()), Some(6));
    } else {
        panic!("expected SequenceReset");
    }
    assert_eq!(s.state().next_inbound_seq(), 6);
    assert_eq!(s.state().state(), State::Active);
}

#[test]
fn app_message_surfaces_header() {
    let mut s = session();
    let now = Instant::now();
    s.on_message(&logon(1, 30), now).unwrap();

    let msg = app_msg(2);
    let m = s.on_message(&msg, now).unwrap();
    assert!(matches!(m, Message::Application { .. }));
}

#[test]
fn comp_id_mismatch_disconnects() {
    let mut s = session();
    let now = Instant::now();
    s.on_message(&logon(1, 30), now).unwrap();

    let msg = b"34=2\x0135=0\x0149=WRONG\x0156=SENDER\x01";
    let m = s.on_message(msg, now).unwrap();
    assert!(matches!(
        m,
        Message::Disconnected {
            reason: DisconnectReason::CompIdMismatch
        }
    ));
    assert_eq!(s.state().state(), State::Disconnected);
}
