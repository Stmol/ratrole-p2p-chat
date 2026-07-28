# Rathole

Rathole is a decentralised communication foundation for exchanging useful payloads such as messages and files. It is intentionally not a replacement for feature-complete consumer messengers. Its priorities are independent operation, extensibility, and compatibility with multiple clients.

The first client is a Rust terminal application. Future compatible clients may be TUI, web, native desktop, and mobile applications.

## Product model

- A **user peer ID** is a durable public identity that contacts use.
- Each device owns a distinct **Iroh device identity**. The user identity authorises and can revoke devices without changing the user peer ID.
- A 24-word recovery phrase restores the user identity. Device secrets are held by the operating system keychain, never in plaintext configuration.
- Contacts are local and one-way. Presence metadata is shared only with contacts selected by its owner.
- Rathole ships a versioned bootstrap relay list, including n0 relay infrastructure. Users will be able to inspect, add, remove, and replace relay entries.

Payload transfer, secure identity storage, device authorisation, presence exchange, contact persistence, and relay mutation are deliberately future subsystems. This repository currently provides their boundaries, not a misleading mock implementation.

## Run

Install the stable Rust toolchain, then run:

```sh
cargo run
```

Launching without arguments opens the terminal UI preview. Explicit commands are reserved for focused automation and will report their bootstrap status until their domain behaviour is implemented.

```sh
cargo run -- --version
cargo test
```

## Terminal UI preview

Running `rathole` without arguments opens a keyboard-first UI preview backed by
in-memory demo data. Contacts, presence values, relay state, chat history, and
context-menu changes are not loaded from or written to persistent storage.
Message submission is deliberately unavailable.

- `Tab` / `Shift+Tab`: cycle List, Chat, and Details
- `j/k` or arrow keys: select or scroll in the focused panel
- `h/l` or left/right: switch Contacts and Relays while List is focused
- `x`: open the focused item’s context menu in Normal mode
- `i` or `Enter`: enter Chat Insert mode
- `Esc`: leave Insert mode or close a modal
- `Ctrl+C`: quit

The UI shows all three panels at 120 columns and wider, two panels from 80 to
119 columns, and one focused panel from 40 to 79 columns. Smaller terminals
show resize guidance.

## Project layout

```text
src/
  application.rs    Command dispatch and application boundary
  cli.rs            Command-line parsing
  domain/           Transport-independent product concepts
  network/          Iroh transport boundary
  storage/          Application data path boundary
  tui/              Ratatui presentation and input loop
tests/              Black-box binary tests
```
