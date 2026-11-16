//! UART TX+RX in one capture and decoder error rows (operator-upgrade logic B4, finding L6) — Opus spec tests.
//! Model checks are pure; engine checks use real sigrok-cli on synthetic bytes (skipped when absent).
#![allow(clippy::unwrap_used)]

use std::time::{Duration, Instant};

use loom_logic::model::{Decoder, StartConfig, Synth, parse_annotation, row_role};
use loom_logic::worker::LogicWorker;
use loom_logic::{KIND_ANNOTATIONS, KIND_CAPTURE_STATE};
use loom_proto::error::ErrorCode;
use loom_testkit::FakeWorker;
use serde_json::{Value, json};

const FILTER_RX: &str = "uart=rx-data:rx-warnings:rx-parity-err:rx-break";
const FILTER_TX: &str = "uart=tx-data:tx-warnings:tx-parity-err:tx-break";

#[test]
fn duplex_spec_parses_to_two_stacks() {
    let d = Decoder::parse("uart:rx=D0:tx=D1:baudrate=100000").unwrap();
    let canon = "uart:rx=D0:tx=D1:baudrate=100000:data_bits=8:parity=none:stop_bits=1:bit_order=lsb-first";
    assert_eq!(d.canonical(), canon);
    assert_eq!(
        Decoder::parse("uart:tx=D1:rx=D0:baudrate=100000")
            .unwrap()
            .canonical(),
        canon
    );
    assert_eq!(Decoder::parse(canon).unwrap(), d, "canonical round-trips");
    assert_eq!(
        d.pd_args(),
        [
            "uart:rx=0:baudrate=100000:data_bits=8:parity=none:stop_bits=1.0:bit_order=lsb-first:format=hex",
            "uart:tx=1:baudrate=100000:data_bits=8:parity=none:stop_bits=1.0:bit_order=lsb-first:format=hex"
        ]
    );
    assert_eq!(
        d.annotation_filter(),
        format!("{FILTER_RX}:{}", &FILTER_TX["uart=".len()..])
    );
    assert_eq!(d.mask(), 0b11);
    assert_eq!(
        d.tracks(),
        json!([{"id": "uart-1", "line": "rx", "channel": "D0"}, {"id": "uart-2", "line": "tx", "channel": "D1"}])
    );
    let e = Decoder::parse("uart:rx=D2:tx=D2").unwrap_err();
    assert_eq!(e.code, ErrorCode::Usage);
    // decoder argv carries one -P per stack, then the shared -A filter
    let c = StartConfig::from_params(
        &json!({"source": "sigrok-demo", "decoders": ["uart:rx=D0:tx=D1:baudrate=100000"]}),
    )
    .unwrap();
    let a = c.decoder_args().unwrap();
    let ps: Vec<&String> = a
        .iter()
        .enumerate()
        .filter(|(i, _)| *i > 0 && a[i - 1] == "-P")
        .map(|(_, v)| v)
        .collect();
    assert_eq!(ps.len(), 2);
    assert_eq!(a[a.len() - 2..], ["-A".to_owned(), d.annotation_filter()]);
}

#[test]
fn single_line_and_spi_shapes() {
    let rx = Decoder::parse("uart:rx=D3:baudrate=9600").unwrap();
    assert_eq!(rx.pd_args(), [format!("{}:format=hex", rx.pd_arg())]);
    assert_eq!(rx.annotation_filter(), FILTER_RX);
    assert_eq!(
        rx.tracks(),
        json!([{"id": "uart-1", "line": "rx", "channel": "D3"}])
    );
    let tx = Decoder::parse("uart:tx=D4:baudrate=9600").unwrap();
    assert_eq!(tx.annotation_filter(), FILTER_TX);
    assert_eq!(
        tx.tracks(),
        json!([{"id": "uart-1", "line": "tx", "channel": "D4"}])
    );
    let spi = Decoder::parse("spi:clk=D0:mosi=D1:miso=D2:annotate=miso").unwrap();
    assert_eq!(spi.pd_args(), [spi.pd_arg()]);
    assert_eq!(spi.annotation_filter(), "spi=miso-data");
    assert_eq!(
        spi.tracks(),
        json!([{"id": "spi-1", "line": "miso", "channel": "D2"}])
    );
}

#[test]
fn rows_carry_a_loom_derived_role_and_class_stays_null() {
    for (t, r) in [
        ("41", "data"),
        ("141", "data"),
        ("DEADBEEF", "data"),
        ("123456789", "error"),
        ("4c", "error"),
        ("Frame error", "error"),
        ("Parity error", "error"),
        ("", "error"),
    ] {
        assert_eq!(row_role(t), r, "{t:?}");
    }
    let e = parse_annotation("420-430 uart-1: Frame error").to_row(1_000_000);
    assert_eq!(
        (e["role"].clone(), e["class"].clone()),
        (json!("error"), Value::Null)
    );
    assert_eq!(parse_annotation("210-290 uart-2: 61").to_row(1)["role"], "data");
    assert_eq!(parse_annotation("garbage").to_row(1)["role"], Value::Null);
}

