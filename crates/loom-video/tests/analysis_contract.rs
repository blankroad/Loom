//! B4 contract tests (Opus-authored spec; FUNCTIONS.md §Batch 4). Pure: no ffmpeg, no worker.
//! JPEG fixtures in `tests/fixtures/` were generated with ffmpeg lavfi (full-range yuvj / gray):
//! black, white, split (left half black, right half white) 64×48; red 16×16 (Y≈76); gray128 64×48
//! (1-component); wide 4112×16 (over the 4096 decode bound).
#![allow(clippy::unwrap_used)]

use loom_video::analysis::*;
use serde_json::{Value, json};

const BLACK: &[u8] = include_bytes!("fixtures/black_64x48.jpg");
const WHITE: &[u8] = include_bytes!("fixtures/white_64x48.jpg");
const SPLIT: &[u8] = include_bytes!("fixtures/split_64x48.jpg");
const RED: &[u8] = include_bytes!("fixtures/red_16x16.jpg");
const GRAY: &[u8] = include_bytes!("fixtures/gray128_64x48.jpg");
const WIDE: &[u8] = include_bytes!("fixtures/wide_4112x16.jpg");

fn ms(x: u64) -> u64 {
    x * 1_000_000
}

fn spec(extra: Value) -> RoiSpec {
    let mut base = json!({"name": "lamp", "x": 0.0, "y": 0.0, "w": 1.0, "h": 1.0, "metric": "mean_abs_diff",
        "enter": 10.0, "exit": 5.0, "dwell_ms": 300, "cooldown_ms": 1000});
    for (k, v) in extra.as_object().unwrap() {
        base[k] = v.clone();
    }
    RoiSpec::from_json(&base).unwrap()
}

fn val(t_ms: u64, v: f64) -> Observation {
    Observation::Value {
        mono_ns: ms(t_ms),
        value: v,
    }
}

fn unk(t_ms: u64, reason: UnknownReason) -> Observation {
    Observation::Unknown {
        mono_ns: ms(t_ms),
        reason,
    }
}

fn flat(width: u32, height: u32, v: u8) -> LumaFrame {
    LumaFrame {
        width,
        height,
        luma: vec![v; (width * height) as usize],
    }
}

/// Drive a judge into `normal` at t=300 ms (value 0) and return it.
fn normal_judge(s: &RoiSpec) -> RoiJudge {
    let mut j = RoiJudge::new();
    assert_eq!(j.step(s, val(0, 0.0)), None);
    let t = j.step(s, val(300, 0.0)).unwrap();
    assert_eq!((t.from, t.to, t.seq), (RoiState::Unknown, RoiState::Normal, 1));
    j
}

// ---------------------------------------------------------------------------------------------
// A: names

#[test]
fn wire_names() {
    for (m, s, max) in [
        (Metric::LumaDelta, "luma_delta", 255.0),
        (Metric::MeanAbsDiff, "mean_abs_diff", 100.0),
        (Metric::ChangedPx, "changed_px", 100.0),
    ] {
        assert_eq!(m.as_str(), s);
        assert_eq!(Metric::parse(s), Some(m));
        assert_eq!(m.max_threshold(), max);
    }
    assert_eq!(Metric::parse("ocr"), None);
    assert_eq!(
        [RoiState::Unknown, RoiState::Normal, RoiState::Changed].map(RoiState::as_str),
        ["unknown", "normal", "changed"]
    );
    use UnknownReason::*;
    assert_eq!(
        [
            NotLive,
            Stale,
            NoBaseline,
            BaselineMismatch,
            RoiTooSmall,
            DecodeError,
            AnalysisGap,
            ConfigChanged
        ]
        .map(UnknownReason::as_str),
        [
            "not_live",
            "stale",
            "no_baseline",
            "baseline_mismatch",
            "roi_too_small",
            "decode_error",
            "analysis_gap",
            "config_changed"
        ]
    );
}

