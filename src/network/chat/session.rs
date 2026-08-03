use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use iroh::{EndpointId, endpoint::Connection};
use tokio::{
    sync::{Mutex, OwnedSemaphorePermit, mpsc, watch},
    task::JoinHandle,
    time::{self, Instant},
};

use super::framing::{FrameError, read_single_document, write_document};
use super::transport::TransportInner;
use super::{
    CompletionSlot, DeliveryCancellation, DeliveryError, DeliveryReceipt, INBOUND_STREAM_TIMEOUT,
    IncomingText, OUTGOING_QUEUE_CAPACITY, resolve_once, unix_ms_now,
};
use crate::logging::{self, LogFields};
use crate::protocol::{ChatFrame, MessageId, RejectionCode, WireEnvelope};

#[derive(Clone)]
pub(super) struct QueuedDelivery {
    pub(super) envelope: WireEnvelope,
    pub(super) message_id: MessageId,
    pub(super) completion: CompletionSlot,
    pub(super) cancellation: Arc<DeliveryCancellation>,
    pub(super) deadline: Instant,
    pub(super) queued_at: Instant,
}

#[derive(Clone)]
pub(super) struct PeerSession {
    tx: mpsc::Sender<QueuedDelivery>,
    control: mpsc::UnboundedSender<SessionControl>,
    join: Arc<Mutex<Option<JoinHandle<()>>>>,
    queued: Arc<AtomicUsize>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum SessionState {
    Disconnected,
    Connecting,
    Connected,
    Closing,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum ConnectionOrigin {
    Inbound,
    Outbound,
}

impl ConnectionOrigin {
    fn as_str(self) -> &'static str {
        match self {
            Self::Inbound => "inbound",
            Self::Outbound => "outbound",
        }
    }
}

struct ConnectionSlot {
    connection: Connection,
    origin: ConnectionOrigin,
    stop_accept: watch::Sender<bool>,
    active_handlers: usize,
    active_outbound: usize,
}

struct ActiveDelivery {
    command: QueuedDelivery,
    connection_id: Option<usize>,
    task: Option<JoinHandle<()>>,
}

enum SessionControl {
    Attach {
        connection: Connection,
        origin: ConnectionOrigin,
    },
    DialFinished {
        result: Result<Connection, String>,
    },
    ConnectionClosed {
        connection_id: usize,
        reason: String,
    },
    AcceptLoopStopped,
    InboundStream {
        connection_id: usize,
        send: iroh::endpoint::SendStream,
        recv: iroh::endpoint::RecvStream,
        permit: OwnedSemaphorePermit,
    },
    InboundFinished {
        connection_id: usize,
    },
    OutboundFinished {
        message_id: MessageId,
        connection_id: usize,
        result: Result<DeliveryReceipt, DeliveryError>,
    },
    Shutdown(DeliveryError),
}

impl PeerSession {
    pub(super) fn spawn(
        peer: EndpointId,
        inner: Arc<TransportInner>,
        session_permit: Option<OwnedSemaphorePermit>,
    ) -> Self {
        let (tx, rx) = mpsc::channel(OUTGOING_QUEUE_CAPACITY);
        let (control, control_rx) = mpsc::unbounded_channel();
        let queued = Arc::new(AtomicUsize::new(0));
        let queued_for_actor = queued.clone();
        let actor_control = control.clone();
        let join = tokio::spawn(async move {
            let mut actor = SessionActor::new(
                peer,
                inner,
                rx,
                control_rx,
                actor_control,
                session_permit,
                queued_for_actor,
            );
            actor.run().await;
        });
        Self {
            tx,
            control,
            join: Arc::new(Mutex::new(Some(join))),
            queued,
        }
    }

