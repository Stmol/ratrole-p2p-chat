## Context

The current chat transport creates one long-lived Iroh `Connection` per contact. `PeerSession` marks the contact `Connected` after the Iroh connection handshake, then opens one bidirectional stream per message on that connection. The session already listens for Iroh path events, but those events are used only to refresh selected-path diagnostics; message delivery does not intentionally start a second connection or wait for a direct path.

Iroh's documented model is relay-first: a connection can carry traffic through a relay while Iroh tries to establish a direct path, and traffic remains on the relay when the direct path is not possible. Iroh 1.0.3 also keeps the selected path separate from the logical connection and selects a path only after it is functional. The implementation must preserve that model rather than treating a selected direct path as a prerequisite for chat.

The existing production delivery deadline covers queueing, stream I/O, and receipt handling. It must not be extended as a workaround for path migration. The existing path diagnostics are runtime-only and already distinguish the selected path from the configured `auto` or `relay-only` mode.

References:

- [Iroh README](https://github.com/n0-computer/iroh) — relay first, direct path when possible, relay fallback.
- [Iroh FAQ](https://docs.iroh.computer/about/faq) — data normally starts over the relay and moves to a direct connection later.
- [Iroh NAT traversal guide](https://docs.iroh.computer/concepts/nat-traversal) — direct-path discovery is automatic and relay fallback is expected.

## Goals / Non-Goals

**Goals:**

- Make the existing relay-backed `Connection` immediately usable for message streams.
- Keep direct-path discovery and selection owned by Iroh and independent from application message admission.
- Ensure path events and diagnostics cannot pause the session actor, cancel an active delivery, or replace a working relay connection.
- Preserve one logical contact session, existing delivery receipts, authenticated contacts, queue bounds, and protocol limits across path changes.
- Produce enough structured evidence to distinguish an application wait from a delay inside Iroh path migration, and verify the result on two real peers.

**Non-Goals:**

- Do not require or wait for a direct IP path before the connection can be used, disable Iroh's automatic direct-path attempts, make relay-only the default, or expose a user choice between the two paths.
- Do not implement a second application-level direct-connection algorithm or a second message connection per contact.
- Do not add message retry, offline storage, deduplication, ordering, a new wire-format field, or a longer global delivery deadline.
- Do not show a separate `Upgrading` or `Waiting for direct` state in the TUI.
- Do not persist path state or change relay configuration, contact storage, or device identity behavior.

## Decisions

### 1. Use the existing long-lived Iroh connection as the working connection

The handshake that creates the Iroh `Connection` is the readiness boundary for chat. Once that connection has a working relay path, `PeerSession` may admit and start normal message delivery. The selected path is a transport detail and must not be used as an additional readiness gate.

This keeps the current one-session-per-contact ownership model and lets Iroh migrate paths inside the authenticated QUIC connection. Creating a separate relay connection for messages and another connection for direct-path discovery is rejected: it would introduce competing sessions, complicate inbound stream ownership, and make delivery and path diagnostics harder to correlate without solving the underlying path-selection behavior.

### 2. Let Iroh perform the direct-path upgrade; do not add an application probing loop

`IrohPathMode::Auto` remains the normal mode. The application will not call a second `Endpoint::connect`, wait for `SelectedPathKind::DirectIp`, or close the relay path when it observes a direct candidate. Iroh remains responsible for address exchange, hole punching, path validation, path selection, and relay fallback.

The session path-event task continues to send a lightweight control notification. The session actor re-reads the selected-path snapshot for diagnostics, but that refresh performs no network wait and does not alter the active delivery. The relay path remains under Iroh's control until the direct path is usable; the application does not manually retire it.

Alternatives considered:

- **Wait for `DirectIp` before marking the contact connected:** rejected because it turns a usable relay into an artificial connection delay and contradicts Iroh's relay-first behavior.
- **Force `relay-only`:** rejected because it removes the performance improvement and is useful only as a diagnostic control.
- **Manually dial a direct address from Rathole:** rejected because it duplicates Iroh's NAT traversal and fallback logic and would make the authenticated session lifecycle ambiguous.

### 3. Keep path observation separate from message delivery

Path-change handling will remain an observation/update path, not part of the send path. `run_outbound_stream` will continue to open, write, finish, and read the receipt against the current long-lived `Connection` under the existing delivery deadline. No `PathChanged` handling may hold the session actor while awaiting network work, reset `ActiveDelivery`, or turn a failed direct probe into `DeliveryError::Transport`.

If controlled evidence shows that the application is not adding a wait and that Iroh 1.0.3 itself stalls stream progress during migration, the implementation will use the smallest upstream-supported Iroh fix or dependency upgrade available. Increasing the delivery deadline or adding an application retry is explicitly not the first-line solution because it would hide the path-transition failure and change delivery semantics.

### 4. Preserve the relay as a transparent fallback

The session stays `Connected` while at least one working path remains. A direct-path failure or later direct-path loss is handled by Iroh's path selection; Rathole only publishes the resulting selected-path diagnostic. A connection-close event remains the only transport-owned reason to move the logical session to `Not connected`.

The selected-path event keeps the existing stable connection ID and path snapshot. Diagnostics may change from `Relay` to `Direct IP` or back without resetting `connected_since`, replacing the logical session, or changing the message protocol.

### 5. Instrument the transition before judging the fix

The existing JSONL events already expose message, stream, and connection correlation points. Add path-transition evidence at the same boundary, including event kind, path ID, selected path kind/address, and timestamps, while excluding message bodies and secrets. The live acceptance check will correlate:

```text
connection attached/selected
path opened/selected/closed
message delivery started
stream opened
text frame written
text frame received on the remote peer
receipt received
```

Run the same scenario in `auto` and `relay-only` modes. Interpret the control run in both directions: if relay-only is fast while auto stalls around a path event, the remaining work is specifically path-transition handling or the Iroh version; if auto is fast while relay-only remains on the relay with packet loss and variable latency, the evidence points to relay-path quality instead. Neither result points to contact authorization, queue admission, or the wire protocol.

### 6. Test behavior at two levels

Unit and local transport tests will cover that path refreshes do not change connection state, logical session duration, message identity, or delivery outcome. A real two-peer acceptance run will cover the behavior that cannot be reproduced reliably with loopback routes: relay-connected first message, messages during direct-path discovery, direct-path success, and direct-path failure with continued relay delivery.

The acceptance run will measure remote receipt and sender completion separately. A local `text_frame_written` event is not treated as proof that the remote peer received the message.

## Risks / Trade-offs

- [Iroh 1.0.3 may contain a path-migration stall that Rathole cannot fix through session bookkeeping] → Keep the controlled `auto` versus `relay-only` experiment and allow a minimal supported Iroh upgrade or upstream fix; do not mask the issue with a longer message timeout.
- [A path-event burst could create unnecessary diagnostic updates] → Keep the existing snapshot-based update, accept lagged events by re-reading the current snapshot, and avoid awaiting network work in the event path.
- [A direct path may be usable on one peer before the other peer selects it] → Treat both peers' JSONL logs as required evidence and keep the relay as the working path until Iroh reports a usable selected path.
- [Path addresses are sensitive local diagnostics] → Keep them in the existing runtime diagnostics/logging boundary; never add them to message bodies, persisted contacts, or protocol frames.
- [A live relay test depends on network and relay availability] → Keep deterministic local tests for session invariants and record the two-peer acceptance environment and results separately.
- [Relay-only delivery can remain slow even when the logical connection is healthy] → Treat packet loss and relay-route quality as a separate operational investigation; use relay-side metrics or an alternate relay before changing the path-upgrade contract.

## Migration Plan

No data or wire migration is required. Implement the transport/session changes and tests together, run formatting, linting, and the full test suite, then perform the paired `auto` and `relay-only` acceptance run. Rollback is a source-only revert; the existing relay-backed connection behavior, storage, and protocol remain compatible.
