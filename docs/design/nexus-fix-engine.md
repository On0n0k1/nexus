# nexus-fix-engine — Session Layer

Sans-IO FIX session state machine, TCP framing, and persistence.

Depends on `nexus-fix-codec` for parsing primitives. Uses
`nexus-shm`'s ShmJournal for durable message storage.

---

## Sans-IO Session State Machine

Same pattern as nexus-net's WebSocket codec. Pure state machine
with poll-based event dispatch and caller-owned encode buffer.

```
handle_message(msg, now) -> pushes events to internal buffer
handle_timeout(now)      -> pushes timer events
poll_event()             -> drains events, caller processes
encode_pending(kind, buf)-> encodes into caller's buffer
```

**Key design:** Session never allocates. The caller provides a
"workhorse buffer" for encoding. Events are `Copy` enum variants
(no borrowed data).

### State Diagram

```
Disconnected -> LogonSent -> Active <-> Resending -> LogoutPending -> Disconnected
```

All admin messages (Logon, Logout, Heartbeat, TestRequest,
ResendRequest, SequenceReset, Reject) handled internally by the
state machine. Application messages emitted as events for user
code.

---

## TCP Framing

Splits a TCP byte stream into individual FIX messages. Scans for
the `8=FIX...` header, parses BodyLength (tag 9), extracts exactly
that many bytes plus the trailer (tag 10). Validates checksum
before handing the message to the session state machine.

The framer is a sans-IO state machine — it consumes `&[u8]` chunks
from the caller and produces framed messages. No sockets, no
buffering policy. The caller decides how to read from TCP.

```rust
pub struct FixFramer {
    state: FramerState,
}

impl FixFramer {
    /// Feed bytes from TCP. Returns framed messages and
    /// the number of bytes consumed.
    pub fn decode<'a>(&mut self, buf: &'a [u8])
        -> Result<(Option<&'a [u8]>, usize), FrameError>;
}
```

---

## Persistence

### Trait-based storage

Pluggable via traits so users can swap implementations:

```rust
pub trait MessageStore {
    type Error;
    fn store(&mut self, session_id: SessionId, direction: Direction,
             seq_num: u32, msg: &[u8]) -> Result<(), Self::Error>;
    fn retrieve(&self, session_id: SessionId, range: SeqRange)
             -> impl Iterator<Item = Result<StoredMessage<'_>, Self::Error>>;
}

pub trait SessionStore {
    type Error;
    fn load(&self, id: SessionId) -> Result<Option<SessionState>, Self::Error>;
    fn save(&mut self, id: SessionId, state: &SessionState) -> Result<(), Self::Error>;
}
```

### ShmJournal integration

Default implementations backed by `nexus-shm`'s ShmJournal.

```
TCP read -> raw bytes
  -> journal.append(...)     <- mmap write, ~ns
  -> fix_codec.parse(bytes)  <- zero-copy from read buffer
  -> session.handle_message  <- state machine
  -> poll_event loop         <- dispatch

Resend request:
  -> journal.read_range(...) <- mmap read, zero-copy
  -> TCP write
```

---

## Performance Targets

| Operation | Target |
|-----------|--------|
| Session logic (admin msg handling) | < 100ns |
| Message store write (ShmJournal) | < 500ns |
| Full inbound path (frame + codec + session + store) | < 1μs |

Codec-level targets (SOH scan, field access, encode) are in
[nexus-fix-codec.md](nexus-fix-codec.md).

---

## Open Questions

- **Heartbeat timer integration:** Should the session own its
  timer state, or delegate to the caller's timer wheel? Owning
  simplifies the API but couples to a time source. Delegating
  is more composable but requires the caller to wire up
  `handle_timeout` calls.

- **Session multiplexing:** One session per TCP connection is
  standard FIX. But some venues multiplex. Should the engine
  support multiple sessions on one transport, or is that a
  higher-level concern?

- **Multi-message framing:** Should the framer live in
  nexus-fix-codec (pure byte-level concern) or here in the
  engine (since it's TCP-stream-aware)? Framing needs BodyLength
  parsing which is codec-level, but the framer is only useful
  in a streaming context which is engine-level.

---

## References

- **Artio** — Adaptive's FIX engine. Session/engine separation,
  archive-based replay. Validates the sans-IO approach.
- **QuickFIX** — Canonical FIX engine. Session management and
  persistence patterns are well-established.
- **nexus-net** — Existing sans-IO WebSocket implementation. Same
  architectural pattern for the session state machine.
