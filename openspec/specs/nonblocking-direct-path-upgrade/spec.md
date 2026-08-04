# nonblocking-direct-path-upgrade Specification

## Purpose

Keep a relay-backed chat immediately usable while the transport quietly attempts a faster direct path and safely falls back to the relay whenever that attempt does not succeed.

## Requirements

### Requirement: A relay-backed session is ready for messages

Once a contact session is `Connected` and its selected working path is a relay, the session SHALL be eligible for normal message delivery. Message delivery SHALL NOT wait for a direct path to be discovered, validated, or selected.

#### Scenario: First message while direct-path discovery is unfinished

- **WHEN** a contact is `Connected` through a relay and a direct path attempt is still in progress
- **THEN** sending a message starts the normal message exchange over the currently working path without waiting for the direct attempt
- **AND** the message is not kept pending solely because the direct path is not ready

#### Scenario: New message during a direct-path attempt

- **WHEN** a contact remains `Connected` through a relay while the transport probes a direct path
- **THEN** a newly admitted message can use the relay and follows the existing delivery and receipt rules

### Requirement: Direct-path improvement is non-blocking

The transport SHALL attempt to establish a direct path independently of ordinary message exchange. A direct-path attempt SHALL NOT pause, cancel, replace, or delay a working relay path before the direct path is usable.

#### Scenario: Relay continues during direct-path probing

- **WHEN** the transport is probing a possible direct path for a relay-backed session
- **THEN** the relay remains available for new and in-flight message streams
- **AND** the contact remains `Connected` unless the working connection itself is lost

#### Scenario: Direct path becomes usable

- **WHEN** a direct path is successfully established and selected by the transport
- **THEN** subsequent traffic may use the direct path
- **AND** the logical contact session remains the same connected session
- **AND** path diagnostics report the newly selected path without presenting a reconnect or delivery failure

### Requirement: Relay remains the safe fallback

If a direct path attempt fails, times out, is rejected by the network, or becomes unusable later, the transport SHALL keep the relay path as the working fallback whenever the relay connection is still available. A failed direct-path attempt SHALL NOT by itself change the contact to `Not connected` or fail a message.

#### Scenario: Direct-path attempt cannot be established

- **WHEN** the relay-backed session is connected but the direct-path attempt fails or times out
- **THEN** the contact remains `Connected` through the relay
- **AND** messages continue to use the relay under the existing delivery deadline and error semantics
- **AND** the user is not shown a direct-path error as a message-delivery failure

#### Scenario: Direct path is later lost

- **WHEN** the selected direct path stops working while the relay remains available
- **THEN** traffic returns to the relay without requiring a new logical contact connection
- **AND** the contact remains `Connected`

### Requirement: Path changes do not duplicate or silently discard messages

Path discovery, validation, and selection changes SHALL preserve the existing per-message delivery contract. A path change SHALL NOT create an additional user-visible send, silently discard an admitted message, or make a message wait for direct-path readiness.

#### Scenario: Message is in flight during path migration

- **WHEN** a message stream is in flight and the selected path changes between relay and direct or back to relay
- **THEN** the existing stream continues according to the normal delivery and receipt rules
- **AND** the sender reports one final delivery outcome for that message

#### Scenario: Path migration does not alter message identity

- **WHEN** a message is sent while the session changes paths
- **THEN** the message keeps its original message identity and timestamp
- **AND** no retry or reconnect is introduced solely to wait for a direct path

### Requirement: Path-upgrade state remains transparent to users

The user-facing contact state SHALL describe logical connectivity, not an intermediate direct-path attempt. The selected-path diagnostic MAY change as the transport changes paths, but the application SHALL NOT expose a separate waiting state for direct-path discovery.

#### Scenario: Direct-path probing is visible only through existing diagnostics

- **WHEN** a connected session moves from relay to direct or remains on relay after a failed attempt
- **THEN** the contact remains shown as `Connected`
- **AND** the existing selected-path diagnostics may show the current relay or direct path
- **AND** the wire protocol, contact storage, and relay configuration remain unchanged
