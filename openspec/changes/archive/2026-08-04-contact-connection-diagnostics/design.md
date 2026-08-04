## Context

The proposal and capability specification define the observable behavior. The current transport emits only contact state updates, while the session actor already owns the selected primary connection and distinguishes it from draining connections. The TUI contact view currently stores only the peer ID, unread count, and connection state.

Iroh 1.0.3 exposes a snapshot of open paths through `Connection::paths()` and a stream of `PathEvent` values through `Connection::path_events()`. A path exposes a remote `TransportAddr`, which can represent a relay, an IP address, or a custom transport. Iroh can have multiple open paths at once, so the diagnostic must track the selected path rather than the configured `IrohPathMode`.

## Goals / Non-Goals

**Goals:**

- Provide an app-neutral selected-path value with a path kind and optional remote address.
- Publish path changes while a logical contact session remains connected.
- Preserve a monotonic logical-session start time across path migration and render its elapsed duration live.
- Keep diagnostics in memory and preserve the existing transport, protocol, storage, and TUI ownership boundaries.
- Ignore path updates from replaced or draining connections.

**Non-Goals:**

- Showing every simultaneously open path, path history, RTT, packet counters, or local socket address.
- Changing Iroh path selection, relay configuration, or `RATHOLE_IROH_PATH_MODE` behavior.
- Persisting connection diagnostics or changing the CBOR protocol and contact storage schema.
- Treating a selected path or connected state as proof of remote presence or message read status.

## Decisions

### 1. Represent path diagnostics without an Iroh dependency in domain/UI data

Introduce an app-neutral connection-path representation containing:

- a kind: `DirectIp`, `Relay`, `Custom`, or `Unknown`;
- an optional owned remote-address string.

The network adapter converts Iroh's selected `TransportAddr` into this value. Relay and IP addresses are retained as diagnostic strings; an unavailable or future/unclassifiable address maps to `Unknown` rather than being guessed. This keeps domain and TUI data independent of Iroh runtime objects while preserving the exact selected address required by the specification.

Alternatives considered:

- Storing `iroh::TransportAddr` in `ContactView`: rejected because it violates the domain/UI boundary and couples rendering to Iroh.
- Storing only a path label: rejected because it loses the selected relay URL or IP socket address.
- Reusing `IrohPathMode`: rejected because `auto` and `relay-only` describe configuration, not the observed selected path.

### 2. Enrich connection updates instead of creating a second UI data source

Extend the runtime connection update with the current state, selected-path diagnostic, and optional `connected_since: Instant`. A connected path refresh uses the same logical session start time as the original `Connected` update. The application forwards the enriched update to one TUI command, and `TuiApp` remains the only owner that mutates `TuiData`.

The existing state-only connection query remains state-oriented. Diagnostics are delivered as runtime events because path selection can change without changing the state.

### 3. Monitor path events, then re-read the selected-path snapshot

Each attached Iroh connection gets a path-event monitor alongside its existing accept and close monitoring. On `Opened`, `Closed`, `Selected`, or `Lagged`, the monitor signals the session actor with the connection ID. The actor re-reads `Connection::paths()` and extracts the currently selected path.

Re-reading the snapshot is required after `Lagged`, because individual events may have been dropped while the current open-path state remains recoverable from the connection. It also gives all event types one consistent classification path.

The session actor publishes a path update only when the event's connection ID still matches the primary connection. Updates from a draining or replaced connection are ignored, and a close event removes the primary before any later stale path update can affect the UI.

### 4. Measure logical connection lifetime with a monotonic timestamp

The session actor records `Instant::now()` when its external state first enters `Connected` and clears the timestamp when the primary session becomes disconnected. Replacing a primary connection or migrating between paths while the external state remains `Connected` does not reset the timestamp; the UI label represents logical contact connectivity rather than one physical QUIC connection.

The TUI composition boundary derives the current `Duration` from that timestamp. Details rendering receives the derived duration and remains formatting-only, so component tests can use deterministic durations without constructing the application runtime clock.

### 5. Render a state-dependent, scrollable diagnostic block

The contact details panel keeps the existing peer ID and connection state, then adds:

```text
Path: Direct IP
Address: ip:192.0.2.10:44321
Connected for: 00:03:17
```

For `Connecting`, path is shown as detecting and address/duration are unavailable. For `Not connected`, all three runtime diagnostics are unavailable. For a connected session without a selected path, path is `Unknown` and address is unavailable. The existing wrapped and scrollable details paragraph is retained for long relay URLs and addresses.

## Risks / Trade-offs

- [Path event stream lag] → Re-read `Connection::paths()` after every path event, including `Lagged`, and expose `Unknown` when no selected path is available.
- [Stale event from a draining connection overwrites current diagnostics] → Carry the stable connection ID through the session-control path and accept updates only from the current primary connection.
- [Remote addresses can change and may be sensitive diagnostic data] → Show only the selected remote address in the local details panel; do not add addresses to message bodies, persisted contact data, or protocol frames.
- [A monotonic timestamp cannot survive restart] → Keep diagnostics explicitly runtime-only; a new process begins a new logical session and does not restore old duration data.
- [Relay URLs or custom addresses can exceed the panel width] → Reuse the existing wrapping and details scrolling instead of truncating the value or silently changing its meaning.

## Migration Plan

No data migration is required. Implement the runtime event and TUI model changes together, add unit/component/network coverage, and verify with `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, and `cargo test`. Rollback is a source-only revert because no persisted or wire-format data changes.
