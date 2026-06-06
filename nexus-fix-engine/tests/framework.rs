use std::time::{Duration, Instant};

use nexus_fix_codec::{FieldView, FixDictionary, FixHeader, FixTimestamp, find_tag};
use nexus_fix_engine::{AdminMsg, CompId, Event, Out, Session, SessionConfig, SessionState, State};

// ── minimal mock dictionary ──────────────────────────────────────────────────

struct MockDict;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum MockMsgType {}

impl FixDictionary for MockDict {
    type MsgType = MockMsgType;
    type Header<'buf> = MockHeader<'buf>;
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

fn admin_msgs(out: Out) -> Vec<AdminMsg> {
    out.admin_messages().collect()
}

// Minimal valid Logon buffer — tags needed by Session<MockDict>.on_message.
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
    let (out, hdr) = s.on_message(&msg, Instant::now()).unwrap();
    assert!(hdr.is_none());
    assert_eq!(s.state().state(), State::Active);
    assert_eq!(out.event(), Some(Event::Established { heart_bt_int_s: 30 }));
    let admins = admin_msgs(out);
    assert_eq!(admins.len(), 1);
    assert!(matches!(
        admins[0],
        AdminMsg::Logon {
            seq: 1,
            heart_bt_int_s: 30
        }
    ));
}

#[test]
fn initiator_logon_reply() {
    let mut s = session();
    let now = Instant::now();
    s.state_mut().connect(now);
    assert_eq!(s.state().state(), State::LogonSent);

    let msg = logon(1, 30);
    let (out, hdr) = s.on_message(&msg, now).unwrap();
    assert!(hdr.is_none());
    assert_eq!(s.state().state(), State::Active);
    assert_eq!(admin_msgs(out).len(), 0);
}

#[test]
fn logout_round_trip() {
    let mut s = session();
    let now = Instant::now();
    let msg = logon(1, 30);
    s.on_message(&msg, now).unwrap();

    let msg = logout(2);
    let (out, _) = s.on_message(&msg, now).unwrap();
    assert_eq!(s.state().state(), State::Disconnected);
    assert_eq!(
        out.event(),
        Some(Event::Disconnected {
            reason: nexus_fix_engine::DisconnectReason::Logout,
        })
    );
    assert!(
        admin_msgs(out)
            .iter()
            .any(|a| matches!(a, AdminMsg::Logout { .. }))
    );
}

#[test]
fn heartbeat_is_handled() {
    let mut s = session();
    let now = Instant::now();
    s.on_message(&logon(1, 30), now).unwrap();

    let msg = heartbeat(2);
    let (out, hdr) = s.on_message(&msg, now).unwrap();
    assert!(hdr.is_none());
    assert_eq!(admin_msgs(out).len(), 0);
}

#[test]
fn test_request_echoed_as_heartbeat() {
    let mut s = session();
    let now = Instant::now();
    s.on_message(&logon(1, 30), now).unwrap();

    let msg = test_request(2, "PROBE1");
    let (out, hdr) = s.on_message(&msg, now).unwrap();
    assert!(hdr.is_none());
    let admins = admin_msgs(out);
    assert_eq!(admins.len(), 1);
    match admins[0] {
        AdminMsg::Heartbeat {
            echo: Some((id, len)),
            ..
        } => {
            assert_eq!(&id[..len as usize], b"PROBE1");
        }
        _ => panic!("expected Heartbeat with echo"),
    }
}

#[test]
fn resend_request_triggers_gap_fill() {
    let mut s = session();
    let now = Instant::now();
    s.on_message(&logon(1, 30), now).unwrap();
    s.state_mut().allocate_seq(now); // seq 2
    s.state_mut().allocate_seq(now); // seq 3

    let msg = resend_request(2, 2, 3);
    let (out, _) = s.on_message(&msg, now).unwrap();
    assert!(matches!(
        out.event(),
        Some(Event::ResendRange { begin: 2, end: 3 })
    ));
    assert!(
        admin_msgs(out)
            .iter()
            .any(|a| matches!(a, AdminMsg::SequenceReset { .. }))
    );
}

#[test]
fn sequence_reset_gap_fill() {
    let mut s = session();
    let now = Instant::now();
    s.on_message(&logon(1, 30), now).unwrap();

    // trigger a gap
    let msg = app_msg(5);
    s.on_message(&msg, now).unwrap();
    assert_eq!(s.state().state(), State::Resending);

    let msg = sequence_reset(2, 6, true);
    let (out, _) = s.on_message(&msg, now).unwrap();
    assert_eq!(s.state().next_inbound_seq(), 6);
    assert_eq!(s.state().state(), State::Active);
    assert_eq!(out.event(), Some(Event::SequenceReset { new_seq: 6 }));
}

#[test]
fn app_message_surfaces_header() {
    let mut s = session();
    let now = Instant::now();
    s.on_message(&logon(1, 30), now).unwrap();

    let msg = app_msg(2);
    let (out, hdr) = s.on_message(&msg, now).unwrap();
    assert!(hdr.is_some());
    assert_eq!(
        out.event(),
        Some(Event::App {
            seq_num: 2,
            poss_dup: false
        })
    );
}

#[test]
fn comp_id_mismatch_disconnects() {
    let mut s = session();
    let now = Instant::now();
    s.on_message(&logon(1, 30), now).unwrap();

    // Wrong SenderCompID
    let msg = b"34=2\x0135=0\x0149=WRONG\x0156=SENDER\x01";
    let (out, _) = s.on_message(msg, now).unwrap();
    assert_eq!(s.state().state(), State::Disconnected);
    assert!(matches!(
        out.event(),
        Some(Event::Disconnected {
            reason: nexus_fix_engine::DisconnectReason::CompIdMismatch
        })
    ));
}
