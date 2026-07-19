# Rathole Development Guide

## Scope and architecture

- Keep CLI parsing, application orchestration, domain types, storage, network transport, and TUI rendering in separate modules.
- Keep domain types independent from Ratatui, Crossterm, filesystem access, and Iroh runtime objects.
- Treat the user peer ID as a durable root identity and Iroh node identities as per-device transport identities.
- Contacts are local and one-way. Presence is shared only with contacts permitted by the owner.
- The built-in relay set is bootstrap configuration, not a central authority.

## Security

- Never persist recovery phrases, private keys, or device secrets in plaintext files, logs, snapshots, or tests.
- Store future device secrets in the platform keychain and keep only public configuration in the application data directory.
- Do not claim that a network, relay, identity, or encrypted payload operation happened unless the implementation actually performed it.

## Engineering workflow

- Prefer small, focused modules with explicit public interfaces.
- Format with `cargo fmt`.
- Lint with `cargo clippy --all-targets -- -D warnings`.
- Test with `cargo test`.
- Do not commit changes unless the user explicitly requests a commit.