    pub(super) fn try_enqueue(&self, delivery: QueuedDelivery) -> Result<(), DeliveryError> {
        let reserved = self
            .queued
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                (current < OUTGOING_QUEUE_CAPACITY).then_some(current + 1)
            })
            .is_ok();
        if !reserved {
            return Err(DeliveryError::QueueFull);
        }
        match self.tx.try_send(delivery) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.queued.fetch_sub(1, Ordering::AcqRel);
                Err(DeliveryError::QueueFull)
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                self.queued.fetch_sub(1, Ordering::AcqRel);
                Err(DeliveryError::ShutDown)
            }
        }
    }

    pub(super) fn attach_inbound(&self, connection: Connection) {
        let _ = self.control.send(SessionControl::Attach {
            connection,
            origin: ConnectionOrigin::Inbound,
        });
    }

    pub(super) async fn shutdown(&self, reason: DeliveryError) {
        let _ = self.control.send(SessionControl::Shutdown(reason));
        if let Some(join) = self.join.lock().await.take() {
            let _ = join.await;
        }
    }
}

struct SessionActor {
    peer: EndpointId,
    inner: Arc<TransportInner>,
    rx: mpsc::Receiver<QueuedDelivery>,
    control_rx: mpsc::UnboundedReceiver<SessionControl>,
    control: mpsc::UnboundedSender<SessionControl>,
    session_permit: Option<OwnedSemaphorePermit>,
    queue: VecDeque<QueuedDelivery>,
    state: SessionState,
    primary: Option<ConnectionSlot>,
    draining: Vec<ConnectionSlot>,
    active: Option<ActiveDelivery>,
    dial_in_progress: bool,
    stopping: bool,
    queued: Arc<AtomicUsize>,
}

impl SessionActor {
    fn new(
        peer: EndpointId,
        inner: Arc<TransportInner>,
        rx: mpsc::Receiver<QueuedDelivery>,
        control_rx: mpsc::UnboundedReceiver<SessionControl>,
        control: mpsc::UnboundedSender<SessionControl>,
        session_permit: Option<OwnedSemaphorePermit>,
        queued: Arc<AtomicUsize>,
    ) -> Self {
        Self {
            peer,
            inner,
            rx,
            control_rx,
            control,
            session_permit,
            queue: VecDeque::new(),
            state: SessionState::Disconnected,
            primary: None,
            draining: Vec::new(),
            active: None,
            dial_in_progress: false,
            stopping: false,
            queued,
        }
    }

    async fn run(&mut self) {
        self.log_state(SessionState::Disconnected);
        loop {
            if self.stopping {
                break;
            }
            self.start_next_if_possible().await;
            tokio::select! {
                biased;
                control = self.control_rx.recv() => {
                    match control {
                        Some(control) => self.handle_control(control).await,
                        None => self.stopping = true,
                    }
                }
                delivery = self.rx.recv(), if !self.stopping => {
                    match delivery {
                        Some(delivery) => self.queue.push_back(delivery),
                        None => self.stopping = true,
                    }
                }
            }
        }
        self.finish_shutdown(DeliveryError::ShutDown).await;
    }

