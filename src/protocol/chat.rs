use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const PROTOCOL_VERSION: u16 = 1;
pub const MAX_TEXT_BYTES: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MessageId(#[serde(with = "serde_bytes")] [u8; 16]);

impl MessageId {
    pub const fn new(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WireEnvelope {
    pub protocol_version: u16,
    pub frame: ChatFrame,
}

impl WireEnvelope {
    pub fn new(frame: ChatFrame) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            frame,
        }
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.protocol_version != PROTOCOL_VERSION {
            return Err(ValidationError::UnsupportedProtocolVersion {
                actual: self.protocol_version,
            });
        }
        self.frame.validate()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ChatFrame {
    Text {
        message_id: MessageId,
        sent_at_unix_ms: i64,
        body: String,
    },
    Accepted {
        message_id: MessageId,
        received_at_unix_ms: i64,
    },
    Rejected {
        message_id: MessageId,
        code: RejectionCode,
    },
}

impl ChatFrame {
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

    pub const fn accepted(message_id: MessageId, received_at_unix_ms: i64) -> Self {
        Self::Accepted {
            message_id,
            received_at_unix_ms,
        }
    }

    pub const fn rejected(message_id: MessageId, code: RejectionCode) -> Self {
        Self::Rejected { message_id, code }
    }

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

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectionCode {
    UnknownContact,
    MessageTooLarge,
    UnsupportedProtocolVersion,
    InvalidMessage,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ValidationError {
    #[error("message body must not be empty")]
    EmptyText,
    #[error("message body exceeds the {MAX_TEXT_BYTES}-byte limit")]
    TextTooLarge,
    #[error("unsupported protocol version {actual}; expected {PROTOCOL_VERSION}")]
    UnsupportedProtocolVersion { actual: u16 },
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
