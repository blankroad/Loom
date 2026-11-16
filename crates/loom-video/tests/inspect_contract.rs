//! B5a contract tests (Opus-authored spec; FUNCTIONS.md §Batch 5a). Pure: no ffmpeg, no worker.
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;

use loom_video::analysis::{LumaFrame, RoiSpec};
use loom_video::inspect::*;
use serde_json::{Value, json};

const MS: u64 = 1_000_000;

fn roi(name: &str, extra: Value) -> RoiSpec {
    let mut v = json!({"name": name, "x": 0.0, "y": 0.0, "w": 0.5, "h": 0.5, "enter": 10.0, "exit": 5.0, "dwell_ms": 0, "cooldown_ms": 0});
    for (k, x) in extra.as_object().unwrap() {
        v[k] = x.clone();
    }
    RoiSpec::from_json(&v).unwrap()
}

fn flat(v: u8) -> LumaFrame {
    LumaFrame {
        width: 64,
        height: 48,
        luma: vec![v; 64 * 48],
    }
}

const SHA: &str = "0000000000000000000000000000000000000000000000000000000000000000";

fn fr(id: u64, luma: &LumaFrame) -> FrameRef<'_> {
    FrameRef {
        id,
        sha256: SHA,
        recv_mono_ns: id * 100 * MS,
        recv_utc_ns: 1_700_000_000_000_000_000 + id,
        luma: Ok(luma),
    }
}

fn cfg(specs: Vec<RoiSpec>) -> RoiConfig {
    let mut c = RoiConfig::default();
    c.replace(specs, None).unwrap();
    c
}

fn rois_json(i: &Inspector) -> Vec<Value> {
    i.json()["rois"].as_array().unwrap().clone()
}

// ---------------------------------------------------------------------------------------------
// C: config

#[test]
fn config_edits_and_revisions() {
    let mut c = RoiConfig::default();
    assert_eq!((c.rev, c.json()), (0, json!({"config_rev": 0, "rois": []})));
    c.put(roi("a", json!({})), None).unwrap();
    c.put(roi("b", json!({})), Some(1)).unwrap();
    assert_eq!(c.rev, 2);
    c.put(roi("a", json!({"enter": 20.0})), None).unwrap();
    assert_eq!(c.rev, 3);
    assert_eq!(
        c.specs.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
        ["a", "b"],
        "put replaces in place"
    );
    assert_eq!(c.get("a").unwrap().enter, 20.0);
    assert!(c.get("zz").is_none());
    let e = c.put(roi("c", json!({})), Some(2)).unwrap_err();
    assert_eq!(e, ConfigError::RevMismatch { current: 3 });
    assert_eq!(
        e.detail(),
        json!({"reason": "config_rev_mismatch", "config_rev": 3})
    );
    assert_eq!(e.message(), "config_rev mismatch: current 3");
    assert_eq!(c.rev, 3, "refused edits change nothing");
    for i in 0..6 {
        c.put(roi(&format!("r{i}"), json!({})), None).unwrap();
    }
    assert_eq!((c.rev, c.specs.len()), (9, 8));
    let e = c.put(roi("ninth", json!({})), None).unwrap_err();
    assert!(
        matches!(&e, ConfigError::Invalid(m) if m.starts_with("roi:")),
        "{e:?}"
    );
    assert_eq!(e.detail()["reason"], "invalid");
    assert!(e.detail()["error"].as_str().unwrap().starts_with("roi:"));
    assert_eq!((c.rev, c.specs.len()), (9, 8));
    assert_eq!(c.remove("zz", None), Ok(false));
    assert_eq!(c.rev, 9, "removing an absent ROI is not an edit");
    assert_eq!(
        c.remove("zz", Some(1)),
        Err(ConfigError::RevMismatch { current: 9 }),
        "revision is checked first"
    );
    assert_eq!(c.remove("b", Some(9)), Ok(true));
    assert_eq!((c.rev, c.specs.len()), (10, 7));
    let dup = vec![roi("x", json!({})), roi("x", json!({}))];
    assert!(matches!(c.replace(dup, None), Err(ConfigError::Invalid(m)) if m.starts_with("roi.name:")));
    c.replace(vec![], Some(10)).unwrap();
    assert_eq!(c.json(), json!({"config_rev": 11, "rois": []}));
    c.put(roi("a", json!({})), None).unwrap();
    assert_eq!(c.json()["rois"][0], roi("a", json!({})).to_json());
}