    async fn handle_control(&mut self, control: SessionControl) {
        match control {
            SessionControl::Attach { connection, origin } => {
                if self.stopping {
                    connection.close(0u32.into(), b"session closing");
                } else {
                    self.attach_connection(connection, origin).await;
                    self.start_pending_delivery();
                }
            }
            SessionControl::DialFinished { result } => {
                self.dial_in_progress = false;
                match result {
                    Ok(connection) => {
                        self.attach_connection(connection, ConnectionOrigin::Outbound)
                            .await;
                        self.start_pending_delivery();
                    }
                    Err(error) => {
                        if self.primary.is_none() {
                            self.log_state(SessionState::Disconnected);
                        }
                        if self
                            .active
                            .as_ref()
                            .is_some_and(|active| active.connection_id.is_none())
                            && let Some(active) = self.active.take()
                        {
                            let delivery_error = if error == "connection dial timed out" {
                                DeliveryError::TimedOut
                            } else {
                                DeliveryError::Transport(error.clone())
                            };
                            force_finish_command(&active.command, Err(delivery_error)).await;
                        }
                    }
                }
            }
            SessionControl::ConnectionClosed {
                connection_id,
                reason,
            } => {
                self.connection_closed(connection_id, reason).await;
            }
            SessionControl::AcceptLoopStopped => {
                self.close_draining_finished();
            }
            SessionControl::InboundStream {
                connection_id,
                send,
                recv,
                permit,
            } => {
                if let Some(slot) = self.find_slot_mut(connection_id) {
                    slot.active_handlers += 1;
                    let peer = self.peer;
                    let inner = self.inner.clone();
                    let control = self.control.clone();
                    let connection = self
                        .find_slot_mut(connection_id)
                        .map(|slot| slot.connection.clone());
                    tokio::spawn(async move {
                        if let Some(connection) = connection {
                            handle_inbound_stream(
                                peer,
                                inner,
                                connection,
                                connection_id,
                                send,
                                recv,
                            )
                            .await;
                        } else {
                            let mut send = send;
                            let mut recv = recv;
                            reset_stream(&mut send, &mut recv);
                        }
                        drop(permit);
                        let _ = control.send(SessionControl::InboundFinished { connection_id });
                    });
                } else {
                    let mut send = send;
                    let mut recv = recv;
                    reset_stream(&mut send, &mut recv);
                    drop(permit);
                }
            }
            SessionControl::InboundFinished { connection_id } => {
                if let Some(slot) = self.find_slot_mut(connection_id) {
                    slot.active_handlers = slot.active_handlers.saturating_sub(1);
                }
                self.close_draining_finished();
            }
            SessionControl::OutboundFinished {
                message_id,
                connection_id,
                result,
            } => {
                if let Some(slot) = self.find_slot_mut(connection_id) {
                    slot.active_outbound = slot.active_outbound.saturating_sub(1);
                }
                let matches_active = self.active.as_ref().is_some_and(|active| {
                    active.command.message_id == message_id
                        && active.connection_id == Some(connection_id)
                });
                if matches_active && let Some(active) = self.active.take() {
                    finish_command(&active.command, result).await;
                }
                self.close_draining_finished();
            }
            SessionControl::Shutdown(reason) => {
                self.stopping = true;
                self.finish_shutdown(reason).await;
            }
        }
    }

    async fn start_next_if_possible(&mut self) {
        if self.active.is_some() {
            return;
        }
        while let Some(command) = self.queue.pop_front() {
            if command.cancellation.is_cancelled() {
                self.release_queued_slot();
                continue;
            }
            if !self.inner.contacts.contains(&self.peer).await {
                self.release_queued_slot();
                finish_command(&command, Err(DeliveryError::NotAContact)).await;
                continue;
            }
            if Instant::now() >= command.deadline {
                self.release_queued_slot();
                finish_command(&command, Err(DeliveryError::TimedOut)).await;
                continue;
            }
            let Some(primary) = self.primary.as_ref() else {
                let deadline = command.deadline;
                self.release_queued_slot();
                self.active = Some(ActiveDelivery {
                    command,
                    connection_id: None,
                    task: None,
                });
                self.begin_dial(deadline).await;
                return;
            };
            let connection = primary.connection.clone();
            self.release_queued_slot();
            self.active = Some(self.spawn_active_delivery(command, connection));
            return;
        }
    }

    fn spawn_active_delivery(
        &mut self,
        command: QueuedDelivery,
        connection: Connection,
    ) -> ActiveDelivery {
        let connection_id = connection.stable_id();
        if let Some(primary) = self.primary.as_mut()
            && primary.connection.stable_id() == connection_id
        {
            primary.active_outbound += 1;
        }
        let task = spawn_outbound_stream(
            self.peer,
            self.inner.clone(),
            connection,
            command.clone(),
            self.control.clone(),
        );
        ActiveDelivery {
            command,
            connection_id: Some(connection_id),
            task: Some(task),
        }
    }

