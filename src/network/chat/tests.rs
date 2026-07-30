use std::time::Duration;

use iroh::{Endpoint, EndpointAddr, SecretKey, TransportAddr, endpoint::presets};
use tokio::sync::mpsc;

use super::framing::{read_document, write_document};
use super::transport::{exhaust_inbound_handler_budget, inbound_handler_budget, set_test_route};
use super::{
    CHAT_ALPN, ChatClient, ChatTransport, DeliveryError, INCOMING_QUEUE_CAPACITY, IncomingText,
    OUTGOING_QUEUE_CAPACITY,
};
use crate::domain::identity::PeerId;
use crate::network::identity::peer_id_from_secret;
use crate::protocol::{
    ChatFrame, MAX_FRAME_BYTES, MAX_TEXT_BYTES, MessageId, RejectionCode, ValidationError,
    WireEnvelope,
};
use iroh::EndpointId;

struct TestPeer {
    peer_id: PeerId,
    endpoint_id: EndpointId,
    transport: ChatTransport,
    client: ChatClient,
    incoming: mpsc::Receiver<IncomingText>,
}

async fn local_peer(secret: SecretKey, contacts: impl IntoIterator<Item = PeerId>) -> TestPeer {
    let peer_id = peer_id_from_secret(&secret);
    let endpoint_id = secret.public();
    let (transport, client, incoming) = ChatTransport::start(secret, contacts).await.unwrap();
    TestPeer {
        peer_id,
        endpoint_id,
        transport,
        client,
        incoming,
    }
}

