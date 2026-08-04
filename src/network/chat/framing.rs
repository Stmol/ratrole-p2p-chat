//! Length-delimited protocol documents carried over one QUIC bidi stream.
//!
//! Each document starts with a four-byte big-endian length, followed by one
//! bounded CBOR envelope. The helpers intentionally support both persistent
//! stream readers and the single-document request/response streams used by the
//! chat session actor.

use std::io;

use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::protocol::{MAX_FRAME_BYTES, ProtocolError, WireEnvelope, decode, encode};

/// Errors raised while framing or decoding one stream document.
#[derive(Debug, Error)]
pub(super) enum FrameError {
    /// The underlying async stream read or write failed.
    #[error("stream I/O failed: {0}")]
    Io(#[from] io::Error),
    /// The length prefix exceeded the protocol allocation limit.
    #[error("declared CBOR document is {declared} bytes; maximum is {MAX_FRAME_BYTES}")]
    DeclaredFrameTooLarge { declared: usize },
    /// A single-document request/response stream contained extra bytes.
    #[error("stream contains bytes after the complete CBOR document")]
    TrailingData,
    /// The framed bytes were not a valid protocol envelope.
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
}

/// Writes one validated envelope with its big-endian byte-length prefix.
pub(super) async fn write_document<W>(
    writer: &mut W,
    envelope: &WireEnvelope,
) -> Result<(), FrameError>
where
    W: AsyncWrite + Unpin,
{
    let bytes = encode(envelope)?;
    let length = u32::try_from(bytes.len()).expect("MAX_FRAME_BYTES fits u32");
    writer.write_all(&length.to_be_bytes()).await?;
    writer.write_all(&bytes).await?;
    writer.flush().await?;
    Ok(())
}

/// Reads one length-prefixed envelope without waiting for stream EOF.
///
/// The declared size is checked before allocating the body buffer. This helper
/// is suitable for streams that contain multiple back-to-back documents.
pub(super) async fn read_document<R>(reader: &mut R) -> Result<WireEnvelope, FrameError>
where
    R: AsyncRead + Unpin,
{
    let mut prefix = [0_u8; 4];
    reader.read_exact(&mut prefix).await?;
    let declared = u32::from_be_bytes(prefix) as usize;
    if declared > MAX_FRAME_BYTES {
        return Err(FrameError::DeclaredFrameTooLarge { declared });
    }
    let mut bytes = vec![0_u8; declared];
    reader.read_exact(&mut bytes).await?;
    Ok(decode(&bytes)?)
}

/// Reads exactly one envelope and rejects any trailing stream byte.
pub(super) async fn read_single_document<R>(reader: &mut R) -> Result<WireEnvelope, FrameError>
where
    R: AsyncRead + Unpin,
{
    let envelope = read_document(reader).await?;
    let mut trailing = [0_u8; 1];
    if reader.read(&mut trailing).await? != 0 {
        return Err(FrameError::TrailingData);
    }
    Ok(envelope)
}

#[cfg(test)]
mod tests {
    use tokio::io::AsyncWriteExt;

    use super::*;
    use crate::protocol::{ChatFrame, MessageId, RejectionCode};

    #[tokio::test]
    async fn reads_two_documents_without_waiting_for_stream_eof() {
        let (mut writer, mut reader) = tokio::io::duplex(70_000);
        let first = WireEnvelope::new(ChatFrame::accepted(MessageId::new([1; 16]), 10));
        let second = WireEnvelope::new(ChatFrame::rejected(
            MessageId::new([2; 16]),
            RejectionCode::UnknownContact,
        ));

        let first_for_write = first.clone();
        let second_for_write = second.clone();
        let task = tokio::spawn(async move {
            write_document(&mut writer, &first_for_write).await.unwrap();
            write_document(&mut writer, &second_for_write)
                .await
                .unwrap();
        });

        assert_eq!(read_document(&mut reader).await.unwrap(), first);
        assert_eq!(read_document(&mut reader).await.unwrap(), second);
        task.await.unwrap();
    }

    #[tokio::test]
    async fn rejects_oversized_declared_length_before_body_allocation() {
        let (mut writer, mut reader) = tokio::io::duplex(8);
        writer
            .write_all(&((MAX_FRAME_BYTES as u32) + 1).to_be_bytes())
            .await
            .unwrap();

        assert!(matches!(
            read_document(&mut reader).await,
            Err(FrameError::DeclaredFrameTooLarge { declared }) if declared == MAX_FRAME_BYTES + 1
        ));
    }

    #[tokio::test]
    async fn rejects_trailing_stream_data_after_one_document() {
        let (mut writer, mut reader) = tokio::io::duplex(70_000);
        let envelope = WireEnvelope::new(ChatFrame::accepted(MessageId::new([4; 16]), 10));
        write_document(&mut writer, &envelope).await.unwrap();
        writer.write_all(b"extra").await.unwrap();

        assert!(matches!(
            read_single_document(&mut reader).await,
            Err(FrameError::TrailingData)
        ));
    }
}
