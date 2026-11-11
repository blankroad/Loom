//! P1 regression cases: each must reject ambiguity before reporting a new successful operation.

use std::future::{Future, poll_fn};
use std::task::Poll;
use std::time::Duration;

use loom_uds_doip::{
    ActivationCode, ClientConfig, DoipClient, DoipEntity, DoipError, EntityConfig, HEADER_LEN, Message,
    ProtocolVersion, parse_header,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{Instant, timeout};

const TESTER: u16 = 0x0e80;
const ECU: u16 = 0x1001;
const ECU_B: u16 = 0x1002;
const ECU_C: u16 = 0x1003;
const MAX_TEST_WAIT: Duration = Duration::from_secs(2);

async fn read_message(stream: &mut TcpStream) -> Message {
    timeout(MAX_TEST_WAIT, async {
        let mut head = [0; HEADER_LEN];
        stream.read_exact(&mut head).await.expect("peer header");
        let header = parse_header(&head).expect("bounded header");
        let mut payload = vec![0; header.payload_len];
        stream.read_exact(&mut payload).await.expect("peer payload");
        Message::decode(header.payload_type, &payload).expect("peer message")
    })
    .await
    .expect("bounded peer read")
}

async fn write_message(stream: &mut TcpStream, message: Message) {
    let bytes = message
        .encode(ProtocolVersion::Iso2012)
        .expect("encode bounded fixture");
    timeout(MAX_TEST_WAIT, stream.write_all(&bytes))
        .await
        .expect("bounded peer write")
        .expect("write");
}

async fn activate_peer(stream: &mut TcpStream, tester: u16) {
    assert!(matches!(
        read_message(stream).await,
        Message::RoutingActivationRequest { .. }
    ));
    write_message(
        stream,
        Message::RoutingActivationResponse {
            tester,
            entity: 0x1000,
            code: ActivationCode::SUCCESS,
            oem: None,
        },
    )
    .await;
}

async fn entity() -> DoipEntity {
    DoipEntity::start(
        EntityConfig::loopback(0x1000, vec![ECU]),
        |_la, _now, _req: &[u8]| vec![(Duration::ZERO, vec![0x6e, 0xf1, 0xa0])],
    )
    .await
    .expect("entity")
}

#[tokio::test]
async fn expired_submit_never_reaches_entity() {
    let entity = entity().await;
    let (client, _events) = DoipClient::connect(ClientConfig::new(entity.tcp_addr(), TESTER))
        .await
        .expect("connect");
    let target = client.open_target(ECU).expect("target");
    let result = target
        .submit(&[0x2e, 0xf1, 0xa0, 1], Instant::now() - Duration::from_millis(1))
        .await;
    assert!(matches!(result, Err(DoipError::TxTimeout)));
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(
        entity.stats().diag_received,
        0,
        "expired queued TX must not be written later"
    );
}

#[tokio::test]
async fn submit_expiring_while_queued_is_rejected_before_write() {
    let entity = entity().await;
    let (client, _events) = DoipClient::connect(ClientConfig::new(entity.tcp_addr(), TESTER))
        .await
        .expect("connect");
    let target = client.open_target(ECU).expect("target");
    let mut submit =
        Box::pin(target.submit(&[0x2e, 0xf1, 0xa0, 1], Instant::now() + Duration::from_millis(5)));
    assert!(poll_fn(|cx| Poll::Ready(submit.as_mut().poll(cx).is_pending())).await);
    // Hold this current-thread runtime until the queued frame's deadline is already past.
    std::thread::sleep(Duration::from_millis(15));
    tokio::task::yield_now().await;
    assert!(matches!(submit.await, Err(DoipError::TxTimeout)));
    assert_eq!(entity.stats().diag_received, 0);
    target
        .send(&[0x3e, 0], Instant::now() + MAX_TEST_WAIT)
        .await
        .expect("unused ACK slot remains reusable");
    assert_eq!(entity.stats().diag_received, 1);
}

#[tokio::test]
async fn cancelled_queued_submit_never_reaches_entity_and_slot_is_reusable() {
    let entity = entity().await;
    let (client, _events) = DoipClient::connect(ClientConfig::new(entity.tcp_addr(), TESTER))
        .await
        .expect("connect");
    let mut target = client.open_target(ECU).expect("target");
    let until = Instant::now() + MAX_TEST_WAIT;
    let mut submitted = Box::pin(target.submit(&[0x2e, 0xf1, 0xa0, 1], until));
    assert!(poll_fn(|cx| Poll::Ready(submitted.as_mut().poll(cx).is_pending())).await);
    drop(submitted);
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(
        entity.stats().diag_received,
        0,
        "drop before writer handoff must cancel TX"
    );
    target
        .send(&[0x2e, 0xf1, 0xa0, 2], Instant::now() + MAX_TEST_WAIT)
        .await
        .expect("safe new request");
    let _ = target
        .recv_until(Instant::now() + MAX_TEST_WAIT)
        .await
        .expect("response");
    assert_eq!(entity.stats().diag_received, 1);
}

#[tokio::test]
async fn wrong_tester_routing_activation_is_rejected() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listen");
    let addr = listener.local_addr().expect("addr");
    let server = tokio::spawn(async move {
        let (mut peer, _) = listener.accept().await.expect("accept");
        activate_peer(&mut peer, TESTER + 1).await;
        let mut byte = [0];
        timeout(MAX_TEST_WAIT, peer.read(&mut byte))
            .await
            .expect("closed promptly")
            .expect("peer read")
    });
    let result = DoipClient::connect(ClientConfig::new(addr, TESTER)).await;
    assert!(
        result.is_err(),
        "a response for another tester cannot activate this client"
    );
    assert_eq!(server.await.expect("server"), 0);
}

