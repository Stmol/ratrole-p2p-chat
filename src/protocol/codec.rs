//! Size-bounded CBOR serialization for complete chat envelopes.
//!
//! The codec does not read from a stream and does not decide whether an Iroh
//! peer is allowed to communicate. It only converts already-authenticated
//! protocol values to and from a single bounded CBOR document.

use thiserror::Error;

use super::{ValidationError, WireEnvelope};

/// Maximum encoded CBOR document size accepted by the protocol boundary.
pub const MAX_FRAME_BYTES: usize = 32 * 1024;

/// Validates and encodes one envelope into a bounded CBOR document.
///
/// Validation runs before serialization and the encoded result is checked
/// again because a valid text body can still produce an oversized document.
pub fn encode(envelope: &WireEnvelope) -> Result<Vec<u8>, ProtocolError> {
    envelope.validate().map_err(ProtocolError::Validation)?;
    let bytes =
        serde_cbor::to_vec(envelope).map_err(|error| ProtocolError::Encoding(error.to_string()))?;
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(ProtocolError::FrameTooLarge);
    }
    Ok(bytes)
}

/// Decodes one complete bounded CBOR document into a validated envelope.
///
/// Malformed input, unknown fields, unsupported variants, and invalid payloads
/// are reported as protocol errors rather than being partially accepted.
pub fn decode(bytes: &[u8]) -> Result<WireEnvelope, ProtocolError> {
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(ProtocolError::FrameTooLarge);
    }
    let envelope: WireEnvelope =
        serde_cbor::from_slice(bytes).map_err(|_| ProtocolError::MalformedCbor)?;
    envelope.validate().map_err(ProtocolError::Validation)?;
    Ok(envelope)
}

/// Errors raised while encoding or decoding a chat document.
#[derive(Debug, Error)]
pub enum ProtocolError {
    /// The serialized document exceeds [`MAX_FRAME_BYTES`].
    #[error("CBOR frame exceeds the {MAX_FRAME_BYTES}-byte limit")]
    FrameTooLarge,
    /// CBOR is malformed or contains fields/variants outside the strict schema.
    #[error("CBOR frame is malformed or contains unsupported fields")]
    MalformedCbor,
    /// The serializer failed after protocol validation succeeded.
    #[error("CBOR frame cannot be encoded: {0}")]
    Encoding(String),
    /// The decoded envelope violates a protocol-level field constraint.
    #[error(transparent)]
    Validation(#[from] ValidationError),
}

impl PartialEq for ProtocolError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::FrameTooLarge, Self::FrameTooLarge)
            | (Self::MalformedCbor, Self::MalformedCbor) => true,
            (Self::Validation(left), Self::Validation(right)) => left == right,
            (Self::Encoding(left), Self::Encoding(right)) => left == right,
            _ => false,
        }
    }
}

impl Eq for ProtocolError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{ChatFrame, MAX_TEXT_BYTES, MessageId, RejectionCode, WireEnvelope};

    fn message_id(byte: u8) -> MessageId {
        MessageId::new([byte; 16])
    }

    #[test]
    fn cbor_round_trip_preserves_each_supported_frame() {
        let frames = [
            ChatFrame::text(message_id(1), 1_753_747_200_123, "Привет, 👋").unwrap(),
            ChatFrame::accepted(message_id(2), 1_753_747_200_456),
            ChatFrame::rejected(message_id(3), RejectionCode::UnknownContact),
        ];

        for frame in frames {
            let envelope = WireEnvelope::new(frame);
            assert_eq!(decode(&encode(&envelope).unwrap()).unwrap(), envelope);
        }
    }

    #[test]
    fn decoder_rejects_a_document_larger_than_the_frame_limit() {
        assert_eq!(
            decode(&vec![0_u8; MAX_FRAME_BYTES + 1]).unwrap_err(),
            ProtocolError::FrameTooLarge,
        );
    }

    #[test]
    fn decoder_rejects_malformed_cbor_and_an_unsupported_version() {
        assert_eq!(decode(&[0xff]).unwrap_err(), ProtocolError::MalformedCbor);

        let incompatible = WireEnvelope {
            protocol_version: 2,
            frame: ChatFrame::accepted(message_id(4), 0),
        };
        let bytes = serde_cbor::to_vec(&incompatible).unwrap();
        assert_eq!(
            decode(&bytes).unwrap_err(),
            ProtocolError::Validation(ValidationError::UnsupportedProtocolVersion { actual: 2 }),
        );
    }

    #[test]
    fn decoder_rejects_unknown_fields_instead_of_ignoring_them() {
        #[derive(serde::Serialize)]
        struct EnvelopeWithExtraField {
            protocol_version: u16,
            frame: FrameWithExtraField,
        }

        #[derive(serde::Serialize)]
        #[serde(tag = "type", rename_all = "snake_case")]
        enum FrameWithExtraField {
            Text {
                message_id: MessageId,
                sent_at_unix_ms: i64,
                body: String,
                ignored_by_permissive_decoders: bool,
            },
        }

        let bytes = serde_cbor::to_vec(&EnvelopeWithExtraField {
            protocol_version: 1,
            frame: FrameWithExtraField::Text {
                message_id: message_id(5),
                sent_at_unix_ms: 0,
                body: "strict".to_owned(),
                ignored_by_permissive_decoders: true,
            },
        })
        .unwrap();

        assert_eq!(decode(&bytes).unwrap_err(), ProtocolError::MalformedCbor);
    }

    #[test]
    fn encoder_refuses_invalid_text_even_if_a_caller_constructs_the_enum_directly() {
        let envelope = WireEnvelope::new(ChatFrame::Text {
            message_id: message_id(6),
            sent_at_unix_ms: 0,
            body: "x".repeat(MAX_TEXT_BYTES + 1),
        });

        assert_eq!(
            encode(&envelope).unwrap_err(),
            ProtocolError::Validation(ValidationError::TextTooLarge),
        );
    }
}
