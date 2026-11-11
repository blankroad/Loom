//! DoIP M1 gate tests over loopback: client ↔ entity.

use std::net::SocketAddr;
use std::time::Duration;

use loom_uds_doip::frame::{diag_code, header_nack, payload_type};
use loom_uds_doip::{
    ActivationCode, ActivationType, ClientConfig, DiscoveryRequest, DoipClient, DoipEntity, DoipError,
    EntityConfig, EntityFault, LinkEvent, LinkState, Message, ProtocolVersion, VehicleIdFilter,
    collect_announcements, decode_frame, discover,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::time::{Instant, sleep};

const TESTER: u16 = 0x0E00;
const GATEWAY: u16 = 0x1000;
const ECU_A: u16 = 0x0E80;
const ECU_B: u16 = 0x0E81;
const VIN: &[u8; 17] = b"LOOMSIM0000000001";

/// Positive echo: `22 F190` → `62 F190 <VIN>`, anything else → `SID+0x40 <rest>` plus the target
/// LA's low byte, `10 03` is answered after 300 ms, `31 01 FF 00` → 40 replies.
fn handler(target: u16, _now: Duration, req: &[u8]) -> Vec<(Duration, Vec<u8>)> {
    let Some(&sid) = req.first() else {
        return Vec::new();
    };
    if req == [0x22, 0xF1, 0x90] {
        let mut r = vec![0x62, 0xF1, 0x90];
        r.extend_from_slice(VIN);
        return vec![(Duration::ZERO, r)];
    }
    if req == [0x10, 0x03] {
        return vec![(
            Duration::from_millis(300),
            vec![0x50, 0x03, 0x00, 0x32, 0x01, 0xF4],
        )];
    }
    if req == [0x31, 0x01, 0xFF, 0x00] {
        return (0..40u8)
            .map(|i| (Duration::ZERO, vec![0x7F, 0x31, 0x78, i]))
            .collect();
    }
    let mut r = vec![sid.wrapping_add(0x40)];
    r.extend_from_slice(&req[1..]);
    r.push(target.to_be_bytes()[1]);
    vec![(Duration::ZERO, r)]
}

async fn entity() -> DoipEntity {
    DoipEntity::start(EntityConfig::loopback(GATEWAY, vec![ECU_A, ECU_B]), handler)
        .await
        .expect("entity starts")
}

fn cfg(e: &DoipEntity) -> ClientConfig {
    ClientConfig::new(e.tcp_addr(), TESTER)
}

fn soon(ms: u64) -> Instant {
    Instant::now() + Duration::from_millis(ms)
}

#[tokio::test]
async fn loopback_roundtrip() {
    let e = entity().await;
    let (client, mut events) = DoipClient::connect(cfg(&e)).await.expect("connect");
    assert_eq!(client.entity_la(), Some(GATEWAY));
    assert!(matches!(
        events.recv().await,
        Some(LinkEvent::Activated {
            entity_la: GATEWAY,
            ..
        })
    ));
    let mut t = client.open_target(ECU_A).expect("open");
    let (handed, acked) = t.send(&[0x22, 0xF1, 0x90], soon(1000)).await.expect("send");
    assert!(acked >= handed);
    let rx = t.recv_until(soon(1000)).await.expect("response");
    assert_eq!(&rx.data[..3], &[0x62, 0xF1, 0x90]);
    assert_eq!(&rx.data[3..], VIN);
    assert_eq!(e.stats().activations, 1);
    assert!(matches!(
        client.open_target(ECU_A),
        Err(DoipError::TargetInUse(ECU_A))
    ));
}

#[tokio::test]
async fn protocol_version_2019_roundtrip() {
    let mut ec = EntityConfig::loopback(GATEWAY, vec![ECU_A]);
    ec.version = ProtocolVersion::Iso2019;
    let e = DoipEntity::start(ec, handler).await.expect("entity");
    let mut c = cfg(&e);
    c.version = ProtocolVersion::Iso2019;
    let (client, _ev) = DoipClient::connect(c).await.expect("connect");
    let mut t = client.open_target(ECU_A).expect("open");
    t.send(&[0x3E, 0x00], soon(1000)).await.expect("send");
    let rx = t.recv_until(soon(1000)).await.expect("response");
    assert_eq!(rx.data, vec![0x7E, 0x00, 0x80]);
}

#[tokio::test]
async fn activation_response_codes() {
    let e = entity().await;
    for code in [0x00u8, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x11, 0xE5] {
        e.set_activation_code(Some(ActivationCode(code)));
        match DoipClient::connect(cfg(&e)).await {
            Err(DoipError::ActivationDenied(c)) => assert_eq!(c.0, code),
            other => panic!("code 0x{code:02X}: expected denial, got {other:?}"),
        }
    }
    e.set_activation_code(None);
    DoipClient::connect(cfg(&e))
        .await
        .expect("normal activation succeeds");
    assert_eq!(
        ActivationCode(0x02).name(),
        "denied_source_address_mismatch_on_socket"
    );
}

#[tokio::test]
async fn unsupported_activation_type_gets_0x06() {
    let e = entity().await;
    let mut c = cfg(&e);
    c.activation_type = ActivationType(0x02);
    match DoipClient::connect(c).await {
        Err(DoipError::ActivationDenied(code)) => assert_eq!(code, ActivationCode::DENIED_UNSUPPORTED_TYPE),
        other => panic!("expected 0x06, got {other:?}"),
    }
}

#[tokio::test]
async fn central_security_0xe0_accepted() {
    let e = entity().await;
    let mut c = cfg(&e);
    c.activation_type = ActivationType::CENTRAL_SECURITY;
    c.oem = Some([0xDE, 0xAD, 0xBE, 0xEF]);
    let (client, _ev) = DoipClient::connect(c).await.expect("0xE0 activation accepted");
    assert_eq!(e.stats().activations, 1);
    let mut t = client.open_target(ECU_A).expect("open");
    t.send(&[0x22, 0xF1, 0x90], soon(1000)).await.expect("send");
    t.recv_until(soon(1000)).await.expect("response");
}

#[tokio::test]
async fn same_tester_on_second_socket_gets_0x03() {
    let e = entity().await;
    let (_first, _ev) = DoipClient::connect(cfg(&e)).await.expect("first");
    match DoipClient::connect(cfg(&e)).await {
        Err(DoipError::ActivationDenied(c)) => {
            assert_eq!(c, ActivationCode::DENIED_SA_ACTIVE_ON_OTHER_SOCKET)
        }
        other => panic!("expected 0x03, got {other:?}"),
    }
}

#[tokio::test]
async fn alive_check_answered_while_waiting_for_response() {
    let e = entity().await;
    let (client, _ev) = DoipClient::connect(cfg(&e)).await.expect("connect");
    let mut t = client.open_target(ECU_A).expect("open");
    // 10 03 is answered after 300 ms.
    t.send(&[0x10, 0x03], soon(1000)).await.expect("send");
    sleep(Duration::from_millis(50)).await;
    e.send_alive_check();
    let deadline = soon(250);
    while e.stats().alive_check_responses == 0 {
        assert!(
            Instant::now() < deadline,
            "alive check not answered while a response is pending"
        );
        sleep(Duration::from_millis(5)).await;
    }
    assert!(
        t.try_recv().is_none(),
        "the diagnostic response must still be pending"
    );
    assert_eq!(client.stats().alive_checks_answered, 1);
    let rx = t.recv_until(soon(1000)).await.expect("delayed response");
    assert_eq!(rx.data[..2], [0x50, 0x03]);
}

#[tokio::test]
async fn entity_rejects_u32_max_length_header() {
    let e = entity().await;
    let mut s = TcpStream::connect(e.tcp_addr()).await.expect("connect");
    s.write_all(&[0x02, 0xFD, 0x80, 0x01, 0xFF, 0xFF, 0xFF, 0xFF])
        .await
        .expect("write");
    let mut nack = [0u8; 9];
    s.read_exact(&mut nack).await.expect("generic NACK");
    assert_eq!(
        nack,
        [
            0x02,
            0xFD,
            0x00,
            0x00,
            0x00,
            0x00,
            0x00,
            0x01,
            header_nack::MESSAGE_TOO_LARGE
        ]
    );
    let mut rest = [0u8; 1];
    let n = tokio::time::timeout(Duration::from_secs(1), s.read(&mut rest))
        .await
        .expect("closed promptly")
        .expect("read");
    assert_eq!(n, 0, "entity closes the socket");
    assert_eq!(e.stats().header_nacks, 1);
}

/// A hostile entity: activates normally, then sends a `u32::MAX` length header.
#[tokio::test]
async fn client_rejects_u32_max_length_header() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (go_tx, go_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        let (mut s, _) = listener.accept().await.expect("accept");
        let mut req = [0u8; 15];
        s.read_exact(&mut req).await.expect("RA request");
        let resp = Message::RoutingActivationResponse {
            tester: TESTER,
            entity: GATEWAY,
            code: ActivationCode::SUCCESS,
            oem: None,
        }
        .encode(ProtocolVersion::Iso2012)
        .expect("encode");
        s.write_all(&resp).await.expect("RA response");
        go_rx.await.expect("go signal");
        s.write_all(&[0x02, 0xFD, 0x80, 0x01, 0xFF, 0xFF, 0xFF, 0xFF])
            .await
            .expect("evil header");
        let mut nack = [0u8; 9];
        s.read_exact(&mut nack).await.expect("client NACK");
        nack
    });
    let (client, mut events) = DoipClient::connect(ClientConfig::new(addr, TESTER))
        .await
        .expect("connect");
    let mut t = client.open_target(ECU_A).expect("open");
    go_tx.send(()).expect("server waiting");
    let mut saw_error = false;
    while let Some(ev) = tokio::time::timeout(Duration::from_secs(1), events.recv())
        .await
        .expect("event")
    {
        if let LinkEvent::Error { reason } = ev {
            assert!(reason.contains("exceeds"), "{reason}");
            saw_error = true;
            break;
        }
    }
    assert!(saw_error);
    assert!(client.is_closed());
    assert_eq!(*client.link_state().borrow(), LinkState::Closed);
    assert!(t.recv().await.is_none());
    let nack = server.await.expect("server");
    assert_eq!(nack[8], header_nack::MESSAGE_TOO_LARGE);
}

