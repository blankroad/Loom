use loom_eth_capture::*;
use loom_eth_pcapng::{Packet, PcapNgWriter};
use std::{
    io::Cursor,
    thread,
    time::{Duration, Instant},
};
fn interface() -> Interface {
    Interface {
        linktype: 1,
        snaplen: 65535,
        name: "test".into(),
        tsresol: TimestampResolution::NANO,
    }
}
fn fixture(n: usize, size: usize, interval: u128) -> Vec<u8> {
    let mut w = PcapNgWriter::new(Vec::new()).expect("SHB");
    w.write_interface(&interface()).expect("IDB");
    for i in 0..n {
        w.write_packet(&Packet {
            ts_ns: i as u128 * interval,
            caplen: size as u32,
            origlen: size as u32,
            data: vec![i as u8; size],
            ..Packet::default()
        })
        .expect("EPB");
    }
    w.into_inner()
}
fn config(max_frames: usize, max_bytes: usize) -> CaptureConfig {
    CaptureConfig {
        queue: QueueConfig {
            max_frames,
            max_bytes,
        },
        first_seq: 1,
    }
}
fn raw(n: usize) -> RawFrame {
    RawFrame {
        ts_ns: 0,
        caplen: n as u32,
        origlen: n as u32,
        data: vec![1; n],
        interface_id: 0,
        section_index: 0,
        interface: interface(),
        comments: Vec::new(),
        meta: None,
    }
}
fn captured(seq: u64, n: usize) -> CapturedFrame {
    CapturedFrame {
        source_seq: seq,
        host_mono_ns: 0,
        ts_source: TimestampSource::Replay,
        raw: raw(n),
    }
}
fn endpoints() -> Connection {
    Connection {
        client: Endpoint {
            mac: [2, 0, 0, 0, 0, 1],
            ip: [192, 0, 2, 1],
            port: 50000,
        },
        server: Endpoint {
            mac: [2, 0, 0, 0, 0, 2],
            ip: [192, 0, 2, 2],
            port: 13400,
        },
    }
}
#[test]
fn max_replay_r1_sample_sequence_bytes_and_source() {
    let bytes = include_bytes!("../../loom-eth-pcapng/tests/fixtures/r1-sol.pcapng");
    let src = ReplaySource::new(Cursor::new(bytes), ReplayOptions::default()).expect("replay");
    let run = CaptureLoop::start(src, config(16, 128 * 1024)).expect("start");
    let q = run.queue();
    let summary = run.join().expect("capture");
    let mut expected = loom_eth_pcapng::PcapNgReader::new(Cursor::new(bytes));
    let mut last_mono = 0;
    for seq in 1..=9 {
        let frame = q.pop_timeout(Duration::ZERO).expect("frame");
        let p = expected.next_packet().expect("read").expect("packet");
        assert_eq!(frame.source_seq, seq);
        assert_eq!(frame.raw.data, p.data);
        assert_eq!(frame.ts_source, TimestampSource::Replay);
        assert!(frame.host_mono_ns >= last_mono);
        last_mono = frame.host_mono_ns;
    }
    assert!(q.is_empty());
    assert_eq!(
        (
            summary.frames_seen,
            summary.frames_enqueued,
            summary.record_dropped
        ),
        (9, 9, 0)
    );
    assert!(q.try_gap().is_none());
    assert!(q.is_closed());
}
#[test]
fn slow_consumer_drop_newest_reports_exact_range() {
    let source =
        ReplaySource::new(Cursor::new(fixture(100, 64, 0)), ReplayOptions::default()).expect("replay");
    let run = CaptureLoop::start(source, config(2, 4096)).expect("start");
    let q = run.queue();
    let summary = run.join().expect("done");
    assert_eq!(
        (
            summary.frames_seen,
            summary.frames_enqueued,
            summary.record_dropped
        ),
        (100, 2, 98)
    );
    assert_eq!(q.pop_timeout(Duration::ZERO).expect("1").source_seq, 1);
    assert_eq!(q.pop_timeout(Duration::ZERO).expect("2").source_seq, 2);
    let gap = q.try_gap().expect("gap");
    assert_eq!(
        (gap.kind, gap.first_seq, gap.last_seq, gap.count),
        (GapKind::Record, Some(3), Some(100), Some(98))
    );
    assert!(!gap.coalesced);
    assert!(q.try_gap().is_none());
    assert!(q.byte_usage() <= 4096);
}
#[test]
fn byte_bound_and_noncontiguous_drops_are_not_claimed_contiguous() {
    let q = BoundedFrameQueue::new(QueueConfig {
        max_frames: 1,
        max_bytes: 1024,
    })
    .expect("queue");
    assert!(q.push(captured(1, 64)).expect("push1"));
    assert!(!q.push(captured(2, 64)).expect("drop2"));
    q.pop_timeout(Duration::ZERO).expect("pop1");
    assert!(q.push(captured(3, 64)).expect("push3"));
    assert!(!q.push(captured(4, 64)).expect("drop4"));
    for seq in [2, 4] {
        let gap = q.try_gap().expect("gap");
        assert_eq!(
            (gap.first_seq, gap.last_seq, gap.count),
            (Some(seq), Some(seq), Some(1))
        );
    }
    q.pop_timeout(Duration::ZERO).expect("pop3");
    assert!(!q.push(captured(5, 2048)).expect("drop byte bound"));
    assert_eq!(q.len(), 0);
    assert_eq!(q.byte_usage(), q.reserved_bytes());
}
#[derive(Debug)]
struct UnknownStatsSource {
    stop: StopSignal,
}
impl CaptureSource for UnknownStatsSource {
    fn interface(&self) -> Interface {
        interface()
    }
    fn next(&mut self, timeout: Duration) -> Result<Option<RawFrame>> {
        thread::sleep(timeout);
        Ok(None)
    }
    fn stats(&mut self) -> DropStats {
        DropStats::UNAVAILABLE
    }
    fn timestamp_source(&self) -> TimestampSource {
        TimestampSource::Sim
    }
    fn set_stop_signal(&mut self, s: StopSignal) {
        self.stop = s;
    }
}
#[test]
fn unavailable_stats_are_unknown_and_quiet_stop_under_200ms() {
    let run = CaptureLoop::start(
        UnknownStatsSource {
            stop: StopSignal::default(),
        },
        config(2, 4096),
    )
    .expect("start");
    let q = run.queue();
    let deadline = Instant::now();
    while q.gap_len() == 0 && deadline.elapsed() < Duration::from_secs(1) {
        thread::sleep(Duration::from_millis(1));
    }
    let start = Instant::now();
    run.stop_handle().stop();
    run.join().expect("stop");
    assert!(start.elapsed() < Duration::from_millis(200));
    let g = q.try_gap().expect("unknown gap");
    assert_eq!(g.cause, GapCause::StatsUnavailable);
    assert_eq!(g.count, None);
    assert!(q.try_gap().is_none());
}
#[test]
fn realtime_stop_interrupts_large_original_time_gap() {
    let source = ReplaySource::new(
        Cursor::new(fixture(2, 64, 1_000_000_000_000)),
        ReplayOptions {
            speed: ReplaySpeed::Realtime { factor: 1.0 },
            repeat: false,
        },
    )
    .expect("source");
    let run = CaptureLoop::start(source, config(4, 4096)).expect("start");
    let q = run.queue();
    q.pop_timeout(Duration::from_secs(1)).expect("first frame");
    let start = Instant::now();
    run.stop_handle().stop();
    let summary = run.join().expect("stop");
    assert!(start.elapsed() < Duration::from_millis(200));
    assert_eq!(summary.frames_seen, 1);
}
#[test]
fn repeat_and_factor_multiply_original_intervals() {
    let data = fixture(2, 64, 20_000_000);
    let mut s = ReplaySource::new(
        Cursor::new(data),
        ReplayOptions {
            speed: ReplaySpeed::Realtime { factor: 2.0 },
            repeat: true,
        },
    )
    .expect("source");
    s.next(Duration::ZERO).expect("first").expect("first");
    let start = Instant::now();
    assert!(s.next(Duration::from_millis(5)).expect("wait").is_none());
    let p = s
        .next(Duration::from_millis(100))
        .expect("second")
        .expect("second");
    assert_eq!(p.ts_ns, 20_000_000);
    assert!(start.elapsed() >= Duration::from_millis(35));
    assert_eq!(s.next(Duration::ZERO).expect("repeat").expect("frame").ts_ns, 0);
    assert!(!s.is_exhausted());
    for factor in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(
            ReplaySource::new(
                Cursor::new(fixture(1, 1, 0)),
                ReplayOptions {
                    speed: ReplaySpeed::Realtime { factor },
                    repeat: false
                }
            )
            .is_err()
        );
    }
}
#[test]
fn empty_repeat_finishes_instead_of_spinning() {
    let mut s = ReplaySource::new(
        Cursor::new(fixture(0, 0, 0)),
        ReplayOptions {
            repeat: true,
            ..Default::default()
        },
    )
    .expect("empty source");
    assert!(s.next(Duration::ZERO).expect("empty").is_none());
    assert!(s.is_exhausted());
}
#[test]
fn replay_preserves_interface_and_section_identity() {
    let mut w = PcapNgWriter::new(Vec::new()).expect("SHB");
    w.write_interface(&interface()).expect("IDB0");
    let mut second = interface();
    second.linktype = 101;
    second.name = "raw-ip".into();
    w.write_interface(&second).expect("IDB1");
    w.write_packet(&Packet {
        interface_id: 1,
        caplen: 1,
        origlen: 1,
        data: vec![1],
        ..Default::default()
    })
    .expect("p1");
    w.start_section().expect("section");
    w.write_interface(&interface()).expect("IDB");
    w.write_packet(&Packet {
        caplen: 1,
        origlen: 1,
        data: vec![2],
        ..Default::default()
    })
    .expect("p2");
    let mut s = ReplaySource::new(Cursor::new(w.into_inner()), ReplayOptions::default()).expect("source");
    let p = s.next(Duration::ZERO).expect("read").expect("p1");
    assert_eq!(
        (p.interface_id, p.section_index, p.interface.linktype),
        (1, 0, 101)
    );
    let p = s.next(Duration::ZERO).expect("read").expect("p2");
    assert_eq!((p.interface_id, p.section_index, p.interface.linktype), (0, 1, 1));
}
#[test]
fn classic_pcap_replay() {
    let mut bytes = Vec::new();
    bytes.extend(0xa1b23c4du32.to_le_bytes());
    bytes.extend(2u16.to_le_bytes());
    bytes.extend(4u16.to_le_bytes());
    for v in [0u32, 0, 65535, 1, 2, 3, 2, 4] {
        bytes.extend(v.to_le_bytes());
    }
    bytes.extend([0xaa, 0xbb]);
    let mut s = ReplaySource::new(Cursor::new(bytes), ReplayOptions::default()).expect("pcap");
    let p = s.next(Duration::ZERO).expect("read").expect("p");
    assert_eq!(p.ts_ns, 2_000_000_003);
    assert_eq!(p.data, [0xaa, 0xbb]);
    assert_eq!(p.origlen, 4);
}
#[test]
fn sim_tcp_syn_data_fin_seq_ack_and_udp_parse_with_etherparse() {
    let mut sim = SimTapSource::new();
    let c = endpoints();
    let syn = TcpFlags {
        syn: true,
        fin: false,
    };
    let fin = TcpFlags {
        syn: false,
        fin: true,
    };
    sim.push_tcp_with_flags(c, Direction::Tx, &[], syn).expect("SYN");
    sim.push_tcp_with_flags(c, Direction::Rx, &[], syn)
        .expect("SYN ACK");
    sim.push_tcp(c, Direction::Tx, b"abc").expect("data");
    sim.push_tcp(c, Direction::Rx, b"xy").expect("response");
    sim.push_tcp_with_flags(c, Direction::Tx, &[], fin).expect("FIN");
    sim.push_tcp_with_flags(c, Direction::Rx, &[], fin)
        .expect("FIN ACK");
    assert_eq!(sim.handle().connection_count(), 1);
    sim.push_tcp(c, Direction::Tx, &[]).expect("final ACK");
    assert_eq!(sim.handle().connection_count(), 0);
    for (seq, ack, syn, fin, payload) in [
        (1000, 0, true, false, &b""[..]),
        (2000, 1001, true, false, &b""[..]),
        (1001, 2001, false, false, &b"abc"[..]),
        (2001, 1004, false, false, &b"xy"[..]),
        (1004, 2003, false, true, &b""[..]),
        (2003, 1005, false, true, &b""[..]),
        (1005, 2004, false, false, &b""[..]),
    ] {
        let f = sim.next(Duration::ZERO).expect("next").expect("frame");
        let p = etherparse::SlicedPacket::from_ethernet(&f.data).expect("parse");
        let Some(etherparse::TransportSlice::Tcp(t)) = p.transport else {
            panic!("TCP required")
        };
        assert_eq!(
            (
                t.sequence_number(),
                t.acknowledgment_number(),
                t.syn(),
                t.fin(),
                t.payload()
            ),
            (seq, ack, syn, fin, payload)
        );
    }
    sim.push_udp(c.client, c.server, b"hello").expect("UDP");
    let f = sim.next(Duration::ZERO).expect("next").expect("frame");
    let p = etherparse::SlicedPacket::from_ethernet(&f.data).expect("UDP parse");
    let Some(etherparse::TransportSlice::Udp(u)) = p.transport else {
        panic!("UDP required")
    };
    assert_eq!(u.payload(), b"hello");
    assert_eq!(sim.timestamp_source(), TimestampSource::Sim);
}
#[test]
fn sim_queue_and_connection_bounds_reject_without_advancing_seq() {
    let mut sim = SimTapSource::new();
    let c = endpoints();
    let data = [1u8; 1500];
    let mut accepted = 0;
    while sim.push_udp(c.client, c.server, &data).is_ok() {
        accepted += 1;
        assert!(sim.handle().byte_usage() <= 4 * 1024 * 1024);
        assert!(accepted <= 4096);
    }
    assert!(accepted > 0);
    assert!(sim.push_tcp(c, Direction::Tx, &data).is_err());
    assert_eq!(sim.handle().connection_count(), 0);
    while sim.next(Duration::ZERO).expect("drain").is_some() {}
    sim.push_tcp(c, Direction::Tx, b"ok").expect("after drain");
    let f = sim.next(Duration::ZERO).expect("read").expect("frame");
    let p = etherparse::SlicedPacket::from_ethernet(&f.data).expect("TCP parse");
    let Some(etherparse::TransportSlice::Tcp(t)) = p.transport else {
        panic!("TCP")
    };
    assert_eq!(t.sequence_number(), 1000);
    sim.handle().forget_connection(&c);
    for i in 0..1024 {
        let mut conn = c;
        conn.client.port = i;
        sim.push_tcp(conn, Direction::Tx, &[]).expect("connection");
        sim.next(Duration::ZERO).expect("drain").expect("frame");
    }
    let mut extra = c;
    extra.client.port = 60000;
    assert!(sim.push_tcp(extra, Direction::Tx, &[]).is_err());
    assert_eq!(sim.handle().connection_count(), 1024);
}
#[test]
fn sim_handle_feeds_running_capture_and_stop_wakes() {
    let sim = SimTapSource::new();
    let h = sim.handle();
    let run = CaptureLoop::start(sim, config(8, 4096)).expect("run");
    let q = run.queue();
    let c = endpoints();
    h.push_udp(c.client, c.server, b"live-sim").expect("push");
    let f = q.pop_timeout(Duration::from_secs(1)).expect("frame");
    assert_eq!(f.ts_source, TimestampSource::Sim);
    let start = Instant::now();
    run.stop_handle().stop();
    run.join().expect("join");
    assert!(start.elapsed() < Duration::from_millis(200));
}
#[test]
fn restart_emits_gap_and_continues_source_sequence() {
    let sim = SimTapSource::new();
    let h = sim.handle();
    let c = endpoints();
    h.push_udp(c.client, c.server, b"first").expect("push");
    let run = CaptureLoop::start(sim, config(8, 4096)).expect("run");
    let oldq = run.queue();
    let first = oldq.pop_timeout(Duration::from_secs(1)).expect("first");
    let source = ReplaySource::new(Cursor::new(fixture(1, 64, 0)), ReplayOptions::default()).expect("replay");
    let run = run.restart(source).expect("restart");
    let q = run.queue();
    run.join().expect("join");
    let f = q.pop_timeout(Duration::ZERO).expect("second");
    assert_eq!(f.source_seq, first.source_seq + 1);
    assert!(f.host_mono_ns >= first.host_mono_ns);
    assert_eq!(q.try_gap().expect("gap").cause, GapCause::CaptureRestart);
}