#[test]
fn parse_list_names_the_item() {
    assert_eq!(
        RoiConfig::parse_list(&json!({"a": 1})).unwrap_err(),
        "rois: expected an array"
    );
    let e = RoiConfig::parse_list(
        &json!([roi("a", json!({})).to_json(), {"name": "b", "x": 0, "y": 0, "w": 1, "h": 1}]),
    )
    .unwrap_err();
    assert!(e.starts_with("rois[1]: roi.enter:"), "{e}");
    let e = RoiConfig::parse_list(&json!([
        roi("a", json!({})).to_json(),
        roi("a", json!({})).to_json()
    ]))
    .unwrap_err();
    assert!(e.starts_with("roi.name: duplicate"), "{e}");
    let ok = RoiConfig::parse_list(&json!([
        roi("a", json!({})).to_json(),
        roi("b", json!({})).to_json()
    ]))
    .unwrap();
    assert_eq!(ok, vec![roi("a", json!({})), roi("b", json!({}))]);
    assert_eq!(RoiConfig::parse_list(&json!([])).unwrap(), vec![]);
}

#[test]
fn liveness_reasons() {
    assert_eq!(Liveness::Live.reason(), None);
    assert_eq!(Liveness::NotLive.reason().map(|r| r.as_str()), Some("not_live"));
    assert_eq!(Liveness::Stale.reason().map(|r| r.as_str()), Some("stale"));
}

// ---------------------------------------------------------------------------------------------
// I: inspector

#[test]
fn sync_adds_changes_and_removes() {
    let mut i = Inspector::new();
    assert_eq!(
        i.json(),
        json!({"config_rev": null, "analysis": {"analysed": 0, "skipped": 0, "decode_errors": 0, "last_frame_id": null}, "rois": [], "note": PIXEL_NOTE})
    );
    let mut c = cfg(vec![roi("a", json!({})), roi("b", json!({}))]);
    assert_eq!(
        i.sync(&c, 0),
        Vec::<Value>::new(),
        "new ROIs start UNKNOWN(config_changed) silently"
    );
    let r = rois_json(&i);
    assert_eq!(r.len(), 2);
    assert_eq!(
        r[0],
        json!({"roi": "a", "state": "unknown", "reason": "config_changed", "transition_seq": 0, "candidate": null,
               "baseline": null, "metrics": null, "metrics_frame_id": null, "history": []})
    );
    assert_eq!(i.seq("a"), Some(0));
    assert_eq!(i.seq("zz"), None);
    assert_eq!(i.sync(&c, 0), Vec::<Value>::new(), "same revision: no-op");
    // drive `a` to normal, then change only its threshold → one config_changed transition
    let black = flat(0);
    i.set_baseline(None, 1, SHA, &black).unwrap();
    let ev = i.round(Liveness::Live, Some(fr(2, &black)), 0);
    assert_eq!(
        ev.iter()
            .map(|e| (e["roi"].as_str().unwrap(), e["state"].as_str().unwrap()))
            .collect::<Vec<_>>(),
        [("a", "normal"), ("b", "normal")]
    );
    c.put(roi("a", json!({"enter": 30.0})), None).unwrap();
    c.remove("b", None).unwrap();
    let ev = i.sync(&c, 250 * MS);
    assert_eq!(ev.len(), 1);
    assert_eq!(
        (
            ev[0]["roi"].as_str(),
            ev[0]["state"].as_str(),
            ev[0]["reason"].as_str()
        ),
        (Some("a"), Some("unknown"), Some("config_changed"))
    );
    assert_eq!(ev[0]["config_rev"], c.rev);
    assert_eq!(
        (ev[0]["frame_id"].clone(), ev[0]["metrics"].clone()),
        (Value::Null, Value::Null)
    );
    assert_eq!(ev[0]["transition"]["mono_ns"], 250 * MS);
    let r = rois_json(&i);
    assert_eq!(r.len(), 1, "removed ROI dropped");
    assert_eq!(r[0]["transition_seq"], 2);
    assert!(r[0]["baseline"].is_object(), "same rect keeps the baseline");
    // a rect change drops the baseline (it was sampled at the old rect)
    c.put(roi("a", json!({"enter": 30.0, "x": 0.25})), None).unwrap();
    i.sync(&c, 300 * MS);
    assert_eq!(rois_json(&i)[0]["baseline"], Value::Null);
    assert_eq!(i.json()["config_rev"], c.rev);
}