    fn start_pending_delivery(&mut self) {
        let Some(active) = self.active.take() else {
            return;
        };
        if active.connection_id.is_some() {
            self.active = Some(active);
            return;
        }
        if active.command.cancellation.is_cancelled() || Instant::now() >= active.command.deadline {
            return;
        }
        let Some(primary) = self.primary.as_ref() else {
            self.active = Some(active);
            return;
        };
        self.active = Some(self.spawn_active_delivery(active.command, primary.connection.clone()));
    }

    async fn begin_dial(&mut self, deadline: Instant) {
        if self.dial_in_progress || self.stopping {
            return;
        }
        self.dial_in_progress = true;
        self.log_state(SessionState::Connecting);
        let inner = self.inner.clone();
        let peer = self.peer;
        let control = self.control.clone();
        tokio::spawn(async move {
            let result = inner.connect_for(peer, deadline).await;
            let _ = control.send(SessionControl::DialFinished { result });
        });
    }

    async fn attach_connection(&mut self, connection: Connection, origin: ConnectionOrigin) {
        let connection_id = connection.stable_id();
        let candidate_preferred = is_preferred(self.inner.endpoint.id(), self.peer, origin);
        let (stop_accept, stop_rx) = watch::channel(false);
        let slot = ConnectionSlot {
            connection: connection.clone(),
            origin,
            stop_accept,
            active_handlers: 0,
            active_outbound: 0,
        };
        self.inner.log_connection_snapshot(
            "session",
            "peer_connection_attached",
            self.peer,
            None,
            &connection,
            None,
        );
        logging::log_event(
            "session",
            "peer_connection_attached",
            LogFields::default()
                .peer_str(self.peer.to_string())
                .connection(connection_id)
                .direction(origin.as_str())
                .detail("preferred", candidate_preferred.to_string()),
        );
        spawn_connection_tasks(
            self.peer,
            self.inner.clone(),
            connection.clone(),
            origin,
            stop_rx,
            self.control.clone(),
        );

        let replace = match self.primary.as_ref() {
            None => true,
            Some(current) => {
                let current_preferred =
                    is_preferred(self.inner.endpoint.id(), self.peer, current.origin);
                candidate_preferred && !current_preferred
            }
        };
        if replace {
            if let Some(old) = self.primary.replace(slot) {
                self.start_draining(old);
            }
            logging::log_event(
                "session",
                "peer_connection_selected",
                LogFields::default()
                    .peer_str(self.peer.to_string())
                    .connection(connection_id)
                    .direction(origin.as_str()),
            );
            self.log_state(SessionState::Connected);
        } else {
            self.start_draining(slot);
        }
    }

    fn start_draining(&mut self, slot: ConnectionSlot) {
        let _ = slot.stop_accept.send(true);
        logging::log_event(
            "session",
            "peer_connection_draining",
            LogFields::default()
                .peer_str(self.peer.to_string())
                .connection(slot.connection.stable_id())
                .direction(slot.origin.as_str())
                .detail("active_stream_handlers", slot.active_handlers.to_string())
                .detail("active_outbound_streams", slot.active_outbound.to_string()),
        );
        if slot.active_handlers == 0 && slot.active_outbound == 0 {
            slot.connection.close(0u32.into(), b"superseded connection");
        } else {
            self.draining.push(slot);
        }
    }