#[tokio::test]
async fn cancelled_reactivate_closes_ambiguous_attempt_instead_of_sticking_busy() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listen");
    let addr = listener.local_addr().expect("addr");
    let server = tokio::spawn(async move {
        let (mut peer, _) = listener.accept().await.expect("accept");
        activate_peer(&mut peer, TESTER).await;
        let _ = read_message(&mut peer).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        // An old on-wire activation has no request ID. It must never activate a newer attempt.
        let late = Message::RoutingActivationResponse {
            tester: TESTER,
            entity: 0x1000,
            code: ActivationCode::SUCCESS,
            oem: None,
        }
        .encode(ProtocolVersion::Iso2012)
        .expect("late RA");
        let _ = timeout(MAX_TEST_WAIT, peer.write_all(&late)).await;
    });
    let (client, _events) = DoipClient::connect(ClientConfig::new(addr, TESTER))
        .await
        .expect("connect");
    assert!(
        timeout(Duration::from_millis(10), client.reactivate())
            .await
            .is_err()
    );
    assert!(
        client.is_closed(),
        "cancelled on-wire RA must discard its source socket"
    );
    assert!(matches!(client.reactivate().await, Err(DoipError::Closed)));
    server.await.expect("server");
}

#[tokio::test]
async fn late_ack_cannot_confirm_a_different_sid_after_ack_timeout() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listen");
    let addr = listener.local_addr().expect("addr");
    let server = tokio::spawn(async move {
        let (mut peer, _) = listener.accept().await.expect("accept");
        activate_peer(&mut peer, TESTER).await;
        let _ = read_message(&mut peer).await;
        tokio::time::sleep(Duration::from_millis(60)).await;
        write_message(
            &mut peer,
            Message::DiagnosticAck {
                source: ECU,
                target: TESTER,
                code: 0,
                previous: vec![],
            },
        )
        .await;
        assert!(matches!(
            read_message(&mut peer).await,
            Message::DiagnosticMessage { target: ECU_B, .. }
        ));
        write_message(
            &mut peer,
            Message::DiagnosticAck {
                source: ECU_B,
                target: TESTER,
                code: 0,
                previous: vec![],
            },
        )
        .await;
    });
    let mut cfg = ClientConfig::new(addr, TESTER);
    cfg.ack_timeout = Duration::from_millis(10);
    let (client, _events) = DoipClient::connect(cfg).await.expect("connect");
    let target = client.open_target(ECU).expect("target");
    assert!(matches!(
        target
            .send(&[0x22, 0xf1, 0x90], Instant::now() + MAX_TEST_WAIT)
            .await,
        Err(DoipError::AckTimeout)
    ));
    let second = target.submit(&[0x3e, 0x00], Instant::now() + MAX_TEST_WAIT).await;
    assert!(
        matches!(second, Err(DoipError::AckPending(_)) | Err(DoipError::Closed)),
        "uncertain LA cannot be reused by another SID"
    );
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert!(
        matches!(
            target.submit(&[0x3e, 0x00], Instant::now() + MAX_TEST_WAIT).await,
            Err(DoipError::AckPending(_)) | Err(DoipError::Closed)
        ),
        "late ACK is not recovery evidence"
    );
    assert!(!client.is_closed(), "unrelated LA remains usable");
    let other = client.open_target(ECU_B).expect("other LA");
    other
        .send(&[0x3e, 0], Instant::now() + MAX_TEST_WAIT)
        .await
        .expect("other LA ACK");
    server.await.expect("server");
}

