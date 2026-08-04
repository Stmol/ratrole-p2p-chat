## 1. Establish evidence and path-transition observability

- [x] 1.1 Reproduce the first-message scenario with two real peers in `auto` mode and capture or record the observed path and delivery result without recording message bodies or secrets.
- [x] 1.2 Repeat the same scenario in `relay-only` mode and record remote receipt time separately from local `text_frame_written` time so the control run can distinguish relay delivery from local write completion.
- [x] 1.3 Extend connection/path logging, where needed, with path event kind, path ID, selected path kind/address, and timestamps, while preserving the existing runtime-only diagnostics and privacy boundary.

## 2. Make relay delivery independent from direct-path upgrades

- [x] 2.1 Audit and update the connection/session readiness path so a completed Iroh handshake with a working relay is sufficient for `Connected` and message admission; selected `DirectIp` diagnostics MUST NOT be an additional gate.
- [x] 2.2 Keep `IrohPathMode::Auto` as the normal behavior and ensure path-event handling only refreshes the current snapshot without starting a second application dial, closing the relay path, awaiting network work, or mutating an active delivery.
- [x] 2.3 Preserve the existing single logical `PeerSession` and delivery deadline while Iroh changes paths; ensure a path event cannot cancel an in-flight stream, reset its message identity, or produce a second user-visible send.
- [x] 2.4 Use the controlled `auto` versus `relay-only` evidence to fix the owning layer: correct any Rathole-level wait or stream interruption found in the session/transport code, or apply the smallest verified upstream Iroh dependency fix if the stall is confirmed inside Iroh 1.0.3.
- [x] 2.5 Verify that direct-path failure or later direct-path loss leaves the relay-backed contact `Connected` and continues message delivery whenever the relay connection remains available.

## 3. Add deterministic transport coverage

- [x] 3.1 Add tests proving that a relay-selected connected session can start the first message before direct-path discovery completes and that delivery follows the existing receipt contract.
- [x] 3.2 Add tests for path refreshes during an active delivery, asserting one final outcome, unchanged message identity and timestamp, and no transition to `NotConnected` while the working connection remains available.
- [x] 3.3 Add tests for direct-path failure/fallback behavior and for preserving `connected_since` across relay-to-direct and direct-to-relay path changes.
- [x] 3.4 Keep existing queue bounds, contact authorization, protocol validation, and timeout tests unchanged except for assertions required by the new non-blocking path behavior.

## 4. Document and validate the user-visible contract

- [x] 4.1 Update the Iroh chat transport documentation to state that relay is an immediately usable path, direct IP is an opportunistic optimization, and selected-path diagnostics do not determine message readiness.
- [x] 4.2 Run `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, and the full network-sensitive test suite with serialized test threads where required; fix regressions without increasing the production delivery deadline or changing the wire protocol.
- [x] 4.3 Perform a two-peer acceptance run in both `auto` and `relay-only` modes, verifying first-message remote receipt, messages during path discovery, direct-path selection when available, relay fallback when direct is unavailable, and one delivery outcome per message.
- [x] 4.4 Record the acceptance result and correlated timing evidence, including whether the remaining delay is in Rathole or the Iroh version, and leave the worktree ready for implementation review without creating a commit.