    async fn connection_closed(&mut self, connection_id: usize, reason: String) {
        let lost_connection = self
            .primary
            .as_ref()
            .filter(|slot| slot.connection.stable_id() == connection_id)
            .map(|slot| slot.connection.clone())
            .or_else(|| {
                self.draining
                    .iter()
                    .find(|slot| slot.connection.stable_id() == connection_id)
                    .map(|slot| slot.connection.clone())
            });
        let was_primary = self
            .primary
            .as_ref()
            .is_some_and(|slot| slot.connection.stable_id() == connection_id);
        let mut removed = false;
        if was_primary {
            self.primary = None;
            removed = true;
            self.log_state(SessionState::Disconnected);
        } else if let Some(index) = self
            .draining
            .iter()
            .position(|slot| slot.connection.stable_id() == connection_id)
        {
            self.draining.swap_remove(index);
            removed = true;
        }
        if !removed {
            return;
        }
        if let Some(connection) = lost_connection.as_ref() {
            self.inner.log_connection_snapshot(
                "session",
                "peer_connection_lost_snapshot",
                self.peer,
                None,
                connection,
                None,
            );
        }
        logging::log_event(
            "session",
            "peer_connection_lost",
            LogFields::default()
                .peer_str(self.peer.to_string())
                .connection(connection_id)
                .reason(reason),
        );
        if self
            .active
            .as_ref()
            .is_some_and(|active| active.connection_id == Some(connection_id))
            && let Some(active) = self.active.take()
        {
            active.command.cancellation.cancel();
            if let Some(task) = active.task {
                task.abort();
            }
            force_finish_command(
                &active.command,
                Err(DeliveryError::Transport("connection lost".into())),
            )
            .await;
        }
    }

    fn find_slot_mut(&mut self, connection_id: usize) -> Option<&mut ConnectionSlot> {
        if self
            .primary
            .as_ref()
            .is_some_and(|slot| slot.connection.stable_id() == connection_id)
        {
            return self.primary.as_mut();
        }
        self.draining
            .iter_mut()
            .find(|slot| slot.connection.stable_id() == connection_id)
    }

    fn close_draining_finished(&mut self) {
        let mut keep = Vec::with_capacity(self.draining.len());
        for slot in self.draining.drain(..) {
            if slot.active_handlers == 0 && slot.active_outbound == 0 {
                slot.connection.close(0u32.into(), b"draining complete");
            } else {
                keep.push(slot);
            }
        }
        self.draining = keep;
    }

    async fn finish_shutdown(&mut self, reason: DeliveryError) {
        if self.state != SessionState::Closing {
            self.log_state(SessionState::Closing);
        }
        self.stopping = true;
        if let Some(active) = self.active.take() {
            if let Some(task) = active.task {
                task.abort();
            }
            force_finish_command(&active.command, Err(reason.clone())).await;
        }
        while let Some(command) = self.queue.pop_front() {
            self.release_queued_slot();
            finish_command(&command, Err(reason.clone())).await;
        }
        while let Ok(command) = self.rx.try_recv() {
            self.release_queued_slot();
            finish_command(&command, Err(reason.clone())).await;
        }
        if let Some(primary) = self.primary.take() {
            primary.connection.close(0u32.into(), b"session shutdown");
        }
        for draining in self.draining.drain(..) {
            draining.connection.close(0u32.into(), b"session shutdown");
        }
        self.dial_in_progress = false;
        self.session_permit.take();
    }

    fn release_queued_slot(&self) {
        self.queued.fetch_sub(1, Ordering::AcqRel);
    }

    fn log_state(&mut self, state: SessionState) {
        if self.state == state {
            return;
        }
        self.state = state;
        logging::log_event(
            "session",
            "peer_session_state_changed",
            LogFields::default()
                .peer_str(self.peer.to_string())
                .status(format!("{state:?}")),
        );
    }
}

fn is_preferred(local: EndpointId, remote: EndpointId, origin: ConnectionOrigin) -> bool {
    match origin {
        ConnectionOrigin::Outbound => local < remote,
        ConnectionOrigin::Inbound => local > remote,
    }
}