#[test]
fn rounds_judge_counts_and_build_payloads() {
    let mut i = Inspector::new();
    let c = cfg(vec![roi("lamp", json!({}))]);
    i.sync(&c, 0);
    let (black, white) = (flat(0), flat(255));
    // no baseline → unknown(no_baseline) with frame identity
    let ev = i.round(Liveness::Live, Some(fr(1, &black)), 0);
    assert_eq!(ev.len(), 1);
    assert_eq!(
        (
            ev[0]["state"].as_str(),
            ev[0]["reason"].as_str(),
            ev[0]["frame_id"].as_u64()
        ),
        (Some("unknown"), Some("no_baseline"), Some(1))
    );
    let items = i.set_baseline(None, 1, SHA, &black).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["roi"], "lamp");
    assert_eq!(items[0]["reason"], Value::Null);
    assert_eq!(items[0]["baseline"]["frame_id"], 1);
    assert_eq!(
        items[0]["baseline"]["rect"],
        json!({"x": 0, "y": 0, "w": 32, "h": 24})
    );
    // baseline frame again → normal (dwell 0)
    let ev = i.round(Liveness::Live, Some(fr(2, &black)), 0);
    assert_eq!(
        ev,
        vec![json!({
            "roi": "lamp", "config_rev": 1, "state": "normal", "reason": null, "transition_seq": 2,
            "transition": {"from": "unknown", "from_reason": "no_baseline", "to": "normal", "reason": null, "transition_seq": 2, "mono_ns": 200 * MS, "value": 0.0},
            "frame_id": 2, "sha256": SHA, "recv_utc_ns": "1700000000000000002", "recv_mono_ns": "200000000",
            "metrics": {"mean_luma": 0.0, "luma_delta": 0.0, "mean_abs_diff": 0.0, "changed_px": 0.0},
            "baseline": {"frame_id": 1, "sha256": SHA}, "evidence": false, "note": PIXEL_NOTE
        })]
    );
    // frames 3 and 4 never analysed; 5 is white → changed, evidence requested
    let ev = i.round(Liveness::Live, Some(fr(5, &white)), 0);
    assert_eq!(
        (
            ev[0]["state"].as_str(),
            ev[0]["evidence"].as_bool(),
            ev[0]["metrics"]["changed_px"].as_f64()
        ),
        (Some("changed"), Some(true), Some(100.0))
    );
    assert_eq!(
        i.json()["analysis"],
        json!({"analysed": 3, "skipped": 2, "decode_errors": 0, "last_frame_id": 5})
    );
    // the same frame is never analysed twice; no new frame → nothing
    assert!(i.round(Liveness::Live, Some(fr(5, &white)), 0).is_empty());
    assert!(i.round(Liveness::Live, None, 0).is_empty());
    assert_eq!(i.json()["analysis"]["analysed"], 3);
    // capture goes non-live → unknown(not_live) immediately, no frame identity, counters untouched
    let ev = i.round(Liveness::NotLive, Some(fr(6, &white)), 650 * MS);
    assert_eq!(
        (
            ev[0]["state"].as_str(),
            ev[0]["reason"].as_str(),
            ev[0]["frame_id"].clone()
        ),
        (Some("unknown"), Some("not_live"), Value::Null)
    );
    assert_eq!(ev[0]["transition"]["mono_ns"], 650 * MS);
    assert_eq!(ev[0]["evidence"], false);
    assert!(
        i.round(Liveness::NotLive, None, 700 * MS).is_empty(),
        "same reason: no repeat"
    );
    let ev = i.round(Liveness::Stale, None, 800 * MS);
    assert_eq!(
        (
            ev[0]["reason"].as_str(),
            ev[0]["transition"]["from_reason"].as_str()
        ),
        (Some("stale"), Some("not_live"))
    );
    assert_eq!(i.json()["analysis"]["last_frame_id"], 5);
    // decode errors are judged UNKNOWN and counted
    let bad = FrameRef {
        id: 7,
        sha256: SHA,
        recv_mono_ns: 700 * MS,
        recv_utc_ns: 7,
        luma: Err("jpeg decode: x"),
    };
    let ev = i.round(Liveness::Live, Some(bad), 0);
    assert_eq!(
        (ev[0]["reason"].as_str(), ev[0]["frame_id"].as_u64()),
        (Some("decode_error"), Some(7))
    );
    assert_eq!(
        i.json()["analysis"],
        json!({"analysed": 4, "skipped": 3, "decode_errors": 1, "last_frame_id": 7})
    );
    // latest metrics are kept even without a transition
    let r = &rois_json(&i)[0];
    assert_eq!(
        (r["metrics"]["mean_luma"].as_f64(), r["metrics_frame_id"].as_u64()),
        (Some(255.0), Some(5))
    );
    assert_eq!(
        i.metric_json(),
        json!({"config_rev": 1, "analysis": {"analysed": 4, "skipped": 3, "decode_errors": 1, "last_frame_id": 7},
        "rois": [{"roi": "lamp", "state": "unknown", "reason": "decode_error", "transition_seq": 6,
                  "metrics": {"mean_luma": 255.0, "luma_delta": 255.0, "mean_abs_diff": 100.0, "changed_px": 100.0}, "metrics_frame_id": 5}],
        "note": PIXEL_NOTE})
    );
}