// ---------------------------------------------------------------------------------------------
// S: spec validation

#[test]
fn spec_defaults_and_round_trip() {
    let s =
        RoiSpec::from_json(&json!({"name": "lamp-1", "x": 0.1, "y": 0.2, "w": 0.3, "h": 0.4, "enter": 10}))
            .unwrap();
    assert_eq!(s.name, "lamp-1");
    assert_eq!(
        s.rect,
        RoiRect {
            x: 0.1,
            y: 0.2,
            w: 0.3,
            h: 0.4
        }
    );
    assert_eq!(s.metric, Metric::MeanAbsDiff);
    assert_eq!((s.enter, s.exit), (10.0, 5.0), "exit defaults to enter/2");
    assert_eq!(
        (s.dwell_ms, s.cooldown_ms, s.pixel_threshold),
        (DWELL_DEFAULT_MS, COOLDOWN_DEFAULT_MS, PIXEL_THRESHOLD_DEFAULT)
    );
    assert_eq!(
        (s.evidence, s.evidence_pre, s.evidence_post),
        (true, EVIDENCE_FRAMES_DEFAULT, EVIDENCE_FRAMES_DEFAULT)
    );
    let j = s.to_json();
    let keys: Vec<&str> = j.as_object().unwrap().keys().map(String::as_str).collect();
    for k in [
        "name",
        "x",
        "y",
        "w",
        "h",
        "metric",
        "enter",
        "exit",
        "dwell_ms",
        "cooldown_ms",
        "pixel_threshold",
        "evidence",
        "evidence_pre",
        "evidence_post",
    ] {
        assert!(keys.contains(&k), "missing {k} in {j}");
    }
    assert_eq!(keys.len(), 14, "{j}");
    assert_eq!(j["metric"], "mean_abs_diff");
    assert_eq!(RoiSpec::from_json(&j).unwrap(), s, "canonical JSON round-trips");
    // null on an optional key = default; edges accepted
    let e = RoiSpec::from_json(&json!({"name": "a", "x": 0, "y": 0, "w": 1, "h": 1, "metric": "luma_delta", "enter": 255,
        "exit": null, "dwell_ms": 0, "cooldown_ms": 60000, "pixel_threshold": 255, "evidence": false, "evidence_pre": 0, "evidence_post": 8}))
    .unwrap();
    assert_eq!(
        (
            e.metric,
            e.exit,
            e.dwell_ms,
            e.cooldown_ms,
            e.evidence,
            e.evidence_post
        ),
        (Metric::LumaDelta, 127.5, 0, 60000, false, 8)
    );
}

