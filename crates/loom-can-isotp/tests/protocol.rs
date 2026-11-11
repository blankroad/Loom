//! Protocol tests over a virtual link with explicit virtual time (no sleeps, no IO).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use loom_can_frame::{CanFrame, CanId};
use loom_can_isotp::*;

fn config(fd: bool, dl: u8) -> IsoTpConfig {
    let mut c = IsoTpConfig::new(CanId::standard(0x7E0).unwrap(), CanId::standard(0x7E8).unwrap());
    c.can_fd = fd;
    c.tx_dl = dl;
    c
}
fn pair(c: &IsoTpConfig) -> (Engine, Engine) {
    let mut peer = c.clone();
    peer.tx_id = c.rx_id;
    peer.rx_id = c.tx_id;
    (Engine::new(c.clone()).unwrap(), Engine::new(peer).unwrap())
}
fn events(e: &mut Engine) -> Vec<Event> {
    let mut out = Vec::new();
    while let Some(event) = e.pop_event() {
        out.push(event);
    }
    out
}
fn submitted(e: &mut Engine, request: FrameRequest, now: u64, confirm: bool) {
    assert!(now >= request.not_before_ns);
    e.on_frame_submitted_at(request.handle, now).unwrap();
    if confirm {
        e.on_frame_confirmed(request.handle).unwrap();
    }
}
fn flush(from: &mut Engine, to: &mut Engine, now: u64, confirm: bool, cf_times: &mut Vec<u64>) -> bool {
    let mut progressed = false;
    while let Some(request) = from.pop_frame() {
        if matches!(decode(&request.frame), Ok(Pci::Consecutive { .. })) {
            cf_times.push(now);
        }
        submitted(from, request, now, confirm);
        to.on_frame_in_generation(&request.frame, to.generation(), now)
            .unwrap();
        progressed = true;
    }
    progressed
}

fn round_trip(c: IsoTpConfig, len: usize, confirm: bool) {
    let (mut a, mut b) = pair(&c);
    let pdu: Vec<_> = (0..len).map(|i| (i.wrapping_mul(37) & 255) as u8).collect();
    let id = a.send(&pdu, 0).unwrap();
    let mut now = 0;
    let mut times = Vec::new();
    let mut peer_cf = Vec::new();
    let mut received = false;
    let mut accepted = false;
    let mut confirmed = false;
    let mut saw_start = false;
    for _ in 0..6000 {
        let _ = flush(&mut a, &mut b, now, confirm, &mut times);
        let _ = flush(&mut b, &mut a, now, confirm, &mut peer_cf);
        for event in events(&mut a) {
            match event {
                Event::TxDone {
                    transfer_id,
                    tx_state,
                } => {
                    assert_eq!(id, transfer_id);
                    if tx_state == TxState::DriverAccepted {
                        accepted = true;
                    }
                    if tx_state == TxState::BusConfirmed {
                        confirmed = true;
                    }
                }
                other => panic!("unexpected sender event: {other:?}"),
            }
        }
        for event in events(&mut b) {
            match event {
                Event::RxStart { expected_length, .. } => {
                    assert!(!saw_start);
                    assert_eq!(expected_length, len);
                    saw_start = true;
                }
                Event::RxComplete { pdu: actual, .. } => {
                    assert_eq!(actual, pdu);
                    received = true;
                }
                other => panic!("unexpected receiver event: {other:?}"),
            }
        }
        if received && accepted {
            assert_eq!(confirmed, confirm);
            assert_eq!(saw_start, len > c.single_frame_capacity());
            for w in times.windows(2) {
                assert!(w[1] - w[0] >= st_min_ns(c.st_min));
            }
            if !confirm {
                a.release_transfer(id).unwrap();
            }
            return;
        }
        if a.output_len().0 + b.output_len().0 > 0 {
            continue;
        }
        let next = [a.poll_at(), b.poll_at()]
            .into_iter()
            .flatten()
            .min()
            .expect("stuck link");
        assert!(next >= now);
        now = next;
        a.on_timer(now).unwrap();
        b.on_timer(now).unwrap();
    }
    panic!("link iteration bound exceeded: {len} bytes");
}

#[test]
fn stmin_entire_byte_table() {
    for value in 0..=255u8 {
        let expected = match value {
            0..=127 => u64::from(value) * 1_000_000,
            0xF1..=0xF9 => u64::from(value - 0xF0) * 100_000,
            _ => 127_000_000,
        };
        assert_eq!(st_min_ns(value), expected, "{value:02X}");
    }
}

#[test]
fn codec_pci_table() {
    let c = config(false, 8);
    let sf = encode_sf(&c, &[0x22, 0xF1, 0x90]).unwrap();
    assert_eq!(&sf.data()[..4], &[3, 0x22, 0xF1, 0x90]);
    assert_eq!(decode(&sf), Ok(Pci::Single(&[0x22, 0xF1, 0x90])));
    let pdu = [0x55; 20];
    let ff = encode_ff(&c, &pdu).unwrap();
    assert_eq!(&ff.data()[..2], &[0x10, 20]);
    assert_eq!(
        decode(&ff),
        Ok(Pci::First {
            length: 20,
            data: &[0x55; 6]
        })
    );
    for sequence in 0..16 {
        let cf = encode_cf(&c, sequence, &[0xAA; 7]).unwrap();
        assert_eq!(
            decode(&cf),
            Ok(Pci::Consecutive {
                sequence,
                data: &[0xAA; 7]
            })
        );
    }
    for status in [FlowStatus::Continue, FlowStatus::Wait, FlowStatus::Overflow] {
        let fc = encode_fc(&c, status, 8, 0xF1).unwrap();
        assert_eq!(
            decode(&fc),
            Ok(Pci::FlowControl {
                status,
                block_size: 8,
                st_min: 0xF1
            })
        );
    }
    assert!(encode_cf(&c, 16, &[1]).is_err());
    assert!(encode_cf(&c, 0, &[]).is_err());
}

