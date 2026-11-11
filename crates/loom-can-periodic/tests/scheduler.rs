#![allow(clippy::unwrap_used, clippy::expect_used)]

use loom_can_frame::{CanFrame, CanId};
use loom_can_periodic::*;

const MS: u64 = 1_000_000;
fn spec(id: u32, origin: Origin) -> JobSpec {
    JobSpec::new(
        "ch0",
        CanFrame::classic(CanId::standard(id).unwrap(), &[0, 0]).unwrap(),
        10 * MS,
        origin,
        "owner",
    )
}
fn drain(s: &mut Scheduler) -> Vec<Event> {
    let mut out = Vec::new();
    while let Some(event) = s.pop_event() {
        out.push(event);
    }
    out
}
fn submit(s: &mut Scheduler, now: u64) -> Due {
    assert_eq!(s.poll(now), Ok(1));
    let due = s.pop_due().unwrap();
    assert!(s.is_due_current(&due, now));
    s.on_dispatch_submitted(due.dispatch, now).unwrap();
    due
}

#[test]
fn ten_ms_one_second_exact_grid_and_offsets() {
    let mut s = Scheduler::new();
    let id = s.start(spec(0x100, Origin::Agent)).unwrap();
    assert_eq!(s.poll(0), Ok(0));
    for tick in 1..=100 {
        let due = submit(&mut s, tick * 10 * MS);
        assert_eq!(due.scheduled_ns, tick * 10 * MS);
        assert_eq!(due.lateness_ns, 0);
    }
    assert_eq!(s.job(id).unwrap().stats.sent, 100);
    assert_eq!(s.poll_at(), Some(1010 * MS));
    assert_eq!(s.job(id).unwrap().stats.p99_lateness_ns, Some(0));
    let mut offset = spec(0x101, Origin::User);
    offset.offset_ns = 3 * MS;
    let mut s = Scheduler::new();
    s.start_at(offset, 100 * MS).unwrap();
    assert_eq!(s.poll_at(), Some(113 * MS));
    assert_eq!(s.poll(112 * MS), Ok(0));
    assert_eq!(submit(&mut s, 113 * MS).scheduled_ns, 113 * MS);
}

#[test]
fn late_poll_skips_three_without_burst_or_drift() {
    let mut s = Scheduler::new();
    let id = s.start(spec(1, Origin::Agent)).unwrap();
    let due = submit(&mut s, 45 * MS);
    assert_eq!(due.scheduled_ns, 10 * MS);
    assert_eq!(due.lateness_ns, 35 * MS);
    assert_eq!(s.job(id).unwrap().stats.missed, 3);
    assert_eq!(s.job(id).unwrap().stats.max_lateness_ns, 35 * MS);
    assert_eq!(s.poll_at(), Some(50 * MS));
    assert_eq!(s.poll(45 * MS), Ok(0));
    assert_eq!(submit(&mut s, 50 * MS).scheduled_ns, 50 * MS);
}

#[test]
fn delayed_submission_and_pending_backpressure_never_catch_up() {
    for poll_while_pending in [false, true] {
        let mut s = Scheduler::new();
        let id = s.start(spec(1, Origin::Agent)).unwrap();
        s.poll(10 * MS).unwrap();
        let old = s.pop_due().unwrap();
        if poll_while_pending {
            assert_eq!(s.poll(45 * MS), Ok(0));
        }
        s.on_dispatch_submitted(old.dispatch, 45 * MS).unwrap();
        assert_eq!(s.job(id).unwrap().stats.missed, 3);
        assert_eq!(s.poll(45 * MS), Ok(0));
        assert_eq!(s.poll_at(), Some(50 * MS));
        assert_eq!(submit(&mut s, 50 * MS).scheduled_ns, 50 * MS);
    }
}