#[tokio::test]
async fn unknown_payload_type_is_discarded_connection_kept() {
    let e = entity().await;
    let mut s = TcpStream::connect(e.tcp_addr()).await.expect("connect");
    s.write_all(&[0x02, 0xFD, 0x12, 0x34, 0x00, 0x00, 0x00, 0x03, 1, 2, 3])
        .await
        .expect("write");
    let mut nack = [0u8; 9];
    s.read_exact(&mut nack).await.expect("NACK");
    assert_eq!(nack[8], header_nack::UNKNOWN_PAYLOAD_TYPE);
    let req = Message::EntityStatusRequest
        .encode(ProtocolVersion::Iso2012)
        .expect("encode");
    s.write_all(&req).await.expect("status request");
    let mut head = [0u8; 8];
    s.read_exact(&mut head).await.expect("status response");
    assert_eq!(
        u16::from_be_bytes([head[2], head[3]]),
        payload_type::ENTITY_STATUS_RESPONSE
    );
}

#[tokio::test]
async fn diagnostic_before_activation_is_nacked_and_closed() {
    let e = entity().await;
    let mut s = TcpStream::connect(e.tcp_addr()).await.expect("connect");
    let diag = Message::DiagnosticMessage {
        source: TESTER,
        target: ECU_A,
        data: vec![0x3E, 0x00],
    }
    .encode(ProtocolVersion::Iso2012)
    .expect("encode");
    s.write_all(&diag).await.expect("write");
    let mut buf = vec![0u8; 64];
    let mut got = Vec::new();
    loop {
        let n = s.read(&mut buf).await.expect("read");
        if n == 0 {
            break;
        }
        got.extend_from_slice(&buf[..n]);
    }
    let (_, msg, _) = decode_frame(&got).expect("valid").expect("one frame");
    assert!(matches!(
        msg,
        Message::DiagnosticNack {
            code: diag_code::INVALID_SOURCE_ADDRESS,
            ..
        }
    ));
}