#[test]
fn sf_ff_boundaries_and_padding() {
    for (fd, dl, sf_max) in [(false, 8, 7), (true, 8, 7), (true, 64, 62)] {
        let c = config(fd, dl);
        for length in [1, 7, 8, 62, 63, 4095, 4096] {
            let pdu = vec![0xAB; length];
            if length <= sf_max {
                assert!(matches!(decode(&encode_sf(&c, &pdu).unwrap()), Ok(Pci::Single(v)) if v == pdu));
                assert!(encode_ff(&c, &pdu).is_err());
            } else if length <= MAX_PDU {
                assert!(encode_sf(&c, &pdu).is_err());
                assert!(
                    matches!(decode(&encode_ff(&c, &pdu).unwrap()), Ok(Pci::First { length: n, .. }) if n == length)
                );
            } else {
                assert!(matches!(encode_ff(&c, &pdu), Err(Error::PduTooLarge { .. })));
            }
        }
    }
    let mut fd = config(true, 64);
    fd.padding = None;
    let escaped = encode_sf(&fd, &[1; 8]).unwrap();
    assert_eq!(escaped.len(), 12);
    assert_eq!(&escaped.data()[..2], &[0, 8]);
    assert_eq!(&escaped.data()[10..], &[0xCC, 0xCC]);
    let short = encode_sf(&fd, &[1; 7]).unwrap();
    assert_eq!(short.len(), 8);
    assert_eq!(short.data()[0], 7);
    fd.padding = Some(0xAA);
    let padded = encode_sf(&fd, &[1; 3]).unwrap();
    assert_eq!(padded.len(), 64);
    assert_eq!(&padded.data()[..2], &[0, 3]);
    assert!(padded.data()[5..].iter().all(|b| *b == 0xAA));
}

#[test]
fn malformed_codec_inputs_never_panic() {
    let id = CanId::standard(0x7E8).unwrap();
    for fd in [false, true] {
        for len in 0..=64 {
            for first in 0..=255u8 {
                let mut data = [0u8; 64];
                data[0] = first;
                let frame = if fd {
                    CanFrame::fd(id, &data[..len], false, false)
                } else {
                    CanFrame::classic(id, &data[..len])
                };
                if let Ok(frame) = frame {
                    let _ = decode(&frame);
                }
            }
        }
    }
    for bytes in [
        &[0x40][..],
        &[0x30, 8][..],
        &[0x33, 0, 0][..],
        &[0x20][..],
        &[0][..],
        &[0x10, 7, 0, 0, 0, 0, 0, 0][..],
    ] {
        assert!(decode(&CanFrame::classic(id, bytes).unwrap()).is_err());
    }
    let big = CanFrame::classic(id, &[0x10, 0, 0, 0, 0x10, 0, 0, 0]).unwrap();
    assert_eq!(decode(&big), Err(Error::PduTooLarge { len: 4096, max: 4095 }));
    assert!(decode(&CanFrame::remote(id, 8).unwrap()).is_err());
}

/// The 18 configurations of the original brief: CC/FD × BS 0/1/8 × STmin 0, 1 ms, 100 µs.
fn matrix_configs() -> Vec<IsoTpConfig> {
    let mut out = Vec::new();
    for fd in [false, true] {
        for bs in [0, 1, 8] {
            for st in [0, 1, 0xF1] {
                let mut c = config(fd, if fd { 64 } else { 8 });
                c.block_size = bs;
                c.st_min = st;
                out.push(c);
            }
        }
    }
    out
}

/// Lengths at every segmentation edge: SF/FF switch, first CF, 1-byte last CF,
/// block boundaries (BS, BS+1), SN wrap (15/16/17 CFs) and the 4095 limit.
fn boundary_lengths(c: &IsoTpConfig) -> Vec<usize> {
    let sf = c.single_frame_capacity();
    let ff = c.tx_dl as usize - 2;
    let cf = c.tx_dl as usize - 1;
    let bs = c.block_size as usize;
    let mut out = vec![1, sf, sf + 1, MAX_PDU - 1, MAX_PDU];
    for k in [1, 2, 7, 8, 9, 15, 16, 17, 32, bs, bs + 1, 2 * bs] {
        if k > 0 {
            out.push(ff + cf * k); // last CF exactly full
            out.push(ff + cf * (k - 1) + 1); // last CF carries 1 byte
        }
    }
    out.retain(|&l| (1..=MAX_PDU).contains(&l));
    out.sort_unstable();
    out.dedup();
    out
}

#[test]
fn pdu_length_matrix_boundaries_and_sample() {
    // Default run: every edge length for all 18 configurations plus a seeded sample.
    // The exhaustive 1..=4095 sweep is `exhaustive_pdu_length_matrix` (ignored).
    let mut state = 0x1505_7a11_u64;
    for c in matrix_configs() {
        for len in boundary_lengths(&c) {
            round_trip(c.clone(), len, true);
        }
        for _ in 0..24 {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            round_trip(c.clone(), 1 + (state >> 33) as usize % MAX_PDU, true);
        }
    }
}

#[test]
#[ignore = "73,710 transfers; run with: cargo test -p loom-can-isotp --release -- --ignored"]
fn exhaustive_pdu_length_matrix() {
    for c in matrix_configs() {
        for len in 1..=MAX_PDU {
            round_trip(c.clone(), len, true);
        }
    }
}

