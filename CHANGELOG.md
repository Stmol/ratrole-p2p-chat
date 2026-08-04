# Changelog

## [0.1.0-alpha.1] - 2026-08-05

First public alpha release of Rathole.

### What this release provides

- Direct peer-to-peer connections when network conditions allow them.
- Relay connectivity when a direct path is unavailable.
- One-to-one text chat between two running clients.
- Local one-way contacts addressed by Iroh Peer ID.
- A terminal user interface with contact management, connection status, path diagnostics, and message delivery status.
- Automatic persistent device identity stored in the operating system Keychain.

### Why download it

This release is intended for real two-device testing. It does not require Rust,
Cargo, Just, development certificates, or a local build environment. Download
the archive for your platform, run the binary from a terminal, exchange Peer
IDs, and add each other as contacts.

### Known limitations

- Messages are online-only and kept in memory.
- There is no offline delivery, durable message history, account system, presence, multi-device sync, or file transfer.
- This is an alpha release and should not be treated as production software.

### macOS first launch

The macOS archive is not notarized. If Gatekeeper blocks the first launch, try
to run the binary once and allow it through System Settings → Privacy & Security
→ Open Anyway.

Do not share the device secret. Share only the public Peer ID.
