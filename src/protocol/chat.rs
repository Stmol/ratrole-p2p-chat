use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Current version of the Rathole chat wire schema.
pub const PROTOCOL_VERSION: u16 = 1;

/// Maximum UTF-8 byte length accepted for a text message body.
pub const MAX_TEXT_BYTES: usize = 16 * 1024;

/// Opaque identifier used to correlate one outgoing message with its receipt.
///
/// The identifier is serialized as exactly sixteen bytes and carries no
/// ordering or timestamp semantics.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MessageId(#[serde(with = "serde_bytes")] [u8; 16]);

impl MessageId {
    /// Creates an identifier from its fixed-size byte representation.
    pub const fn new(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// Borrows the identifier bytes without allocating or exposing ownership.
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

/// Versioned envelope carried by one length-delimited chat document.
///
/// Deserialization rejects unknown fields. The envelope version and contained
/// frame are validated before encoding or after decoding, so callers cannot
/// accidentally place an invalid text body on the wire.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WireEnvelope {
    /// Schema version selected by the sender.
    pub protocol_version: u16,
    /// Request or response payload represented by this document.
    pub frame: ChatFrame,
}

impl WireEnvelope {
    /// Wraps a frame with the current protocol version.
    pub fn new(frame: ChatFrame) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            frame,
        }
    }

    /// Checks the envelope version and delegates payload validation to its frame.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.protocol_version != PROTOCOL_VERSION {
            return Err(ValidationError::UnsupportedProtocolVersion {
                actual: self.protocol_version,
            });
        }
        self.frame.validate()
    }
}

/// Request/response frames supported by the v1 chat protocol.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ChatFrame {
    /// Sends one text message to an authenticated contact.
    Text {
        /// Identifier that must be echoed by the remote receipt.
        message_id: MessageId,
        /// Sender wall-clock timestamp in Unix milliseconds.
        sent_at_unix_ms: i64,
        /// UTF-8 message body subject to [`MAX_TEXT_BYTES`].
        body: String,
    },
    /// Confirms that the remote side accepted the text frame into its local
    /// incoming queue.
    Accepted {
        /// Identifier of the accepted message.
        message_id: MessageId,
        /// Receiver wall-clock timestamp in Unix milliseconds.
        received_at_unix_ms: i64,
    },
    /// Rejects a request while preserving its message correlation identifier.
    Rejected {
        /// Identifier of the rejected message.
        message_id: MessageId,
        /// Machine-readable reason for rejecting the request.
        code: RejectionCode,
    },
}

impl ChatFrame {
    /// Builds and validates a text frame before returning it.
    ///
    /// `body` is converted into an owned `String`; empty and oversized bodies
    /// are rejected using the same rules applied during envelope validation.
    pub fn text(
        message_id: MessageId,
        sent_at_unix_ms: i64,
        body: impl Into<String>,
    ) -> Result<Self, ValidationError> {
        let frame = Self::Text {
            message_id,
            sent_at_unix_ms,
            body: body.into(),
        };
        frame.validate()?;
        Ok(frame)
    }

    /// Builds a receipt for a message accepted by the remote peer.
    pub const fn accepted(message_id: MessageId, received_at_unix_ms: i64) -> Self {
        Self::Accepted {
            message_id,
            received_at_unix_ms,
        }
    }

    /// Builds a rejection for a message that the remote peer did not accept.
    pub const fn rejected(message_id: MessageId, code: RejectionCode) -> Self {
        Self::Rejected { message_id, code }
    }

    /// Validates the fields whose constraints are independent of transport.
    ///
    /// Response timestamps and message IDs are intentionally not interpreted
    /// here; the session layer validates receipt correlation against the
    /// outstanding request.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if let Self::Text { body, .. } = self {
            if body.is_empty() {
                return Err(ValidationError::EmptyText);
            }
            if body.len() > MAX_TEXT_BYTES {
                return Err(ValidationError::TextTooLarge);
            }
        }
        Ok(())
    }
}

/// Machine-readable reasons a peer can reject a chat frame.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectionCode {
    /// The authenticated sender is not in the receiver's contact allowlist.
    UnknownContact,
    /// The request body exceeds the supported size limit.
    MessageTooLarge,
    /// The sender selected a protocol version this implementation cannot use.
    UnsupportedProtocolVersion,
    /// The request is structurally invalid for the current chat flow.
    InvalidMessage,
}

/// Validation failures that can be reported without involving a transport.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ValidationError {
    /// The text frame contains no message body bytes.
    #[error("message body must not be empty")]
    EmptyText,
    /// The text body exceeds [`MAX_TEXT_BYTES`].
    #[error("message body exceeds the {MAX_TEXT_BYTES}-byte limit")]
    TextTooLarge,
    /// The envelope version is not the current [`PROTOCOL_VERSION`].
    #[error("unsupported protocol version {actual}; expected {PROTOCOL_VERSION}")]
    UnsupportedProtocolVersion {
        /// Version received from the peer.
        actual: u16,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message_id(byte: u8) -> MessageId {
        MessageId::new([byte; 16])
    }

    #[test]
    fn text_frame_accepts_unicode_and_preserves_its_fields() {
        let frame =
            ChatFrame::text(message_id(7), 1_753_747_200_123, "Привет, 👋\nRathole").unwrap();

        assert_eq!(
            frame,
            ChatFrame::Text {
                message_id: message_id(7),
                sent_at_unix_ms: 1_753_747_200_123,
                body: "Привет, 👋\nRathole".to_owned(),
            }
        );
    }

    #[test]
    fn text_frame_rejects_an_empty_or_oversized_body() {
        assert_eq!(
            ChatFrame::text(message_id(1), 0, "").unwrap_err(),
            ValidationError::EmptyText,
        );
        assert_eq!(
            ChatFrame::text(message_id(1), 0, "x".repeat(MAX_TEXT_BYTES + 1)).unwrap_err(),
            ValidationError::TextTooLarge,
        );
    }

    #[test]
    fn envelope_requires_the_current_protocol_version() {
        let envelope = WireEnvelope {
            protocol_version: PROTOCOL_VERSION + 1,
            frame: ChatFrame::accepted(message_id(3), 42),
        };

        assert_eq!(
            envelope.validate().unwrap_err(),
            ValidationError::UnsupportedProtocolVersion {
                actual: PROTOCOL_VERSION + 1,
            },
        );
    }
}