#[test]
fn replay_yields_during_long_unknown_block_runs_and_remains_stoppable() {
    let mut writer = PcapNgWriter::new(Vec::new()).expect("header");
    writer.write_interface(&interface()).expect("interface");
    writer
        .write_packet(&Packet {
            caplen: 1,
            origlen: 1,
            data: vec![1],
            ..Default::default()
        })
        .expect("first");
    let mut bytes = writer.into_inner();
    for _ in 0..2000 {
        bytes.extend(0xdeadbeefu32.to_le_bytes());
        bytes.extend(12u32.to_le_bytes());
        bytes.extend(12u32.to_le_bytes());
    }
    let mut suffix = PcapNgWriter::new(Vec::new()).expect("second section");
    suffix.write_interface(&interface()).expect("interface");
    suffix
        .write_packet(&Packet {
            caplen: 1,
            origlen: 1,
            data: vec![2],
            ..Default::default()
        })
        .expect("second");
    bytes.extend(suffix.into_inner());
    let mut source = ReplaySource::new(Cursor::new(bytes), ReplayOptions::default()).expect("source");
    assert_eq!(
        source.next(Duration::ZERO).expect("first").expect("first").data,
        [1]
    );
    assert!(source.next(Duration::ZERO).expect("yield").is_none());
    assert!(!source.is_exhausted());
    let mut found = false;
    for _ in 0..100 {
        if let Some(p) = source.next(Duration::ZERO).expect("scan") {
            assert_eq!(p.data, [2]);
            found = true;
            break;
        }
    }
    assert!(found);
}