#[test]
fn fd_dl_ladder_and_no_padding() {
    for dl in [8, 12, 16, 20, 24, 32, 48, 64] {
        for padding in [None, Some(0), Some(0xCC)] {
            let mut c = config(true, dl);
            c.padding = padding;
            c.block_size = 1;
            c.st_min = 0xF9;
            for len in [1, 7, 8, 10, 11, 62, 63, 64, 65, 130, 511, 4095] {
                round_trip(c.clone(), len, true);
            }
        }
    }
}

#[test]
fn deterministic_randomized_round_trips() {
    let mut state = 0x6c6f6f6d_u64;
    for _ in 0..512 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let fd = state & 1 == 0;
        let dl = [8, 12, 16, 20, 24, 32, 48, 64][(state >> 1) as usize % 8];
        let mut c = config(fd, if fd { dl } else { 8 });
        c.tx_id = CanId::extended(0x18DA10F1).unwrap();
        c.rx_id = CanId::extended(0x18DAF110).unwrap();
        c.block_size = (state >> 8) as u8;
        // Own FC STmin must be a legal value (reserved ones are tested on receive).
        c.st_min = match (state >> 16) as u8 {
            v @ (0..=0x7F | 0xF1..=0xF9) => v,
            v => v & 0x0F,
        };
        if state & 4 == 0 {
            c.padding = None;
        }
        round_trip(c, 1 + ((state >> 24) as usize % MAX_PDU), state & 8 != 0);
    }
}

#[test]
fn deadlines_busy_and_driver_acceptance() {
    let c = config(false, 8);
    let (mut a, mut b) = pair(&c);
    let id = a.send(&[1; 20], 5).unwrap();
    assert_eq!(a.send(&[2], 5), Err(Error::Busy));
    assert_eq!(a.poll_at(), None); // Caller has not submitted the FF yet.
    let ff = a.pop_frame().unwrap();
    submitted(&mut a, ff, 10, false);
    assert_eq!(a.poll_at(), Some(1_000_000_010));
    b.on_frame(&ff.frame, 10).unwrap();
    assert!(matches!(
        b.pop_event(),
        Some(Event::RxStart {
            expected_length: 20,
            ..
        })
    ));
    let fc = b.pop_frame().unwrap();
    submitted(&mut b, fc, 20, false);
    assert_eq!(b.poll_at(), Some(1_000_000_020));
    a.on_timer(1_000_000_010).unwrap();
    assert_eq!(
        events(&mut a),
        vec![Event::TxError {
            transfer_id: id,
            error: Error::NBsTimeout,
            residual_tx_possible: 1
        }]
    );
    b.on_timer(1_000_000_020).unwrap();
    assert_eq!(
        events(&mut b),
        vec![Event::RxError {
            generation: 0,
            error: Error::NCrTimeout
        }]
    );
    assert_eq!(a.poll_at(), None);
    assert_eq!(b.poll_at(), None);
    assert!(matches!(
        a.send(&[0; 4096], 1_000_000_020),
        Err(Error::PduTooLarge { .. })
    ));
}

#[test]
fn completion_is_monotonic_and_correlated() {
    let mut a = Engine::new(config(false, 8)).unwrap();
    let id = a.send(&[1, 2, 3], 0).unwrap();
    let frame = a.pop_frame().unwrap();
    assert_eq!(
        a.on_frame_confirmed(frame.handle),
        Err(Error::ConfirmationBeforeSubmission)
    );
    assert!(events(&mut a).is_empty());
    a.on_frame_submitted(frame.handle).unwrap();
    assert_eq!(
        events(&mut a),
        vec![Event::TxDone {
            transfer_id: id,
            tx_state: TxState::DriverAccepted
        }]
    );
    a.on_frame_submitted(frame.handle).unwrap();
    assert!(events(&mut a).is_empty());
    a.on_frame_confirmed(frame.handle).unwrap();
    assert_eq!(
        events(&mut a),
        vec![Event::TxDone {
            transfer_id: id,
            tx_state: TxState::BusConfirmed
        }]
    );
    a.on_frame_confirmed(frame.handle).unwrap();
    assert!(events(&mut a).is_empty());
    round_trip(config(false, 8), 4095, false);
}

#[test]
fn every_frame_must_be_confirmed_not_just_last() {
    let (mut a, mut b) = pair(&config(false, 8));
    let id = a.send(&[1; 20], 0).unwrap();
    let first = a.pop_frame().unwrap();
    submitted(&mut a, first, 0, false);
    b.on_frame(&first.frame, 0).unwrap();
    events(&mut b);
    let fc = b.pop_frame().unwrap();
    submitted(&mut b, fc, 0, true);
    a.on_frame(&fc.frame, 0).unwrap();
    let second = a.pop_frame().unwrap();
    submitted(&mut a, second, 0, true);
    a.on_timer(0).unwrap();
    let last = a.pop_frame().unwrap();
    submitted(&mut a, last, 0, true);
    assert_eq!(
        events(&mut a),
        vec![Event::TxDone {
            transfer_id: id,
            tx_state: TxState::DriverAccepted
        }]
    );
    a.on_frame_confirmed(first.handle).unwrap();
    assert_eq!(
        events(&mut a),
        vec![Event::TxDone {
            transfer_id: id,
            tx_state: TxState::BusConfirmed
        }]
    );
}