fn spawn_connection_tasks(
    peer: EndpointId,
    inner: Arc<TransportInner>,
    connection: Connection,
    origin: ConnectionOrigin,
    mut stop_rx: watch::Receiver<bool>,
    control: mpsc::UnboundedSender<SessionControl>,
) {
    let connection_id = connection.stable_id();
    let accept_control = control.clone();
    let accept_inner = inner.clone();
    let accept_connection = connection.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                biased;
                changed = stop_rx.changed() => {
                    if changed.is_err() || *stop_rx.borrow() {
                        break;
                    }
                }
                accepted = accept_connection.accept_bi() => {
                    match accepted {
                        Ok((send, recv)) => {
                            let Some(permit) = accept_inner.inbound_stream_handlers.clone().try_acquire_owned().ok() else {
                                logging::log_warn(
                                    "session",
                                    "inbound_stream_limit_reached",
                                    LogFields::default()
                                        .peer_str(peer.to_string())
                                        .connection(connection_id)
                                        .reason("stream_handler_budget"),
                                );
                                let mut send = send;
                                let mut recv = recv;
                                reset_stream(&mut send, &mut recv);
                                logging::log_warn(
                                    "session",
                                    "stream_reset",
                                    LogFields::default()
                                        .peer_str(peer.to_string())
                                        .connection(connection_id)
                                        .stream(u64::from(send.id()))
                                        .reason("stream_handler_budget"),
                                );
                                continue;
                            };
                            logging::log_event(
                                "session",
                                "inbound_stream_accepted",
                                LogFields::default()
                                    .peer_str(peer.to_string())
                                    .connection(connection_id)
                                    .stream(u64::from(send.id()))
                                    .direction(origin.as_str()),
                            );
                            if accept_control.send(SessionControl::InboundStream { connection_id, send, recv, permit }).is_err() {
                                break;
                            }
                        }
                        Err(error) => {
                            let _ = accept_control.send(SessionControl::AcceptLoopStopped);
                            logging::log_warn(
                                "session",
                                "accept_loop_stopped",
                                LogFields::default()
                                    .peer_str(peer.to_string())
                                    .connection(connection_id)
                                    .error(&error),
                            );
                            break;
                        }
                    }
                }
            }
        }
        let _ = accept_control.send(SessionControl::AcceptLoopStopped);
    });

    let monitor_control = control;
    tokio::spawn(async move {
        let reason = connection.closed().await;
        let _ = monitor_control.send(SessionControl::ConnectionClosed {
            connection_id,
            reason: format!("{reason:?}"),
        });
    });
}

fn spawn_outbound_stream(
    peer: EndpointId,
    inner: Arc<TransportInner>,
    connection: Connection,
    command: QueuedDelivery,
    control: mpsc::UnboundedSender<SessionControl>,
) -> JoinHandle<()> {
    let connection_id = connection.stable_id();
    tokio::spawn(async move {
        let message_id = command.message_id;
        let result = run_outbound_stream(peer, inner, connection, command.clone()).await;
        finish_command(&command, result.clone()).await;
        let _ = control.send(SessionControl::OutboundFinished {
            message_id,
            connection_id,
            result,
        });
    })
}

async fn run_outbound_stream(
    peer: EndpointId,
    inner: Arc<TransportInner>,
    connection: Connection,
    command: QueuedDelivery,
) -> Result<DeliveryReceipt, DeliveryError> {
    let connection_id = connection.stable_id();
    logging::log_event(
        "session",
        "message_delivery_started",
        LogFields::default()
            .peer_str(peer.to_string())
            .message(&command.message_id)
            .connection(connection.stable_id())
            .detail(
                "queue_wait_ms",
                Instant::now()
                    .saturating_duration_since(command.queued_at)
                    .as_millis()
                    .to_string(),
            ),
    );
    let (mut send, mut recv) = match run_stream_stage(
        command.deadline,
        connection.open_bi(),
        &command.cancellation,
    )
    .await
    {
        Ok(Ok(streams)) => streams,
        Ok(Err(error)) => return Err(DeliveryError::Transport(error.to_string())),
        Err(error) => return Err(error),
    };
    let stream_id = u64::from(send.id());
    inner.log_connection_snapshot(
        "session",
        "stream_opened",
        peer,
        Some(&command.message_id),
        &connection,
        Some(stream_id),
    );
    if let Err(error) = run_stream_stage(
        command.deadline,
        write_document(&mut send, &command.envelope),
        &command.cancellation,
    )
    .await
    .and_then(|result| result.map_err(map_frame_error))
    {
        log_outbound_stream_reset(
            peer,
            connection_id,
            &command.message_id,
            stream_id,
            &mut send,
            &mut recv,
            &error,
        );
        return Err(error);
    }
    inner.log_connection_snapshot(
        "session",
        "text_frame_written",
        peer,
        Some(&command.message_id),
        &connection,
        Some(stream_id),
    );
    if let Err(error) = send.finish() {
        log_outbound_stream_reset(
            peer,
            connection_id,
            &command.message_id,
            stream_id,
            &mut send,
            &mut recv,
            &error,
        );
        return Err(DeliveryError::Transport(error.to_string()));
    }
    let receipt = match run_stream_stage(
        command.deadline,
        read_single_document(&mut recv),
        &command.cancellation,
    )
    .await
    {
        Ok(Ok(envelope)) => validate_receipt(command.message_id, envelope.frame),
        Ok(Err(error)) => Err(map_frame_error(error)),
        Err(error) => Err(error),
    };
    if receipt.is_err()
        && let Err(error) = &receipt
    {
        log_outbound_stream_reset(
            peer,
            connection_id,
            &command.message_id,
            stream_id,
            &mut send,
            &mut recv,
            error,
        );
    }
    if let Ok(receipt) = receipt {
        inner.log_connection_snapshot(
            "session",
            "receipt_received",
            peer,
            Some(&command.message_id),
            &connection,
            Some(stream_id),
        );
        Ok(receipt)
    } else {
        receipt
    }
}

