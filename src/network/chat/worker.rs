use std::{future::Future, sync::Arc};

use iroh::{EndpointId, endpoint::Connection};
use tokio::{
    sync::{Mutex, Notify, mpsc},
    task::JoinHandle,
    time::{self, Instant},
};

use super::framing::{FrameError, read_single_document, write_document};
use super::transport::TransportInner;
use super::{
    CompletionSlot, DeliveryCancellation, DeliveryError, DeliveryReceipt, OUTGOING_QUEUE_CAPACITY,
    resolve_once,
};
use crate::logging::{self, LogFields};
use crate::protocol::{ChatFrame, MessageId, WireEnvelope};

pub(super) struct QueuedDelivery {
    pub(super) envelope: WireEnvelope,
    pub(super) message_id: MessageId,
    pub(super) completion: CompletionSlot,
    pub(super) cancellation: Arc<DeliveryCancellation>,
    pub(super) deadline: Instant,
    pub(super) queued_at: Instant,
}

#[derive(Clone)]
struct WorkerShutdown {
    reason: Arc<Mutex<Option<DeliveryError>>>,
    notify: Arc<Notify>,
}

impl WorkerShutdown {
    fn new() -> Self {
        Self {
            reason: Arc::new(Mutex::new(None)),
            notify: Arc::new(Notify::new()),
        }
    }

    async fn trigger(&self, reason: DeliveryError) {
        *self.reason.lock().await = Some(reason);
        self.notify.notify_waiters();
    }

    async fn wait(&self) -> DeliveryError {
        loop {
            if let Some(reason) = self.reason.lock().await.clone() {
                return reason;
            }
            self.notify.notified().await;
        }
    }

    async fn reason(&self) -> Option<DeliveryError> {
        self.reason.lock().await.clone()
    }
}

