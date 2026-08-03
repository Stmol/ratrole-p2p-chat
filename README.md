# Rathole

Rathole is a decentralised communication foundation for exchanging useful payloads such as messages and files. It is intentionally not a replacement for feature-complete consumer messengers. Its priorities are independent operation, extensibility, and compatibility with multiple clients.

The first client is a Rust terminal application. Future compatible clients may be TUI, web, native desktop, and mobile applications.

## Product model

- The current MVP peer ID is this installation’s Iroh `EndpointId`.
- The Iroh device secret is held by the operating system keychain, never in plaintext configuration.
- Contacts are local and one-way. Adding a contact only validates and stores an EndpointId locally.
- Rathole defines a versioned bootstrap relay set, including the Iroh N0 preset used to bootstrap transport, but the current relay list UI starts empty. Relay persistence remains future work; in-memory relay add/remove/toggle already exists in the TUI.
- A durable multi-device `UserPeerId`, recovery phrases, and presence exchange are future subsystems.

## Live chat MVP

- Launching `rathole` starts one online Iroh chat transport for that TUI session.
- Added local contacts can exchange 1:1 text while both clients are running and reachable. Contacts remain local and one-way.
- Outgoing messages first show `Pending`. `Delivered` means the remote Rathole runtime accepted the message; it is not a read receipt and does not make the message durable.
- Incoming messages for another contact receive a local unread badge, cleared when that chat is selected.
- Message bodies, delivery states, drafts, and unread counts exist only in memory. Restarting Rathole clears them.
- There is no offline delivery, retry, durable history, read receipt, presence, multi-device sync, user account, or file transfer in this MVP.

## Identity and contacts (MVP)

- The displayed peer ID is this installation’s Iroh EndpointId.
- The device secret is held by the OS keychain and must not be copied or shared.
- Use Contacts → x → Copy my ID to share the public ID.
- Use Contacts → x → Add contact to paste another Iroh EndpointId.
- Adding a contact only validates and saves the ID locally. It does not verify that the peer is online, reachable, or has accepted the contact.
- contacts.toml is temporary public storage and will later migrate to SQLite.
- Replacing a device key changes the peer ID; UserPeerId and multi-device migration are future work.

## Chat wire protocol (foundation)

- The standalone v1 contract is in `src/protocol/`; it serialises one complete CBOR document and has no Iroh, TUI, storage, or domain-contact dependency.
- Each document is `WireEnvelope { protocol_version: 1, frame }`; unknown fields and versions are rejected.
- Supported frames are `Text { message_id, sent_at_unix_ms, body }`, `Accepted { message_id, received_at_unix_ms }`, and `Rejected { message_id, code }`.
- `MessageId` is exactly 16 binary bytes. Text is non-empty UTF-8, preserves emoji and newlines, and is limited to 16 KiB; a full encoded document is limited to 32 KiB.
- This codec is the shared v1 document shape used by the live Iroh chat transport. Stream framing and peer authorisation come from the authenticated Iroh connection, not from fields inside CBOR.

## Online chat transport (MVP)

- `src/network/chat/` provides the Iroh ALPN `rathole/chat/1` transport. Launching the TUI starts exactly one transport for that session and shuts it down on exit.
- Delivery is allowed only to locally stored contacts. Incoming authorisation uses the authenticated Iroh `EndpointId`, not an identity field inside CBOR.
- Each delivery creates one Iroh connection and one bidirectional stream. The stream carries exactly one `Text → Accepted | Rejected` exchange and is closed after its terminal receipt; connections are not cached.
- Each stream document has a four-byte big-endian length prefix and a 32 KiB maximum CBOR body. Incoming and per-peer outgoing queues are capped at 64 messages. A full per-peer outgoing queue returns `QueueFull` immediately instead of blocking the caller.
- Messages from one sender worker are attempted FIFO. Every message gets a fresh connection, so timeouts and simultaneous bidirectional sends do not provide a durable global order. `connection_id` is local to one process; correlate two machine logs with `message_id`.
- Delivery is online-only with a 30-second local deadline that starts when `send_text` accepts a message into the per-peer outgoing queue and covers queueing, dial, and stream I/O. There is no persistence, retry, deduplication, sequence number, offline mailbox, or presence claim yet.
- Removing a contact cancels its active one-message connection, drains that peer's outbound worker, and cancels queued deliveries with `NotAContact`. Inbound connections from non-contacts are limited to a bounded handler budget with stream timeouts.