async fn direct_loopback_addr(transport: &ChatTransport) -> EndpointAddr {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let mut addr = transport.endpoint().addr();
        addr.addrs.retain(|transport_addr| {
            matches!(
                transport_addr,
                TransportAddr::Ip(socket) if socket.ip().is_loopback()
            )
        });
        if addr.ip_addrs().next().is_some() {
            return addr;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for local direct addresses"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn connect_test_routes(alice: &TestPeer, bob: &TestPeer) {
    let alice_addr = direct_loopback_addr(&alice.transport).await;
    let bob_addr = direct_loopback_addr(&bob.transport).await;
    set_test_route(alice.transport.inner(), bob.endpoint_id, bob_addr).await;
    set_test_route(bob.transport.inner(), alice.endpoint_id, alice_addr).await;
}

async fn local_pair() -> (TestPeer, TestPeer) {
    let alice_secret = SecretKey::from_bytes(&[51; 32]);
    let bob_secret = SecretKey::from_bytes(&[52; 32]);
    let alice_id = peer_id_from_secret(&alice_secret);
    let bob_id = peer_id_from_secret(&bob_secret);

    let alice = local_peer(alice_secret, [bob_id]).await;
    let bob = local_peer(bob_secret, [alice_id]).await;
    connect_test_routes(&alice, &bob).await;
    (alice, bob)
}

#[tokio::test]
async fn authorised_contacts_exchange_unicode_text_and_an_ack() {
    let (alice, mut bob) = local_pair().await;

    let handle = alice
        .client
        .send_text(bob.peer_id.clone(), "Привет, 👋\nRathole")
        .await
        .unwrap();
    let incoming = tokio::time::timeout(Duration::from_secs(5), bob.incoming.recv())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(incoming.peer_id, alice.peer_id);
    assert_eq!(incoming.body, "Привет, 👋\nRathole");
    assert_eq!(incoming.message_id, handle.message_id);
    assert_eq!(handle.wait().await.unwrap().message_id, incoming.message_id);

    alice.transport.shutdown().await.unwrap();
    bob.transport.shutdown().await.unwrap();
}

#[tokio::test]
async fn one_peer_worker_preserves_queue_order_across_two_message_streams() {
    let (alice, mut bob) = local_pair().await;
    let first = alice
        .client
        .send_text(bob.peer_id.clone(), "first")
        .await
        .unwrap();
    let second = alice
        .client
        .send_text(bob.peer_id.clone(), "second")
        .await
        .unwrap();

    assert_eq!(bob.incoming.recv().await.unwrap().body, "first");
    assert_eq!(bob.incoming.recv().await.unwrap().body, "second");
    first.wait().await.unwrap();
    second.wait().await.unwrap();

    alice.transport.shutdown().await.unwrap();
    bob.transport.shutdown().await.unwrap();
}

#[tokio::test]
async fn unauthorised_sender_gets_unknown_contact_without_delivery() {
    let mut alice = local_peer(SecretKey::from_bytes(&[61; 32]), []).await;
    let bob = local_peer(SecretKey::from_bytes(&[62; 32]), [alice.peer_id.clone()]).await;
    connect_test_routes(&alice, &bob).await;

    let result = bob
        .client
        .send_text(alice.peer_id.clone(), "not authorised by Alice")
        .await
        .unwrap()
        .wait()
        .await;
    assert!(matches!(
        result,
        Err(DeliveryError::Rejected(RejectionCode::UnknownContact))
    ));
    assert!(
        tokio::time::timeout(Duration::from_millis(50), alice.incoming.recv())
            .await
            .is_err()
    );

    alice.transport.shutdown().await.unwrap();
    bob.transport.shutdown().await.unwrap();
}

#[tokio::test]
async fn oversized_body_fails_before_transport_work() {
    let (alice, bob) = local_pair().await;
    assert!(matches!(
        alice
            .client
            .send_text(bob.peer_id, "x".repeat(MAX_TEXT_BYTES + 1))
            .await,
        Err(DeliveryError::Validation(ValidationError::TextTooLarge))
    ));
    alice.transport.shutdown().await.unwrap();
    bob.transport.shutdown().await.unwrap();
}

#[tokio::test]
async fn simultaneous_bidirectional_sends_are_delivered_without_global_order_claim() {
    let (mut alice, mut bob) = local_pair().await;
    let alice_handle = alice
        .client
        .send_text(bob.peer_id.clone(), "from Alice")
        .await
        .unwrap();
    let bob_handle = bob
        .client
        .send_text(alice.peer_id.clone(), "from Bob")
        .await
        .unwrap();

    assert_eq!(alice.incoming.recv().await.unwrap().body, "from Bob");
    assert_eq!(bob.incoming.recv().await.unwrap().body, "from Alice");
    alice_handle.wait().await.unwrap();
    bob_handle.wait().await.unwrap();

    alice.transport.shutdown().await.unwrap();
    bob.transport.shutdown().await.unwrap();
}

#[tokio::test]
async fn incoming_backpressure_holds_the_sixty_fifth_accepted() {
    let (alice, mut bob) = local_pair().await;

    let mut completed = Vec::new();
    for index in 0..64 {
        let handle = alice
            .client
            .send_text(bob.peer_id.clone(), format!("queued-{index}"))
            .await
            .unwrap();
        completed.push(handle);
    }
    for handle in completed {
        tokio::time::timeout(Duration::from_secs(10), handle.wait())
            .await
            .expect("delivery within timeout")
            .unwrap();
    }

    let blocked = alice
        .client
        .send_text(bob.peer_id.clone(), "blocked-65")
        .await
        .unwrap();
    let wait_task = tokio::spawn(async move { blocked.wait().await });
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!wait_task.is_finished());

    let _ = bob.incoming.recv().await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), wait_task)
        .await
        .expect("65th delivery unblocked")
        .unwrap()
        .unwrap();

    alice.transport.shutdown().await.unwrap();
    bob.transport.shutdown().await.unwrap();
}

#[tokio::test]
async fn removing_contact_after_delivery_blocks_the_next_send() {
    let (alice, mut bob) = local_pair().await;
    let handle = alice
        .client
        .send_text(bob.peer_id.clone(), "once")
        .await
        .unwrap();
    assert_eq!(bob.incoming.recv().await.unwrap().body, "once");
    handle.wait().await.unwrap();

    alice.transport.replace_contacts([]).await.unwrap();
    assert!(matches!(
        alice.client.send_text(bob.peer_id.clone(), "twice").await,
        Err(DeliveryError::NotAContact)
    ));

    alice.transport.shutdown().await.unwrap();
    bob.transport.shutdown().await.unwrap();
}

