## 1. Runtime connection diagnostic model

- [x] 1.1 Add app-neutral selected-path types for `DirectIp`, `Relay`, `Custom`, and `Unknown`, with an optional owned remote-address string and unit coverage for state-independent formatting data.
- [x] 1.2 Extend the runtime connection update and TUI command payloads to carry connection state, selected-path diagnostics, and an optional monotonic logical-session start timestamp without introducing Iroh types into domain or TUI data.
- [x] 1.3 Extend the contact view and details props so runtime path, address, and connected-since data can be applied by `TuiApp` and rendered through typed component props.



## 2. Iroh path observation and session lifecycle

- [x] 2.1 Implement selected-path snapshot conversion from Iroh `Connection::paths()` into the app-neutral diagnostic, preserving relay URLs and IP socket addresses and mapping unavailable or future path kinds to `Unknown`.
- [x] 2.2 Add per-connection path-event monitoring for `Opened`, `Closed`, `Selected`, and `Lagged`, and make each event trigger a fresh selected-path snapshot rather than relying on the event payload alone.
- [x] 2.3 Route path refreshes through the session actor with the stable connection ID and ignore updates that no longer belong to the current primary connection or arrive after it has been removed.
- [x] 2.4 Record the logical session `Instant` on the first transition to `Connected`, reuse it for path-only updates, clear it on `NotConnected`, and emit enriched updates for initial connection, path changes, and disconnects.
- [x] 2.5 Forward enriched runtime updates through the application session while preserving unknown-contact filtering and the existing state-only connection query behavior.



## 3. Contact details presentation

- [x] 3.1 Apply enriched connection updates in `TuiApp`, retaining the duration timestamp across path migration and clearing path, address, and duration when the contact is no longer connected.
- [x] 3.2 Derive a live elapsed duration at the TUI composition boundary and format it deterministically as `Connected for: HH:MM:SS` for the details component.
- [x] 3.3 Render state-dependent `Path`, `Address`, and `Connected for` rows with `Direct IP`, `Relay`, `Custom`, `Unknown`, detecting, and unavailable values while retaining existing wrapping and scrolling behavior.



## 4. Automated verification

- [x] 4.1 Add unit tests for path-kind/address conversion, unknown-path handling, duration formatting, and state-dependent diagnostic clearing.
- [x] 4.2 Add network/session tests that observe initial selected-path metadata, selected-path refreshes, `Lagged` snapshot recovery, logical duration preservation across path updates, and stale updates from draining connections.
- [x] 4.3 Add application/TUI tests that apply synthetic enriched connection updates and verify the details panel shows the selected path and exact remote address without claiming presence or persistence.



## 5. Validation and live acceptance

- [x] 5.1 Run `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, and `cargo test`, fixing regressions without changing protocol, storage, or relay configuration behavior.
- [ ] 5.2 Perform a two-laptop acceptance run in `auto` and `relay-only` modes where available, verifying that the panel reports the selected path and its remote address, updates after path migration, and keeps the logical connection duration continuous.