#[test]
fn repeat_with_only_many_unknown_blocks_reaches_eof() {
    let mut data = fixture(0, 0, 0);
    for _ in 0..1000 {
        data.extend(0xdeadbeefu32.to_le_bytes());
        data.extend(12u32.to_le_bytes());
        data.extend(12u32.to_le_bytes());
    }
    let mut source = ReplaySource::new(
        Cursor::new(data),
        ReplayOptions {
            repeat: true,
            ..Default::default()
        },
    )
    .expect("source");
    for _ in 0..100 {
        assert!(source.next(Duration::ZERO).expect("scan").is_none());
        if source.is_exhausted() {
            break;
        }
    }
    assert!(source.is_exhausted());
}

#[test]
fn idle_capture_samples_stats_at_100ms_without_packet_arrivals() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    #[derive(Debug)]
    struct Idle {
        calls: Arc<AtomicUsize>,
    }
    impl CaptureSource for Idle {
        fn interface(&self) -> Interface {
            interface()
        }
        fn next(&mut self, timeout: Duration) -> Result<Option<RawFrame>> {
            thread::sleep(timeout);
            Ok(None)
        }
        fn stats(&mut self) -> DropStats {
            let n = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            DropStats {
                kernel_drop: Some(n as u64),
                ..DropStats::ZERO
            }
        }
        fn timestamp_source(&self) -> TimestampSource {
            TimestampSource::Sim
        }
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let run = CaptureLoop::start(Idle { calls: calls.clone() }, config(2, 4096)).expect("run");
    let q = run.queue();
    let start = Instant::now();
    while calls.load(Ordering::SeqCst) < 3 && start.elapsed() < Duration::from_secs(2) {
        thread::sleep(Duration::from_millis(2));
    }
    assert!(calls.load(Ordering::SeqCst) >= 3);
    assert!(start.elapsed() >= Duration::from_millis(180));
    run.stop_handle().stop();
    let summary = run.join().expect("join");
    let mut lost = 0;
    while let Some(g) = q.try_gap() {
        lost += g.counts.kernel_drop;
    }
    assert_eq!(lost, calls.load(Ordering::SeqCst) as u64);
    assert_eq!(summary.frames_seen, 0);
}