#[tokio::test]
async fn discovery_unicast_and_filters() {
    let e = entity().await;
    let udp = e.udp_addr().expect("udp");
    let bind: SocketAddr = "127.0.0.1:0".parse().expect("addr");
    let mut req = DiscoveryRequest {
        dest: udp,
        bind,
        filter: VehicleIdFilter::All,
        timeout: Duration::from_millis(300),
        max_results: 1,
    };
    let found = discover(&req).await.expect("discover");
    assert_eq!(found.len(), 1);
    assert_eq!(&found[0].announcement.vin, VIN);
    assert_eq!(found[0].announcement.logical_address, GATEWAY);

    req.filter = VehicleIdFilter::Vin(*VIN);
    assert_eq!(discover(&req).await.expect("vin").len(), 1);
    req.filter = VehicleIdFilter::Eid([9; 6]);
    assert!(
        discover(&req).await.expect("eid").is_empty(),
        "EID mismatch: no answer"
    );
}

#[tokio::test]
async fn announcements_are_collected() {
    let e = entity().await;
    let sock = UdpSocket::bind("127.0.0.1:0").await.expect("bind");
    let me = sock.local_addr().expect("addr");
    e.send_announcement(me).await.expect("announce");
    e.send_announcement(me).await.expect("announce twice");
    let found = collect_announcements(&sock, soon(200), 8).await.expect("collect");
    assert_eq!(found.len(), 1, "duplicates merged");
    assert_eq!(found[0].announcement.eid, e.announcement().eid);
}