#[test]
fn wrong_sn_and_new_ff_replace_partial_rx() {
    let c = config(false, 8);
    let (mut a, mut b) = pair(&c);
    a.send(&[1; 30], 0).unwrap();
    let ff = a.pop_frame().unwrap();
    submitted(&mut a, ff, 0, true);
    b.on_frame(&ff.frame, 0).unwrap();
    events(&mut b);
    let fc = b.pop_frame().unwrap();
    submitted(&mut b, fc, 0, true);
    let mut peer = c.clone();
    peer.tx_id = c.tx_id;
    let bad = encode_cf(&peer, 2, &[1; 7]).unwrap();
    b.on_frame(&bad, 1).unwrap();
    assert_eq!(
        events(&mut b),
        vec![Event::RxError {
            generation: 0,
            error: Error::WrongSequence {
                expected: 1,
                actual: 2
            }
        }]
    );
    b.on_frame(&ff.frame, 2).unwrap();
    events(&mut b);
    let replacement = encode_ff(&peer, &[2; 40]).unwrap();
    b.on_frame(&replacement, 3).unwrap();
    assert_eq!(
        events(&mut b),
        vec![
            Event::RxError {
                generation: 0,
                error: Error::UnexpectedPdu
            },
            Event::RxStart {
                generation: 0,
                expected_length: 40
            }
        ]
    );
    assert_eq!(b.output_len().0, 1); // Old unsent CTS was removed.
    let sf = encode_sf(&peer, &[9]).unwrap();
    b.on_frame(&sf, 4).unwrap();
    assert_eq!(
        events(&mut b),
        vec![
            Event::RxError {
                generation: 0,
                error: Error::UnexpectedPdu
            },
            Event::RxComplete {
                generation: 0,
                pdu: vec![9]
            }
        ]
    );
    assert_eq!(b.output_len().0, 0);
}

#[test]
fn overflow_wait_and_unrelated_fc() {
    for (status, expected) in [
        (FlowStatus::Overflow, Error::FlowOverflow),
        (FlowStatus::Wait, Error::WaitLimit),
    ] {
        let c = config(false, 8);
        let (mut a, b) = pair(&c);
        let mut remote = b.config().clone();
        remote.wft_max = 1;
        let id = a.send(&[1; 20], 0).unwrap();
        let ff = a.pop_frame().unwrap();
        submitted(&mut a, ff, 0, false);
        let fc = encode_fc(&remote, status, 0, 0).unwrap();
        let count = if status == FlowStatus::Wait { 11 } else { 1 };
        for _ in 0..count {
            a.on_frame(&fc, 0).unwrap();
        }
        assert_eq!(
            events(&mut a),
            vec![Event::TxError {
                transfer_id: id,
                error: expected,
                residual_tx_possible: 1
            }]
        );
        a.on_frame(&fc, 0).unwrap();
        assert!(a.stats().ignored_flow_controls > 0);
    }
}

#[test]
fn abort_fences_pending_and_late_work() {
    let c = config(false, 8);
    let (mut a, b) = pair(&c);
    let old = a.generation();
    a.send(&[1; 20], 0).unwrap();
    let ff = a.pop_frame().unwrap();
    submitted(&mut a, ff, 0, false);
    let fc = encode_fc(b.config(), FlowStatus::Continue, 0, 0).unwrap();
    a.on_frame(&fc, 0).unwrap();
    let removed = a.pop_frame().unwrap();
    let report = a.abort(0).unwrap();
    assert!(report.aborted_tx);
    assert_eq!(report.residual_tx_possible, 1);
    assert_eq!(report.cancelled_frames, 1);
    events(&mut a);
    let new = a.send(&[2; 20], 0).unwrap();
    let new_ff = a.pop_frame().unwrap();
    submitted(&mut a, new_ff, 0, false);
    a.on_frame_in_generation(&fc, old, 0).unwrap();
    a.on_frame_submitted(removed.handle).unwrap();
    a.on_frame_confirmed(ff.handle).unwrap();
    assert_eq!(a.output_len().0, 0);
    assert_eq!(a.stats().ignored_generations, 1);
    assert_eq!(a.stats().ignored_callbacks, 2);
    a.on_frame(&fc, 0).unwrap();
    let current = a.pop_frame().unwrap();
    assert_eq!(current.transfer_id, Some(new));
    a.abort(0).unwrap();
    events(&mut a);
    let late_cf = encode_cf(b.config(), 1, &[0; 7]).unwrap();
    a.on_frame(&late_cf, 0).unwrap();
    a.on_frame(&fc, 0).unwrap();
    assert_eq!(a.stats().ignored_consecutive_frames, 1);
    assert_eq!(a.stats().ignored_flow_controls, 1);
}

#[test]
fn rx_abort_removes_delayed_and_queued_fc() {
    let mut c = config(false, 8);
    c.faults.fc_delay_ms = 10;
    let (mut a, mut b) = pair(&c);
    a.send(&[1; 20], 0).unwrap();
    let ff = a.pop_frame().unwrap();
    submitted(&mut a, ff, 0, true);
    b.on_frame(&ff.frame, 0).unwrap();
    assert_eq!(b.output_len().0, 0);
    assert_eq!(b.poll_at(), Some(10_000_000));
    let r = b.abort(0).unwrap();
    assert!(r.aborted_rx);
    assert_eq!(b.poll_at(), None);
    events(&mut b);
    b.on_timer(10_000_000).unwrap();
    assert_eq!(b.output_len().0, 0);
}

