## Why

When Iroh establishes a connection through a relay, Rathole can report the contact as connected while Iroh is still trying to find a direct path. The first outgoing message may then remain pending until that path change finishes, sometimes for tens of seconds. This makes a working relay connection feel unavailable to the user.

The relay must be treated as a complete working path. A direct path is an optimization that Iroh may try in the background; it must never delay normal message exchange. If the direct path cannot be established, the relay should continue carrying traffic without user-visible failure.

## What Changes

- Make a relay-backed connection ready for message delivery as soon as the logical connection is established.
- Keep Iroh's direct-path discovery and upgrade attempt in the background while the relay remains usable.
- Ensure path discovery, validation, and migration do not block new or in-flight message streams or their delivery receipts.
- Keep using the relay when a direct path is unavailable, fails, or is later lost; do not turn that outcome into a message-delivery error.
- Preserve one logical contact session and avoid duplicate sends, implicit reconnects, or user-visible "waiting for direct connection" states during path changes.
- Add focused tests, logs, and a two-peer acceptance check that verify first-message delivery while the selected path changes.
- Keep the existing selected-path diagnostics, wire protocol, contact storage, relay configuration, and online-only delivery semantics unchanged.

## Capabilities

### New Capabilities

- `nonblocking-direct-path-upgrade`: Keep relay traffic immediately usable while attempting a direct path in the background, with relay fallback when the attempt fails.

### Modified Capabilities

- None.

## Impact

- Affects the Iroh chat session and delivery scheduling in `src/network/chat/session.rs` and related transport code.
- Extends transport-level tests and cross-machine diagnostics so path changes can be correlated with message delivery and receipts.
- Uses the existing Iroh path-selection behavior; no new relay service, wire-format field, persisted data, or user configuration is required.
- The existing `contact-connection-diagnostics` capability remains the source of truth for displaying the path currently selected by Iroh.
