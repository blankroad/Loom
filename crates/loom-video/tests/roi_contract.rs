//! B5b contract tests (Opus-authored spec; FUNCTIONS.md §Batch 5b). No FFmpeg, no device.
//! ROI RPC registration/risk, hooks, CLI conversion, `video.wait --roi` matching, `video.read` kinds,
//! reducer kinds, evidence artifact ids and serving. Sonnet must not edit this file.
#![allow(clippy::unwrap_used)]

use loom_cli_kit::DomainCli as _;
use loom_domain_api::{DomainModule as _, Registry, RoleSet, TargetRef, WriteHooks as _, WriteRequest};
use loom_proto::event::{Env, Origin, Risk};
use loom_proto::policy::WriteClass;
use loom_video::cli::VideoCli;
use loom_video::gateway::{ROI_STATES, Video, VideoHooks, roi_wait_match};
use loom_video::model::{self, ArtifactKind};
use loom_video::{KIND_EVIDENCE, KIND_ROI_CONFIG, KIND_ROI_METRIC, KIND_ROI_STATE};
use serde_json::{Value, json};

fn registry() -> Registry {
    let mut r = Registry::new("video").unwrap();
    Video.register(&mut r).unwrap();
    r
}

fn props(r: &Registry, m: &str) -> Value {
    let c = r
        .commands()
        .iter()
        .find(|c| c.method == m)
        .unwrap_or_else(|| panic!("{m} registered"));
    serde_json::to_value(&c.params).unwrap()["properties"].clone()
}

#[test]
fn roi_rpcs_registration_and_risk() {
    let r = registry();
    for m in [
        "video.roi.put",
        "video.roi.remove",
        "video.roi.replace",
        "video.roi.baseline",
    ] {
        let c = r
            .commands()
            .iter()
            .find(|c| c.method == m)
            .unwrap_or_else(|| panic!("{m} registered"));
        assert_eq!(c.write_class, Some(WriteClass::Request), "{m}");
        assert_eq!(
            c.roles,
            RoleSet::WRITE,
            "{m}: shared inspection config needs operate"
        );
        let row = r
            .risk_table_ref()
            .unwrap()
            .rows
            .iter()
            .find(|x| x.method == m)
            .unwrap_or_else(|| panic!("{m} risk row"));
        assert_eq!(row.risk, Risk::R0, "{m}");
    }
    let list = r
        .commands()
        .iter()
        .find(|c| c.method == "video.roi.list")
        .unwrap();
    assert_eq!((list.write_class, list.roles), (None, RoleSet::READ));
    assert!(props(&r, "video.roi.put")["roi"].is_object());
    assert!(props(&r, "video.roi.put")["expect_rev"].is_object());
    assert!(props(&r, "video.roi.remove")["roi"].is_object());
    assert!(props(&r, "video.roi.replace")["rois"].is_object());
    for k in ["capture_id", "frame_id", "roi"] {
        assert!(props(&r, "video.roi.baseline")[k].is_object(), "baseline {k}");
    }
    for k in ["roi", "roi_state", "new"] {
        assert!(props(&r, "video.wait")[k].is_object(), "wait {k}");
    }
    assert_eq!(props(&r, "video.wait")["roi_state"]["enum"], json!(ROI_STATES));
    let kinds = props(&r, "video.read")["kind"]["enum"].clone();
    for k in [
        "state", "health", "snapshot", "record", "roi", "metric", "evidence", "all",
    ] {
        assert!(
            kinds.as_array().unwrap().contains(&json!(k)),
            "read kind {k}: {kinds}"
        );
    }
    let ek: Vec<&str> = r.event_kinds().iter().map(|k| k.kind.as_str()).collect();
    for k in [KIND_ROI_CONFIG, KIND_ROI_STATE, KIND_ROI_METRIC, KIND_EVIDENCE] {
        assert!(ek.contains(&k), "event kind {k}");
        // the shared hub treats `<domain>.*_state` on a channel as that channel's connectivity
        assert!(
            !k.ends_with("_state") && !k.ends_with(".state"),
            "{k} must not look like a channel state kind"
        );
    }
    assert_eq!(KIND_ROI_STATE, "video.roi_judgment");
    let rk = r.reducers()[0].kinds().to_vec();
    for k in [KIND_ROI_CONFIG, KIND_ROI_STATE, KIND_ROI_METRIC, KIND_EVIDENCE] {
        assert!(rk.contains(&k), "reducer kind {k}");
    }
    let help = serde_json::to_string(r.help_ref().unwrap()).unwrap();
    for s in [
        "loom video roi put",
        "loom video roi baseline",
        "--roi",
        "no scene understanding",
    ] {
        assert!(help.contains(s), "help mentions {s}");
    }
    // unchanged contracts
    assert_eq!(
        r.commands()
            .iter()
            .find(|c| c.method == "video.pin")
            .unwrap()
            .roles,
        RoleSet::READ
    );
}