fn run_fault(mut c: IsoTpConfig, receiver_fault: bool, expected: Error, len: usize) {
    let faults = c.faults;
    c.faults = FaultPlan::default();
    let (mut a, mut b) = pair(&c);
    if receiver_fault {
        let mut p = b.config().clone();
        p.faults = faults;
        b = Engine::new(p).unwrap();
    } else {
        let mut p = a.config().clone();
        p.faults = faults;
        a = Engine::new(p).unwrap();
    }
    a.send(&vec![1; len], 0).unwrap();
    let mut now = 0;
    let mut cf = Vec::new();
    let mut pc = Vec::new();
    let mut found = false;
    for _ in 0..3000 {
        flush(&mut a, &mut b, now, true, &mut cf);
        flush(&mut b, &mut a, now, true, &mut pc);
        for e in events(&mut a).into_iter().chain(events(&mut b)) {
            if matches!(e,Event::TxError {error,..}|Event::RxError {error,..} if error==expected) {
                found = true;
            }
        }
        if found {
            return;
        }
        if a.output_len().0 + b.output_len().0 > 0 {
            continue;
        }
        let Some(next) = [a.poll_at(), b.poll_at()].into_iter().flatten().min() else {
            break;
        };
        now = next;
        a.on_timer(now).unwrap();
        b.on_timer(now).unwrap();
    }
    panic!("fault did not produce {expected:?}");
}

#[test]
fn fault_hooks_drop_wrong_sn_overflow_wait_delay() {
    let mut c = config(false, 8);
    c.faults.drop_cf_number = Some(1);
    run_fault(c, false, Error::NCrTimeout, 8);
    let mut c = config(false, 8);
    c.faults.wrong_sn_cf_number = Some(2);
    run_fault(
        c,
        false,
        Error::WrongSequence {
            expected: 2,
            actual: 3,
        },
        30,
    );
    let mut c = config(false, 8);
    c.faults.fc_overflow = true;
    run_fault(c, true, Error::FlowOverflow, 30);
    let mut c = config(false, 8);
    c.faults.fc_wait_count = 11;
    run_fault(c, true, Error::WaitLimit, 30);
    let mut c = config(false, 8);
    c.n_bs_ms = 5;
    c.faults.fc_delay_ms = 10;
    run_fault(c, true, Error::NBsTimeout, 30);
}

#[test]
fn fault_wait_within_limit_and_delay_can_succeed() {
    let mut c = config(false, 8);
    c.faults.fc_wait_count = 2;
    c.faults.fc_delay_ms = 1;
    round_trip(c, 200, true);
}

#[test]
fn queue_limits_and_confirmation_retention_are_bounded() {
    let limits = QueueLimits {
        frames: 2,
        events: 4,
        transfers: 1,
    };
    let mut a = Engine::with_limits(config(false, 8), limits).unwrap();
    let id = a.send(&[1], 0).unwrap();
    let f = a.pop_frame().unwrap();
    submitted(&mut a, f, 0, false);
    events(&mut a);
    assert_eq!(a.send(&[2], 0), Err(Error::ResourceExhausted));
    assert_eq!(a.release_transfer(id), Ok(1));
    for _ in 0..1000 {
        let id = a.send(&[2], 0).unwrap();
        let f = a.pop_frame().unwrap();
        submitted(&mut a, f, 0, false);
        events(&mut a);
        assert_eq!(a.release_transfer(id), Ok(1));
    }
    let peer = config(false, 8);
    let incoming = CanFrame::classic(peer.rx_id, &[1, 7]).unwrap();
    a.on_frame(&incoming, 0).unwrap();
    assert_eq!(a.on_frame(&incoming, 0), Err(Error::OutputFull));
    assert_eq!(a.output_len(), (0, 1));
    assert_eq!(a.stats().output_full, 1);
    a.abort(0).unwrap();
    assert!(a.output_len().1 <= 4);
    assert!(
        Engine::with_limits(
            peer,
            QueueLimits {
                frames: 65,
                events: 4,
                transfers: 1
            }
        )
        .is_err()
    );
}

#[test]
fn bounded_full_duplex_output_and_abort_with_full_queues() {
    let mut a = Engine::with_limits(
        config(false, 8),
        QueueLimits {
            frames: 2,
            events: 8,
            transfers: 1,
        },
    )
    .unwrap();
    let mut peer = config(false, 8);
    peer.tx_id = peer.rx_id;
    a.send(&[1; 20], 0).unwrap();
    let ff = encode_ff(&peer, &[2; 20]).unwrap();
    a.on_frame(&ff, 0).unwrap();
    assert_eq!(a.output_len().0, 2);
    assert_eq!(a.on_timer(0), Err(Error::OutputFull));
    let report = a.abort(0).unwrap();
    assert!(report.aborted_tx && report.aborted_rx);
    assert_eq!(report.cancelled_frames, 2);
    assert_eq!(a.output_len().0, 0);
    assert_eq!(a.output_len().1, 2);
    assert_eq!(a.poll_at(), None);
}

#[test]
fn unsupported_addressing_and_invalid_configuration() {
    for addressing in [
        Addressing::Extended {
            tx_address: 1,
            rx_address: 2,
        },
        Addressing::Mixed { address_extension: 3 },
    ] {
        let mut c = config(false, 8);
        c.addressing = addressing;
        assert_eq!(c.validate(), Err(Error::Unsupported));
    }
    for dl in [0, 7, 9, 11, 65] {
        assert!(Engine::new(config(true, dl)).is_err());
    }
    let mut c = config(false, 8);
    c.max_pdu = 4096;
    assert!(Engine::new(c).is_err());
    let mut c = config(false, 8);
    c.n_cr_ms = 0;
    assert!(Engine::new(c).is_err());
    let mut c = config(false, 8);
    c.faults.fc_wait_count = 33;
    assert!(Engine::new(c).is_err());
}