#[test]
fn patch_cas_is_atomic_on_next_actual_tick() {
    let mut s = Scheduler::new();
    let id = s.start(spec(1, Origin::User)).unwrap();
    let first = submit(&mut s, 10 * MS);
    assert_eq!(s.patch(id, 0, Patch::masked(0, &[0x0F], &[0xAB]).unwrap()), Ok(1));
    assert_eq!(
        s.patch(id, 0, Patch::replace(&[9, 9]).unwrap()),
        Err(Error::RevisionConflict { current: 1 })
    );
    assert_eq!(s.job(id).unwrap().spec.frame.data(), &[0, 0]);
    assert_eq!(s.job(id).unwrap().effective_revision, 0);
    assert_eq!(s.patch(id, 1, Patch::masked(1, &[0xF0], &[0xCD]).unwrap()), Ok(2));
    assert_eq!(first.frame.data(), &[0, 0]);
    let due = submit(&mut s, 20 * MS);
    assert_eq!(due.frame.data(), &[0x0B, 0xC0]);
    assert_eq!(due.revision, 2);
    assert_eq!(s.pending_patches(), 0);
    assert_eq!(s.job(id).unwrap().effective_revision, 2);
}

#[test]
fn queued_and_removed_due_do_not_change_when_patched() {
    let mut s = Scheduler::new();
    let id = s.start(spec(1, Origin::Agent)).unwrap();
    s.poll(10 * MS).unwrap();
    let old = s.pop_due().unwrap();
    s.patch(id, 0, Patch::replace(&[7, 8]).unwrap()).unwrap();
    assert!(s.is_due_current(&old, 10 * MS));
    assert_eq!(s.poll(20 * MS), Ok(0));
    assert_eq!(s.pending_patches(), 1);
    assert_eq!(s.job(id).unwrap().effective_revision, 0);
    s.on_dispatch_submitted(old.dispatch, 20 * MS).unwrap();
    assert_eq!(old.frame.data(), &[0, 0]);
    let due = submit(&mut s, 30 * MS);
    assert_eq!(due.revision, 1);
    assert_eq!(due.frame.data(), &[7, 8]);
}

#[test]
fn patch_validates_fd_lengths_flags_and_mask_ranges() {
    let mut s = Scheduler::new();
    let mut job = spec(1, Origin::User);
    job.frame = CanFrame::fd(job.frame.id(), &[0x55; 64], true, true).unwrap();
    let id = s.start(job).unwrap();
    assert_eq!(
        s.patch(id, 0, Patch::replace(&[0; 9]).unwrap()),
        Err(Error::InvalidPatch)
    );
    assert_eq!(s.job(id).unwrap().revision, 0);
    assert_eq!(s.pending_patches(), 0);
    s.patch(id, 0, Patch::replace(&[0xFF; 12]).unwrap()).unwrap();
    assert_eq!(
        s.patch(id, 1, Patch::masked(12, &[1], &[0]).unwrap()),
        Err(Error::InvalidPatch)
    );
    let due = submit(&mut s, 10 * MS);
    assert_eq!(due.frame.len(), 12);
    assert_eq!(
        due.frame.kind(),
        loom_can_frame::FrameKind::Fd { brs: true, esi: true }
    );
    assert!(Patch::replace(&[0; 65]).is_err());
    assert!(Patch::masked(64, &[1], &[1]).is_err());
    assert!(Patch::masked(0, &[], &[]).is_err());
    assert!(Patch::masked(0, &[1], &[1, 2]).is_err());
}

#[test]
fn patch_queue_256_cap_and_revision_do_not_grow_on_failure() {
    let mut s = Scheduler::new();
    let id = s.start(spec(1, Origin::Agent)).unwrap();
    for revision in 0..256 {
        assert_eq!(
            s.patch(id, revision, Patch::masked(0, &[255], &[revision as u8]).unwrap()),
            Ok(revision + 1)
        );
    }
    assert_eq!(
        s.patch(id, 256, Patch::replace(&[1]).unwrap()),
        Err(Error::ResourceExhausted)
    );
    assert_eq!(s.pending_patches(), 256);
    assert_eq!(s.job(id).unwrap().revision, 256);
    let due = submit(&mut s, 10 * MS);
    assert_eq!(due.revision, 256);
    assert_eq!(due.frame.data(), &[255, 0]);
    assert_eq!(s.pending_patches(), 0);
}