## Run

Install the stable Rust toolchain, then run:

```sh
cargo run
```

Launching without arguments opens the terminal UI. Explicit commands are reserved for focused automation and will report their bootstrap status until their domain behaviour is implemented.

```sh
cargo run -- --version
cargo test
just run
# equivalent shortcut: just r
```

## Chat diagnostics

Every launch creates one flushed JSONL diagnostic file. The application prints
the exact path before entering the terminal UI; by default it is under the
Rathole local data directory in `logs/`. Set `RATHOLE_LOG_FILE` when collecting
two runs so the files are easy to identify:

The repository shortcut `just run` (or `just r`) stores a timestamped log next
to the debug binary in `target/debug/rathole-logs/`.

```sh
RATHOLE_LOG_FILE=/tmp/rathole-client-a.jsonl cargo run
RATHOLE_LOG_FILE=/tmp/rathole-client-b.jsonl cargo run
```

Set `RATHOLE_IROH_PATH_MODE=relay-only` (or run `just run-relay`, alias `just rr`) to force relay-only paths for diagnostics. The default is `auto`; an invalid value stops startup with an explicit error.

Send both files unchanged when reporting a chat problem. Each record contains
the local `local_peer_id`, per-run `instance_id`, `event_id`, `seq`, wall-clock
`ts_unix_ms`/`ts_utc`, local `monotonic_ms`, remote `peer_id`, and when relevant
the shared `message_id` and `connection_id`. Use `ts_unix_ms` to align the two
computers approximately; use `seq` and `monotonic_ms` to establish exact order
inside each client. Message bodies, private keys, and device secrets are not
written to the diagnostic log; only message sizes and protocol metadata are
recorded. JSONL schema version 2 also records `stream_id`, path mode, selected
path kind/id, RTT, and UDP byte/datagram/loss counters when Iroh exposes them.
`receipt_write_finished`, `receipt_delivery_confirmed`, and `receipt_received`
are separate events; a local stream `finish` is not treated as delivery
confirmation.

Useful quick filters:

```sh
jq 'select(.message_id == "<message-id>")' /tmp/rathole-client-a.jsonl
jq 'select(.event | test("connection|receipt|delivery"))' /tmp/rathole-client-b.jsonl
```

## Terminal UI

Running `rathole` without arguments opens a keyboard-first UI backed by the
local peer identity, contact list, and one live chat transport for the session.

The UI is composed from internal TUI components. `TuiApp` owns view data and
applies UI commands, while List, Chat, Details, and modal state stay local to
their presentation components. Colours, geometry, and component spacing are
provided by an in-memory `UiConfig` preset when the app is created; Rathole
does not read a TUI configuration file or switch presets at runtime.

- `Tab` / `Shift+Tab`: cycle List, Chat, and Details
- `j/k` or arrow keys: select or scroll in the focused panel
- `h/l` or left/right: switch Contacts and Relays while List is focused
- `x`: open the Contacts / Relays / Chat context menu in Normal mode
- `i` or `Enter`: enter Chat Insert mode
- `Esc`: leave Insert mode or close a modal
- `Ctrl+C`: quit

The UI shows all three panels at 120 columns and wider, two panels from 80 to
119 columns, and one focused panel from 40 to 79 columns. Smaller terminals
show resize guidance.

## Project layout

```text
src/
  application.rs            Bootstrap, chat session lifecycle, and command dispatch
  application/chat_session.rs  Async bridge between TUI effects and ChatTransport
  cli.rs                    Command-line parsing
  domain/                   Transport-independent product concepts
  network/                  Iroh EndpointId / SecretKey boundary and chat transport
  storage/                  Keychain device secret and contacts.toml repository
  tui/                      Ratatui presentation and input loop
    components/             Typed panel renderers and local UI state
      editor.rs             Multibyte-safe chat draft editing helper
tests/                      Black-box binary tests
```