#[test]
fn smaller_max_pdu_overflows_without_reassembly_allocation() {
    let mut c = config(false, 8);
    c.max_pdu = 16;
    let (_, mut b) = pair(&c);
    let ff = encode_ff(&config(false, 8), &[0; 17]).unwrap();
    b.on_frame(&ff, 0).unwrap();
    assert!(!b.is_receiving());
    assert!(matches!(
        b.pop_event(),
        Some(Event::RxError {
            error: Error::PduTooLarge { len: 17, max: 16 },
            ..
        })
    ));
    let overflow = b.pop_frame().unwrap();
    assert!(matches!(
        decode(&overflow.frame),
        Ok(Pci::FlowControl {
            status: FlowStatus::Overflow,
            ..
        })
    ));
}

#[test]
fn wrong_ids_modes_clock_and_empty_pdu() {
    let mut a = Engine::new(config(false, 8)).unwrap();
    assert_eq!(a.send(&[], 0), Err(Error::InvalidLength));
    let f = CanFrame::classic(CanId::extended(0x7E8).unwrap(), &[1, 1]).unwrap();
    a.on_frame(&f, 1).unwrap();
    assert_eq!(a.stats().ignored_identifiers, 1);
    assert!(events(&mut a).is_empty());
    let f = CanFrame::fd(a.config().rx_id, &[1, 1], false, false).unwrap();
    a.on_frame(&f, 2).unwrap();
    assert!(matches!(
        a.pop_event(),
        Some(Event::RxError {
            error: Error::InvalidLength,
            ..
        })
    ));
    assert_eq!(a.on_timer(1), Err(Error::TimeWentBackwards));
}

#[test]
fn full_duplex_with_different_transmit_lengths() {
    let mut left = config(true, 12);
    left.st_min = 0xF1;
    left.block_size = 1;
    let mut right = config(true, 64);
    right.tx_id = left.rx_id;
    right.rx_id = left.tx_id;
    right.st_min = 1;
    right.block_size = 8;
    let mut a = Engine::new(left).unwrap();
    let mut b = Engine::new(right).unwrap();
    let pa = vec![0xA5; 1024];
    let pb = vec![0x5A; 511];
    a.send(&pa, 0).unwrap();
    b.send(&pb, 0).unwrap();
    let mut now = 0;
    let mut times = Vec::new();
    let mut peer_times = Vec::new();
    let mut got_a = false;
    let mut got_b = false;
    let mut done_a = false;
    let mut done_b = false;
    for _ in 0..3000 {
        flush(&mut a, &mut b, now, true, &mut times);
        flush(&mut b, &mut a, now, true, &mut peer_times);
        for event in events(&mut a) {
            match event {
                Event::RxComplete { pdu, .. } => {
                    assert_eq!(pdu, pb);
                    got_a = true;
                }
                Event::TxDone {
                    tx_state: TxState::BusConfirmed,
                    ..
                } => done_a = true,
                Event::RxError { .. } | Event::TxError { .. } => panic!("{event:?}"),
                _ => {}
            }
        }
        for event in events(&mut b) {
            match event {
                Event::RxComplete { pdu, .. } => {
                    assert_eq!(pdu, pa);
                    got_b = true;
                }
                Event::TxDone {
                    tx_state: TxState::BusConfirmed,
                    ..
                } => done_b = true,
                Event::RxError { .. } | Event::TxError { .. } => panic!("{event:?}"),
                _ => {}
            }
        }
        if got_a && got_b && done_a && done_b {
            return;
        }
        if a.output_len().0 + b.output_len().0 > 0 {
            continue;
        }
        now = [a.poll_at(), b.poll_at()].into_iter().flatten().min().unwrap();
        a.on_timer(now).unwrap();
        b.on_timer(now).unwrap();
    }
    panic!("full duplex did not finish");
}

#[test]
fn stmin_uses_actual_submission_across_blocks() {
    let mut c = config(false, 8);
    c.st_min = 10;
    c.block_size = 1;
    let (mut a, mut b) = pair(&c);
    a.send(&[0x55; 20], 0).unwrap();
    let ff = a.pop_frame().unwrap();
    submitted(&mut a, ff, 0, true);
    b.on_frame(&ff.frame, 0).unwrap();
    events(&mut b);
    let fc = b.pop_frame().unwrap();
    submitted(&mut b, fc, 0, true);
    a.on_frame(&fc.frame, 0).unwrap();
    let first_cf = a.pop_frame().unwrap();
    submitted(&mut a, first_cf, 5_000_000, true);
    b.on_frame(&first_cf.frame, 5_000_000).unwrap();
    let fc = b.pop_frame().unwrap();
    submitted(&mut b, fc, 5_000_000, true);
    a.on_frame(&fc.frame, 5_000_000).unwrap();
    assert_eq!(a.poll_at(), Some(15_000_000));
    assert!(a.pop_frame().is_none());
    a.on_timer(14_999_999).unwrap();
    assert!(a.pop_frame().is_none());
    a.on_timer(15_000_000).unwrap();
    let last = a.pop_frame().unwrap();
    assert_eq!(last.not_before_ns, 15_000_000);
    submitted(&mut a, last, 15_000_000, true);
    b.on_frame(&last.frame, 15_000_000).unwrap();
    assert!(
        events(&mut b)
            .iter()
            .any(|e| matches!(e,Event::RxComplete {pdu,..} if pdu==&[0x55;20]))
    );
}