#[test]
fn finite_count_and_failed_attempt_semantics() {
    let mut s = Scheduler::new();
    let mut job = spec(1, Origin::Agent);
    job.count = Some(3);
    let id = s.start(job).unwrap();
    for tick in 1..=3 {
        submit(&mut s, tick * 10 * MS);
    }
    assert!(s.is_empty());
    assert_eq!(s.poll_at(), None);
    assert!(
        matches!(&drain(&mut s)[..],[Event::Stopped{job,reason:StopReason::Completed,stats,..}] if *job==id && stats.sent==3)
    );
    let mut job = spec(1, Origin::Script);
    job.count = Some(1);
    let id = s.start(job).unwrap();
    s.poll(40 * MS).unwrap();
    let due = s.pop_due().unwrap();
    s.on_send_failed(id, 40 * MS).unwrap();
    assert!(matches!(s.pop_event(),Some(Event::SendFailed{dispatch,..}) if dispatch==due.dispatch));
    assert_eq!(s.job(id).unwrap().remaining, Some(1));
    submit(&mut s, 50 * MS);
    assert!(
        matches!(s.pop_event(),Some(Event::Stopped{reason:StopReason::Completed,stats,..}) if stats.sent==1 && stats.failed==1)
    );
}

#[test]
fn lease_expiry_renew_and_immutable_lifetime_cap() {
    let mut s = Scheduler::new();
    let mut job = spec(1, Origin::Agent);
    job.lease_ns = Some(30 * MS);
    job.max_duration_ns = 100 * MS;
    let id = s.start(job).unwrap();
    assert_eq!(s.renew(id, 20 * MS), Ok(50 * MS));
    assert_eq!(s.renew(id, 40 * MS), Ok(70 * MS));
    assert_eq!(s.renew(id, 60 * MS), Ok(90 * MS));
    assert_eq!(s.renew(id, 80 * MS), Err(Error::LeaseCapExceeded));
    assert_eq!(s.job(id).unwrap().lease_end_ns, Some(90 * MS));
    s.poll(90 * MS).unwrap();
    assert!(s.is_empty());
    assert!(matches!(
        s.pop_event(),
        Some(Event::Stopped {
            reason: StopReason::LeaseExpired,
            ..
        })
    ));
    let mut s = Scheduler::new();
    let mut job = spec(1, Origin::User);
    job.lease_ns = Some(10 * MS);
    let id = s.start(job).unwrap();
    assert_eq!(s.renew(id, 10 * MS), Err(Error::Expired));
    s.poll(10 * MS).unwrap();
    assert!(s.pop_due().is_none());
}

#[test]
fn max_duration_wins_over_a_tick_and_never_restarts() {
    let mut s = Scheduler::new();
    let mut job = spec(1, Origin::User);
    job.max_duration_ns = 100 * MS;
    let id = s.start(job).unwrap();
    for tick in 1..10 {
        submit(&mut s, tick * 10 * MS);
    }
    s.poll(100 * MS).unwrap();
    assert_eq!(s.job(id), Err(Error::UnknownJob));
    assert!(s.pop_due().is_none());
    assert!(
        matches!(s.pop_event(),Some(Event::Stopped{reason:StopReason::MaxDuration,stats,..}) if stats.sent==9)
    );
}

#[test]
fn stop_all_latches_all_origins_and_clearing_does_not_resume() {
    let mut s = Scheduler::new();
    let mut held = Vec::new();
    for (i, origin) in [Origin::Agent, Origin::Script, Origin::User]
        .into_iter()
        .enumerate()
    {
        s.start(spec(i as u32, origin)).unwrap();
    }
    s.poll(10 * MS).unwrap();
    while let Some(d) = s.pop_due() {
        held.push(d);
    }
    s.latch(true).unwrap();
    assert!(s.is_empty());
    assert!(s.is_latched());
    for old in &held {
        assert!(!s.is_due_current(old, 10 * MS));
        assert!(s.on_dispatch_submitted(old.dispatch, 10 * MS).is_err());
    }
    assert_eq!(drain(&mut s).len(), 3);
    for origin in [Origin::Agent, Origin::Script, Origin::User] {
        assert_eq!(s.start(spec(1, origin)), Err(Error::Latched));
    }
    s.latch(false).unwrap();
    s.poll(100 * MS).unwrap();
    assert!(s.pop_due().is_none());
    assert!(s.is_empty());
    assert!(s.start(spec(1, Origin::Agent)).is_ok());
}