#[test]
fn spec_refusals_name_the_key() {
    let ok = json!({"name": "lamp", "x": 0.1, "y": 0.1, "w": 0.5, "h": 0.5, "enter": 10});
    let with = |k: &str, v: Value| {
        let mut o = ok.clone();
        o[k] = v;
        o
    };
    let without = |k: &str| {
        let mut o = ok.clone();
        o.as_object_mut().unwrap().remove(k);
        o
    };
    let cases: Vec<(Value, &str)> = vec![
        (json!([1]), "roi:"),
        (with("colour", json!("red")), "roi.colour:"),
        (without("name"), "roi.name:"),
        (with("name", json!("")), "roi.name:"),
        (with("name", json!("a b")), "roi.name:"),
        (with("name", json!("x".repeat(33))), "roi.name:"),
        (with("x", json!(1.0)), "roi.x:"),
        (with("x", json!(-0.1)), "roi.x:"),
        (without("y"), "roi.y:"),
        (with("w", json!(0)), "roi.w:"),
        (with("w", json!(0.95)), "roi.w:"), // x + w > 1
        (with("h", json!("0.5")), "roi.h:"),
        (with("metric", json!("ocr")), "roi.metric:"),
        (without("enter"), "roi.enter:"),
        (with("enter", json!(0)), "roi.enter:"),
        (with("enter", json!(100.5)), "roi.enter:"), // > 100 for a percentage metric
        (with("exit", json!(10.5)), "roi.exit:"),
        (with("exit", json!(-1)), "roi.exit:"),
        (with("dwell_ms", json!(60_001)), "roi.dwell_ms:"),
        (with("dwell_ms", json!(1.5)), "roi.dwell_ms:"),
        (with("cooldown_ms", json!(-1)), "roi.cooldown_ms:"),
        (with("pixel_threshold", json!(0)), "roi.pixel_threshold:"),
        (with("pixel_threshold", json!(256)), "roi.pixel_threshold:"),
        (with("evidence", json!("yes")), "roi.evidence:"),
        (with("evidence_pre", json!(9)), "roi.evidence_pre:"),
        (with("evidence_post", json!(-1)), "roi.evidence_post:"),
    ];
    for (v, prefix) in cases {
        let e = RoiSpec::from_json(&v).unwrap_err();
        assert!(e.starts_with(prefix), "{v} → {e:?}, expected prefix {prefix}");
    }
    // first failure in contract order wins
    let e = RoiSpec::from_json(&json!({"name": "bad name", "x": 2, "enter": 0})).unwrap_err();
    assert!(e.starts_with("roi.name:"), "{e}");
    // luma_delta allows thresholds up to 255
    let lum = json!({"name": "l", "x": 0, "y": 0, "w": 1, "h": 1, "metric": "luma_delta", "enter": 200});
    assert_eq!(RoiSpec::from_json(&lum).unwrap().enter, 200.0);
}

#[test]
fn set_bounds_and_unique_names() {
    let mk = |i: usize| spec(json!({"name": format!("r{i}")}));
    let eight: Vec<RoiSpec> = (0..MAX_ROIS).map(mk).collect();
    assert_eq!(check_set(&eight), Ok(()));
    assert_eq!(check_set(&[]), Ok(()));
    let nine: Vec<RoiSpec> = (0..=MAX_ROIS).map(mk).collect();
    assert!(check_set(&nine).unwrap_err().starts_with("roi:"));
    let dup = vec![mk(1), mk(2), mk(1)];
    let e = check_set(&dup).unwrap_err();
    assert!(e.starts_with("roi.name:") && e.contains("r1"), "{e}");
}

// ---------------------------------------------------------------------------------------------
// D: decode

#[test]
fn decode_luma_of_fixtures() {
    let b = decode_luma(BLACK).unwrap();
    assert_eq!((b.width, b.height, b.luma.len()), (64, 48, 64 * 48));
    assert!(b.luma.iter().all(|&v| v <= 3), "black");
    let w = decode_luma(WHITE).unwrap();
    assert!(w.luma.iter().all(|&v| v >= 252), "white");
    let s = decode_luma(SPLIT).unwrap();
    for row in 0..48 {
        let r = &s.luma[row * 64..row * 64 + 64];
        assert!(
            r[..28].iter().all(|&v| v <= 3) && r[36..].iter().all(|&v| v >= 252),
            "row {row}: {r:?}"
        );
    }
    let red = decode_luma(RED).unwrap();
    assert_eq!((red.width, red.height), (16, 16));
    assert!(
        red.luma.iter().all(|&v| (73..=79).contains(&v)),
        "red luma ≈ 76: {:?}",
        &red.luma[..4]
    );
    let g = decode_luma(GRAY).unwrap();
    assert!(
        g.luma.iter().all(|&v| (126..=130).contains(&v)),
        "1-component gray 128"
    );
}

#[test]
fn decode_refusals() {
    for (name, bytes) in [
        ("wide", WIDE),
        ("garbage", b"not a jpeg".as_slice()),
        ("empty", &[][..]),
        ("truncated head", &BLACK[..20]),
    ] {
        let e = decode_luma(bytes).unwrap_err();
        assert!(e.starts_with("jpeg decode:"), "{name}: {e}");
    }
}