#[derive(Clone)]
pub(super) struct PeerWorker {
    tx: mpsc::Sender<QueuedDelivery>,
    shutdown: WorkerShutdown,
    join: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl PeerWorker {
    pub(super) fn spawn(peer: EndpointId, inner: Arc<TransportInner>) -> Self {
        let (tx, mut rx) = mpsc::channel::<QueuedDelivery>(OUTGOING_QUEUE_CAPACITY);
        let shutdown = WorkerShutdown::new();
        let shutdown_for_task = shutdown.clone();
        let handle = tokio::spawn(async move {
            logging::log_event(
                "worker",
                "peer_worker_loop_started",
                LogFields::default().peer_str(peer.to_string()),
            );
            loop {
                tokio::select! {
                    biased;
                    _ = shutdown_for_task.wait() => break,
                    command = rx.recv() => {
                        let Some(command) = command else { break };
                        logging::log_event(
                            "worker",
                            "worker_item_received",
                            LogFields::default()
                                .peer_str(peer.to_string())
                                .message(&command.message_id),
                        );
                        process_delivery(
                            peer,
                            inner.clone(),
                            command,
                            shutdown_for_task.clone(),
                        )
                        .await;
                    }
                }
            }

            let reason = shutdown_for_task
                .reason()
                .await
                .unwrap_or(DeliveryError::ShutDown);
            while let Ok(command) = rx.try_recv() {
                cancel_delivery(command, reason.clone()).await;
            }
            logging::log_event(
                "worker",
                "peer_worker_loop_stopped",
                LogFields::default()
                    .peer_str(peer.to_string())
                    .reason(reason.to_string()),
            );
        });
        Self {
            tx,
            shutdown,
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
        self.shutdown.trigger(reason).await;
        drop(self.tx);
        if let Some(handle) = self.join.lock().await.take() {
            let _ = handle.await;
        }
    }
}

enum StageAbort {
    TimedOut,
    Cancelled,
    Shutdown(DeliveryError),
}

async fn run_stage<T, E, F>(
    deadline: Instant,
    operation: F,
    cancellation: &DeliveryCancellation,
    shutdown: &WorkerShutdown,
) -> Result<Result<T, E>, StageAbort>
where
    F: Future<Output = Result<T, E>>,
{
    let operation = time::timeout_at(deadline, operation);
    tokio::pin!(operation);
    tokio::select! {
        result = &mut operation => result.map_err(|_| StageAbort::TimedOut),
        _ = cancellation.wait() => Err(StageAbort::Cancelled),
        reason = shutdown.wait() => Err(StageAbort::Shutdown(reason)),
    }
}

async fn cancel_delivery(command: QueuedDelivery, error: DeliveryError) {
    let _ = complete_delivery(command, Err(error)).await;
}

async fn complete_delivery(
    command: QueuedDelivery,
    result: Result<DeliveryReceipt, DeliveryError>,
) -> bool {
    if command.cancellation.cancel() {
        resolve_once(&command.completion, result).await;
        true
    } else {
        false
    }
}

async fn abort_stage(
    peer: EndpointId,
    inner: &TransportInner,
    command: QueuedDelivery,
    connection: Option<&Connection>,
    phase: &'static str,
    abort: StageAbort,
) {
    if let Some(connection) = connection {
        inner.log_connection_snapshot(
            "worker",
            "delivery_aborted",
            peer,
            Some(&command.message_id),
            connection,
            None,
        );
        inner.log_connection_snapshot(
            "worker",
            "connection_closed",
            peer,
            Some(&command.message_id),
            connection,
            None,
        );
        connection.close(0u32.into(), b"delivery aborted");
    }
    match abort {
        StageAbort::TimedOut => {
            logging::log_warn(
                "worker",
                "message_delivery_timed_out",
                LogFields::default()
                    .peer_str(peer.to_string())
                    .message(&command.message_id)
                    .reason(phase),
            );
            let _ = complete_delivery(command, Err(DeliveryError::TimedOut)).await;
        }
        StageAbort::Cancelled => {
            logging::log_event(
                "worker",
                "message_delivery_cancelled",
                LogFields::default()
                    .peer_str(peer.to_string())
                    .message(&command.message_id)
                    .reason(phase),
            );
        }
        StageAbort::Shutdown(reason) => {
            logging::log_event(
                "worker",
                "message_delivery_cancelled",
                LogFields::default()
                    .peer_str(peer.to_string())
                    .message(&command.message_id)
                    .reason(reason.to_string()),
            );
            let _ = complete_delivery(command, Err(reason)).await;
        }
    }
}

async fn fail_delivery(
    peer: EndpointId,
    inner: &TransportInner,
    command: QueuedDelivery,
    connection: Option<&Connection>,
    error: DeliveryError,
) {
    let mut fields = LogFields::default()
        .peer_str(peer.to_string())
        .message(&command.message_id)
        .error(&error);
    if let Some(connection) = connection {
        fields = fields.connection(connection.stable_id());
        inner.log_connection_snapshot(
            "worker",
            "message_delivery_failed",
            peer,
            Some(&command.message_id),
            connection,
            None,
        );
        inner.log_connection_snapshot(
            "worker",
            "connection_closed",
            peer,
            Some(&command.message_id),
            connection,
            None,
        );
        connection.close(0u32.into(), b"delivery failed");
    }
    logging::log_warn("worker", "message_delivery_failed", fields);
    let _ = complete_delivery(command, Err(error)).await;
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

async fn process_delivery(
    peer: EndpointId,
    inner: Arc<TransportInner>,
    command: QueuedDelivery,
    shutdown: WorkerShutdown,
) {
    logging::log_event(
        "worker",
        "message_delivery_started",
        LogFields::default()
            .peer_str(peer.to_string())
            .message(&command.message_id)
            .detail(
                "queue_wait_ms",
                Instant::now()
                    .saturating_duration_since(command.queued_at)
                    .as_millis()
                    .to_string(),
            ),
    );

    if command.cancellation.is_cancelled() {
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

    let connection = match run_stage(
        command.deadline,
        inner.connect_for(peer, command.deadline),
        &command.cancellation,
        &shutdown,
    )
    .await
    {
        Ok(Ok(connection)) => connection,
        Ok(Err(error)) => {
            let error = if error == "connection dial timed out" {
                DeliveryError::TimedOut
            } else {
                DeliveryError::Transport(error)
            };
            fail_delivery(peer, inner.as_ref(), command, None, error).await;
            return;
        }
        Err(abort) => {
            abort_stage(peer, inner.as_ref(), command, None, "connection", abort).await;
            return;
        }
    };
    inner.log_connection_snapshot(
        "worker",
        "connection_ready",
        peer,
        Some(&command.message_id),
        &connection,
        None,
    );

    let (mut send, mut recv) = match run_stage(
        command.deadline,
        connection.open_bi(),
        &command.cancellation,
        &shutdown,
    )
    .await
    {
        Ok(Ok(streams)) => streams,
        Ok(Err(error)) => {
            fail_delivery(
                peer,
                inner.as_ref(),
                command,
                Some(&connection),
                DeliveryError::Transport(error.to_string()),
            )
            .await;
            return;
        }
        Err(abort) => {
            abort_stage(
                peer,
                inner.as_ref(),
                command,
                Some(&connection),
                "stream_open",
                abort,
            )
            .await;
            return;
        }
    };
    let stream_id = u64::from(send.id());
    inner.log_connection_snapshot(
        "worker",
        "stream_opened",
        peer,
        Some(&command.message_id),
        &connection,
        Some(stream_id),
    );

    if !inner.contacts.contains(&peer).await {
        fail_delivery(
            peer,
            inner.as_ref(),
            command,
            Some(&connection),
            DeliveryError::NotAContact,
        )
        .await;
        return;
    }

    match run_stage(
        command.deadline,
        write_document(&mut send, &command.envelope),
        &command.cancellation,
        &shutdown,
    )
    .await
    {
        Ok(Ok(())) => inner.log_connection_snapshot(
            "worker",
            "text_frame_written",
            peer,
            Some(&command.message_id),
            &connection,
            Some(stream_id),
        ),
        Ok(Err(error)) => {
            fail_delivery(
                peer,
                inner.as_ref(),
                command,
                Some(&connection),
                map_frame_error(error),
            )
            .await;
            return;
        }
        Err(abort) => {
            abort_stage(
                peer,
                inner.as_ref(),
                command,
                Some(&connection),
                "write",
                abort,
            )
            .await;
            return;
        }
    }

    if let Err(error) = send.finish() {
        fail_delivery(
            peer,
            inner.as_ref(),
            command,
            Some(&connection),
            DeliveryError::Transport(error.to_string()),
        )
        .await;
        return;
    }

    let receipt = match run_stage(
        command.deadline,
        read_single_document(&mut recv),
        &command.cancellation,
        &shutdown,
    )
    .await
    {
        Ok(Ok(envelope)) => {
            inner.log_connection_snapshot(
                "worker",
                "receipt_received",
                peer,
                Some(&command.message_id),
                &connection,
                Some(stream_id),
            );
            validate_receipt(command.message_id, envelope.frame)
        }
        Ok(Err(error)) => {
            fail_delivery(
                peer,
                inner.as_ref(),
                command,
                Some(&connection),
                map_frame_error(error),
            )
            .await;
            return;
        }
        Err(abort) => {
            abort_stage(
                peer,
                inner.as_ref(),
                command,
                Some(&connection),
                "receipt_read",
                abort,
            )
            .await;
            return;
        }
    };

    let result = receipt;
    let message_id = command.message_id;
    let status = match &result {
        Ok(_) => "accepted",
        Err(DeliveryError::Rejected(_)) => "rejected",
        Err(_) => "invalid",
    };
    logging::log_event(
        "worker",
        "receipt_received",
        LogFields::default()
            .peer_str(peer.to_string())
            .message(&message_id)
            .connection(connection.stable_id())
            .stream(stream_id)
            .status(status),
    );
    if matches!(result, Err(DeliveryError::ProtocolViolation)) {
        fail_delivery(
            peer,
            inner.as_ref(),
            command,
            Some(&connection),
            DeliveryError::ProtocolViolation,
        )
        .await;
        return;
    }
    if !complete_delivery(command, result).await {
        logging::log_warn(
            "worker",
            "message_delivery_completion_lost",
            LogFields::default()
                .peer_str(peer.to_string())
                .message(&message_id)
                .connection(connection.stable_id())
                .stream(stream_id)
                .reason("deadline_already_won"),
        );
    }
    inner.log_connection_snapshot(
        "worker",
        "connection_closed",
        peer,
        Some(&message_id),
        &connection,
        Some(stream_id),
    );
    connection.close(0u32.into(), b"delivery complete");
}

fn map_frame_error(error: FrameError) -> DeliveryError {
    match error {
        FrameError::Io(error) => DeliveryError::Transport(error.to_string()),
        FrameError::DeclaredFrameTooLarge { declared } => {
            DeliveryError::Transport(format!("declared frame too large: {declared}"))
        }
        FrameError::TrailingData | FrameError::Protocol(_) => DeliveryError::ProtocolViolation,
    }
}

pub(super) fn spawn_deadline(
    deadline: Instant,
    completion: CompletionSlot,
    cancellation: Arc<DeliveryCancellation>,
    peer: EndpointId,
    message_id: MessageId,
) {
    tokio::spawn(async move {
        time::sleep_until(deadline).await;
        if cancellation.cancel() {
            logging::log_warn(
                "worker",
                "message_delivery_timed_out",
                LogFields::default()
                    .peer_str(peer.to_string())
                    .message(&message_id)
                    .reason("delivery_deadline"),
            );
            resolve_once(&completion, Err(DeliveryError::TimedOut)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::RejectionCode;
    use tokio::sync::oneshot;

    fn queued_delivery_with(
        completion: CompletionSlot,
        cancellation: Arc<DeliveryCancellation>,
    ) -> QueuedDelivery {
        let message_id = MessageId::new([9; 16]);
        QueuedDelivery {
            envelope: WireEnvelope::new(ChatFrame::text(message_id, 0, "hello").unwrap()),
            message_id,
            completion,
            cancellation,
            deadline: Instant::now(),
            queued_at: Instant::now(),
        }
    }

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

    #[test]
    fn wrong_response_frame_is_a_protocol_violation() {
        let expected = MessageId::new([6; 16]);
        assert!(matches!(
            validate_receipt(
                expected,
                ChatFrame::text(MessageId::new([7; 16]), 0, "not a receipt").unwrap(),
            ),
            Err(DeliveryError::ProtocolViolation)
        ));
    }

    #[tokio::test]
    async fn complete_delivery_reports_when_this_caller_wins() {
        let (tx, rx) = oneshot::channel();
        let completion = Arc::new(Mutex::new(Some(tx)));
        let cancellation = Arc::new(DeliveryCancellation::new());
        let receipt = DeliveryReceipt {
            message_id: MessageId::new([8; 16]),
            received_at_unix_ms: 42,
        };
        let won = complete_delivery(
            queued_delivery_with(completion, cancellation.clone()),
            Ok(receipt),
        )
        .await;
        assert!(won);
        assert!(cancellation.is_cancelled());
        assert_eq!(rx.await.unwrap().unwrap(), receipt);
    }

    #[tokio::test]
    async fn complete_delivery_reports_when_timeout_already_won() {
        let (tx, rx) = oneshot::channel();
        let completion = Arc::new(Mutex::new(Some(tx)));
        let cancellation = Arc::new(DeliveryCancellation::new());
        let won = complete_delivery(
            queued_delivery_with(completion.clone(), cancellation.clone()),
            Err(DeliveryError::TimedOut),
        )
        .await;
        assert!(won);
        assert!(cancellation.is_cancelled());
        assert!(completion.lock().await.is_none());

        let late_receipt = DeliveryReceipt {
            message_id: MessageId::new([10; 16]),
            received_at_unix_ms: 43,
        };
        let late_won = complete_delivery(
            queued_delivery_with(completion, cancellation),
            Ok(late_receipt),
        )
        .await;
        assert!(!late_won);
        assert!(matches!(rx.await.unwrap(), Err(DeliveryError::TimedOut)));
    }
}