#[test]
fn conflict_key_includes_channel_and_identifier_format() {
    let mut s = Scheduler::new();
    s.start(spec(1, Origin::Agent)).unwrap();
    assert_eq!(
        s.start(spec(1, Origin::User)),
        Err(Error::Conflict {
            owner: "owner".into()
        })
    );
    let mut other = spec(1, Origin::User);
    other.channel = "ch1".into();
    assert!(s.start(other).is_ok());
    let mut extended = spec(1, Origin::User);
    extended.frame = CanFrame::classic(CanId::extended(1).unwrap(), &[0]).unwrap();
    assert!(s.start(extended).is_ok());
}

#[test]
fn takeover_invalidates_old_work_and_preserves_cap_and_grid() {
    let mut s = Scheduler::new();
    let mut job = spec(1, Origin::Agent);
    job.lease_ns = Some(50 * MS);
    job.max_duration_ns = 100 * MS;
    let old = s.start(job).unwrap();
    s.poll(10 * MS).unwrap();
    let held = s.pop_due().unwrap();
    s.patch(old, 0, Patch::replace(&[9]).unwrap()).unwrap();
    let new = s.takeover(old, "human", Origin::User).unwrap();
    assert_ne!(old, new);
    assert!(!s.is_due_current(&held, 10 * MS));
    assert_eq!(s.pending_patches(), 0);
    assert!(
        matches!(s.pop_event(),Some(Event::Stopped{job,reason:StopReason::Takeover,stats,..}) if job==old && stats.cancelled==1)
    );
    let current = s.job(new).unwrap();
    assert_eq!(current.spec.owner, "human");
    assert_eq!(current.max_end_ns, 100 * MS);
    assert_eq!(current.lease_end_ns, Some(50 * MS));
    assert_eq!(current.revision, 0);
    assert_eq!(submit(&mut s, 20 * MS).frame.data(), &[0, 0]);
}

#[test]
fn pause_resume_is_explicit_and_stale_callbacks_cannot_touch_new_due() {
    let mut s = Scheduler::new();
    let agent = s.start(spec(1, Origin::Agent)).unwrap();
    let user = s.start(spec(2, Origin::User)).unwrap();
    s.poll(10 * MS).unwrap();
    let mut held = Vec::new();
    while let Some(d) = s.pop_due() {
        held.push(d);
    }
    let old = held.iter().find(|d| d.job == agent).unwrap();
    let user_due = held.iter().find(|d| d.job == user).unwrap();
    assert_eq!(s.pause_channel("ch0", PauseReason::BusOff), Ok(1));
    assert!(!s.is_due_current(old, 10 * MS));
    assert!(s.is_due_current(user_due, 10 * MS));
    s.on_dispatch_submitted(user_due.dispatch, 10 * MS).unwrap();
    assert!(matches!(
        s.pop_event(),
        Some(Event::Stopped {
            reason: StopReason::BusOff,
            resumable: true,
            ..
        })
    ));
    s.poll(45 * MS).unwrap();
    let running = s.pop_due().unwrap();
    assert_eq!(running.job, user);
    s.on_dispatch_submitted(running.dispatch, 45 * MS).unwrap();
    assert_eq!(s.job(agent).unwrap().stats.sent, 0);
    assert_eq!(s.resume_channel("ch0"), Ok(1));
    drain(&mut s);
    assert_eq!(s.job(agent).unwrap().stats.paused_skipped, 3);
    s.poll(50 * MS).unwrap();
    let mut next = Vec::new();
    while let Some(d) = s.pop_due() {
        next.push(d);
    }
    let resumed = next.iter().find(|d| d.job == agent).unwrap();
    assert_ne!(old.dispatch, resumed.dispatch);
    assert_eq!(
        s.on_dispatch_submitted(old.dispatch, 50 * MS),
        Err(Error::StaleDispatch)
    );
    assert!(s.is_due_current(resumed, 50 * MS));
}

