# Rathole

Rathole is a decentralised communication foundation for exchanging useful payloads such as messages and files. It is intentionally not a replacement for feature-complete consumer messengers. Its priorities are independent operation, extensibility, and compatibility with multiple clients.

The first client is a Rust terminal application. Future compatible clients may be TUI, web, native desktop, and mobile applications.

## Product model

- The current MVP peer ID is this installation’s Iroh `EndpointId`.
- The Iroh device secret is held by the operating system keychain, never in plaintext configuration.
- Contacts are local and one-way. Adding a contact only validates and stores an EndpointId locally.
- Rathole ships a versioned bootstrap relay list, including n0 relay infrastructure. Relay persistence and mutation remain future work.
- A durable multi-device `UserPeerId`, recovery phrases, and presence exchange are future subsystems. An online chat transport library exists under `src/network/chat/` but is not wired into the CLI/TUI yet.

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
- This is not live chat yet. A future Iroh adapter must supply stream framing and peer authorisation from the authenticated Iroh connection before it invokes this codec.

## Online chat transport (MVP foundation)

- `src/network/chat/` provides a tested library transport using Iroh ALPN `rathole/chat/1`. The current CLI/TUI does not start it or display messages yet.
- Delivery is allowed only to locally stored contacts. Incoming authorisation uses the authenticated Iroh `EndpointId`, not an identity field inside CBOR.
- A live Iroh connection is reused when available, but each message has its own bidirectional stream and `Text → Accepted | Rejected` exchange.
- Each stream document has a four-byte big-endian length prefix and a 32 KiB maximum CBOR body. Incoming and per-peer outgoing queues are capped at 64 messages. A full per-peer outgoing queue returns `QueueFull` immediately instead of blocking the caller.
- Messages from one sender worker are attempted FIFO while its connection remains healthy. Timeouts, reconnects, and simultaneous bidirectional sends do not provide a durable global order.
- Delivery is online-only with a 30-second local deadline that starts when `send_text` accepts a message into the per-peer outgoing queue and covers queueing, dial, and stream I/O. There is no persistence, retry, deduplication, sequence number, offline mailbox, or presence claim yet.
- Removing a contact closes cached connections, drains that peer's outbound worker, and cancels queued deliveries with `NotAContact`. Inbound connections from non-contacts are limited to a bounded handler budget with stream timeouts.

## Run

Install the stable Rust toolchain, then run:

```sh
cargo run
```

Launching without arguments opens the terminal UI. Explicit commands are reserved for focused automation and will report their bootstrap status until their domain behaviour is implemented.

```sh
cargo run -- --version
cargo test
```

## Terminal UI

Running `rathole` without arguments opens a keyboard-first UI backed by the
local peer identity and contact list. Messaging is not implemented yet.

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
  application.rs    Bootstrap, effect handling, and command dispatch
  cli.rs            Command-line parsing
  domain/           Transport-independent product concepts
  network/          Iroh EndpointId / SecretKey boundary
  storage/          Keychain device secret and contacts.toml repository
  tui/              Ratatui presentation and input loop
tests/              Black-box binary tests
```