#[test]
fn deadline_ingress_and_queue_retry_do_not_create_false_success() {
    let (mut a, b) = pair(&config(false, 8));
    let id = a.send(&[1; 20], 0).unwrap();
    let ff = a.pop_frame().unwrap();
    submitted(&mut a, ff, 0, false);
    let fc = encode_fc(b.config(), FlowStatus::Continue, 0, 0).unwrap();
    a.on_frame(&fc, 1_000_000_000).unwrap();
    assert_eq!(
        events(&mut a),
        vec![Event::TxError {
            transfer_id: id,
            error: Error::NBsTimeout,
            residual_tx_possible: 1
        }]
    );
    assert_eq!(a.stats().ignored_flow_controls, 1);
    assert!(a.pop_frame().is_none());
    let mut r = Engine::with_limits(
        config(false, 8),
        QueueLimits {
            frames: 2,
            events: 4,
            transfers: 1,
        },
    )
    .unwrap();
    let sf = CanFrame::classic(r.config().rx_id, &[1, 9]).unwrap();
    r.on_frame(&sf, 0).unwrap();
    assert_eq!(r.on_frame(&sf, 1), Err(Error::OutputFull));
    events(&mut r);
    r.on_frame(&sf, 1).unwrap();
    assert_eq!(
        events(&mut r),
        vec![Event::RxComplete {
            generation: 0,
            pdu: vec![9]
        }]
    );
}

/// Sender-side frames toward `b` are built with `c` (its tx_id is b's rx_id).
fn pattern(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|i| (i as u8).wrapping_mul(7).wrapping_add(seed))
        .collect()
}

/// Feed an FF to the receiver and take the CTS it asks to send (not yet submitted).
fn held_cts(b: &mut Engine, c: &IsoTpConfig, pdu: &[u8], now: u64) -> FrameRequest {
    b.on_frame(&encode_ff(c, pdu).unwrap(), now).unwrap();
    events(b);
    let fc = b.pop_frame().unwrap();
    assert!(matches!(
        decode(&fc.frame),
        Ok(Pci::FlowControl {
            status: FlowStatus::Continue,
            ..
        })
    ));
    fc
}

fn feed_cfs(b: &mut Engine, c: &IsoTpConfig, pdu: &[u8], now: u64) {
    let ff_part = c.tx_dl as usize - 2;
    for (i, chunk) in pdu[ff_part..].chunks(c.tx_dl as usize - 1).enumerate() {
        let sn = ((i + 1) & 15) as u8;
        b.on_frame(&encode_cf(c, sn, chunk).unwrap(), now).unwrap();
    }
}

#[test]
fn stale_fc_handle_after_rx_replacement_cannot_touch_new_receive() {
    // Regression for the generation-less case: the receive state is replaced inside the
    // same generation while the caller still holds the old FC request.
    let c = config(false, 8);
    for how in ["new_ff", "sf_then_ff", "timeout_then_ff", "early_cf_then_ff"] {
        let (_, mut b) = pair(&c);
        let old = pattern(20, 1);
        let stale = held_cts(&mut b, &c, &old, 0);
        assert!(b.is_frame_pending(stale.handle), "{how}");
        let mut t = 10;
        match how {
            "sf_then_ff" => b.on_frame(&encode_sf(&c, &[9]).unwrap(), t).unwrap(),
            "timeout_then_ff" => {
                t = 1_000_000_000;
                b.on_timer(t).unwrap();
            }
            // CF before our CTS was even submitted: fails the old receive.
            "early_cf_then_ff" => b.on_frame(&encode_cf(&c, 1, &[0; 7]).unwrap(), t).unwrap(),
            _ => {}
        }
        let fresh_pdu = pattern(30, 2);
        let fresh = held_cts(&mut b, &c, &fresh_pdu, t + 1);
        assert!(!b.is_frame_pending(stale.handle), "{how}");
        assert!(b.is_frame_pending(fresh.handle), "{how}");
        let deadline = b.poll_at();
        assert_eq!(deadline, Some(t + 1 + 1_000_000_000), "{how}");

        // The stale handle is ignored: no error, no event, no timer change.
        let ignored = b.stats().ignored_callbacks;
        b.on_frame_submitted_at(stale.handle, t + 2).unwrap();
        b.on_frame_confirmed(stale.handle).unwrap();
        assert_eq!(b.stats().ignored_callbacks, ignored + 2, "{how}");
        assert!(events(&mut b).is_empty(), "{how}");
        assert_eq!(b.poll_at(), deadline, "{how}");
        assert!(b.is_frame_pending(fresh.handle), "{how}");

        // The new receive completes once its own CTS goes out.
        submitted(&mut b, fresh, t + 3, false);
        feed_cfs(&mut b, &c, &fresh_pdu, t + 4);
        assert_eq!(
            events(&mut b),
            vec![Event::RxComplete {
                generation: 0,
                pdu: fresh_pdu
            }],
            "{how}"
        );
    }
}

#[test]
fn stale_cts_does_not_unlock_cf_for_new_receive() {
    let c = config(false, 8);
    let (_, mut b) = pair(&c);
    let stale = held_cts(&mut b, &c, &pattern(20, 1), 0);
    let _fresh = held_cts(&mut b, &c, &pattern(30, 2), 1);
    events(&mut b);
    b.on_frame_submitted_at(stale.handle, 2).unwrap();
    // The new receive never sent its CTS, so a CF is still unexpected.
    b.on_frame(&encode_cf(&c, 1, &[0; 7]).unwrap(), 3).unwrap();
    assert_eq!(
        events(&mut b),
        vec![Event::RxError {
            generation: 0,
            error: Error::UnexpectedPdu
        }]
    );
}

