# Rathole Development Guide

## Scope and architecture

- Keep CLI parsing, application orchestration, domain types, storage, network transport, and TUI rendering in separate modules.
- Keep domain types independent from Ratatui, Crossterm, filesystem access, and Iroh runtime objects.
- Treat the user peer ID as a durable root identity and Iroh node identities as per-device transport identities.
- Contacts are local and one-way. Presence is shared only with contacts permitted by the owner.
- The built-in relay set is bootstrap configuration, not a central authority.

## TUI component architecture

- Keep `TuiApp` as the application orchestrator: it owns `TuiData`, global panel focus, the selected `UiConfig`, and application status. Only `TuiApp` may apply `UiCommand` values that mutate `TuiData`.
- Keep local, temporary presentation state in the relevant component state module: selection and tabs in Sidebar, drafts/cursors/transcript scroll in Chat, details scroll in Details, and menu/confirmation selection in Overlay. Do not move this state back into domain types or storage.
- Renderers must receive typed component props plus their component config and `UiTheme`; do not import, accept, or inspect `TuiApp` from `src/tui/components/`.
- Build immutable props at the composition boundary. A component may read only the contacts, relays, messages, focus, and local UI state that it needs for that frame.
- Make component changes through `UiConfig`, `UiTheme`, `LayoutSpec`, and the relevant component config. Do not introduce renderer-local colour, breakpoint, padding, or geometry constants when the setting belongs to a reusable preset.
- Keep input mapping independent of application ownership: `input.rs` consumes `InputContext` and returns `Action`; it must not mutate data or depend on `TuiApp`.
- Keep demo data explicit in `demo::sample_data()` and construct preview mode through `TuiApp::demo()`. Do not add fixtures or demo-only data constructors to neutral view/data types.
- Keep rendering side-effect free. Components request data mutations by returning `UiCommand`; `TuiApp` validates and applies the command.
- Test components with local typed props and `TestBackend`, without constructing `TuiApp`. Never disable or gate tests to bypass a props/config migration; migrate or replace them with equivalent direct-component coverage.

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