#[test]
fn replay_initialization_skips_unknown_blocks_before_first_idb() {
    let file = fixture(1, 64, 0);
    let mut bytes = file[..28].to_vec();
    for _ in 0..100 {
        bytes.extend(0xdeadbeefu32.to_le_bytes());
        bytes.extend(12u32.to_le_bytes());
        bytes.extend(12u32.to_le_bytes());
    }
    bytes.extend(&file[28..]);
    let mut source = ReplaySource::new(Cursor::new(bytes), ReplayOptions::default()).expect("source");
    assert_eq!(
        source
            .next(Duration::ZERO)
            .expect("read")
            .expect("frame")
            .data
            .len(),
        64
    );
}

#[test]
fn replay_preserves_x10_user_comments_and_queue_byte_accounting() {
    let meta = LoomMeta {
        seq: 41240,
        dir: Direction::Tx,
        req: Some("tx-original".into()),
        org: loom_eth_pcapng::Origin::Agent,
    };
    let comments = vec![
        "user note 한글".to_owned(),
        "loom1 seq=bad dir=tx org=agent".into(),
    ];
    let mut w = PcapNgWriter::new(Vec::new()).expect("SHB");
    w.write_interface(&interface()).expect("IDB");
    w.write_packet(&Packet {
        ts_ns: 1,
        caplen: 3,
        origlen: 3,
        data: vec![1, 2, 3],
        comments: comments.clone(),
        meta: Some(meta.clone()),
        ..Default::default()
    })
    .expect("EPB");
    let source = ReplaySource::new(Cursor::new(w.into_inner()), ReplayOptions::default()).expect("source");
    let run = CaptureLoop::start(source, config(4, 64 * 1024)).expect("loop");
    let q = run.queue();
    run.join().expect("finished");
    let frame = q.pop_timeout(Duration::ZERO).expect("packet");
    assert_eq!(frame.raw.comments, comments);
    assert_eq!(frame.raw.meta, Some(meta));
    frame.raw.validate().expect("valid raw");
    let mut too_many = frame.raw.clone();
    too_many.comments = vec![String::new(); loom_eth_pcapng::MAX_COMMENTS + 1];
    assert!(too_many.validate().is_err());
    let mut too_big = frame.raw;
    too_big.comments = vec!["x".repeat(loom_eth_pcapng::MAX_COMMENT_BYTES + 1)];
    assert!(too_big.validate().is_err());
}
