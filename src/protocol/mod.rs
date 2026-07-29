//! Rathole's standalone, versioned application wire protocol.
//!
//! This module validates and encodes one complete CBOR document. Reading a
//! document from a QUIC stream and authorising its Iroh peer are transport
//! responsibilities and intentionally do not live here.
//!
//! Invariants:
//! - All CBOR input is size-bounded and fails closed on malformed data,
//!   unsupported versions, unknown fields, and unknown frame variants.
//! - `Text` is validated before it enters the public model.
//! - This module receives no Iroh peer identity and makes no contact-policy
//!   decision; a transport adapter must authorise the authenticated peer first.
//! - This module does not delimit QUIC streams or persist messages.

mod chat;
mod codec;

pub use chat::{
    ChatFrame, MAX_TEXT_BYTES, MessageId, PROTOCOL_VERSION, RejectionCode, ValidationError,
    WireEnvelope,
};
pub use codec::{MAX_FRAME_BYTES, ProtocolError, decode, encode};