// ---------------------------------------------------------------------------------------------
// P: geometry, sampling, metrics

#[test]
fn rect_px_rounding_and_minimum() {
    let r = |x, y, w, h| RoiRect { x, y, w, h };
    assert_eq!(
        roi_rect_px(&r(0.25, 0.5, 0.5, 0.25), 64, 48),
        Some(PxRect {
            x: 16,
            y: 24,
            w: 32,
            h: 12
        })
    );
    assert_eq!(
        roi_rect_px(&r(0.0, 0.0, 1.0, 1.0), 64, 48),
        Some(PxRect {
            x: 0,
            y: 0,
            w: 64,
            h: 48
        })
    );
    // 6.4 → 6, 22.4 → 22
    assert_eq!(
        roi_rect_px(&r(0.1, 0.1, 0.25, 0.5), 64, 48),
        Some(PxRect {
            x: 6,
            y: 5,
            w: 16,
            h: 24
        })
    );
    // 32 → 32.64 rounds to 33: one column → too small
    assert_eq!(roi_rect_px(&r(0.5, 0.0, 0.01, 1.0), 64, 48), None);
    assert_eq!(roi_rect_px(&r(0.0, 0.0, 1.0, 1.0), 1, 48), None);
    assert_eq!(roi_rect_px(&r(0.0, 0.0, 1.0, 1.0), 0, 0), None);
    assert_eq!(
        roi_rect_px(&r(0.0, 0.0, 1.0, 1.0), 2, 2),
        Some(PxRect {
            x: 0,
            y: 0,
            w: 2,
            h: 2
        })
    );
}

#[test]
fn sample_step_is_smallest_within_bound() {
    let count = |w: u32, h: u32, s: u32| (w.div_ceil(s) as usize) * (h.div_ceil(s) as usize);
    let px = |w, h| PxRect { x: 0, y: 0, w, h };
    assert_eq!(sample_step(&px(64, 64)), 1);
    assert_eq!(sample_step(&px(65, 64)), 2);
    assert_eq!(sample_step(&px(1920, 1080)), 23);
    assert_eq!(sample_step(&px(4096, 2)), 2);
    for (w, h) in [
        (2, 2),
        (129, 129),
        (4096, 3),
        (4095, 5),
        (1280, 720),
        (3840, 2160),
        (4096, 4096),
        (333, 77),
        (4096, 1024),
    ] {
        let s = sample_step(&px(w, h));
        assert!(s >= 1 && count(w, h, s) <= MAX_SAMPLES, "{w}x{h} step {s}");
        assert!(
            s == 1 || count(w, h, s - 1) > MAX_SAMPLES,
            "{w}x{h} step {s} not smallest"
        );
    }
}

#[test]
fn sample_roi_grid_order() {
    let f = LumaFrame {
        width: 8,
        height: 6,
        luma: (0..48u8).collect(),
    };
    let s = sample_roi(
        &f,
        &PxRect {
            x: 2,
            y: 1,
            w: 4,
            h: 3,
        },
    );
    assert_eq!(
        (s.width, s.height, s.step, s.rect),
        (
            8,
            6,
            1,
            PxRect {
                x: 2,
                y: 1,
                w: 4,
                h: 3
            }
        )
    );
    assert_eq!(s.values, vec![10, 11, 12, 13, 18, 19, 20, 21, 26, 27, 28, 29]);
    // step 2 on a 65×64 ROI: ceil(65/2)=33 columns × 32 rows, origin first, row-major
    let big = LumaFrame {
        width: 65,
        height: 64,
        luma: (0..65 * 64)
            .map(|i| ((i % 65) as u8).wrapping_add((i / 65) as u8))
            .collect(),
    };
    let s = sample_roi(
        &big,
        &PxRect {
            x: 0,
            y: 0,
            w: 65,
            h: 64,
        },
    );
    assert_eq!((s.step, s.values.len()), (2, 33 * 32));
    assert_eq!(&s.values[..3], &[0, 2, 4]);
    assert_eq!(s.values[33], 2, "second sampled row is y=2 (x=0)");
}