#[test]
fn roi_hooks_are_memory_only_r0() {
    let hooks = VideoHooks;
    let target = TargetRef {
        channel: None,
        target: None,
        generation: None,
    };
    let args = json!({});
    for m in [
        "video.roi.put",
        "video.roi.remove",
        "video.roi.replace",
        "video.roi.baseline",
    ] {
        let c = hooks.classify(&WriteRequest {
            domain: "video",
            method: m,
            target: &target,
            bytes: None,
            args: &args,
            origin: Origin::Agent,
            client_id: "c",
            env: Env::Sim,
            write_class: WriteClass::Request,
            job: None,
        });
        assert_eq!(c.risk, Risk::R0, "{m}");
        assert!(
            c.reason.contains("ROI") && c.reason.contains("memory"),
            "{m}: {}",
            c.reason
        );
    }
}

// ---- CLI ------------------------------------------------------------------------------------------

fn call(args: &[&str]) -> Result<loom_cli_kit::RpcCall, String> {
    let m = VideoCli::command()
        .try_get_matches_from(std::iter::once("video").chain(args.iter().copied()))
        .map_err(|e| e.to_string())?;
    VideoCli::to_call(&m).map_err(|e| e.to_string())
}

#[test]
fn cli_roi_put_remove_list_replace_baseline() {
    let c = call(&[
        "roi",
        "put",
        "--name",
        "lamp",
        "--rect",
        "0.1,0.2,0.3,0.4",
        "--enter",
        "12",
    ])
    .unwrap();
    assert_eq!(c.method, "video.roi.put");
    assert_eq!(
        c.params,
        json!({"roi": {"name": "lamp", "x": 0.1, "y": 0.2, "w": 0.3, "h": 0.4, "enter": 12.0}}),
        "only given keys; the daemon fills defaults"
    );
    let full = call(&[
        "roi",
        "put",
        "--name",
        "door",
        "--rect",
        "0,0,1,1",
        "--enter",
        "40",
        "--metric",
        "changed_px",
        "--exit",
        "3",
        "--dwell",
        "500ms",
        "--cooldown",
        "2s",
        "--pixel-threshold",
        "30",
        "--no-evidence",
        "--pre",
        "1",
        "--post",
        "3",
        "--expect-rev",
        "4",
    ])
    .unwrap();
    assert_eq!(
        full.params,
        json!({"roi": {"name": "door", "x": 0.0, "y": 0.0, "w": 1.0, "h": 1.0, "enter": 40.0, "metric": "changed_px", "exit": 3.0,
                       "dwell_ms": 500, "cooldown_ms": 2000, "pixel_threshold": 30, "evidence": false, "evidence_pre": 1, "evidence_post": 3},
               "expect_rev": 4})
    );
    for bad in [
        &[
            "roi",
            "put",
            "--name",
            "a",
            "--rect",
            "0.1,0.2,0.3",
            "--enter",
            "1",
        ][..],
        &["roi", "put", "--name", "a", "--rect", "a,b,c,d", "--enter", "1"],
        &["roi", "put", "--name", "a", "--rect", "0,0,1,1"],
        &["roi", "put", "--rect", "0,0,1,1", "--enter", "1"],
        &["roi", "put", "--name", "a b", "--rect", "0,0,1,1", "--enter", "1"],
        &[
            "roi", "put", "--name", "a", "--rect", "0,0,1,1", "--enter", "1", "--pre", "9",
        ],
        &[
            "roi", "put", "--name", "a", "--rect", "0,0,1,1", "--enter", "1", "--dwell", "61s",
        ],
    ] {
        assert!(call(bad).is_err(), "{bad:?}");
    }
    let e = call(&[
        "roi",
        "put",
        "--name",
        "a",
        "--rect",
        "0.9,0,0.2,0.5",
        "--enter",
        "1",
    ])
    .unwrap_err();
    assert!(
        e.contains("roi.w"),
        "validated locally with the daemon's rules: {e}"
    );

    assert_eq!(
        call(&["roi", "remove", "--name", "lamp"]).unwrap().params,
        json!({"roi": "lamp"})
    );
    let r = call(&["roi", "remove", "--name", "lamp", "--expect-rev", "2"]).unwrap();
    assert_eq!(
        (r.method.as_str(), r.params.clone()),
        ("video.roi.remove", json!({"roi": "lamp", "expect_rev": 2}))
    );
    assert!(call(&["roi", "remove"]).is_err());
    let l = call(&["roi", "list"]).unwrap();
    assert_eq!(
        (l.method.as_str(), l.params.clone()),
        ("video.roi.list", json!({}))
    );

    let one = json!({"name": "lamp", "x": 0, "y": 0, "w": 1, "h": 1, "enter": 5});
    let arr = serde_json::to_string(&json!([one])).unwrap();
    let rp = call(&["roi", "replace", "--rois", &arr]).unwrap();
    assert_eq!(
        (rp.method.as_str(), rp.params.clone()),
        ("video.roi.replace", json!({"rois": [one]}))
    );
    // a recipe saved from `roi list` (object with "rois") re-applies as-is
    let recipe = serde_json::to_string(&json!({"config_rev": 7, "rois": [one]})).unwrap();
    assert_eq!(
        call(&["roi", "replace", "--rois", &recipe, "--expect-rev", "7"])
            .unwrap()
            .params,
        json!({"rois": [one], "expect_rev": 7})
    );
    assert_eq!(
        call(&["roi", "replace", "--rois", "[]"]).unwrap().params,
        json!({"rois": []}),
        "empty clears"
    );
    assert!(call(&["roi", "replace", "--rois", "{nope"]).is_err());
    let e = call(&["roi", "replace", "--rois", r#"[{"name":"x"}]"#]).unwrap_err();
    // RF-1: the payload flag is `--rois`; `--json` is the loom-wide output flag and is not a replace argument
    assert!(call(&["roi", "replace", "--json", &arr]).is_err());
    assert!(e.contains("rois[0]"), "{e}");

    let b = call(&["roi", "baseline"]).unwrap();
    assert_eq!(
        (b.method.as_str(), b.params.clone()),
        ("video.roi.baseline", json!({}))
    );
    assert_eq!(
        call(&[
            "roi",
            "baseline",
            "--frame",
            "7",
            "--name",
            "lamp",
            "--capture",
            "v0123456789ab"
        ])
        .unwrap()
        .params,
        json!({"capture_id": "v0123456789ab", "frame_id": 7, "roi": "lamp"})
    );
    assert!(call(&["roi", "baseline", "--frame", "0"]).is_err());
}

#[test]
fn cli_wait_roi_and_read_kinds() {
    let w = call(&[
        "wait",
        "--roi",
        "lamp",
        "--roi-state",
        "changed",
        "--new",
        "--timeout",
        "5s",
    ])
    .unwrap();
    assert_eq!(
        w.params,
        json!({"roi": "lamp", "roi_state": "changed", "new": true, "timeout_ms": 5000})
    );
    assert_eq!(
        call(&["wait", "--roi", "lamp"]).unwrap().params,
        json!({"roi": "lamp", "timeout_ms": 10000})
    );
    for bad in [
        &["wait", "--roi-state", "changed"][..],
        &["wait", "--new"],
        &["wait"],
        &["wait", "--roi", "lamp", "--roi-state", "on"],
    ] {
        assert!(call(bad).is_err(), "{bad:?}");
    }
    let e = call(&["wait"]).unwrap_err();
    assert!(e.contains("--roi"), "{e}");
    for k in ["roi", "metric", "evidence"] {
        assert_eq!(call(&["read", "--kind", k]).unwrap().params, json!({"kind": k}));
    }
}

// ---- wait matching (gateway, pure) --------------------------------------------------------------

fn status() -> Value {
    let h3 = json!({"roi": "lamp", "state": "unknown", "reason": "no_baseline", "transition_seq": 3});
    let h4 = json!({"roi": "lamp", "state": "normal", "reason": null, "transition_seq": 4});
    json!({
        "capture_id": "v0123456789ab",
        "roi": {
            "config": {"config_rev": 3, "rois": [{"name": "lamp"}, {"name": "door"}]},
            "inspect": {"config_rev": 3, "rois": [
                {"roi": "lamp", "state": "normal", "reason": null, "transition_seq": 4, "candidate": null, "baseline": null,
                 "metrics": null, "metrics_frame_id": null, "history": [h3, h4]}
            ]}
        }
    })
}

#[test]
fn roi_wait_matching() {
    let st = status();
    let e = roi_wait_match(&st, "zz", None, None).unwrap_err();
    assert!(e.contains("zz") && e.contains("not configured"), "{e}");
    let e = roi_wait_match(&st, "lamp", Some("on"), None).unwrap_err();
    assert!(e.starts_with("roi_state:"), "{e}");
    assert_eq!(
        roi_wait_match(&st, "door", None, None),
        Ok(None),
        "configured but not judged yet"
    );
    let mut no_cap = st.clone();
    no_cap["roi"]["inspect"] = Value::Null;
    assert_eq!(
        roi_wait_match(&no_cap, "lamp", None, None),
        Ok(None),
        "no capture yet"
    );
    let cur = roi_wait_match(&st, "lamp", None, None).unwrap().unwrap();
    assert_eq!(
        (cur["state"].as_str(), cur["transition_seq"].as_u64()),
        (Some("normal"), Some(4))
    );
    assert!(cur.get("history").is_none(), "current row without its history");
    assert!(
        roi_wait_match(&st, "lamp", Some("normal"), None)
            .unwrap()
            .is_some()
    );
    assert_eq!(roi_wait_match(&st, "lamp", Some("changed"), None), Ok(None));
    // new transitions only: the first history entry after the given seq that fits
    assert_eq!(
        roi_wait_match(&st, "lamp", None, Some(3)).unwrap().unwrap()["transition_seq"],
        4
    );
    assert_eq!(
        roi_wait_match(&st, "lamp", Some("unknown"), Some(2))
            .unwrap()
            .unwrap()["transition_seq"],
        3
    );
    assert_eq!(roi_wait_match(&st, "lamp", None, Some(4)), Ok(None));
    assert_eq!(roi_wait_match(&st, "lamp", Some("changed"), Some(2)), Ok(None));
}

// ---- evidence artifacts ---------------------------------------------------------------------------

#[test]
fn evidence_artifact_ids_paths_and_serving() {
    let cap = "v0123456789ab";
    let id = model::artifact_id(ArtifactKind::Evidence, cap, 3);
    assert_eq!(id, format!("evid-{cap}-3"));
    assert_eq!(model::parse_artifact_id(&id), Some((ArtifactKind::Evidence, cap)));
    assert_eq!(
        (
            ArtifactKind::Evidence.prefix(),
            ArtifactKind::Evidence.ext(),
            ArtifactKind::Evidence.content_type()
        ),
        ("evid", "json", "application/json")
    );
    for bad in [
        "evid-../x-1",
        "evid-v0123456789ab-",
        "EVID-v0123456789ab-1",
        "evid-v0123456789ab-1-2",
        "evidence-v0123456789ab-1",
    ] {
        assert_eq!(model::parse_artifact_id(bad), None, "{bad}");
    }
    let session = std::env::temp_dir().join(format!("loom-video-roi-{}", std::process::id()));
    let (kind, path) = model::artifact_path(&session, &id).unwrap();
    assert_eq!(kind, ArtifactKind::Evidence);
    assert_eq!(path, model::capture_dir(&session, cap).join(format!("{id}.json")));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, br#"{"schema":"loom.video.evidence/1"}"#).unwrap();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let r = rt.block_on(loom_video::media::artifact(&session, &id));
    assert_eq!((r.status, r.content_type), (200, "application/json"));
    let h = r
        .headers
        .iter()
        .find(|(n, _)| *n == "content-disposition")
        .map(|(_, v)| v.clone());
    assert_eq!(
        h.as_deref(),
        Some(format!("attachment; filename=\"{id}.json\"").as_str())
    );
    // existing kinds unchanged
    assert_eq!(
        model::parse_artifact_id(&format!("snap-{cap}-1")).map(|x| x.0),
        Some(ArtifactKind::Snapshot)
    );
    assert_eq!(
        model::parse_artifact_id(&format!("rec-{cap}-1")).map(|x| x.0),
        Some(ArtifactKind::Recording)
    );
    let _ = std::fs::remove_dir_all(&session);
}
