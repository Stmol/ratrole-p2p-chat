use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use iroh::EndpointId;
use tokio::{
    sync::{Mutex, mpsc},
    task::JoinHandle,
    time::{self, Instant},
};

use super::framing::{FrameError, read_document, write_document};
use super::transport::TransportInner;
use super::{
    CompletionSlot, DeliveryError, DeliveryReceipt, OUTGOING_QUEUE_CAPACITY, resolve_once,
};
use crate::protocol::{ChatFrame, MessageId, WireEnvelope};

pub(super) struct QueuedDelivery {
    pub(super) envelope: WireEnvelope,
    pub(super) message_id: MessageId,
    pub(super) completion: CompletionSlot,
    pub(super) cancelled: Arc<AtomicBool>,
    pub(super) deadline: Instant,
}

#[derive(Clone)]
pub(super) struct PeerWorker {
    tx: mpsc::Sender<QueuedDelivery>,
    shutdown_reason: Arc<Mutex<Option<DeliveryError>>>,
    join: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl PeerWorker {
    pub(super) fn spawn(peer: EndpointId, inner: Arc<TransportInner>) -> Self {
        let (tx, mut rx) = mpsc::channel(OUTGOING_QUEUE_CAPACITY);
        let shutdown_reason = Arc::new(Mutex::new(None));
        let shutdown_for_task = shutdown_reason.clone();
        let handle = tokio::spawn(async move {
            while let Some(command) = rx.recv().await {
                process_delivery(peer, inner.clone(), command).await;
            }
            let reason = shutdown_for_task
                .lock()
                .await
                .take()
                .unwrap_or(DeliveryError::ShutDown);
            while let Ok(command) = rx.try_recv() {
                cancel_delivery(command, reason.clone()).await;
            }
        });
        Self {
            tx,
            shutdown_reason,
            join: Arc::new(Mutex::new(Some(handle))),
        }
    }

    pub(super) fn try_enqueue(&self, command: QueuedDelivery) -> Result<(), DeliveryError> {
        self.tx.try_send(command).map_err(|error| match error {
            mpsc::error::TrySendError::Full(_) => DeliveryError::QueueFull,
            mpsc::error::TrySendError::Closed(_) => DeliveryError::ShutDown,
        })
    }

    pub(super) async fn shutdown_drain(self, reason: DeliveryError) {
        *self.shutdown_reason.lock().await = Some(reason);
        drop(self.tx);
        if let Some(handle) = self.join.lock().await.take() {
            let _ = handle.await;
        }
    }
}

async fn cancel_delivery(command: QueuedDelivery, error: DeliveryError) {
    if !command.cancelled.swap(true, Ordering::AcqRel) {
        resolve_once(&command.completion, Err(error)).await;
    }
}

fn validate_receipt(
    expected: MessageId,
    frame: ChatFrame,
) -> Result<DeliveryReceipt, DeliveryError> {
    match frame {
        ChatFrame::Accepted {
            message_id,
            received_at_unix_ms,
        } if message_id == expected => Ok(DeliveryReceipt {
            message_id,
            received_at_unix_ms,
        }),
        ChatFrame::Rejected { message_id, code } if message_id == expected => {
            Err(DeliveryError::Rejected(code))
        }
        _ => Err(DeliveryError::ProtocolViolation),
    }
}

async fn process_delivery(peer: EndpointId, inner: Arc<TransportInner>, command: QueuedDelivery) {
    if command.cancelled.load(Ordering::Acquire) {
        return;
    }

    if !inner.contacts.contains(&peer).await {
        cancel_delivery(command, DeliveryError::NotAContact).await;
        return;
    }

    if Instant::now() >= command.deadline {
        cancel_delivery(command, DeliveryError::TimedOut).await;
        return;
    }

    let connection = match inner.connection_for(peer, command.deadline).await {
        Ok(connection) => connection,
        Err(error) => {
            if !command.cancelled.swap(true, Ordering::AcqRel) {
                resolve_once(&command.completion, Err(error)).await;
            }
            return;
        }
    };

    if command.cancelled.load(Ordering::Acquire) {
        inner.evict_connection(peer, &connection).await;
        return;
    }

    let open = time::timeout_at(command.deadline, connection.open_bi()).await;
    let (mut send, mut recv) = match open {
        Ok(Ok(pair)) => pair,
        Ok(Err(error)) => {
            inner.evict_connection(peer, &connection).await;
            if !command.cancelled.swap(true, Ordering::AcqRel) {
                resolve_once(
                    &command.completion,
                    Err(DeliveryError::Transport(error.to_string())),
                )
                .await;
            }
            return;
        }
        Err(_) => {
            inner.evict_connection(peer, &connection).await;
            cancel_delivery(command, DeliveryError::TimedOut).await;
            return;
        }
    };

    if command.cancelled.load(Ordering::Acquire) {
        inner.evict_connection(peer, &connection).await;
        return;
    }

    if !inner.contacts.contains(&peer).await {
        inner.evict_connection(peer, &connection).await;
        cancel_delivery(command, DeliveryError::NotAContact).await;
        return;
    }

    match time::timeout_at(
        command.deadline,
        write_document(&mut send, &command.envelope),
    )
    .await
    {
        Ok(Ok(())) => {}
        Ok(Err(frame_error)) => {
            inner.evict_connection(peer, &connection).await;
            if !command.cancelled.swap(true, Ordering::AcqRel) {
                resolve_once(&command.completion, Err(map_frame_error(frame_error))).await;
            }
            return;
        }
        Err(_) => {
            inner.evict_connection(peer, &connection).await;
            cancel_delivery(command, DeliveryError::TimedOut).await;
            return;
        }
    }

    if let Err(error) = send.finish() {
        inner.evict_connection(peer, &connection).await;
        if !command.cancelled.swap(true, Ordering::AcqRel) {
            resolve_once(
                &command.completion,
                Err(DeliveryError::Transport(error.to_string())),
            )
            .await;
        }
        return;
    }

    if command.cancelled.load(Ordering::Acquire) {
        inner.evict_connection(peer, &connection).await;
        return;
    }

    let receipt = match time::timeout_at(command.deadline, read_document(&mut recv)).await {
        Ok(Ok(envelope)) => validate_receipt(command.message_id, envelope.frame),
        Ok(Err(error)) => {
            inner.evict_connection(peer, &connection).await;
            if !command.cancelled.swap(true, Ordering::AcqRel) {
                resolve_once(&command.completion, Err(map_frame_error(error))).await;
            }
            return;
        }
        Err(_) => {
            inner.evict_connection(peer, &connection).await;
            cancel_delivery(command, DeliveryError::TimedOut).await;
            return;
        }
    };

    match &receipt {
        Ok(_) | Err(DeliveryError::Rejected(_)) => {}
        Err(DeliveryError::ProtocolViolation) => {
            inner.evict_connection(peer, &connection).await;
        }
        Err(_) => {
            inner.evict_connection(peer, &connection).await;
        }
    }

    if !command.cancelled.swap(true, Ordering::AcqRel) {
        resolve_once(&command.completion, receipt).await;
    } else {
        // Timed out while in-flight: indeterminate — evict so the next message does not reuse it.
        inner.evict_connection(peer, &connection).await;
    }
}

fn map_frame_error(error: FrameError) -> DeliveryError {
    match error {
        FrameError::Io(error) => DeliveryError::Transport(error.to_string()),
        FrameError::DeclaredFrameTooLarge { declared } => {
            DeliveryError::Transport(format!("declared frame too large: {declared}"))
        }
        FrameError::Protocol(error) => DeliveryError::Transport(error.to_string()),
    }
}

pub(super) fn spawn_deadline(
    deadline: Instant,
    completion: CompletionSlot,
    cancelled: Arc<AtomicBool>,
) {
    tokio::spawn(async move {
        time::sleep_until(deadline).await;
        if !cancelled.swap(true, Ordering::AcqRel) {
            resolve_once(&completion, Err(DeliveryError::TimedOut)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::RejectionCode;

    #[test]
    fn receipt_must_match_the_current_message_id() {
        let expected = MessageId::new([1; 16]);
        let wrong = MessageId::new([2; 16]);

        assert!(matches!(
            validate_receipt(expected, ChatFrame::accepted(wrong, 10)),
            Err(DeliveryError::ProtocolViolation)
        ));
    }

    #[test]
    fn accepted_and_rejected_receipts_have_distinct_results() {
        let id = MessageId::new([3; 16]);

        assert_eq!(
            validate_receipt(id, ChatFrame::accepted(id, 11)).unwrap(),
            DeliveryReceipt {
                message_id: id,
                received_at_unix_ms: 11
            },
        );
        assert!(matches!(
            validate_receipt(id, ChatFrame::rejected(id, RejectionCode::UnknownContact)),
            Err(DeliveryError::Rejected(RejectionCode::UnknownContact))
        ));
    }

    #[tokio::test]
    async fn wrong_response_frame_is_a_protocol_violation() {
        let expected = MessageId::new([6; 16]);
        assert!(matches!(
            validate_receipt(
                expected,
                ChatFrame::text(MessageId::new([7; 16]), 0, "not a receipt").unwrap(),
            ),
            Err(DeliveryError::ProtocolViolation)
        ));
    }
}