async fn run_stream_stage<T, E, F>(
    deadline: Instant,
    operation: F,
    cancellation: &DeliveryCancellation,
) -> Result<Result<T, E>, DeliveryError>
where
    F: std::future::Future<Output = Result<T, E>>,
{
    tokio::pin!(operation);
    tokio::select! {
        result = time::timeout_at(deadline, &mut operation) => result.map_err(|_| DeliveryError::TimedOut),
        _ = cancellation.wait() => Err(DeliveryError::TimedOut),
    }
}

async fn handle_inbound_stream(
    peer: EndpointId,
    inner: Arc<TransportInner>,
    connection: Connection,
    connection_id: usize,
    mut send: iroh::endpoint::SendStream,
    mut recv: iroh::endpoint::RecvStream,
) {
    let stream_id = u64::from(send.id());
    let envelope =
        match time::timeout(INBOUND_STREAM_TIMEOUT, read_single_document(&mut recv)).await {
            Ok(Ok(envelope)) => envelope,
            Ok(Err(error)) => {
                reset_stream_with_log(peer, connection_id, stream_id, &mut send, &mut recv, &error);
                return;
            }
            Err(_) => {
                reset_stream_with_log(
                    peer,
                    connection_id,
                    stream_id,
                    &mut send,
                    &mut recv,
                    "read timeout",
                );
                return;
            }
        };
    let ChatFrame::Text {
        message_id,
        sent_at_unix_ms,
        body,
    } = envelope.frame
    else {
        reset_stream_with_log(
            peer,
            connection_id,
            stream_id,
            &mut send,
            &mut recv,
            "expected text frame",
        );
        return;
    };
    inner.log_connection_snapshot(
        "session",
        "text_frame_received",
        peer,
        Some(&message_id),
        &connection,
        Some(stream_id),
    );
    if !inner.contacts.contains(&peer).await {
        let rejected = WireEnvelope::new(ChatFrame::rejected(
            message_id,
            RejectionCode::UnknownContact,
        ));
        if write_document(&mut send, &rejected).await.is_ok() {
            let _ = send.finish();
        }
        return;
    }
    let incoming = IncomingText {
        peer_id: crate::domain::identity::PeerId::from_canonical(peer.to_string()),
        message_id,
        sent_at_unix_ms,
        body,
    };
    if inner.incoming_tx.send(incoming).await.is_err() {
        reset_stream_with_log(
            peer,
            connection_id,
            stream_id,
            &mut send,
            &mut recv,
            "incoming queue closed",
        );
        return;
    }
    let accepted = WireEnvelope::new(ChatFrame::accepted(message_id, unix_ms_now()));
    if let Err(error) = write_document(&mut send, &accepted).await {
        reset_stream_with_log(peer, connection_id, stream_id, &mut send, &mut recv, &error);
        return;
    }
    if let Err(error) = send.finish() {
        reset_stream_with_log(peer, connection_id, stream_id, &mut send, &mut recv, &error);
        return;
    }
    inner.log_connection_snapshot(
        "session",
        "receipt_write_finished",
        peer,
        Some(&message_id),
        &connection,
        Some(stream_id),
    );
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

fn map_frame_error(error: FrameError) -> DeliveryError {
    match error {
        FrameError::Io(error) => DeliveryError::Transport(error.to_string()),
        FrameError::DeclaredFrameTooLarge { declared } => {
            DeliveryError::Transport(format!("declared frame too large: {declared}"))
        }
        FrameError::TrailingData | FrameError::Protocol(_) => DeliveryError::ProtocolViolation,
    }
}

async fn finish_command(command: &QueuedDelivery, result: Result<DeliveryReceipt, DeliveryError>) {
    if command.cancellation.cancel() {
        resolve_once(&command.completion, result).await;
    }
}

async fn force_finish_command(
    command: &QueuedDelivery,
    result: Result<DeliveryReceipt, DeliveryError>,
) {
    command.cancellation.cancel();
    resolve_once(&command.completion, result).await;
}

fn reset_stream(send: &mut iroh::endpoint::SendStream, recv: &mut iroh::endpoint::RecvStream) {
    let _ = send.reset(0u32.into());
    let _ = recv.stop(0u32.into());
}

fn reset_stream_with_log(
    peer: EndpointId,
    connection_id: usize,
    stream_id: u64,
    send: &mut iroh::endpoint::SendStream,
    recv: &mut iroh::endpoint::RecvStream,
    reason: impl std::fmt::Display,
) {
    reset_stream(send, recv);
    logging::log_warn(
        "session",
        "stream_reset",
        LogFields::default()
            .peer_str(peer.to_string())
            .connection(connection_id)
            .stream(stream_id)
            .reason(reason.to_string()),
    );
}

fn log_outbound_stream_reset(
    peer: EndpointId,
    connection_id: usize,
    message_id: &MessageId,
    stream_id: u64,
    send: &mut iroh::endpoint::SendStream,
    recv: &mut iroh::endpoint::RecvStream,
    reason: impl std::fmt::Display,
) {
    reset_stream(send, recv);
    logging::log_warn(
        "session",
        "stream_reset",
        LogFields::default()
            .peer_str(peer.to_string())
            .connection(connection_id)
            .message(message_id)
            .stream(stream_id)
            .reason(reason.to_string()),
    );
}

#[cfg(test)]
mod tests {
    use iroh::SecretKey;

    use super::*;

    #[test]
    fn canonical_connection_origin_is_deterministic() {
        let first = SecretKey::from_bytes(&[1; 32]).public();
        let second = SecretKey::from_bytes(&[2; 32]).public();
        let (lower, higher) = if first < second {
            (first, second)
        } else {
            (second, first)
        };

        assert!(is_preferred(lower, higher, ConnectionOrigin::Outbound));
        assert!(is_preferred(higher, lower, ConnectionOrigin::Inbound));
        assert!(!is_preferred(higher, lower, ConnectionOrigin::Outbound));
        assert!(!is_preferred(lower, higher, ConnectionOrigin::Inbound));
    }

    #[test]
    fn receipt_validation_rejects_wrong_message_and_frame() {
        let expected = MessageId::new([1; 16]);
        let wrong = MessageId::new([2; 16]);
        assert!(matches!(
            validate_receipt(expected, ChatFrame::accepted(wrong, 0)),
            Err(DeliveryError::ProtocolViolation)
        ));
        assert!(matches!(
            validate_receipt(
                expected,
                ChatFrame::text(wrong, 0, "not a receipt").unwrap()
            ),
            Err(DeliveryError::ProtocolViolation)
        ));
    }
}