#[tokio::test]
async fn cancelled_queued_reactivation_removes_only_its_own_attempt() {
    let entity = entity().await;
    let (client, _events) = DoipClient::connect(ClientConfig::new(entity.tcp_addr(), TESTER))
        .await
        .expect("connect");
    let mut activation = Box::pin(client.reactivate());
    assert!(poll_fn(|cx| Poll::Ready(activation.as_mut().poll(cx).is_pending())).await);
    drop(activation);
    assert!(!client.is_closed(), "queued RA emitted no bytes");
    client.reactivate().await.expect("new attempt is not stuck busy");
    assert_eq!(
        entity.stats().activations,
        2,
        "cancelled queued RA was not written"
    );
}

#[tokio::test]
async fn rx_byte_quotas_release_on_receive_and_drop_and_health_survives_event_overflow() {
    use loom_uds_doip::client::{MAX_RX_BYTES_PER_CONNECTION, MAX_RX_BYTES_PER_TARGET};
    use loom_uds_doip::{ClientHealth, LinkEvent, MAX_UDS_PDU};
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listen");
    let addr = listener.local_addr().expect("addr");
    let (ready, ready_rx) = tokio::sync::oneshot::channel();
    let (resume, resume_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut peer, _) = listener.accept().await.expect("accept");
        activate_peer(&mut peer, TESTER).await;
        let _ = read_message(&mut peer).await;
        write_message(
            &mut peer,
            Message::DiagnosticAck {
                source: ECU,
                target: TESTER,
                code: 0,
                previous: vec![],
            },
        )
        .await;
        for source in [ECU, ECU_B, ECU_C] {
            for _ in 0..20 {
                write_message(
                    &mut peer,
                    Message::DiagnosticMessage {
                        source,
                        target: TESTER,
                        data: vec![0x62; MAX_UDS_PDU],
                    },
                )
                .await;
            }
        }
        // TCP-order fence: alive replies must still work with all RX byte quotas full.
        write_message(&mut peer, Message::AliveCheckRequest).await;
        assert!(matches!(
            read_message(&mut peer).await,
            Message::AliveCheckResponse { .. }
        ));
        ready.send(()).expect("reader fence");
        timeout(MAX_TEST_WAIT, resume_rx)
            .await
            .expect("resume bounded")
            .expect("resume");
        write_message(
            &mut peer,
            Message::DiagnosticMessage {
                source: ECU_C,
                target: TESTER,
                data: vec![0x62; MAX_UDS_PDU],
            },
        )
        .await;
    });
    let (client, mut events) = DoipClient::connect(ClientConfig::new(addr, TESTER))
        .await
        .expect("connect");
    let mut a = client.open_target(ECU).expect("A");
    let b = client.open_target(ECU_B).expect("B");
    let mut c = client.open_target(ECU_C).expect("C");
    a.send(&[0x22, 0xf1, 0x90], Instant::now() + MAX_TEST_WAIT)
        .await
        .expect("ACK");
    timeout(MAX_TEST_WAIT, ready_rx)
        .await
        .expect("bounded fence")
        .expect("fence");
    assert!(a.buffered_bytes() <= MAX_RX_BYTES_PER_TARGET);
    assert!(b.buffered_bytes() <= MAX_RX_BYTES_PER_TARGET);
    assert!(client.stats().rx_buffered_bytes <= MAX_RX_BYTES_PER_CONNECTION);
    assert_eq!(c.buffered_bytes(), 0, "connection byte quota blocks third target");
    assert!(client.stats().rx_dropped_total > 0);
    assert!(client.stats().dropped_link_events > 0);
    assert_eq!(*client.health().borrow(), ClientHealth::RxOverflowed);
    let mut saw_overflow = false;
    while let Ok(event) = events.try_recv() {
        saw_overflow |= matches!(event, LinkEvent::RxOverflow { .. });
    }
    assert!(saw_overflow);
    let before = client.stats().rx_buffered_bytes;
    let received = a.try_recv().expect("queued A");
    assert_eq!(client.stats().rx_buffered_bytes, before - received.data.len());
    resume.send(()).expect("resume peer");
    assert_eq!(
        c.recv_until(Instant::now() + MAX_TEST_WAIT)
            .await
            .expect("reclaimed quota reusable")
            .data
            .len(),
        MAX_UDS_PDU
    );
    drop((a, b, c));
    assert_eq!(
        client.stats().rx_buffered_bytes,
        0,
        "drop releases every reservation"
    );
    server.await.expect("server");
}