#[test]
fn paused_channels_bound_new_jobs_and_expired_jobs_do_not_resume() {
    let mut s = Scheduler::new();
    s.pause_channel("ch0", PauseReason::ErrorPassive).unwrap();
    let mut job = spec(1, Origin::Script);
    job.lease_ns = Some(20 * MS);
    let id = s.start(job).unwrap();
    assert_eq!(s.job(id).unwrap().paused, Some(PauseReason::ErrorPassive));
    drain(&mut s);
    assert_eq!(s.resume_channel_at("ch0", 20 * MS), Ok(0));
    assert!(s.is_empty());
    assert!(matches!(
        s.pop_event(),
        Some(Event::Stopped {
            reason: StopReason::LeaseExpired,
            resumable: false,
            ..
        })
    ));
    let limits = Limits {
        paused_channels: 1,
        ..Limits::default()
    };
    let mut s = Scheduler::with_limits(limits).unwrap();
    s.pause_channel("a", PauseReason::BusOff).unwrap();
    assert_eq!(
        s.pause_channel("b", PauseReason::BusOff),
        Err(Error::ResourceExhausted)
    );
    s.resume_channel("a").unwrap();
    assert!(s.pause_channel("b", PauseReason::BusOff).is_ok());
}

#[test]
fn job_129_is_rejected_before_admission_and_slots_are_reclaimed() {
    let mut s = Scheduler::new();
    let mut first = None;
    for id in 0..128 {
        let job = s.start(spec(id, Origin::Agent)).unwrap();
        first = first.or(Some(job));
    }
    assert_eq!(s.len(), 128);
    assert_eq!(s.start(spec(128, Origin::Agent)), Err(Error::ResourceExhausted));
    s.stop(first.unwrap()).unwrap();
    s.pop_event();
    assert!(s.start(spec(128, Origin::Agent)).is_ok());
    assert_eq!(s.len(), 128);
}

#[test]
fn poll_output_pressure_is_atomic_including_patch_and_missed_counters() {
    let limits = Limits {
        dues: 1,
        ..Limits::default()
    };
    let mut s = Scheduler::with_limits(limits).unwrap();
    let first = s.start(spec(1, Origin::Agent)).unwrap();
    let second = s.start(spec(2, Origin::Agent)).unwrap();
    s.patch(first, 0, Patch::replace(&[1]).unwrap()).unwrap();
    assert_eq!(s.poll(45 * MS), Err(Error::OutputFull));
    assert_eq!(s.output_len(), (0, 0));
    assert_eq!(s.poll_at(), Some(10 * MS));
    let before = s.job(first).unwrap();
    assert_eq!(before.effective_revision, 0);
    assert_eq!(before.stats.missed, 0);
    assert_eq!(s.pending_patches(), 1);
    s.stop(second).unwrap();
    drain(&mut s);
    let due = submit(&mut s, 45 * MS);
    assert_eq!(due.revision, 1);
    assert_eq!(s.job(first).unwrap().stats.missed, 3);
}

#[test]
fn event_pressure_does_not_partially_latch_pause_stop_or_expire() {
    let mut s = Scheduler::with_limits(Limits {
        events: 1,
        ..Limits::default()
    })
    .unwrap();
    let first = s.start(spec(1, Origin::Agent)).unwrap();
    let second = s.start(spec(2, Origin::Script)).unwrap();
    assert_eq!(s.latch(true), Err(Error::OutputFull));
    assert!(!s.is_latched());
    assert_eq!(s.len(), 2);
    assert_eq!(
        s.pause_channel("ch0", PauseReason::BusOff),
        Err(Error::OutputFull)
    );
    assert_eq!(s.job(first).unwrap().paused, None);
    s.stop(first).unwrap();
    assert_eq!(s.stop(second), Err(Error::OutputFull));
    assert_eq!(s.len(), 1);
    drain(&mut s);
    s.latch(true).unwrap();
    assert!(s.is_empty());
    let mut s = Scheduler::with_limits(Limits {
        events: 1,
        ..Limits::default()
    })
    .unwrap();
    let mut a = spec(1, Origin::Agent);
    a.max_duration_ns = 10 * MS;
    let mut b = a.clone();
    b.frame = spec(2, Origin::Agent).frame;
    s.start(a).unwrap();
    s.start(b).unwrap();
    assert_eq!(s.poll(10 * MS), Err(Error::OutputFull));
    assert_eq!(s.len(), 2);
    assert_eq!(s.poll_at(), Some(10 * MS));
}