#[tokio::test]
async fn socket_close_emits_link_event() {
    let e = entity().await;
    let (client, mut events) = DoipClient::connect(cfg(&e)).await.expect("connect");
    let mut t = client.open_target(ECU_A).expect("open");
    assert!(matches!(events.recv().await, Some(LinkEvent::Activated { .. })));
    e.close_all_connections();
    let ev = tokio::time::timeout(Duration::from_secs(1), events.recv())
        .await
        .expect("event in time");
    assert!(matches!(ev, Some(LinkEvent::Closed { .. })), "{ev:?}");
    assert!(t.recv().await.is_none());
    assert!(matches!(
        t.submit(&[0x3E, 0x00], soon(100)).await,
        Err(DoipError::Closed)
    ));
    assert!(matches!(client.open_target(ECU_B), Err(DoipError::Closed)));
}

#[tokio::test]
async fn fault_close_on_next_diag() {
    let e = entity().await;
    let (client, _ev) = DoipClient::connect(cfg(&e)).await.expect("connect");
    let t = client.open_target(ECU_A).expect("open");
    e.inject(EntityFault::CloseOnNextDiag).expect("inject");
    let r = t.send(&[0x3E, 0x00], soon(1000)).await;
    assert!(matches!(r, Err(DoipError::Closed)), "{r:?}");
}