#[test]
fn metrics_values_and_geometry_guard() {
    let px = PxRect {
        x: 0,
        y: 0,
        w: 4,
        h: 4,
    };
    let base = sample_roi(&flat(4, 4, 100), &px);
    let cur = sample_roi(&flat(4, 4, 150), &px);
    let m = metrics(&cur, &base, 24).unwrap();
    assert_eq!((m.mean_luma, m.luma_delta, m.changed_px), (150.0, 50.0, 100.0));
    assert!((m.mean_abs_diff - 50.0 / 255.0 * 100.0).abs() < 1e-9);
    assert_eq!(
        metrics(&cur, &base, 51).unwrap().changed_px,
        0.0,
        "threshold is ≥, 50 < 51"
    );
    assert_eq!(metrics(&cur, &base, 50).unwrap().changed_px, 100.0);
    // half the samples flip black → white; darker = negative delta, judged as magnitude
    let mut half = flat(4, 4, 0);
    half.luma[8..].fill(255);
    let m = metrics(&sample_roi(&half, &px), &sample_roi(&flat(4, 4, 0), &px), 24).unwrap();
    assert_eq!((m.luma_delta, m.mean_abs_diff, m.changed_px), (127.5, 50.0, 50.0));
    let d = metrics(&sample_roi(&flat(4, 4, 0), &px), &sample_roi(&half, &px), 24).unwrap();
    assert_eq!(d.luma_delta, -127.5);
    assert_eq!(
        (
            d.value(Metric::LumaDelta),
            d.value(Metric::MeanAbsDiff),
            d.value(Metric::ChangedPx)
        ),
        (127.5, 50.0, 50.0)
    );
    // JSON rounded to 2 decimals
    let j = metrics(&cur, &base, 24).unwrap().to_json();
    assert_eq!(
        j,
        json!({"mean_luma": 150.0, "luma_delta": 50.0, "mean_abs_diff": 19.61, "changed_px": 100.0})
    );
    // geometry guard: frame size, rect, step
    let other_size = sample_roi(&flat(5, 4, 150), &px);
    assert_eq!(metrics(&other_size, &base, 24), None);
    let other_rect = sample_roi(
        &flat(4, 4, 150),
        &PxRect {
            x: 1,
            y: 0,
            w: 3,
            h: 4,
        },
    );
    assert_eq!(metrics(&other_rect, &base, 24), None);
    let mut other_step = cur.clone();
    other_step.step = 2;
    assert_eq!(metrics(&other_step, &base, 24), None);
}

#[test]
fn baseline_identity_json() {
    let px = PxRect {
        x: 1,
        y: 2,
        w: 3,
        h: 2,
    };
    let b = Baseline {
        frame_id: 42,
        sha256: "ab".repeat(32),
        samples: sample_roi(&flat(8, 8, 7), &px),
    };
    assert_eq!(
        b.to_json(),
        json!({"frame_id": 42, "sha256": "ab".repeat(32), "width": 8, "height": 8, "rect": {"x": 1, "y": 2, "w": 3, "h": 2}, "step": 1, "mean_luma": 7.0})
    );
}

// ---------------------------------------------------------------------------------------------
// O: observe

