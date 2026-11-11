//! Fault injection: reply order and timing for each action, rule limits, network fan-out.

use std::time::Duration;

use loom_uds_codec::{ResponseKind, classify_response};
use loom_uds_sim::{
    FaultAction, FaultMatch, FaultRule, MAX_FAULT_RULES, MAX_REPLIES_PER_REQUEST, MAX_SIM_ECUS, SimEcu,
    SimNetwork,
};

fn ms(v: u64) -> Duration {
    Duration::from_millis(v)
}

fn sched(ecu: &mut SimEcu, req: &[u8]) -> Vec<(u64, Vec<u8>)> {
    ecu.handle(ms(0), req)
        .into_vec()
        .into_iter()
        .map(|r| (u64::try_from(r.delay.as_millis()).expect("small"), r.bytes))
        .collect()
}

#[test]
fn pending_then_final() -> Result<(), loom_uds_sim::SimError> {
    let mut ecu = SimEcu::bench();
    ecu.add_fault(FaultRule::times(
        FaultMatch::sid_id(0x22, 0xF190),
        FaultAction::PendingThen {
            count: 3,
            every: ms(1000),
        },
        1,
    ))?;
    let s = sched(&mut ecu, &[0x22, 0xF1, 0x90]);
    assert_eq!(s.len(), 4);
    for (i, (at, bytes)) in s.iter().take(3).enumerate() {
        assert_eq!(
            (*at, bytes.as_slice()),
            (i as u64 * 1000, &[0x7F, 0x22, 0x78][..])
        );
        assert_eq!(
            classify_response(&[0x22, 0xF1, 0x90], bytes),
            ResponseKind::Pending
        );
    }
    assert_eq!(s[3].0, 3000);
    assert_eq!(
        classify_response(&[0x22, 0xF1, 0x90], &s[3].1),
        ResponseKind::Positive
    );
    assert_eq!(sched(&mut ecu, &[0x22, 0xF1, 0x90]).len(), 1, "rule fired once");
    Ok(())
}

#[test]
fn pending_count_is_clamped() -> Result<(), loom_uds_sim::SimError> {
    let mut ecu = SimEcu::bench();
    ecu.add_fault(FaultRule::always(
        FaultMatch::sid(0x22),
        FaultAction::PendingThen {
            count: 255,
            every: ms(1),
        },
    ))?;
    assert_eq!(
        sched(&mut ecu, &[0x22, 0xF1, 0x90]).len(),
        MAX_REPLIES_PER_REQUEST
    );
    Ok(())
}

#[test]
fn force_nrc_does_not_execute() -> Result<(), loom_uds_sim::SimError> {
    let mut ecu = SimEcu::bench();
    ecu.add_fault(FaultRule::always(
        FaultMatch::sid(0x10),
        FaultAction::ForceNrc(0x22),
    ))?;
    assert_eq!(sched(&mut ecu, &[0x10, 0x03]), [(0, vec![0x7F, 0x10, 0x22])]);
    assert_eq!(ecu.session(), 0x01);
    Ok(())
}

#[test]
fn drop_executes_without_reply() -> Result<(), loom_uds_sim::SimError> {
    let mut ecu = SimEcu::bench();
    sched(&mut ecu, &[0x10, 0x03]);
    ecu.add_fault(FaultRule::times(
        FaultMatch::sid_id(0x2E, 0xF1A0),
        FaultAction::Drop,
        1,
    ))?;
    assert!(sched(&mut ecu, &[0x2E, 0xF1, 0xA0, 0x09]).is_empty());
    assert_eq!(
        ecu.did(0xF1A0),
        Some(&[0x09][..]),
        "write happened: outcome unknown to the tester"
    );
    Ok(())
}

