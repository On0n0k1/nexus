use crate::field::FieldView;
use crate::types::FixTimestamp;
use nexus_ascii::AsciiTextStr;

/// Zero-copy decoder for a session-level admin message.
///
/// Implemented by every generated admin message type in `admin::*`.
/// The session framework calls `decode` to construct the decoder and hands it
/// to the caller via [`Message`](crate::Message); the caller then uses the
/// typed accessor methods to read fields.
pub trait FixAdminMsg<'buf>: Sized {
    /// Construct the decoder from a raw FIX message buffer.
    fn decode(buf: &'buf [u8]) -> Result<Self, crate::DecodeError>;
}

/// Dictionary-level knowledge for a specific FIX version.
///
/// Generated per dictionary (FIX 4.2, FIX 4.4, etc.) by `nexus-fix-codegen`.
/// The implementing type is a zero-sized struct — all information is
/// compile-time. The `Session` is generic over this trait, so
/// FIX-version dispatch monomorphizes away with no vtable or runtime branching.
pub trait FixDictionary {
    /// The dictionary's message-type enum (generated, closed set).
    type MsgType: Copy + Eq + core::fmt::Debug;

    /// The dictionary's generated header decoder type.
    type Header<'buf>: FixHeader<'buf>;

    /// Decoder for Logon (35=A).
    type Logon<'buf>: FixAdminMsg<'buf>;
    /// Decoder for Logout (35=5).
    type Logout<'buf>: FixAdminMsg<'buf>;
    /// Decoder for Heartbeat (35=0).
    type Heartbeat<'buf>: FixAdminMsg<'buf>;
    /// Decoder for TestRequest (35=1).
    type TestRequest<'buf>: FixAdminMsg<'buf>;
    /// Decoder for ResendRequest (35=2).
    type ResendRequest<'buf>: FixAdminMsg<'buf>;
    /// Decoder for SequenceReset (35=4).
    type SequenceReset<'buf>: FixAdminMsg<'buf>;
    /// Decoder for Reject (35=3).
    type Reject<'buf>: FixAdminMsg<'buf>;

    /// The `BeginString` value for this FIX version (e.g. `b"FIX.4.4"`).
    const BEGIN_STRING: &'static [u8];

    /// Whether the given message type is an admin (session-level) message.
    fn is_admin(msg_type: Self::MsgType) -> bool;
}

/// Session-level header field access.
///
/// Implemented by every generated `HeaderDecoder`. Provides the protocol-
/// mandatory fields that session-layer code needs for sequencing, routing,
/// and heartbeat logic — without knowing which dictionary is in use.
pub trait FixHeader<'buf>: Sized {
    /// Decode the header from a raw FIX message buffer.
    fn decode(buf: &'buf [u8]) -> Self;

    /// Raw `MsgType` bytes (tag 35) for session-layer admin detection.
    fn raw_msg_type(&self) -> Option<FieldView<'buf, &'buf [u8]>>;

    /// `MsgSeqNum` (tag 34).
    fn msg_seq_num(&self) -> Option<FieldView<'buf, u64>>;

    /// `SenderCompID` (tag 49).
    fn sender_comp_id(&self) -> Option<FieldView<'buf, &'buf AsciiTextStr>>;

    /// `TargetCompID` (tag 56).
    fn target_comp_id(&self) -> Option<FieldView<'buf, &'buf AsciiTextStr>>;

    /// `PossDupFlag` (tag 43).
    fn poss_dup_flag(&self) -> Option<FieldView<'buf, bool>>;

    /// `SendingTime` (tag 52).
    fn sending_time(&self) -> Option<FieldView<'buf, FixTimestamp>>;
}
