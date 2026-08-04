# Rathole

Rathole is a decentralised communication foundation for exchanging useful payloads such as messages and files. It is intentionally not a replacement for a feature-complete consumer messenger: its priorities are independent operation, extensibility, and compatibility with multiple clients.

The first client is a Rust terminal application. Future compatible clients may be TUI, web, native desktop, and mobile applications.

## Current MVP

- The Rust TUI uses this installation's Iroh `EndpointId` as its displayed peer ID.
- Contacts are local and one-way. Adding a contact stores an ID locally; it does not prove that the peer is online or reachable.
- Two running, connected clients can exchange 1:1 text online through the Iroh chat transport.
- `Delivered` means the remote Rathole runtime accepted the message. It is not a read receipt and does not make the message durable.
- Messages, drafts, delivery states, unread counts, and connection state are in memory and disappear on restart. There is no offline delivery, retry, durable history, presence, multi-device sync, user account, or file transfer yet.
- The normal profile stores the device secret in the operating-system keychain. Never copy or share that secret; use `just dev` for local testing.

## Requirements

- Stable Rust 1.91 or newer.
- `just` is optional but provides the repository's development shortcuts.
- Node.js 20.19.0 or newer is needed only for the optional OpenSpec development workflow.

## Quick start

Install the stable Rust toolchain, then launch the TUI:

```sh
cargo run
```

Useful local commands:

```sh
cargo run -- --version
cargo test
just run
just dev
```

`just dev` uses a separate file-backed development identity. See the [development guide](docs/development.md) for storage, validation, and OpenSpec instructions.

## Basic controls

- `Tab` / `Shift+Tab`: cycle List, Chat, and Details.
- `1` / `2`: select the Contacts or Relays list tab.
- `j` / `k` or arrow keys: select and scroll in the focused panel.
- `i` or `Enter`: enter chat insert mode; `Esc` returns to normal mode.
- `x`: open the context menu for the focused area.
- `Ctrl+C` or `q`: quit.

## Documentation

- [Architecture](docs/architecture.md) — runtime layers, identity boundaries, chat flow, and TUI ownership.
- [Development guide](docs/development.md) — local profiles, repository checks, and OpenSpec workflow.
- [Chat diagnostics](docs/diagnostics.md) — paired-client JSONL collection and correlation.
- [Chat wire protocol](src/protocol/README.md) — strict versioned CBOR contract.
- [Iroh chat transport](src/network/chat/README.md) — connections, streams, bounds, and delivery semantics.
- [Engineering guide](AGENTS.md) — project-wide architecture, security, and contributor rules.

OpenSpec is a development-time planning layer, not a Rathole runtime dependency. Its detailed setup and command forms are maintained in the [development guide](docs/development.md#openspec-workflow).