#[tokio::test]
async fn oversized_declared_length_produces_no_incoming_message() {
    let alice_secret = SecretKey::from_bytes(&[71; 32]);
    let bob_secret = SecretKey::from_bytes(&[72; 32]);
    let alice_id = peer_id_from_secret(&alice_secret);
    let mut bob = local_peer(bob_secret, [alice_id]).await;

    let alice = Endpoint::builder(presets::N0)
        .secret_key(alice_secret)
        .alpns(vec![CHAT_ALPN.to_vec()])
        .relay_mode(iroh::RelayMode::Disabled)
        .clear_ip_transports()
        .bind_addr("127.0.0.1:0")
        .unwrap()
        .bind()
        .await
        .unwrap();

    let bob_addr = direct_loopback_addr(&bob.transport).await;
    let connection = alice.connect(bob_addr, CHAT_ALPN).await.unwrap();
    let (mut send, _recv) = connection.open_bi().await.unwrap();
    let oversized = ((MAX_FRAME_BYTES as u32) + 1).to_be_bytes();
    tokio::io::AsyncWriteExt::write_all(&mut send, &oversized)
        .await
        .unwrap();
    let _ = send.finish();

    assert!(
        tokio::time::timeout(Duration::from_millis(200), bob.incoming.recv())
            .await
            .is_err()
    );

    alice.close().await;
    bob.transport.shutdown().await.unwrap();
}

#[tokio::test]
async fn wrong_receipt_frame_is_a_protocol_violation_and_evicts() {
    let bob_secret = SecretKey::from_bytes(&[82; 32]);
    let alice_secret = SecretKey::from_bytes(&[81; 32]);
    let bob_peer_id = peer_id_from_secret(&bob_secret);

    let bob = Endpoint::builder(presets::N0)
        .secret_key(bob_secret)
        .alpns(vec![CHAT_ALPN.to_vec()])
        .relay_mode(iroh::RelayMode::Disabled)
        .clear_ip_transports()
        .bind_addr("127.0.0.1:0")
        .unwrap()
        .bind()
        .await
        .unwrap();
    let bob_endpoint_id = bob.id();

    let accept = {
        let bob = bob.clone();
        tokio::spawn(async move {
            let incoming = bob.accept().await.unwrap();
            let connection = incoming.await.unwrap();
            let (mut send, mut recv) = connection.accept_bi().await.unwrap();
            let envelope = read_document(&mut recv).await.unwrap();
            assert!(matches!(envelope.frame, ChatFrame::Text { .. }));
            let bogus = WireEnvelope::new(
                ChatFrame::text(MessageId::new([9; 16]), 0, "not a receipt").unwrap(),
            );
            write_document(&mut send, &bogus).await.unwrap();
            send.finish().unwrap();
            // Keep the connection alive until the peer has read the reply.
            let _ = send.stopped().await;
            tokio::time::sleep(Duration::from_millis(50)).await;
            connection
        })
    };

    let alice = local_peer(alice_secret, [bob_peer_id.clone()]).await;
    let mut bob_addr = bob.addr();
    bob_addr.addrs.retain(|transport_addr| {
        matches!(
            transport_addr,
            TransportAddr::Ip(socket) if socket.ip().is_loopback()
        )
    });
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while bob_addr.ip_addrs().next().is_none() {
        assert!(tokio::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
        bob_addr = bob.addr();
        bob_addr.addrs.retain(|transport_addr| {
            matches!(
                transport_addr,
                TransportAddr::Ip(socket) if socket.ip().is_loopback()
            )
        });
    }
    set_test_route(alice.transport.inner(), bob_endpoint_id, bob_addr).await;

    let result = alice
        .client
        .send_text(bob_peer_id.clone(), "hello")
        .await
        .unwrap()
        .wait()
        .await;
    assert!(
        matches!(result, Err(DeliveryError::ProtocolViolation)),
        "unexpected delivery result: {result:?}"
    );
    accept.await.unwrap();

    let accept2 = {
        let bob = bob.clone();
        tokio::spawn(async move {
            let incoming = bob.accept().await.unwrap();
            let connection = incoming.await.unwrap();
            let (mut send, mut recv) = connection.accept_bi().await.unwrap();
            let envelope = read_document(&mut recv).await.unwrap();
            let ChatFrame::Text { message_id, .. } = envelope.frame else {
                panic!("expected text");
            };
            let accepted = WireEnvelope::new(ChatFrame::accepted(message_id, 1));
            write_document(&mut send, &accepted).await.unwrap();
            send.finish().unwrap();
            let _ = send.stopped().await;
            tokio::time::sleep(Duration::from_millis(50)).await;
            connection
        })
    };
    let second = alice
        .client
        .send_text(bob_peer_id, "again")
        .await
        .unwrap()
        .wait()
        .await;
    assert!(second.is_ok());
    accept2.await.unwrap();

    alice.transport.shutdown().await.unwrap();
    bob.close().await;
}

#[tokio::test]
async fn removing_contact_cancels_queued_outbound_deliveries() {
    let (alice, mut bob) = local_pair().await;

    let mut handles = Vec::with_capacity(OUTGOING_QUEUE_CAPACITY);
    for index in 0..OUTGOING_QUEUE_CAPACITY {
        handles.push(
            alice
                .client
                .send_text(bob.peer_id.clone(), format!("queued-{index}"))
                .await
                .unwrap(),
        );
    }

    alice.transport.replace_contacts([]).await.unwrap();

    let mut not_a_contact = 0;
    for handle in handles {
        if matches!(handle.wait().await, Err(DeliveryError::NotAContact)) {
            not_a_contact += 1;
        }
    }
    assert!(
        not_a_contact > 0,
        "expected at least one queued delivery to be cancelled"
    );

    let mut received = 0;
    while tokio::time::timeout(Duration::from_millis(50), bob.incoming.recv())
        .await
        .is_ok()
    {
        received += 1;
    }
    assert!(
        received < OUTGOING_QUEUE_CAPACITY,
        "removed contact should not deliver the full queued batch"
    );

    alice.transport.shutdown().await.unwrap();
    bob.transport.shutdown().await.unwrap();
}

#[tokio::test]
async fn outbound_queue_full_returns_immediately() {
    let (alice, _bob) = local_pair().await;

    for index in 0..INCOMING_QUEUE_CAPACITY {
        alice
            .client
            .send_text(_bob.peer_id.clone(), format!("fill-in-{index}"))
            .await
            .unwrap();
    }

    tokio::time::sleep(Duration::from_millis(200)).await;

    alice
        .client
        .send_text(_bob.peer_id.clone(), "blocker")
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(50)).await;

    for index in 0..OUTGOING_QUEUE_CAPACITY {
        alice
            .client
            .send_text(_bob.peer_id.clone(), format!("fill-out-{index}"))
            .await
            .unwrap();
    }

    assert!(matches!(
        alice
            .client
            .send_text(_bob.peer_id.clone(), "overflow")
            .await,
        Err(DeliveryError::QueueFull)
    ));

    alice.transport.shutdown().await.unwrap();
    _bob.transport.shutdown().await.unwrap();
}

