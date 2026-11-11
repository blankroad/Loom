mod support;
use loom_domain_api::{DomainModule, Reducer, Registry};
use loom_proto::{
    error::ErrorCode,
    rpc::{Rights, Role, SubFilter},
};
use loom_test::*;
use serde_json::{Value, json};
use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};
use support::*;
#[tokio::test]
async fn registration_is_core_owned_and_rpc_surface_is_complete() {
    let h = Harness::new();
    assert!(
        loom_testkit::RegistryProbe::run(&h.module)
            .expect_err("reserved domain needs CORE wrapper")
            .contains("test")
    );
    let mut registry = Registry::for_core("test").expect("CORE namespace");
    h.module.register(&mut registry).expect("register");
    for name in loom_test::gateway::METHODS {
        assert!(registry.commands().iter().any(|c| c.method == *name));
    }
    assert_eq!(registry.event_kinds().len(), 10);
    assert_eq!(registry.reducers().len(), 1);
    assert!(
        registry
            .event_kinds()
            .iter()
            .all(|e| e.kind.split('.').count() == 2)
    );
    let spec = registry
        .commands()
        .iter()
        .find(|s| s.method == "test.start")
        .expect("handler");
    let result = spec
        .handler
        .call(&mut h.ctx(), json!({"tc":{"text":TC},"plan":{"text":PLAN}}))
        .await
        .expect("registered call");
    assert_eq!(result["state"], "RUNNING");
    let mut wrong = Registry::new("echo").expect("ordinary");
    assert!(h.module.register_into(&mut wrong).is_err());
}
#[tokio::test]
async fn f1_transitions_paused_gate_and_automatic_step_end() {
    let h = Harness::new();
    let id = h.start(PLAN).await;
    let transitions = h
        .events()
        .iter()
        .filter_map(|e| e.payload.get("state").and_then(Value::as_str).map(str::to_owned))
        .collect::<Vec<_>>();
    assert_eq!(transitions, vec!["PLANNED", "WAITING_APPROVAL", "RUNNING"]);
    h.begin(&id, "S1").await;
    h.call("test.pause", json!({"session":id})).await.expect("pause");
    assert_eq!(
        h.module
            .check_client_write("agent", Role::Agent)
            .expect_err("paused writer")
            .code,
        ErrorCode::SessionPaused
    );
    assert!(h.module.check_client_write("viewer", Role::Viewer).is_ok());
    assert_eq!(
        h.call("test.step", json!({"session":id,"id":"S2"}))
            .await
            .expect_err("paused step")
            .code,
        ErrorCode::SessionPaused
    );
    h.call("test.note", json!({"session":id,"text":"user intervention"}))
        .await
        .expect("metadata note while paused");
    h.call("test.resume", json!({"session":id}))
        .await
        .expect("resume");
    h.begin(&id, "S2").await;
    let rows = h.events();
    let end = rows
        .iter()
        .position(|r| r.kind == "test.step_ended" && r.payload["step_id"] == "S1")
        .expect("auto end");
    let next = rows
        .iter()
        .position(|r| r.kind == "test.step_started" && r.payload["step_id"] == "S2")
        .expect("next");
    assert!(end < next);
    let snap = h.module.snapshot(&id).expect("snapshot");
    assert_eq!(snap["current_step"], "S2");
    assert_eq!(snap["steps"][0]["status"], "ended");
    let e = h
        .call("test.end", json!({"session":id}))
        .await
        .expect_err("unfinished verdicts");
    assert_eq!(e.code.exit_code(), 8);
    assert_eq!(e.detail["result"], "incomplete");
    assert_eq!(e.detail["state"], "DONE");
    assert_eq!(
        h.call("test.note", json!({"session":id,"text":"after"}))
            .await
            .expect_err("immutable")
            .code,
        ErrorCode::Locked
    );
}
#[tokio::test]
async fn proof_is_required_and_notwired_never_confirms_pass() {
    let h = Harness::config(SessionConfig::default(), false);
    let id = h.start(PLAN).await;
    h.begin(&id, "S1").await;
    assert_eq!(
        h.call(
            "test.verdict",
            json!({"session":id,"step":"S1","verdict":"pass","evidence":[]})
        )
        .await
        .expect_err("zero evidence")
        .code,
        ErrorCode::EvidenceMissing
    );
    assert_eq!(
        h.verdict(&id, "S1", "pass").await.expect_err("notwired").code,
        ErrorCode::NotImplemented
    );
    let rows = h.events();
    let last = rows
        .iter()
        .rev()
        .find(|r| r.kind == "test.verdict")
        .expect("rejected event");
    assert!(last.payload["effective"].is_null());
    assert_eq!(last.payload["rejected"]["code"], "E_NOT_IMPLEMENTED");
    assert!(h.module.snapshot(&id).expect("snapshot")["steps"][0]["accepted"].is_null());
    let ok = h
        .call(
            "test.verdict",
            json!({"session":id,"step":"S1","verdict":"inconclusive","reason":"verifier unavailable"}),
        )
        .await
        .expect("record honest inconclusive");
    assert_eq!(ok["verdict"]["effective"], "inconclusive");
}
#[tokio::test]
async fn verified_verdicts_own_tx_rejection_and_exit_code_aggregation() {
    let h = Harness::new();
    let id = h.start(PLAN).await;
    h.begin(&id, "S1").await;
    let own = h
        .call(
            "test.verdict",
            json!({"session":id,"step":"S1","verdict":"pass","evidence":["uart:dut@e1:seq:99"]}),
        )
        .await
        .expect_err("own TX cannot pass");
    assert_eq!(own.code, ErrorCode::EvidenceMismatch);
    h.verdict(&id, "S1", "pass").await.expect("verified pass");
    h.begin(&id, "S2").await;
    h.verdict(&id, "S2", "pass").await.expect("pass 2");
    let end = h.call("test.end", json!({"session":id})).await.expect("passed");
    assert_eq!(end["result"], "passed");
    assert_eq!(end["exit_code"], 0);
    assert!(h.services.journal.scans.load(Ordering::Acquire) >= 3);
    let h = Harness::new();
    let id = h.start(PLAN).await;
    h.begin(&id, "S1").await;
    h.verdict(&id, "S1", "fail").await.expect("failure");
    let end = h
        .call("test.end", json!({"session":id}))
        .await
        .expect_err("failed result");
    assert_eq!(end.code, ErrorCode::TestFailed);
    assert_eq!(end.detail["exit_code"], 11);
    let h = Harness::new();
    let id = h.start(PLAN).await;
    h.begin(&id, "S1").await;
    h.verifier.mode.store(1, Ordering::Release);
    let e = h.verdict(&id, "S1", "pass").await.expect_err("gap downgrades");
    assert_eq!(e.code, ErrorCode::Partial);
    assert_eq!(e.detail["verdict"]["effective"], "inconclusive");
}
#[tokio::test]
async fn role_authority_override_record_health_and_limits() {
    let h = Harness::new();
    let id = h.start(PLAN).await;
    h.begin(&id, "S1").await;
    assert_eq!(h.call("test.verdict",json!({"session":id,"step":"S1","verdict":"pass","override":true,"reason":"fake user","origin":"user","evidence":[]})).await.expect_err("agent override").code,ErrorCode::Denied);
    let args = json!({"session":id,"step":"S1","verdict":"pass","override":true,"reason":"human inspection"});
    assert_eq!(
        h.module
            .call(&mut h.viewer(Rights::Operate), "test.verdict", args.clone())
            .await
            .expect_err("need approve")
            .code,
        ErrorCode::Denied
    );
    let accepted = h
        .module
        .call(&mut h.viewer(Rights::Approve), "test.verdict", args)
        .await
        .expect("human assertion from approved viewer");
    assert_eq!(accepted["verdict"]["user_assertion"], true);
    h.hooks.health.store(1, Ordering::Release);
    assert_eq!(
        h.verdict(&id, "S1", "pass")
            .await
            .expect_err("recording failed")
            .code,
        ErrorCode::RecordingFailed
    );
    h.hooks.health.store(0, Ordering::Release);
    let refs = vec!["uart:dut@e1:seq:1"; 65];
    assert_eq!(
        h.call(
            "test.verdict",
            json!({"session":id,"step":"S1","verdict":"pass","evidence":refs})
        )
        .await
        .expect_err("citation ceiling")
        .code,
        ErrorCode::Usage
    );
    assert_eq!(
        h.call(
            "test.note",
            json!({"session":id,"text":"x".repeat(MAX_TEXT_BYTES+1)})
        )
        .await
        .expect_err("note ceiling")
        .code,
        ErrorCode::Usage
    );
    let forged=h.call("test.verdict",json!({"session":id,"step":"S1","verdict":"pass","evidence":[{"ref":"uart:dut@e1:seq:1","pred":{"op":"invented"}}]})).await.expect_err("plan binding");
    assert_eq!(forged.code, ErrorCode::PlanMismatch);
}
#[tokio::test]
async fn policy_approval_amend_invalidation_and_pause_preservation() {
    let h = Harness::config(
        SessionConfig {
            approval: ApprovalMode::Required,
            ..Default::default()
        },
        true,
    );
    let e = h
        .call("test.start", json!({"tc":{"text":TC},"plan":{"text":PLAN}}))
        .await
        .expect_err("policy approval required");
    assert_eq!(e.code.exit_code(), 5);
    let id = e.detail["session_id"].as_str().expect("id").to_owned();
    assert_eq!(
        h.call("test.resume", json!({"session":id}))
            .await
            .expect_err("pending")
            .code,
        ErrorCode::ApprovalRequired
    );
    h.hooks.approved.store(true, Ordering::Release);
    h.call("test.resume", json!({"session":id}))
        .await
        .expect("CORE grant");
    h.begin(&id, "S1").await;
    let amended = PLAN.replace("timeout: 1s", "timeout: 2s");
    let e = h
        .call("test.plan.amend", json!({"session":id,"plan":{"text":amended}}))
        .await
        .expect_err("amend invalidates all grants");
    assert_eq!(e.code, ErrorCode::ApprovalRequired);
    let snap = h.module.snapshot(&id).expect("snapshot");
    assert_eq!(snap["state"], "WAITING_APPROVAL");
    assert_eq!(snap["revision"], "2");
    assert_eq!(snap["steps"][0]["status"], "pending");
    assert_eq!(h.services.approvals.invalidated.lock().expect("lock").len(), 1);
    let h = Harness::new();
    let id = h.start(PLAN).await;
    h.call("test.pause", json!({"session":id})).await.expect("pause");
    h.call("test.plan.amend", json!({"session":id,"plan":{"text":amended}}))
        .await
        .expect("F1 new plan");
    assert_eq!(h.module.snapshot(&id).expect("snapshot")["state"], "PAUSED");
}
#[tokio::test]
async fn retry_is_bounded_and_resets_verification_window() {
    let h = Harness::new();
    let id = h.start(PLAN).await;
    h.begin(&id, "S1").await;
    h.verdict(&id, "S1", "fail").await.expect("fail");
    h.call("test.step.end", json!({"session":id,"id":"S1"}))
        .await
        .expect("end step");
    h.call("test.retry", json!({"session":id,"step":"S1"}))
        .await
        .expect("one planned retry");
    let snap = h.module.snapshot(&id).expect("snapshot");
    assert_eq!(snap["steps"][0]["attempt"], 2);
    assert!(snap["steps"][0]["accepted"].is_null());
    h.call("test.step.end", json!({"session":id,"id":"S1"}))
        .await
        .expect("end");
    assert_eq!(
        h.call("test.retry", json!({"session":id,"step":"S1"}))
            .await
            .expect_err("budget exhausted")
            .code,
        ErrorCode::Denied
    );
    assert!(h.hooks.fences.load(Ordering::Acquire) >= 4);
}
#[tokio::test]
async fn ask_answer_timeout_and_queue_bounds_do_not_fake_user_answers() {
    let h = Harness::new();
    let id = h.start(PLAN).await;
    for i in 0..MAX_OPEN_ASKS {
        h.call(
            "test.ask",
            json!({"session":id,"question":format!("q{i}"),"options":["yes","no"],"timeout_ms":1000}),
        )
        .await
        .expect("ask");
    }
    assert_eq!(
        h.call(
            "test.ask",
            json!({"session":id,"question":"full","timeout_ms":1000})
        )
        .await
        .expect_err("ask cap")
        .code,
        ErrorCode::Locked
    );
    assert_eq!(
        h.call(
            "test.answer",
            json!({"session":id,"ask_id":"ask-1-1","value":"yes"})
        )
        .await
        .expect_err("agent cannot answer")
        .code,
        ErrorCode::Denied
    );
    h.module
        .call(
            &mut h.viewer(Rights::Operate),
            "test.answer",
            json!({"session":id,"ask_id":"ask-1-1","value":"yes"}),
        )
        .await
        .expect("viewer answer");
    let answer = h
        .call(
            "test.wait",
            json!({"session":id,"until":"answer","ask_id":"ask-1-1","timeout_ms":50}),
        )
        .await
        .expect("answer wait");
    assert!(
        answer["answers"]
            .as_array()
            .expect("answers")
            .iter()
            .any(|a| a["value"] == "yes")
    );
    let h = Harness::new();
    let id = h.start(PLAN).await;
    let ask = h
        .call(
            "test.ask",
            json!({"session":id,"question":"expire","timeout_ms":5}),
        )
        .await
        .expect("ask");
    let e = h
        .call(
            "test.wait",
            json!({"session":id,"until":"answer","ask_id":ask["ask_id"],"timeout_ms":100}),
        )
        .await
        .expect_err("expired is not an answer");
    assert_eq!(e.code, ErrorCode::Timeout);
}
#[tokio::test]
async fn stop_abort_and_finally_no_automatic_device_path() {
    let h = Harness::new();
    let cleanup = format!("{PLAN}\nfinally: [{{psu.out: {{state: off}}}}]\n");
    let id = h.start(&cleanup).await;
    h.begin(&id, "S1").await;
    h.module.set_stop(&h.ctx(), true).expect("stop integration");
    let e = h
        .call("test.end", json!({"session":id}))
        .await
        .expect_err("stopped");
    assert_eq!(e.code.exit_code(), 6);
    assert_eq!(h.hooks.finally_calls.load(Ordering::Acquire), 0);
    assert_eq!(h.module.snapshot(&id).expect("snapshot")["state"], "STOPPED");
    let h = Harness::new();
    let id = h.start(&cleanup).await;
    let e = h
        .call("test.end", json!({"session":id}))
        .await
        .expect_err("no fake cleanup");
    assert_eq!(e.code, ErrorCode::NotImplemented);
    assert_eq!(h.module.snapshot(&id).expect("snapshot")["state"], "PAUSED");
    let e = h
        .module
        .call(
            &mut h.viewer(Rights::Operate),
            "test.end",
            json!({"session":id,"abort":true,"run_finally":false}),
        )
        .await
        .expect_err("explicit abort incomplete");
    assert_eq!(e.detail["result"], "incomplete");
    assert_eq!(e.detail["state"], "STOPPED");
    assert_eq!(e.code.exit_code(), 6);
    assert_eq!(h.hooks.finally_calls.load(Ordering::Acquire), 0);
}
#[tokio::test]
async fn tc1_risk_summary_and_snapshot_reducer_use_same_published_records() {
    let h = Harness::new();
    let result=h.call("test.start",json!({"tc":{"text":include_str!("../../loom-test-format/tests/fixtures/TC-1.tc.md")},"plan":{"text":include_str!("../../loom-test-format/tests/fixtures/TC-1.plan.yaml")}})).await.expect("TC1");
    let id = result["session_id"].as_str().expect("id");
    let show = h
        .call("test.plan.show", json!({"session":id}))
        .await
        .expect("show");
    assert_eq!(show["risk_summary"]["R3"], "1");
    assert_eq!(show["risk_summary"]["R2"], "1");
    assert_eq!(show["risk_summary"]["R1"], "2");
    assert_eq!(show["steps"].as_array().expect("steps").len(), 5);
    h.begin(id, "S1").await;
    let mut reducer = TestReducer::default();
    for (i, event) in h.events().iter().enumerate() {
        reducer.apply(&published(i as u64 + 1, event));
    }
    let snap = reducer.snapshot(&SubFilter::default());
    assert_eq!(snap[0]["state"]["session_id"], id);
    assert_eq!(snap[0]["state"]["current_step"], "S1");
    assert_eq!(snap[0]["state"]["steps"][0]["status"], "running");
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_checks_are_bounded_and_paused_results_do_not_commit() {
    let h = Arc::new(Harness::new());
    let id = h.start(PLAN).await;
    h.begin(&id, "S1").await;
    h.verifier.delay_ms.store(70, Ordering::Release);
    let a = {
        let h = h.clone();
        let id = id.clone();
        tokio::spawn(async move { h.verdict(&id, "S1", "pass").await })
    };
    let b = {
        let h = h.clone();
        let id = id.clone();
        tokio::spawn(async move { h.verdict(&id, "S1", "pass").await })
    };
    tokio::time::timeout(Duration::from_secs(1), async {
        while h.verifier.calls.load(Ordering::Acquire) < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("verifiers entered");
    assert_eq!(
        h.verdict(&id, "S1", "pass")
            .await
            .expect_err("two checks max")
            .code,
        ErrorCode::Locked
    );
    h.call("test.pause", json!({"session":id}))
        .await
        .expect("pause remains responsive");
    assert_eq!(
        a.await.expect("join").expect_err("stale paused result").code,
        ErrorCode::SessionPaused
    );
    assert_eq!(
        b.await.expect("join").expect_err("stale paused result").code,
        ErrorCode::SessionPaused
    );
    assert!(h.module.snapshot(&id).expect("snapshot")["steps"][0]["accepted"].is_null());
}
#[tokio::test]
async fn publication_failure_does_not_create_a_hidden_verdict_or_note() {
    let h = Harness::new();
    let id = h.start(PLAN).await;
    h.begin(&id, "S1").await;
    let before = h.module.accounted_bytes();
    *h.services.fail_kind.lock().expect("lock") = Some("test.note".into());
    assert_eq!(
        h.call("test.note", json!({"session":id,"text":"not recorded"}))
            .await
            .expect_err("publish fail")
            .code,
        ErrorCode::RecordingFailed
    );
    assert_eq!(h.module.accounted_bytes(), before);
    assert_eq!(h.module.snapshot(&id).expect("snapshot")["notes_count"], 0);
    *h.services.fail_kind.lock().expect("lock") = Some("test.verdict".into());
    assert_eq!(
        h.verdict(&id, "S1", "pass")
            .await
            .expect_err("verdict publish fail")
            .code,
        ErrorCode::RecordingFailed
    );
    assert!(h.module.snapshot(&id).expect("snapshot")["steps"][0]["accepted"].is_null());
}

#[tokio::test]
async fn stimulus_done_and_conditional_recovery_are_not_counted_as_bare_pass() {
    let h = Harness::new();
    let plan = "loom_tc_plan: 1\nsteps: [{id: S1, uart.send: {port: dut, text: x}}]";
    let id = h.start(plan).await;
    h.begin(&id, "S1").await;
    assert_eq!(
        h.verdict(&id, "S1", "pass")
            .await
            .expect_err("stimulus cannot pass")
            .code,
        ErrorCode::EvidenceMissing
    );
    h.verdict(&id, "S1", "done")
        .await
        .expect("confirmed stimulus done");
    assert_eq!(
        h.call("test.end", json!({"session":id}))
            .await
            .expect("done is complete")["result"],
        "passed"
    );
    let h = Harness::new();
    let plan = "loom_tc_plan: 1\nsteps:\n - id: S1\n   uart.expect: {timeout: 1s}\n   expect: [{filter: {domain: uart, kinds: [uart.data]}, pred: {op: exists}}]\n   on_fail: {goto: S3}\n - id: S2\n   when: {step: S1, verdict: pass}\n   uart.send: {text: not-on-recovery-path}\n - id: S3\n   when: {step: S1, verdict: fail}\n   uart.expect: {timeout: 1s}\n   expect: [{filter: {domain: uart, kinds: [uart.data]}, pred: {op: exists}}]\n";
    let id = h.start(plan).await;
    h.begin(&id, "S1").await;
    h.verdict(&id, "S1", "fail")
        .await
        .expect("original fail retained");
    h.begin(&id, "S3").await;
    h.verdict(&id, "S3", "pass").await.expect("recovery pass");
    assert_eq!(
        h.call("test.end", json!({"session":id}))
            .await
            .expect("declared recovery replaces fail only for aggregate")["result"],
        "passed"
    );
    assert!(h.events().iter().any(|e| e.kind == "test.verdict"
        && e.payload["step_id"] == "S1"
        && e.payload["effective"] == "fail"));
}
#[tokio::test]
async fn note_count_active_session_limit_and_completed_retention_are_bounded() {
    let h = Harness::new();
    let id = h.start(PLAN).await;
    assert_eq!(
        h.call("test.start", json!({"tc":{"text":TC}}))
            .await
            .expect_err("one active by default")
            .code,
        ErrorCode::SessionActive
    );
    for _ in 0..MAX_NOTES {
        h.call("test.note", json!({"session":id,"text":"bounded note"}))
            .await
            .expect("within count");
    }
    assert_eq!(
        h.call("test.note", json!({"session":id,"text":"one too many"}))
            .await
            .expect_err("note count")
            .code,
        ErrorCode::Locked
    );
    assert!(
        TestModule::with_services(
            SessionConfig {
                max_sessions: 5,
                approval: ApprovalMode::Auto
            },
            None,
            Arc::new(NotWired),
            Arc::new(NotWiredHooks)
        )
        .is_err()
    );
    let h = Harness::new();
    let mut ids = Vec::new();
    for _ in 0..10 {
        let start = h
            .call("test.start", json!({"tc":{"text":"natural prose"}}))
            .await
            .expect("next session");
        let id = start["session_id"].as_str().expect("id").to_owned();
        let _ = h.call("test.end", json!({"session":id})).await;
        ids.push(id);
        assert!(h.module.accounted_bytes() <= MAX_ENGINE_BYTES);
    }
    assert_eq!(
        h.module
            .snapshot(&ids[0])
            .expect_err("old completed state evicted at byte/count bound")
            .code,
        ErrorCode::NotFound
    );
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn plan_change_while_verifying_rejects_stale_evidence() {
    let h = Arc::new(Harness::new());
    let id = h.start(PLAN).await;
    h.begin(&id, "S1").await;
    h.verifier.delay_ms.store(80, Ordering::Release);
    let check = {
        let h = h.clone();
        let id = id.clone();
        tokio::spawn(async move { h.verdict(&id, "S1", "pass").await })
    };
    tokio::time::timeout(Duration::from_secs(1), async {
        while h.verifier.calls.load(Ordering::Acquire) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("entered");
    h.call(
        "test.plan.amend",
        json!({"session":id,"plan":{"text":PLAN.replace("timeout: 1s","timeout: 2s")}}),
    )
    .await
    .expect("amend");
    assert_eq!(
        check.await.expect("join").expect_err("old revision").code,
        ErrorCode::PlanMismatch
    );
    assert!(h.module.snapshot(&id).expect("snapshot")["steps"][0]["accepted"].is_null());
}
#[tokio::test]
async fn a_bad_decider_cannot_upgrade_fail_to_pass_or_confirm_missing_iterations() {
    let h = Harness::new();
    let id = h.start(PLAN).await;
    h.begin(&id, "S1").await;
    h.verifier.mode.store(3, Ordering::Release);
    assert_eq!(
        h.verdict(&id, "S1", "fail")
            .await
            .expect_err("decider upgrade rejected")
            .code,
        ErrorCode::Internal
    );
    let h = Harness::new();
    let id = h
        .start(&PLAN.replace("   retry: {count: 1}", "   repeat: {count: 5}"))
        .await;
    h.begin(&id, "S1").await;
    h.verifier.mode.store(4, Ordering::Release);
    assert_eq!(
        h.verdict(&id, "S1", "pass")
            .await
            .expect_err("repeat proof bound")
            .code,
        ErrorCode::EvidenceMismatch
    );
}

#[tokio::test]
async fn completed_finally_is_not_resent_after_session_end_publication_failure() {
    let h = Harness::new();
    h.hooks.finally_wired.store(true, Ordering::Release);
    let cleanup = format!("{PLAN}\nfinally: [{{psu.out: {{state: off}}}}]\n");
    let id = h.start(&cleanup).await;
    h.begin(&id, "S1").await;
    h.verdict(&id, "S1", "pass").await.expect("pass");
    h.begin(&id, "S2").await;
    h.verdict(&id, "S2", "pass").await.expect("pass");
    *h.services.fail_kind.lock().expect("lock") = Some("test.session_ended".into());
    assert_eq!(
        h.call("test.end", json!({"session":id}))
            .await
            .expect_err("end publish fail")
            .code,
        ErrorCode::RecordingFailed
    );
    assert_eq!(h.hooks.finally_calls.load(Ordering::Acquire), 1);
    h.call("test.end", json!({"session":id}))
        .await
        .expect("retry metadata commit only");
    assert_eq!(h.hooks.finally_calls.load(Ordering::Acquire), 1);
}
#[tokio::test]
async fn reducer_caps_evidence_and_rejected_proposals_keep_previous_accepted_result() {
    let h = Harness::new();
    let id = h.start(PLAN).await;
    h.begin(&id, "S1").await;
    h.verdict(&id, "S1", "pass").await.expect("accepted");
    let _ = h
        .call(
            "test.verdict",
            json!({"session":id,"step":"S1","verdict":"pass","evidence":[]}),
        )
        .await;
    let mut reducer = TestReducer::default();
    let events = h.events();
    for (i, event) in events.iter().enumerate() {
        let record = published(i as u64 + 1, event);
        reducer.apply(&record);
        reducer.apply(&record);
    }
    let base = events.len() as u64 + 1;
    for i in 0..250 {
        let event = loom_domain_api::EventDraft::new(
            "test.evidence_checked",
            json!({"session_id":id,"step_id":"S1","evidence_id":format!("e{i}"),"ref":"uart:dut@seq:1","status":"verified","observed":"bounded","complete":true,"gap_in_window":false}),
        );
        reducer.apply(&published(base + i, &event));
    }
    let snapshot = reducer.snapshot(&SubFilter::default());
    let state = &snapshot[0]["state"];
    assert_eq!(
        state["recent_evidence"].as_array().expect("rows").len(),
        MAX_RECENT_EVIDENCE
    );
    assert!(state["steps"][0]["verdict"]["effective"].is_null());
    assert_eq!(state["steps"][0]["accepted"]["effective"], "pass");
    assert!(
        reducer
            .snapshot(&SubFilter {
                domains: vec!["uart".into()],
                ..Default::default()
            })
            .is_empty()
    );
}

#[tokio::test]
async fn natural_tc_markers_are_published_and_never_confirm_automatic_pass() {
    let h = Harness::new();
    let started = h
        .call(
            "test.start",
            json!({"tc":{"text":"A natural-language TC with no steps table."}}),
        )
        .await
        .expect("start without plan");
    let id = started["session_id"].as_str().expect("id");
    *h.services.fail_kind.lock().expect("lock") = Some("test.step_started".into());
    assert!(
        h.call("test.step", json!({"session":id,"id":"observe"}))
            .await
            .is_err()
    );
    assert_eq!(h.module.snapshot(id).expect("snapshot")["steps"], json!([]));
    h.begin(id, "observe").await;
    h.begin(id, "finish").await;
    assert_eq!(
        h.module.snapshot(id).expect("snapshot")["steps"][0]["status"],
        "ended"
    );
    assert_eq!(
        h.verdict(id, "finish", "pass")
            .await
            .expect_err("no registered expectation")
            .code,
        ErrorCode::PlanMismatch
    );
    let mut reducer = TestReducer::default();
    for (i, event) in h.events().iter().enumerate() {
        reducer.apply(&published(i as u64 + 1, event));
    }
    assert_eq!(
        reducer.snapshot(&SubFilter::default())[0]["state"]["steps"]
            .as_array()
            .expect("steps")
            .len(),
        2
    );
}

#[tokio::test]
async fn four_clients_and_repeated_stop_retention_stay_within_engine_budget() {
    let svc = Arc::new(Services::default());
    let module = TestModule::with_services(
        SessionConfig {
            max_sessions: 4,
            approval: ApprovalMode::Auto,
        },
        None,
        Arc::new(NotWired),
        Arc::new(NotWiredHooks),
    )
    .expect("config");
    for i in 0..4 {
        let mut ctx = context(&svc, Role::Agent, None, &format!("agent-{i}"));
        module
            .call(&mut ctx, "test.start", json!({"tc":{"text":"TC"}}))
            .await
            .expect("four slots");
    }
    assert_eq!(
        module
            .call(
                &mut context(&svc, Role::Agent, None, "agent-5"),
                "test.start",
                json!({"tc":{"text":"TC"}})
            )
            .await
            .expect_err("fifth rejected")
            .code,
        ErrorCode::SessionActive
    );
    let ctx = context(&svc, Role::Viewer, Some(Rights::Operate), "viewer");
    module.set_stop(&ctx, true).expect("stop");
    module.set_stop(&ctx, false).expect("release");
    for i in 0..12 {
        module
            .call(
                &mut context(&svc, Role::Agent, None, &format!("next-{i}")),
                "test.start",
                json!({"tc":{"text":"TC"}}),
            )
            .await
            .expect("terminal snapshots can be evicted");
        module.set_stop(&ctx, true).expect("stop");
        module.set_stop(&ctx, false).expect("release");
        assert!(module.accounted_bytes() <= MAX_ENGINE_BYTES);
    }
}

#[tokio::test]
async fn citation_ranges_and_types_fail_before_verifier_and_registered_filters_are_bound() {
    let h = Harness::new();
    let id = h.start(PLAN).await;
    h.begin(&id, "S1").await;
    let cases = [
        json!({"range":{}}),
        json!({"range":{"epoch":1,"from_seq":"1","to_seq":"2"}}),
        json!({"range":{"epoch":"1","from_seq":"2","to_seq":"1"}}),
        json!({"range":"seq:02"}),
        json!({"range":"@step:"}),
        json!({"range":"seq:1","filter":{"domain":false}}),
        json!({"ref":"uart:dut@seq:1","plan_hash":5}),
    ];
    for citation in cases {
        assert_eq!(
            h.call(
                "test.evidence.add",
                json!({"session":id,"step":"S1","evidence":[citation]})
            )
            .await
            .expect_err("bad citation")
            .code,
            ErrorCode::Usage
        );
    }
    assert_eq!(h.verifier.calls.load(Ordering::Acquire), 0);
    h.call("test.evidence.add",json!({"session":id,"step":"S1","range":{"epoch":"1","from_seq":"1","to_seq":"2"},"filter":{"domain":"uart","channel":"dut","kinds":["uart.data"]}})).await.expect("typed whole-window candidate");
    assert_eq!(h.call("test.evidence.add",json!({"session":id,"step":"S1","range":"@step:S1","filter":{"domain":"can","kinds":["can.frame"]}})).await.expect_err("different filter").code,ErrorCode::PlanMismatch);
}

#[tokio::test]
async fn omitting_override_flag_cannot_replace_an_accepted_verdict() {
    let h = Harness::new();
    let id = h.start(PLAN).await;
    h.begin(&id, "S1").await;
    h.verdict(&id, "S1", "pass").await.expect("original verdict");
    let args = json!({"session":id,"step":"S1","verdict":"fail"});
    assert_eq!(
        h.call("test.verdict", args.clone())
            .await
            .expect_err("implicit agent override")
            .code,
        ErrorCode::Denied
    );
    assert_eq!(
        h.module
            .call(&mut h.viewer(Rights::Operate), "test.verdict", args.clone())
            .await
            .expect_err("implicit operate override")
            .code,
        ErrorCode::Denied
    );
    assert_eq!(
        h.module.snapshot(&id).expect("snapshot")["steps"][0]["accepted"]["effective"],
        "pass"
    );
    let mut args = args;
    args["override"] = json!(true);
    args["reason"] = json!("human corrects the verdict");
    h.module
        .call(&mut h.viewer(Rights::Approve), "test.verdict", args)
        .await
        .expect("explicit approved override");
    assert_eq!(
        h.module.snapshot(&id).expect("snapshot")["steps"][0]["accepted"]["effective"],
        "fail"
    );
}
