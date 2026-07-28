# Rathole

Rathole is a decentralised communication foundation for exchanging useful payloads such as messages and files. It is intentionally not a replacement for feature-complete consumer messengers. Its priorities are independent operation, extensibility, and compatibility with multiple clients.

The first client is a Rust terminal application. Future compatible clients may be TUI, web, native desktop, and mobile applications.

## Product model

- The current MVP peer ID is this installation’s Iroh `EndpointId`.
- The Iroh device secret is held by the operating system keychain, never in plaintext configuration.
- Contacts are local and one-way. Adding a contact only validates and stores an EndpointId locally.
- Rathole ships a versioned bootstrap relay list, including n0 relay infrastructure. Relay persistence and mutation remain future work.
- A durable multi-device `UserPeerId`, recovery phrases, presence exchange, and chat transport are future subsystems.

## Identity and contacts (MVP)

- The displayed peer ID is this installation’s Iroh EndpointId.
- The device secret is held by the OS keychain and must not be copied or shared.
- Use Contacts → x → Copy my ID to share the public ID.
- Use Contacts → x → Add contact to paste another Iroh EndpointId.
- Adding a contact only validates and saves the ID locally. It does not verify that the peer is online, reachable, or has accepted the contact.
- contacts.toml is temporary public storage and will later migrate to SQLite.
- Replacing a device key changes the peer ID; UserPeerId and multi-device migration are future work.

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