#[tokio::test]
async fn hung_dial_times_out_within_the_delivery_deadline() {
    let alice_secret = SecretKey::from_bytes(&[93; 32]);
    let bob_secret = SecretKey::from_bytes(&[94; 32]);
    let bob_id = peer_id_from_secret(&bob_secret);
    let bob_endpoint_id = bob_secret.public();

    let alice = local_peer(alice_secret, [bob_id.clone()]).await;

    let dead_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dead_addr = dead_listener.local_addr().unwrap();
    drop(dead_listener);

    let unreachable = EndpointAddr::from_parts(bob_endpoint_id, vec![TransportAddr::Ip(dead_addr)]);
    set_test_route(alice.transport.inner(), bob_endpoint_id, unreachable).await;

    let handle = alice.client.send_text(bob_id, "stuck").await.unwrap();
    let result = tokio::time::timeout(Duration::from_secs(2), handle.wait())
        .await
        .expect("delivery should not hang past test guard");
    assert!(matches!(result, Err(DeliveryError::TimedOut)));

    alice.transport.shutdown().await.unwrap();
}

#[tokio::test]
async fn inbound_handler_budget_rejects_extra_connections() {
    let alice = local_peer(SecretKey::from_bytes(&[95; 32]), []).await;
    let alice_addr = direct_loopback_addr(&alice.transport).await;
    let _budget = exhaust_inbound_handler_budget(alice.transport.inner()).await;
    assert_eq!(inbound_handler_budget(alice.transport.inner()), 0);

    let intruder = Endpoint::builder(presets::N0)
        .secret_key(SecretKey::from_bytes(&[96; 32]))
        .alpns(vec![CHAT_ALPN.to_vec()])
        .relay_mode(iroh::RelayMode::Disabled)
        .clear_ip_transports()
        .bind_addr("127.0.0.1:0")
        .unwrap()
        .bind()
        .await
        .unwrap();

    let connection = intruder.connect(alice_addr, CHAT_ALPN).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        connection.open_bi().await.is_err(),
        "server should close connections when the inbound handler budget is exhausted"
    );

    alice.transport.shutdown().await.unwrap();
    intruder.close().await;
}