#[test]
fn observe_reports_unknown_reasons_in_order() {
    let s = spec(json!({"x": 0.5, "w": 0.5}));
    let f = flat(64, 48, 200);
    assert_eq!(
        observe(&s, &f, None, 7),
        (
            Observation::Unknown {
                mono_ns: 7,
                reason: UnknownReason::NoBaseline
            },
            None
        )
    );
    let tiny = spec(json!({"x": 0.5, "w": 0.01}));
    assert_eq!(
        observe(&tiny, &f, None, 7).0,
        Observation::Unknown {
            mono_ns: 7,
            reason: UnknownReason::RoiTooSmall
        },
        "too small wins over no baseline"
    );
    let px = roi_rect_px(&s.rect, 64, 48).unwrap();
    let base = Baseline {
        frame_id: 1,
        sha256: String::new(),
        samples: sample_roi(&flat(64, 48, 100), &px),
    };
    let (o, m) = observe(&s, &f, Some(&base), 9);
    let m = m.unwrap();
    assert_eq!(
        o,
        Observation::Value {
            mono_ns: 9,
            value: m.mean_abs_diff
        }
    );
    assert!((m.mean_abs_diff - 100.0 / 255.0 * 100.0).abs() < 1e-9);
    // resolution changed since the baseline
    assert_eq!(
        observe(&s, &flat(128, 96, 200), Some(&base), 9),
        (
            Observation::Unknown {
                mono_ns: 9,
                reason: UnknownReason::BaselineMismatch
            },
            None
        )
    );
    // ROI moved since the baseline (same frame size)
    let moved = spec(json!({"x": 0.25, "w": 0.5}));
    assert_eq!(
        observe(&moved, &f, Some(&base), 9).0,
        Observation::Unknown {
            mono_ns: 9,
            reason: UnknownReason::BaselineMismatch
        }
    );
    // metric selection
    let lum = spec(json!({"x": 0.5, "w": 0.5, "metric": "luma_delta", "enter": 50}));
    assert_eq!(
        observe(&lum, &f, Some(&base), 9).0,
        Observation::Value {
            mono_ns: 9,
            value: 100.0
        }
    );
}

