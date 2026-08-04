## Why

The contact details panel currently shows only the peer ID and a coarse connection state. When a contact is connected, users cannot tell whether Iroh is using a direct IP path or a relay, which concrete remote transport address is selected, or how long the current logical connection has been maintained.

This diagnostic information is needed now because Iroh can migrate between relay and direct paths while the contact remains connected, and the UI should expose the active transport without claiming that the configured path mode is the actual selected path.

## What Changes

- Add live connection diagnostics to contact details:
  - the currently selected Iroh path kind: direct IP, relay, custom, or unknown;
  - the selected path's remote transport address, including the relay URL or IP socket address when available;
  - the elapsed time since the contact session entered `Connected`.
- Propagate selected-path changes independently of connection state changes so relay-to-direct and direct-to-relay migration is visible without reconnecting the logical session.
- Clear path, address, and connection-duration values when the contact becomes disconnected; do not persist them in contact storage.
- Keep `IrohPathMode` configuration distinct from the observed selected path.
- Preserve stale-event protection for replaced or draining Iroh connections.

## Capabilities

### New Capabilities

- `contact-connection-diagnostics`: Expose the selected Iroh transport path, its remote address, and live logical connection duration in the contact details panel.

### Modified Capabilities

- None.

## Impact

- `src/domain/connection.rs`: app-neutral path classification used at the transport/UI boundary.
- `src/network/chat/`: selected-path snapshots, Iroh path-event monitoring, connection lifecycle timestamps, and enriched connection events.
- `src/application/chat_session.rs`: forward enriched runtime connection updates to the TUI.
- `src/tui/model.rs`, `src/tui/app.rs`, and `src/tui/components/details.rs`: hold, apply, format, and render live contact diagnostics.
- Network and TUI tests: cover path classification, path migration, stale connection events, state-dependent clearing, and duration formatting.
- No wire protocol, contact TOML schema, keychain data, relay configuration schema, or persisted state changes.