#[test]
fn callback_output_pressure_preserves_pending_dispatch_and_statistics() {
    let mut s = Scheduler::with_limits(Limits {
        events: 1,
        ..Limits::default()
    })
    .unwrap();
    let mut a = spec(1, Origin::Agent);
    a.count = Some(1);
    let id = s.start(a).unwrap();
    let other = s.start(spec(2, Origin::User)).unwrap();
    s.poll(10 * MS).unwrap();
    let due = s.pop_due().unwrap();
    s.stop(other).unwrap();
    assert_eq!(
        s.on_dispatch_submitted(due.dispatch, 10 * MS),
        Err(Error::OutputFull)
    );
    assert_eq!(s.job(id).unwrap().stats.sent, 0);
    assert!(s.is_due_current(&due, 10 * MS));
    drain(&mut s);
    s.on_dispatch_submitted(due.dispatch, 10 * MS).unwrap();
    assert!(s.is_empty());
}

#[test]
fn lifetime_predicate_invalidates_removed_due_before_poll() {
    let mut s = Scheduler::new();
    let mut job = spec(1, Origin::Agent);
    job.lease_ns = Some(15 * MS);
    let id = s.start(job).unwrap();
    s.poll(10 * MS).unwrap();
    let due = s.pop_due().unwrap();
    assert!(!s.is_due_current(&due, 15 * MS));
    s.on_dispatch_submitted(due.dispatch, 16 * MS).unwrap();
    assert!(
        matches!(s.pop_event(),Some(Event::Stopped{job,reason:StopReason::LeaseExpired,stats,..}) if job==id && stats.sent==1 && stats.expired_submissions==1)
    );
}

#[test]
fn bounds_clocks_remote_frames_and_timestamp_overflow() {
    let mut s = Scheduler::new();
    let mut bad = spec(1, Origin::Agent);
    bad.period_ns = 0;
    assert_eq!(s.start(bad), Err(Error::InvalidSpec));
    let mut bad = spec(1, Origin::Agent);
    bad.channel = "x".repeat(65);
    assert_eq!(s.start(bad), Err(Error::InvalidName));
    let mut bad = spec(1, Origin::Agent);
    bad.owner = "x".repeat(129);
    assert_eq!(s.start(bad), Err(Error::InvalidName));
    let mut bad = spec(1, Origin::Agent);
    bad.count = Some(0);
    assert_eq!(s.start(bad), Err(Error::InvalidSpec));
    assert_eq!(
        s.start_at(spec(1, Origin::Agent), u64::MAX - 1),
        Err(Error::TimeOverflow)
    );
    let id = s.start_at(spec(1, Origin::Agent), 100 * MS).unwrap();
    assert_eq!(s.poll(99 * MS), Err(Error::TimeWentBackwards));
    assert_eq!(s.renew(id, 100 * MS), Err(Error::NoLease));
    assert!(
        Scheduler::with_limits(Limits {
            jobs: 129,
            ..Limits::default()
        })
        .is_err()
    );
    let mut remote = spec(2, Origin::User);
    remote.frame = CanFrame::remote(remote.frame.id(), 8).unwrap();
    let id = s.start(remote).unwrap();
    assert_eq!(
        s.patch(id, 0, Patch::replace(&[1]).unwrap()),
        Err(Error::InvalidPatch)
    );
    let mut edge = Scheduler::new();
    let mut job = spec(1, Origin::User);
    job.period_ns = 1;
    job.max_duration_ns = 10;
    let id = edge.start_at(job, u64::MAX - 10).unwrap();
    edge.poll(u64::MAX - 1).unwrap();
    let due = edge.pop_due().unwrap();
    edge.on_submitted(id, u64::MAX - 1).unwrap();
    assert_eq!(due.lateness_ns, 8);
    assert_eq!(edge.poll_at(), Some(u64::MAX));
    edge.poll(u64::MAX).unwrap();
    assert!(edge.is_empty());
}