#[test]
fn glitch_is_synthetic_uart_only_and_round_trips() {
    let ok =
        json!({"source": "synthetic", "decoders": ["uart:rx=D0:tx=D1:baudrate=100000"], "glitch_every": 3});
    let c = StartConfig::from_params(&ok).unwrap();
    assert_eq!(c.glitch_every, Some(3));
    assert_eq!(c.to_params()["glitch_every"], 3);
    assert_eq!(StartConfig::from_params(&c.to_params()).unwrap(), c);
    assert!(
        StartConfig::from_params(&json!({"source": "synthetic", "decoders": ["uart:rx=D0"]}))
            .unwrap()
            .to_params()
            .get("glitch_every")
            .is_none()
    );
    for bad in [
        json!({"source": "synthetic", "decoders": ["uart:rx=D0"], "glitch_every": 0}),
        json!({"source": "synthetic", "decoders": ["uart:rx=D0"], "glitch_every": 1001}),
        json!({"source": "synthetic", "decoders": ["uart:rx=D0"], "glitch_every": "2"}),
        json!({"source": "synthetic", "decoders": ["spi:clk=D0:mosi=D1"], "glitch_every": 2}),
        json!({"source": "sigrok-demo", "decoders": ["uart:rx=D0"], "glitch_every": 2}),
    ] {
        let e = StartConfig::from_params(&bad).unwrap_err();
        assert_eq!(e.code, ErrorCode::Usage, "{bad}");
    }
}

#[test]
fn synthetic_duplex_and_glitch_waveform_shape() {
    // 1 MS/s, 100 kBd → 10 samples/bit. Frame period 100 ms → TX frame starts at period/2 = 50 000.
    let c = StartConfig::from_params(&json!({"source": "synthetic", "samplerate": 1_000_000,
        "decoders": ["uart:rx=D0:tx=D1:baudrate=100000"], "text": "UU", "glitch_every": 2}))
    .unwrap();
    let mut g = Synth::new(&c).unwrap();
    let mut out = Vec::new();
    g.fill(&mut out, 60_000);
    let bit = |s: usize, b: u8| (out[s] >> b) & 1;
    assert!(out.iter().all(|v| v & !0b11 == 0), "only D0/D1 are driven");
    assert!(
        (0..20).all(|s| out[s] == 0b11),
        "both lines idle high before the frames"
    );
    // RX: byte 1 start bit at 20, byte 2 start bit at 120, byte 2 stop bit (glitched) LOW at 210..220
    assert!((20..30).all(|s| bit(s, 0) == 0));
    assert!(
        (110..120).all(|s| bit(s, 0) == 1),
        "byte 1 stop bit is a normal HIGH"
    );
    assert!((120..130).all(|s| bit(s, 0) == 0));
    assert!((210..220).all(|s| bit(s, 0) == 0), "every 2nd byte: LOW stop bit");
    assert!((220..300).all(|s| bit(s, 0) == 1));
    // TX: same bytes on D1 from 50 000 (2 idle bit-times, then start bit), never glitched
    assert!((0..50_020).all(|s| bit(s, 1) == 1));
    assert!((50_020..50_030).all(|s| bit(s, 1) == 0));
    assert!(
        (50_210..50_220).all(|s| bit(s, 1) == 1),
        "TX stop bits are never glitched"
    );
    assert!((50_000..50_300).all(|s| bit(s, 0) == 1), "RX idle while TX talks");
}

fn engine() -> Option<String> {
    let p = loom_logic::model::resolve_engine(None, std::env::var("PATH").ok().as_deref()).ok()?;
    p.to_str().map(str::to_owned)
}

#[test]
fn duplex_capture_decodes_both_lines_and_reports_errors() {
    let Some(e) = engine() else {
        eprintln!("SKIP: sigrok-cli not installed");
        return;
    };
    let mut fw = FakeWorker::new(LogicWorker::new());
    let cfg = json!({"source": "synthetic", "samplerate": 1_000_000, "decoders": ["uart:rx=D0:tx=D1:baudrate=100000"],
                     "text": "LOOM\r\n", "frame_period_ms": 20, "glitch_every": 5, "duration_ms": 1500});
    fw.call("logic.start", json!({"config": cfg, "engine": e}))
        .unwrap();
    let t0 = Instant::now();
    let mut ev = Vec::new();
    let fin = loop {
        ev.extend(fw.drain_published());
        if let Some(s) = ev
            .iter()
            .rev()
            .find(|x| x.kind == KIND_CAPTURE_STATE)
            .map(|x| x.payload.clone())
            && (s["state"] == "stopped" || s["state"] == "failed")
        {
            break s;
        }
        assert!(t0.elapsed() < Duration::from_secs(20));
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(fin["state"], "stopped", "{fin}");
    assert_eq!(
        fin["decoder_tracks"],
        json!([{"id": "uart-1", "line": "rx", "channel": "D0"}, {"id": "uart-2", "line": "tx", "channel": "D1"}])
    );
    let rows: Vec<Value> = ev
        .iter()
        .filter(|x| x.kind == KIND_ANNOTATIONS)
        .flat_map(|x| x.payload["annotations"].as_array().cloned().unwrap_or_default())
        .collect();
    let of = |dec: &str, role: &str| {
        rows.iter()
            .filter(|r| r["decoder"] == dec && r["role"] == role)
            .count()
    };
    assert!(
        of("uart-1", "data") > 20 && of("uart-2", "data") > 20,
        "both lines decoded"
    );
    assert!(of("uart-1", "error") > 0, "glitched RX stop bits are reported");
    assert_eq!(of("uart-2", "error"), 0, "TX is clean");
    assert!(
        rows.iter()
            .filter(|r| r["role"] == "error")
            .all(|r| r["text"] == "Frame error" && r["class"].is_null())
    );
    assert_eq!(
        fin["counters"]["error_rows"].as_u64(),
        Some(of("uart-1", "error") as u64 + of("uart-2", "error") as u64)
    );
    // TX bytes spell the same text on their own track
    let tx: String = rows
        .iter()
        .filter(|r| r["decoder"] == "uart-2")
        .filter_map(|r| u8::from_str_radix(r["text"].as_str()?, 16).ok())
        .map(char::from)
        .take(12)
        .collect();
    assert_eq!(tx, "LOOM\r\nLOOM\r\n");
}