#[test]
fn no_rois_means_no_work_and_unsynced_rounds_are_empty() {
    let mut i = Inspector::new();
    let black = flat(0);
    assert!(i.round(Liveness::Live, Some(fr(1, &black)), 0).is_empty());
    assert!(i.round(Liveness::NotLive, None, 0).is_empty());
    i.sync(&RoiConfig::default(), 0);
    assert!(i.round(Liveness::Live, Some(fr(2, &black)), 0).is_empty());
    assert_eq!(
        i.json()["analysis"]["analysed"],
        0,
        "nothing to analyse → nothing counted"
    );
    assert_eq!(
        i.set_baseline(None, 1, SHA, &black).unwrap_err(),
        "roi: no ROIs configured"
    );
}

#[test]
fn baseline_targets_and_small_rois() {
    let mut i = Inspector::new();
    let c = cfg(vec![roi("big", json!({})), roi("tiny", json!({"w": 0.01}))]);
    i.sync(&c, 0);
    let black = flat(0);
    let e = i.set_baseline(Some("zz"), 1, SHA, &black).unwrap_err();
    assert!(e.starts_with("roi.name:") && e.contains("zz"), "{e}");
    let one = i.set_baseline(Some("big"), 1, SHA, &black).unwrap();
    assert_eq!(one.len(), 1);
    let all = i.set_baseline(None, 3, SHA, &black).unwrap();
    assert_eq!(
        all[1],
        json!({"roi": "tiny", "baseline": null, "reason": "roi_too_small"})
    );
    assert_eq!(all[0]["baseline"]["frame_id"], 3, "re-baseline replaces");
    let ev = i.round(Liveness::Live, Some(fr(4, &black)), 0);
    let by: BTreeMap<&str, &str> = ev
        .iter()
        .map(|e| (e["roi"].as_str().unwrap(), e["reason"].as_str().unwrap_or("-")))
        .collect();
    assert_eq!(by.get("tiny"), Some(&"roi_too_small"));
    assert_eq!(by.get("big"), Some(&"-"), "big judged normal");
}

#[test]
fn history_is_bounded_and_ordered() {
    let mut i = Inspector::new();
    let c = cfg(vec![roi("a", json!({}))]);
    i.sync(&c, 0);
    let (black, white) = (flat(0), flat(255));
    i.set_baseline(None, 1, SHA, &black).unwrap();
    let mut n = 0;
    for id in 2..40u64 {
        n += i
            .round(
                Liveness::Live,
                Some(fr(id, if id % 2 == 0 { &white } else { &black })),
                0,
            )
            .len();
    }
    assert!(n > ROI_HISTORY, "{n} transitions");
    let h = rois_json(&i)[0]["history"].as_array().unwrap().clone();
    assert_eq!(h.len(), ROI_HISTORY);
    let seqs: Vec<u64> = h.iter().map(|e| e["transition_seq"].as_u64().unwrap()).collect();
    assert!(seqs.windows(2).all(|w| w[1] == w[0] + 1), "{seqs:?}");
    assert_eq!(*seqs.last().unwrap(), i.seq("a").unwrap());
}

// ---------------------------------------------------------------------------------------------
// E: evidence plan

fn trig(id: u64) -> EvidenceTrigger {
    EvidenceTrigger {
        n: 1,
        roi: "a".into(),
        transition_seq: 3,
        frame_id: id,
        mono_ns: 5_000 * MS,
    }
}