#[test]
fn mixed_128_distinct_periods_one_virtual_minute() {
    let mut s = Scheduler::new();
    let mut jobs = Vec::with_capacity(128);
    let mut counts = [0u64; 128];
    let end = 60_000 * MS;
    for i in 0..128 {
        let mut job = spec(i, Origin::Agent);
        job.period_ns = (u64::from(i) + 1) * 10 * MS;
        jobs.push(s.start(job).unwrap());
    }
    let mut last = (0, JobId(0));
    while let Some(now) = s.poll_at().filter(|now| *now <= end) {
        s.poll(now).unwrap();
        while let Some(due) = s.pop_due() {
            assert!((due.scheduled_ns, due.job) > last);
            last = (due.scheduled_ns, due.job);
            let i = due.frame.id().raw() as usize;
            counts[i] += 1;
            assert_eq!(due.scheduled_ns, counts[i] * (i as u64 + 1) * 10 * MS);
            assert!(s.is_due_current(&due, now));
            s.on_dispatch_submitted(due.dispatch, now).unwrap();
        }
    }
    for i in 0..128 {
        assert_eq!(counts[i], end / ((i as u64 + 1) * 10 * MS));
        let stats = s.job(jobs[i]).unwrap().stats;
        assert_eq!(stats.sent, counts[i]);
        assert_eq!(stats.missed, 0);
        assert_eq!(stats.max_lateness_ns, 0);
    }
    assert!(drain(&mut s).is_empty());
}

#[test]
#[ignore = "optional macOS/host timer smoke: approximately five seconds, no hardware"]
fn realtime_10ms_500_submissions() {
    std::thread::spawn(|| {
        let mut s=Scheduler::new();let mut job=spec(1,Origin::Agent);job.count=Some(500);s.start(job).unwrap();
        let start=std::time::Instant::now();let mut samples=Vec::with_capacity(500);let mut intervals=Vec::with_capacity(500);let mut previous=0;
        loop {
            let Some(deadline)=s.poll_at() else {break;};let target=start+std::time::Duration::from_nanos(deadline);let before=std::time::Instant::now();
            if before<target {std::thread::sleep(target-before);}
            let actual=u64::try_from(start.elapsed().as_nanos()).unwrap();assert!(actual<9_000*MS,"smoke exceeded 9s");
            s.poll(actual).unwrap();
            while let Some(due)=s.pop_due() {
                assert!(samples.len()<500);assert!(s.is_due_current(&due,actual));samples.push(actual-due.scheduled_ns);
                intervals.push((actual-previous).abs_diff(10*MS));previous=actual;s.on_dispatch_submitted(due.dispatch,actual).unwrap();
            }
        }
        assert_eq!(samples.len(),500);samples.sort_unstable();intervals.sort_unstable();
        let event=s.pop_event().unwrap();let Event::Stopped{stats,..}=event else {panic!("missing completion");};
        println!("SMOKE os={} arch={} n={} elapsed_s={:.6} missed={} lateness_ns[p50={},p99={},max={}] interval_abs_error_ns[p50={},p99={},max={}] histogram_p50_ns={:?} histogram_p99_ns={:?}",
            std::env::consts::OS,std::env::consts::ARCH,samples.len(),start.elapsed().as_secs_f64(),stats.missed,samples[249],samples[494],samples[499],intervals[249],intervals[494],intervals[499],stats.p50_lateness_ns,stats.p99_lateness_ns);
    }).join().unwrap();
}
