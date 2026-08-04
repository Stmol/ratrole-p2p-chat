# contact-connection-diagnostics Specification

## Purpose

Expose the active Iroh transport path, its concrete remote address, and the live duration of a contact's logical connection so users can understand how a connected contact is currently reached.

## Requirements

### Requirement: Connected contacts show selected path diagnostics

The contact details panel SHALL show the selected transport path and its remote address for a connected contact. The selected path SHALL describe the path currently used for application data, not merely a configured path mode.

#### Scenario: Connected contact uses a direct IP path

- **WHEN** a contact is `Connected` and the selected transport path is an IP path
- **THEN** the details panel shows the path as `Direct IP` and shows the selected remote IP socket address

#### Scenario: Connected contact uses a relay path

- **WHEN** a contact is `Connected` and the selected transport path is a relay path
- **THEN** the details panel shows the path as `Relay` and shows the selected relay URL or equivalent relay address

#### Scenario: Connected contact uses a custom or unknown path

- **WHEN** a contact is `Connected` and the selected path is custom or cannot be classified
- **THEN** the details panel shows `Custom` or `Unknown` respectively and does not substitute a different path type

### Requirement: Path migration is reflected while the session stays connected

The contact details panel SHALL update the selected path and remote address when the active transport path changes without requiring a new logical connection-state transition.

#### Scenario: Relay path migrates to direct IP

- **WHEN** Iroh selects a direct IP path after a relay path was selected and the contact remains `Connected`
- **THEN** the details panel changes to `Direct IP` and shows the new selected IP address while retaining the connected session duration

#### Scenario: Direct IP path migrates to relay

- **WHEN** Iroh selects a relay path after a direct IP path was selected and the contact remains `Connected`
- **THEN** the details panel changes to `Relay` and shows the new selected relay address while retaining the connected session duration

#### Scenario: No selected path is available

- **WHEN** the contact remains `Connected` but no selected path is available in the current diagnostic snapshot
- **THEN** the details panel shows `Unknown` with an unavailable address marker and does not infer a path from configuration or a previously selected path

### Requirement: Connection duration represents the logical connected session

The contact details panel SHALL show elapsed time from the transition into `Connected` for the current logical contact session.

#### Scenario: Contact becomes connected

- **WHEN** the contact transitions from `Connecting` to `Connected`
- **THEN** the duration starts at zero and increases while the contact remains logically connected

#### Scenario: Selected path changes during a connected session

- **WHEN** the selected path or its address changes while the contact remains `Connected`
- **THEN** the duration continues from the original `Connected` transition and is not reset

#### Scenario: Contact disconnects

- **WHEN** the contact transitions to `Not connected`
- **THEN** the duration is cleared and the panel no longer presents the previous session as active

### Requirement: Diagnostics are state-scoped and non-persistent

Path, address, and duration diagnostics SHALL be runtime information scoped to the current contact session and SHALL NOT alter contact storage, wire protocol data, or relay configuration.

#### Scenario: Contact is connecting

- **WHEN** the contact is `Connecting` and no connected session has been established
- **THEN** the panel shows a path-detection state and unavailable address and duration values

#### Scenario: Contact is not connected

- **WHEN** the contact is `Not connected`
- **THEN** the panel shows unavailable path, address, and duration values

#### Scenario: Application restarts

- **WHEN** the application restarts before a contact reconnects
- **THEN** no previous path, address, or duration diagnostic is restored from contact storage