#[test]
fn evidence_plan_pre_post_deadline() {
    let s = roi("a", json!({"evidence_pre": 2, "evidence_post": 2}));
    let mut p = EvidencePlan::new(trig(10), &s, 10, Some(5));
    assert_eq!(
        (p.pre.clone(), p.post.clone(), p.post_want, p.partial.clone()),
        (vec![8, 9], vec![], 2, Vec::<String>::new())
    );
    assert_eq!(p.deadline_mono_ns, (5_000 + 2_000 + 200) * MS);
    assert!(!p.complete() && !p.due(7_199 * MS) && p.due(7_200 * MS));
    assert_eq!(p.offer(10), Vec::<u64>::new());
    assert_eq!(p.offer(11), vec![11]);
    assert_eq!(p.offer(15), vec![12]);
    assert_eq!(p.offer(16), Vec::<u64>::new(), "never more than evidence_post");
    assert!(p.complete() && p.due(0));
    // ring already evicted part of the pre window; frame 1 has no predecessors
    let q = EvidencePlan::new(trig(10), &s, 10, Some(9));
    assert_eq!(
        (q.pre.clone(), q.partial.clone()),
        (vec![9], vec!["pre_unavailable".to_owned()])
    );
    let q = EvidencePlan::new(trig(1), &s, 10, Some(1));
    assert_eq!(
        (q.pre.clone(), q.partial.clone()),
        (vec![], vec!["pre_unavailable".to_owned()])
    );
    // fps 0 is treated as 1; zero pre/post want nothing and are complete at once
    let z = roi("a", json!({"evidence_pre": 0, "evidence_post": 0}));
    let q = EvidencePlan::new(trig(10), &z, 0, Some(1));
    assert!(q.pre.is_empty() && q.partial.is_empty() && q.complete());
    let one = roi("a", json!({"evidence_pre": 0, "evidence_post": 1}));
    assert_eq!(
        EvidencePlan::new(trig(10), &one, 0, Some(1)).deadline_mono_ns,
        (5_000 + 2_000 + 1_000) * MS
    );
    // a pre frame that could not be pinned is removed and explained (once)
    p.drop_frame(9, "pin_bytes");
    p.drop_frame(8, "pin_bytes");
    assert_eq!(
        (p.pre.clone(), p.partial.clone()),
        (vec![], vec!["pin_bytes".to_owned()])
    );
}

#[test]
fn evidence_fit_roles_missing_bytes_and_deadline() {
    let s = roi("a", json!({"evidence_pre": 2, "evidence_post": 2}));
    let mut p = EvidencePlan::new(trig(10), &s, 10, Some(1));
    p.offer(12);
    let sizes: BTreeMap<u64, usize> = [(8, 100), (9, 100), (10, 100), (11, 100), (12, 100)].into();
    assert_eq!(
        p.fit(&sizes),
        vec![
            ("pre", 8),
            ("pre", 9),
            ("trigger", 10),
            ("post", 11),
            ("post", 12)
        ]
    );
    assert!(p.partial.is_empty());
    let mut p2 = p.clone();
    let missing: BTreeMap<u64, usize> = [(8, 100), (9, 100), (10, 100), (12, 100)].into();
    assert_eq!(
        p2.fit(&missing),
        vec![("pre", 8), ("pre", 9), ("trigger", 10), ("post", 12)]
    );
    assert_eq!(p2.partial, vec!["frame_unavailable".to_owned()]);
    // budget: trigger first, then pre nearest-first, then post; base64 = ceil(n/3)*4; ≤ is allowed
    let mut p3 = p.clone();
    let big: BTreeMap<u64, usize> = [(8, 1), (9, 6 << 20), (10, 12 << 20), (11, 1), (12, 1)].into();
    assert_eq!(p3.fit(&big), vec![("pre", 9), ("trigger", 10)]);
    assert_eq!(p3.partial, vec!["bytes_cap".to_owned()]);
    // deadline before all post frames arrived
    let mut p4 = EvidencePlan::new(trig(10), &s, 10, Some(1));
    p4.offer(11);
    assert_eq!(
        p4.fit(&sizes),
        vec![("pre", 8), ("pre", 9), ("trigger", 10), ("post", 11)]
    );
    assert_eq!(p4.partial, vec!["post_deadline".to_owned()]);
    assert_eq!(p4.fit(&sizes).len(), 4);
    assert_eq!(
        p4.partial,
        vec!["post_deadline".to_owned()],
        "reasons recorded once"
    );
}