#[test]
fn stale_fc_handle_after_abort_is_fenced_by_generation() {
    let c = config(false, 8);
    let (_, mut b) = pair(&c);
    let stale = held_cts(&mut b, &c, &pattern(20, 1), 0);
    b.abort(1).unwrap();
    events(&mut b);
    let fresh = held_cts(&mut b, &c, &pattern(30, 2), 2);
    assert_ne!(stale.handle.generation, fresh.handle.generation);
    assert!(!b.is_frame_pending(stale.handle));
    b.on_frame_submitted_at(stale.handle, 3).unwrap();
    assert_eq!(b.stats().ignored_callbacks, 1);
    assert!(b.is_frame_pending(fresh.handle));
}

#[test]
fn overflow_controls_retire_on_submission_without_confirms() {
    // A backend without TX confirms must not leak OVFLW control records
    // (they used to fill MAX_CONTROLS and then refuse every FF).
    let mut c = config(false, 8);
    c.max_pdu = 16;
    let (_, mut b) = pair(&c);
    let big = encode_ff(&config(false, 8), &[0; 17]).unwrap();
    for i in 0..(MAX_CONTROLS as u64 * 3) {
        b.on_frame(&big, i).unwrap();
        events(&mut b);
        let ovf = b.pop_frame().unwrap();
        assert!(matches!(
            decode(&ovf.frame),
            Ok(Pci::FlowControl {
                status: FlowStatus::Overflow,
                ..
            })
        ));
        b.on_frame_submitted_at(ovf.handle, i).unwrap();
        assert!(!b.is_frame_pending(ovf.handle));
    }
    // Two queued OVFLW share token 0: submitting one must not drop the other.
    let t = 1_000;
    b.on_frame(&big, t).unwrap();
    b.on_frame(&big, t).unwrap();
    events(&mut b);
    let first = b.pop_frame().unwrap();
    b.on_frame_submitted_at(first.handle, t).unwrap();
    let second = b.pop_frame().expect("second OVFLW still queued");
    assert!(b.is_frame_pending(second.handle));
    b.on_frame_submitted_at(second.handle, t).unwrap();
    // A normal receive still works afterwards.
    let pdu = pattern(16, 3);
    let cts = held_cts(&mut b, &c, &pdu, t + 1);
    submitted(&mut b, cts, t + 1, false);
    feed_cfs(&mut b, &c, &pdu, t + 2);
    assert_eq!(events(&mut b), vec![Event::RxComplete { generation: 0, pdu }]);

    // Same for the OVFLW fault.
    let mut f = config(false, 8);
    f.faults.fc_overflow = true;
    let (_, mut r) = pair(&f);
    for i in 0..(MAX_CONTROLS as u64 * 2) {
        r.on_frame(&encode_ff(&f, &[1; 20]).unwrap(), i).unwrap();
        events(&mut r);
        let ovf = r.pop_frame().unwrap();
        r.on_frame_submitted_at(ovf.handle, i).unwrap();
    }
}

#[test]
fn escape_ff_above_4095_answers_overflow() {
    for fd in [false, true] {
        let c = config(fd, if fd { 64 } else { 8 });
        let (_, mut b) = pair(&c);
        let mut data = vec![0u8; if fd { 64 } else { 8 }];
        data[..6].copy_from_slice(&[0x10, 0, 0, 0, 0x10, 0]); // escape FF_DL = 4096
        let ff = if fd {
            CanFrame::fd(c.tx_id, &data, false, false).unwrap()
        } else {
            CanFrame::classic(c.tx_id, &data).unwrap()
        };
        b.on_frame(&ff, 0).unwrap();
        assert_eq!(
            events(&mut b),
            vec![Event::RxError {
                generation: 0,
                error: Error::PduTooLarge { len: 4096, max: 4095 }
            }]
        );
        let fc = b.pop_frame().unwrap();
        assert!(matches!(
            decode(&fc.frame),
            Ok(Pci::FlowControl {
                status: FlowStatus::Overflow,
                ..
            })
        ));
        assert!(!b.is_receiving());
        // Escape used for a length that fits 12 bits is malformed: error, no FC.
        data[..6].copy_from_slice(&[0x10, 0, 0, 0, 0x0F, 0xFF]);
        let bad = if fd {
            CanFrame::fd(c.tx_id, &data, false, false).unwrap()
        } else {
            CanFrame::classic(c.tx_id, &data).unwrap()
        };
        b.on_frame(&bad, 1).unwrap();
        assert_eq!(
            events(&mut b),
            vec![Event::RxError {
                generation: 0,
                error: Error::InvalidLength
            }]
        );
        assert!(b.pop_frame().is_none());
    }
}

#[test]
fn reserved_stmin_is_refused_in_config_and_means_127ms_on_receive() {
    for v in [0x80u8, 0xF0, 0xFA, 0xFF] {
        let mut c = config(false, 8);
        c.st_min = v;
        assert_eq!(
            Engine::new(c).unwrap_err(),
            Error::InvalidConfig("st_min reserved value")
        );
    }
    for v in [0x80u8, 0xFA] {
        let c = config(false, 8);
        let (mut a, b) = pair(&c);
        a.send(&[3; 20], 0).unwrap();
        let ff = a.pop_frame().unwrap();
        submitted(&mut a, ff, 0, false);
        let fc = encode_fc(b.config(), FlowStatus::Continue, 0, v).unwrap();
        a.on_frame(&fc, 0).unwrap();
        let first = a.pop_frame().unwrap();
        submitted(&mut a, first, 0, false);
        assert_eq!(a.poll_at(), Some(127_000_000), "STmin {v:02X}");
        a.on_timer(126_999_999).unwrap();
        assert!(a.pop_frame().is_none());
        a.on_timer(127_000_000).unwrap();
        assert!(a.pop_frame().is_some());
    }
}