#[test]
fn delay_late_and_wrong_echo() -> Result<(), loom_uds_sim::SimError> {
    let mut ecu = SimEcu::bench();
    ecu.add_fault(FaultRule::times(
        FaultMatch::sid(0x22),
        FaultAction::Delay(ms(20)),
        1,
    ))?;
    ecu.add_fault(FaultRule::times(
        FaultMatch::sid(0x22),
        FaultAction::Late { after: ms(6000) },
        1,
    ))?;
    ecu.add_fault(FaultRule::times(FaultMatch::sid(0x22), FaultAction::WrongEcho, 1))?;
    let req = [0x22, 0xF1, 0xA0];
    assert_eq!(sched(&mut ecu, &req)[0].0, 20);
    assert_eq!(sched(&mut ecu, &req)[0].0, 6000);
    let wrong = sched(&mut ecu, &req);
    assert_eq!(wrong[0].1, [0x62, 0xF1, 0xA1, 0x01]);
    assert!(matches!(
        classify_response(&req, &wrong[0].1),
        ResponseKind::Foreign { .. }
    ));
    assert_eq!(
        sched(&mut ecu, &req)[0],
        (0, vec![0x62, 0xF1, 0xA0, 0x01]),
        "all rules used up"
    );
    Ok(())
}

#[test]
fn sub_function_match_ignores_sprmib() -> Result<(), loom_uds_sim::SimError> {
    let mut ecu = SimEcu::bench();
    ecu.add_fault(FaultRule::always(
        FaultMatch::sid_id(0x3E, 0x00),
        FaultAction::ForceNrc(0x21),
    ))?;
    assert_eq!(sched(&mut ecu, &[0x3E, 0x80]), [(0, vec![0x7F, 0x3E, 0x21])]);
    Ok(())
}

#[test]
fn rule_limit() {
    let mut ecu = SimEcu::bench();
    for _ in 0..MAX_FAULT_RULES {
        assert!(
            ecu.add_fault(FaultRule::always(FaultMatch::sid(0x22), FaultAction::Drop))
                .is_ok()
        );
    }
    assert!(
        ecu.add_fault(FaultRule::always(FaultMatch::sid(0x22), FaultAction::Drop))
            .is_err()
    );
    ecu.clear_faults();
    assert_eq!(sched(&mut ecu, &[0x22, 0xF1, 0xA0]).len(), 1);
}

#[test]
fn network_physical_and_functional() -> Result<(), loom_uds_sim::SimError> {
    let mut net = SimNetwork::new();
    net.insert(0x1001, SimEcu::bench())?;
    net.insert(0x1002, SimEcu::bench())?;
    assert!(net.handle_physical(0x1003, ms(0), &[0x22, 0xF1, 0x90]).is_none());
    let r = net
        .handle_physical(0x1001, ms(0), &[0x22, 0xF1, 0x90])
        .expect("ECU exists");
    assert_eq!(r.len(), 1);
    // Functional TesterPresent: both answer.
    assert_eq!(net.handle_functional(ms(1), &[0x3E, 0x00]).len(), 2);
    // Functional request for an unsupported service: NRC 0x11 is suppressed for functional.
    assert!(net.handle_functional(ms(2), &[0x31, 0x01, 0xFF, 0x00]).is_empty());
    // Functional with a DID one ECU lacks: 0x31 suppressed there, the other answers.
    net.get_mut(0x1002).expect("present").clear_faults();
    let mut only_one = SimEcu::bench();
    only_one.add_fault(FaultRule::always(
        FaultMatch::sid(0x22),
        FaultAction::ForceNrc(0x22),
    ))?;
    net.insert(0x1002, only_one)?;
    let answers = net.handle_functional(ms(3), &[0x22, 0xF1, 0x90]);
    assert_eq!(
        answers.len(),
        2,
        "forced NRCs are not suppressed (fault overrides)"
    );
    Ok(())
}

#[test]
fn network_limit() -> Result<(), loom_uds_sim::SimError> {
    let mut net = SimNetwork::new();
    for a in 0..u16::try_from(MAX_SIM_ECUS).expect("small") {
        net.insert(a, SimEcu::bench())?;
    }
    assert!(net.insert(0xFFFF, SimEcu::bench()).is_err());
    assert!(
        net.insert(0, SimEcu::bench()).is_ok(),
        "replacing an existing address is fine"
    );
    Ok(())
}