#[tokio::test]
async fn dropping_handed_ack_wait_poisoned_la_cannot_be_reopened() {
    let entity = entity().await;
    entity
        .inject(loom_uds_doip::EntityFault::DropNextAck)
        .expect("fault");
    let (client, _events) = DoipClient::connect(ClientConfig::new(entity.tcp_addr(), TESTER))
        .await
        .expect("connect");
    let target = client.open_target(ECU).expect("target");
    let ack = target
        .submit(&[0x22, 0xf1, 0x90], Instant::now() + MAX_TEST_WAIT)
        .await
        .expect("handed");
    drop(ack);
    client
        .reactivate()
        .await
        .expect("same socket RA is permitted but is not ACK recovery");
    assert!(matches!(
        target.submit(&[0x3e, 0], Instant::now() + MAX_TEST_WAIT).await,
        Err(DoipError::AckPending(_)) | Err(DoipError::Closed)
    ));
    drop(target);
    assert!(
        client.open_target(ECU).is_err(),
        "drop/reopen must not erase an uncertain ACK"
    );
}

#[tokio::test]
async fn old_completed_ack_guard_cannot_remove_a_new_attempt_waiter() {
    let entity = entity().await;
    let (client, _events) = DoipClient::connect(ClientConfig::new(entity.tcp_addr(), TESTER))
        .await
        .expect("connect");
    let mut target = client.open_target(ECU).expect("target");
    let first_ack = target
        .submit(&[0x2e, 0xf1, 0xa0, 1], Instant::now() + MAX_TEST_WAIT)
        .await
        .expect("first handoff");
    // The final diagnostic response fences the first ACK on this TCP stream, but its AckWait
    // guard intentionally remains alive when a second attempt is installed.
    let _ = target
        .recv_until(Instant::now() + MAX_TEST_WAIT)
        .await
        .expect("first response");
    let mut second = Box::pin(target.submit(&[0x2e, 0xf1, 0xa0, 2], Instant::now() + MAX_TEST_WAIT));
    assert!(poll_fn(|cx| Poll::Ready(second.as_mut().poll(cx).is_pending())).await);
    drop(first_ack);
    second
        .await
        .expect("second handoff")
        .wait(Instant::now() + MAX_TEST_WAIT)
        .await
        .expect("new waiter was not removed by old cleanup");
    assert_eq!(entity.stats().diag_received, 2);
}
