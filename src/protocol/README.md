# Rathole Chat Wire Protocol

This directory contains Rathole's standalone, versioned application wire contract. It serialises and validates one complete CBOR document. It intentionally has no dependency on Iroh, Tokio, filesystem access, contact storage, application state, or TUI rendering.

## Contract

Every document is a `WireEnvelope`:

```rust
WireEnvelope {
    protocol_version: 1,
    frame: ChatFrame,
}
```

`WireEnvelope::new` always creates the current version. `WireEnvelope::validate` rejects any other version, so a decoder must not silently interpret a newer protocol as v1.

The schema lives in [`chat.rs`](chat.rs). It uses Serde's internally tagged, snake-case representation and `deny_unknown_fields`. Unknown frame variants and extension fields therefore fail closed rather than being ignored.

## v1 frames

| Frame | Fields | Meaning |
| --- | --- | --- |
| `Text` | `message_id`, `sent_at_unix_ms`, `body` | A text payload. The body is non-empty UTF-8 and at most 16 KiB by byte length. |
| `Accepted` | `message_id`, `received_at_unix_ms` | A receipt for the original message ID. |
| `Rejected` | `message_id`, `code` | A closed, machine-readable rejection for the original message ID. |

`Rejected.code` is one of `UnknownContact`, `MessageTooLarge`, `UnsupportedProtocolVersion`, or `InvalidMessage`. It deliberately has no remote free-text diagnostic field.

`MessageId` is exactly 16 binary bytes and is serialised with `serde_bytes`, producing a CBOR byte string rather than an array of integer values. `sent_at_unix_ms` and `received_at_unix_ms` are `i64` UTC Unix milliseconds for display only. They are not an ordering, authentication, expiry, or security authority.

`ChatFrame::text` and `ChatFrame::validate` reject empty and oversized text. Valid text is preserved verbatim: the protocol does not trim, normalise, or otherwise reinterpret Unicode, whitespace, emoji, or newlines.

## Encoding and decoding

[`codec.rs`](codec.rs) provides the public entry points:

```rust
pub fn encode(envelope: &WireEnvelope) -> Result<Vec<u8>, ProtocolError>;
pub fn decode(bytes: &[u8]) -> Result<WireEnvelope, ProtocolError>;
```

Both directions validate the schema. A complete encoded CBOR document is capped at 32 KiB. `decode` rejects a larger input before deserialising it; `encode` rejects an encoded document that exceeds the same limit.

`ProtocolError` separates oversized documents (`FrameTooLarge`), malformed CBOR or unsupported wire fields (`MalformedCbor`), serialisation failures (`Encoding`), and semantic validation failures (`Validation`). The codec never repairs invalid input or silently drops unsupported data.

## Transport boundary

This module handles one document that has already been delimited by its caller. It does not:

- open or configure an Iroh endpoint;
- read a QUIC stream or define stream framing;
- identify or authorise a remote peer;
- persist messages, maintain queues, or claim delivery;
- infer peer presence from relay or network state.

A future Iroh adapter must obtain the authenticated `connection.remote_id()`, authorise that identity against local contacts, apply its own bounded stream framing to obtain one complete document, and then call `decode`. It must call `encode` for its response. Those transport and policy responsibilities must remain outside this directory.