#[tokio::test]
async fn nack_and_missing_ack() {
    let e = entity().await;
    let mut c = cfg(&e);
    c.ack_timeout = Duration::from_millis(150);
    let (client, _ev) = DoipClient::connect(c).await.expect("connect");
    let t = client.open_target(ECU_A).expect("open");
    e.inject(EntityFault::NackNextDiag(diag_code::TARGET_UNREACHABLE))
        .expect("inject");
    assert!(matches!(
        t.send(&[0x3E, 0x00], soon(1000)).await,
        Err(DoipError::Nack(diag_code::TARGET_UNREACHABLE))
    ));
    e.inject(EntityFault::DropNextAck).expect("inject");
    assert!(matches!(
        t.send(&[0x3E, 0x00], soon(1000)).await,
        Err(DoipError::AckTimeout)
    ));
    // An uncertain ACK has no SID/request ID: reuse requires a fresh activated connection.
    assert!(matches!(
        t.send(&[0x3E, 0x00], soon(1000)).await,
        Err(DoipError::AckPending(_))
    ));
    drop(t);
    drop(client);
    tokio::time::timeout(Duration::from_secs(1), async {
        while e.open_connections() != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("old socket closed");
    let (client, _ev) = DoipClient::connect(cfg(&e))
        .await
        .expect("reconnect and reactivate");
    let mut t = client.open_target(ECU_A).expect("new target");
    t.send(&[0x3E, 0x00], soon(1000)).await.expect("normal send");
    assert_eq!(t.recv_until(soon(1000)).await.expect("rx").data[0], 0x7E);
}

#[tokio::test]
async fn unknown_target_is_nacked() {
    let e = entity().await;
    let (client, _ev) = DoipClient::connect(cfg(&e)).await.expect("connect");
    let t = client.open_target(0x0EFF).expect("open");
    assert!(matches!(
        t.send(&[0x3E, 0x00], soon(1000)).await,
        Err(DoipError::Nack(diag_code::UNKNOWN_TARGET_ADDRESS))
    ));
}

#[tokio::test]
async fn shared_socket_routes_by_source_la() {
    let e = entity().await;
    let (client, _ev) = DoipClient::connect(cfg(&e)).await.expect("connect");
    let mut a = client.open_target(ECU_A).expect("a");
    let mut b = client.open_target(ECU_B).expect("b");
    a.send(&[0x3E, 0x00], soon(1000)).await.expect("send a");
    b.send(&[0x3E, 0x00], soon(1000)).await.expect("send b");
    let ra = a.recv_until(soon(1000)).await.expect("a rx");
    let rb = b.recv_until(soon(1000)).await.expect("b rx");
    assert_eq!(ra.data.last(), Some(&0x80));
    assert_eq!(rb.data.last(), Some(&0x81));
    assert!(a.try_recv().is_none() && b.try_recv().is_none());
}

#[tokio::test]
async fn rx_queue_is_bounded() {
    let e = entity().await;
    let (client, mut events) = DoipClient::connect(cfg(&e)).await.expect("connect");
    let mut t = client.open_target(ECU_A).expect("open");
    t.send(&[0x31, 0x01, 0xFF, 0x00], soon(1000)).await.expect("send");
    let mut overflow = None;
    while let Ok(Some(ev)) = tokio::time::timeout(Duration::from_millis(500), events.recv()).await {
        if let LinkEvent::RxOverflow { dropped_total, .. } = ev {
            overflow = Some(dropped_total);
        }
        if overflow == Some(40 - loom_uds_doip::client::CAP_RX_PER_TARGET as u64) {
            break;
        }
    }
    assert_eq!(
        overflow,
        Some(40 - loom_uds_doip::client::CAP_RX_PER_TARGET as u64)
    );
    let mut n = 0;
    while t.try_recv().is_some() {
        n += 1;
    }
    assert_eq!(n, loom_uds_doip::client::CAP_RX_PER_TARGET);
}

#[tokio::test]
async fn reactivation_emits_event() {
    let e = entity().await;
    let (client, mut events) = DoipClient::connect(cfg(&e)).await.expect("connect");
    assert!(matches!(events.recv().await, Some(LinkEvent::Activated { .. })));
    client.reactivate().await.expect("reactivate");
    assert!(matches!(events.recv().await, Some(LinkEvent::Reactivated { .. })));
}

#[test]
fn split_and_coalesced_frames() {
    let one = Message::DiagnosticMessage {
        source: TESTER,
        target: ECU_A,
        data: vec![0x22, 0xF1, 0x90],
    }
    .encode(ProtocolVersion::Iso2012)
    .expect("encode");
    assert!(decode_frame(&one[..5]).expect("partial header").is_none());
    assert!(decode_frame(&one[..10]).expect("partial body").is_none());
    let mut two = one.clone();
    two.extend_from_slice(&one);
    let (_, m1, used) = decode_frame(&two).expect("ok").expect("first");
    let (_, m2, used2) = decode_frame(&two[used..]).expect("ok").expect("second");
    assert_eq!(m1, m2);
    assert_eq!(used + used2, two.len());
}

#[test]
fn every_message_roundtrips() {
    let msgs = vec![
        Message::GenericNack { code: 4 },
        Message::VehicleIdRequest,
        Message::VehicleIdRequestEid { eid: [1; 6] },
        Message::VehicleIdRequestVin { vin: *VIN },
        Message::RoutingActivationRequest {
            source: TESTER,
            activation_type: ActivationType::CENTRAL_SECURITY,
            oem: Some([1, 2, 3, 4]),
        },
        Message::RoutingActivationResponse {
            tester: TESTER,
            entity: GATEWAY,
            code: ActivationCode::SUCCESS,
            oem: None,
        },
        Message::AliveCheckRequest,
        Message::AliveCheckResponse { source: TESTER },
        Message::EntityStatusRequest,
        Message::EntityStatusResponse {
            node_type: 0,
            max_sockets: 4,
            open_sockets: 1,
            max_data_size: Some(4099),
        },
        Message::PowerModeRequest,
        Message::PowerModeResponse { mode: 1 },
        Message::DiagnosticAck {
            source: ECU_A,
            target: TESTER,
            code: 0,
            previous: vec![0x22],
        },
        Message::DiagnosticNack {
            source: ECU_A,
            target: TESTER,
            code: 3,
            previous: vec![],
        },
    ];
    for m in msgs {
        let f = m.encode(ProtocolVersion::Iso2019).expect("encode");
        let (h, back, used) = decode_frame(&f).expect("valid").expect("complete");
        assert_eq!(h.version, 0x03);
        assert_eq!(back, m);
        assert_eq!(used, f.len());
    }
    let too_big = Message::DiagnosticMessage {
        source: 0,
        target: 0,
        data: vec![0; loom_uds_doip::MAX_UDS_PDU + 1],
    };
    assert!(too_big.encode(ProtocolVersion::Iso2012).is_err());
}

// ---- T_TCP_General_Inactivity (ISO 13400-2, entity side) ----

#[tokio::test]
async fn t_tcp_inactivity_closes_idle_connection() {
    let e = entity().await;
    e.set_tcp_general_inactivity(Some(Duration::from_millis(200)));
    let (client, mut events) = DoipClient::connect(cfg(&e)).await.expect("connect");
    let start = Instant::now();
    assert!(matches!(events.recv().await, Some(LinkEvent::Activated { .. })));
    let ev = tokio::time::timeout(Duration::from_secs(2), events.recv())
        .await
        .expect("closed in time");
    assert!(matches!(ev, Some(LinkEvent::Closed { .. })), "{ev:?}");
    assert!(
        start.elapsed() >= Duration::from_millis(150),
        "{:?}",
        start.elapsed()
    );
    assert!(client.is_closed());
    assert_eq!(e.stats().inactivity_closes, 1);
}

#[tokio::test]
async fn alive_check_responses_alone_keep_the_connection_only_while_the_entity_asks() {
    let e = entity().await;
    e.set_tcp_general_inactivity(Some(Duration::from_millis(300)));
    let (client, mut events) = DoipClient::connect(cfg(&e)).await.expect("connect");
    assert!(matches!(events.recv().await, Some(LinkEvent::Activated { .. })));
    // No diagnostics: the entity asks every 100 ms, the client answers → well past 300 ms open.
    for _ in 0..8 {
        sleep(Duration::from_millis(100)).await;
        e.send_alive_check();
    }
    assert!(!client.is_closed(), "alive check responses reset the timer");
    assert_eq!(e.stats().inactivity_closes, 0);
    assert!(client.stats().alive_checks_answered >= 7);
    // Diagnostic traffic resets it too.
    let mut t = client.open_target(ECU_A).expect("open");
    for _ in 0..4 {
        sleep(Duration::from_millis(150)).await;
        t.send(&[0x3E, 0x00], soon(1000)).await.expect("send");
        t.recv_until(soon(1000)).await.expect("response");
    }
    assert!(!client.is_closed());
    // The entity stops asking and the tester sends nothing: closed after the timer.
    let ev = tokio::time::timeout(Duration::from_secs(2), events.recv())
        .await
        .expect("closed in time");
    assert!(matches!(ev, Some(LinkEvent::Closed { .. })), "{ev:?}");
    assert_eq!(e.stats().inactivity_closes, 1);
}