#[test]
fn decoded_split_fixture_against_black_baseline() {
    // end-to-end pure path: right-half ROI changes fully, left-half ROI does not
    let black = decode_luma(BLACK).unwrap();
    let split = decode_luma(SPLIT).unwrap();
    for (x, changed) in [(0.0, false), (0.625, true)] {
        let s = spec(json!({"name": "half", "x": x, "w": 0.375, "metric": "changed_px", "enter": 50}));
        let px = roi_rect_px(&s.rect, 64, 48).unwrap();
        let base = Baseline {
            frame_id: 1,
            sha256: String::new(),
            samples: sample_roi(&black, &px),
        };
        let (_, m) = observe(&s, &split, Some(&base), 0);
        let v = m.unwrap().changed_px;
        assert!(
            if changed { v == 100.0 } else { v == 0.0 },
            "x={x}: changed_px {v}"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// J: judgment state machine

#[test]
fn judge_starts_unknown_config_changed() {
    let j = RoiJudge::new();
    assert_eq!(
        (j.state(), j.reason(), j.seq()),
        (RoiState::Unknown, Some(UnknownReason::ConfigChanged), 0)
    );
    assert_eq!(
        j.to_json(),
        json!({"state": "unknown", "reason": "config_changed", "transition_seq": 0, "candidate": null})
    );
    assert_eq!(RoiJudge::default(), j);
}

#[test]
fn dwell_hysteresis_and_cooldown() {
    let s = spec(json!({}));
    let mut j = normal_judge(&s);
    // → changed needs v ≥ enter for the dwell
    assert_eq!(j.step(&s, val(400, 20.0)), None);
    assert_eq!(
        j.to_json()["candidate"],
        json!({"to": "changed", "since_mono_ns": ms(400)})
    );
    assert_eq!(j.step(&s, val(600, 20.0)), None);
    let t = j.step(&s, val(700, 20.0)).unwrap();
    assert_eq!(
        t,
        Transition {
            from: RoiState::Normal,
            from_reason: None,
            to: RoiState::Changed,
            reason: None,
            seq: 2,
            mono_ns: ms(700),
            value: Some(20.0)
        }
    );
    // between exit and enter: stays changed (hysteresis), no candidate
    assert_eq!(j.step(&s, val(750, 7.0)), None);
    assert_eq!(j.to_json()["candidate"], Value::Null);
    // ≤ exit for the dwell, but cooldown (1000 ms since the 700 ms switch) holds it
    assert_eq!(j.step(&s, val(800, 4.0)), None);
    assert_eq!(j.step(&s, val(1100, 4.0)), None, "dwell met, cooldown not");
    assert_eq!(
        j.step(&s, val(1400, 5.0)),
        None,
        "exit is inclusive, still cooling down"
    );
    let t = j.step(&s, val(1700, 4.0)).unwrap();
    assert_eq!(
        (t.from, t.to, t.seq, t.value),
        (RoiState::Changed, RoiState::Normal, 3, Some(4.0)),
        "cooldown boundary inclusive"
    );
    assert_eq!(j.state(), RoiState::Normal);
}

#[test]
fn interrupted_dwell_restarts() {
    let s = spec(json!({}));
    let mut j = normal_judge(&s);
    assert_eq!(j.step(&s, val(400, 20.0)), None);
    assert_eq!(
        j.step(&s, val(600, 0.0)),
        None,
        "condition broke: candidate cleared"
    );
    assert_eq!(j.step(&s, val(650, 20.0)), None);
    assert_eq!(j.step(&s, val(800, 20.0)), None, "only 150 ms since restart");
    assert_eq!(j.step(&s, val(950, 20.0)).unwrap().to, RoiState::Changed);
}

#[test]
fn from_unknown_target_follows_enter_only() {
    let s = spec(json!({}));
    let mut j = RoiJudge::new();
    // 7 is above exit (5) but below enter (10): from UNKNOWN that is a normal candidate
    assert_eq!(j.step(&s, val(0, 7.0)), None);
    assert_eq!(
        j.step(&s, val(200, 12.0)),
        None,
        "candidate switched to changed at 200"
    );
    assert_eq!(j.step(&s, val(400, 12.0)), None);
    let t = j.step(&s, val(500, 12.0)).unwrap();
    assert_eq!(
        (t.from, t.from_reason, t.to, t.seq),
        (
            RoiState::Unknown,
            Some(UnknownReason::ConfigChanged),
            RoiState::Changed,
            1
        )
    );
}

/// Regression V-02: the capture stalls while an ROI is `normal` → the ROI must become
/// `unknown(stale)` immediately and need a fresh dwell to come back.
#[test]
fn unknown_is_immediate_and_needs_fresh_dwell() {
    let s = spec(json!({}));
    let mut j = normal_judge(&s);
    let t = j.step(&s, unk(400, UnknownReason::Stale)).unwrap();
    assert_eq!(
        t,
        Transition {
            from: RoiState::Normal,
            from_reason: None,
            to: RoiState::Unknown,
            reason: Some(UnknownReason::Stale),
            seq: 2,
            mono_ns: ms(400),
            value: None
        }
    );
    assert_eq!(
        j.step(&s, unk(500, UnknownReason::Stale)),
        None,
        "same reason: no new transition"
    );
    let t = j.step(&s, unk(600, UnknownReason::NotLive)).unwrap();
    assert_eq!(
        (t.from_reason, t.reason, t.seq),
        (Some(UnknownReason::Stale), Some(UnknownReason::NotLive), 3)
    );
    assert_eq!(j.step(&s, val(700, 0.0)), None);
    assert_eq!(j.step(&s, val(900, 0.0)), None);
    assert_eq!(j.step(&s, val(1000, 0.0)).unwrap().to, RoiState::Normal);
    // an UNKNOWN observation also clears a pending candidate
    assert_eq!(j.step(&s, val(1100, 20.0)), None);
    assert_eq!(
        j.step(&s, unk(1150, UnknownReason::DecodeError)).unwrap().to,
        RoiState::Unknown
    );
    assert_eq!(j.to_json()["candidate"], Value::Null);
}

/// No unseen transition is inferred: a gap longer than max(dwell, 1000 ms) between analysed
/// frames makes the ROI UNKNOWN(analysis_gap), even if both values agree.
#[test]
fn analysis_gap_never_infers() {
    let s = spec(json!({}));
    let mut j = normal_judge(&s);
    assert_eq!(j.step(&s, val(1300, 0.0)), None, "exactly 1000 ms is not a gap");
    let t = j.step(&s, val(2301, 20.0)).unwrap();
    assert_eq!(
        (t.from, t.to, t.reason, t.value),
        (
            RoiState::Normal,
            RoiState::Unknown,
            Some(UnknownReason::AnalysisGap),
            None
        )
    );
    // the gap value seeded a fresh candidate (changed, since 2301)
    assert_eq!(
        j.to_json()["candidate"],
        json!({"to": "changed", "since_mono_ns": ms(2301)})
    );
    assert_eq!(j.step(&s, val(2500, 20.0)), None);
    let t = j.step(&s, val(2601, 20.0)).unwrap();
    assert_eq!(
        (t.from, t.from_reason, t.to),
        (
            RoiState::Unknown,
            Some(UnknownReason::AnalysisGap),
            RoiState::Changed
        )
    );
    // a long dwell raises the gap limit
    let slow = spec(json!({"dwell_ms": 2000}));
    let mut k = RoiJudge::new();
    assert_eq!(k.step(&slow, val(0, 0.0)), None);
    assert_eq!(k.step(&slow, val(1500, 0.0)), None, "1500 ms ≤ 2000 ms limit");
    assert_eq!(k.step(&slow, val(2000, 0.0)).unwrap().to, RoiState::Normal);
    assert_eq!(
        k.step(&slow, val(4001, 0.0)).unwrap().reason,
        Some(UnknownReason::AnalysisGap)
    );
}

#[test]
fn zero_dwell_config_change_and_bad_values() {
    let s = spec(json!({"dwell_ms": 0, "cooldown_ms": 0}));
    let mut j = RoiJudge::new();
    assert_eq!(
        j.step(&s, val(0, 0.0)).unwrap().to,
        RoiState::Normal,
        "dwell 0 completes on the same step"
    );
    assert_eq!(j.step(&s, val(10, 50.0)).unwrap().to, RoiState::Changed);
    let t = j.step(&s, unk(20, UnknownReason::ConfigChanged)).unwrap();
    assert_eq!(
        (t.from, t.to, t.reason, t.seq),
        (
            RoiState::Changed,
            RoiState::Unknown,
            Some(UnknownReason::ConfigChanged),
            3
        )
    );
    // non-finite values are decode errors, not judgments
    let t = j.step(&s, val(30, f64::NAN)).unwrap();
    assert_eq!(t.reason, Some(UnknownReason::DecodeError));
    assert_eq!(j.step(&s, val(40, f64::INFINITY)), None, "same reason again");
}

#[test]
fn transition_json_shape() {
    let t = Transition {
        from: RoiState::Normal,
        from_reason: None,
        to: RoiState::Unknown,
        reason: Some(UnknownReason::Stale),
        seq: 4,
        mono_ns: 9,
        value: None,
    };
    assert_eq!(
        t.to_json(),
        json!({"from": "normal", "from_reason": null, "to": "unknown", "reason": "stale", "transition_seq": 4, "mono_ns": 9, "value": null})
    );
    let t = Transition {
        from: RoiState::Unknown,
        from_reason: Some(UnknownReason::NoBaseline),
        to: RoiState::Changed,
        reason: None,
        seq: 1,
        mono_ns: 5,
        value: Some(12.3456),
    };
    assert_eq!(t.to_json()["value"], json!(12.35));
    assert_eq!(t.to_json()["from_reason"], "no_baseline");
}
