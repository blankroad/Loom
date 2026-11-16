//! Capture archive (operator-upgrade logic B2a, finding L1-R2) — Opus-authored specification tests.
//! Pure codec + real files in a per-test temp dir (removed on drop). No engine, no device.
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};

use loom_logic::archive::{
    self, ANN_FILE, ArchiveArgs, ArchiveConfig, ArchiveSink, ArchiveState, BLOCK_MAX_RECORDS,
    BLOCK_MAX_SAMPLES, DEFAULT_MAX_CAPTURES, ExportReq, FORMAT, MANIFEST, Manifest, ManifestMeta, RAW_FILE,
    RAW_HEADER, RAW_INDEX, RawEncoder, RawIndexEntry, ReadReq, decode_block, parse_raw_index,
    valid_capture_id,
};
use loom_logic::model::{Annotation, parse_annotation};
use loom_proto::error::ErrorCode;
use serde_json::{Value, json};

struct Tmp(PathBuf);
impl Tmp {
    fn new(name: &str) -> Tmp {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let p = std::env::temp_dir().join(format!(
            "loom-logic-archive-{name}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&p).unwrap();
        Tmp(p)
    }
}
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Deterministic signal with runs of varying length on all 8 bits.
fn signal(n: usize, seed: u64) -> Vec<u8> {
    let mut x = seed;
    let mut out = Vec::with_capacity(n);
    while out.len() < n {
        x = x
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let run = 1 + ((x >> 33) % 40) as usize;
        let v = (x >> 17) as u8;
        out.extend(std::iter::repeat_n(v, run.min(n - out.len())));
    }
    out
}

/// Reconstruct samples from blocks; asserts tiling and block invariants.
fn reconstruct(blocks: &[archive::EncodedBlock], first: u64) -> Vec<u8> {
    let mut out = Vec::new();
    let mut next = first;
    for b in blocks {
        assert_eq!(b.start_sample, next, "blocks tile the sample axis");
        assert!(b.end_sample > b.start_sample);
        assert!(b.n_records <= BLOCK_MAX_RECORDS);
        assert!(b.end_sample - b.start_sample <= BLOCK_MAX_SAMPLES);
        let (d, used) = decode_block(&b.bytes).unwrap();
        assert_eq!(used, b.bytes.len());
        assert_eq!((d.start_sample, d.state), (b.start_sample, b.state));
        assert_eq!(d.edges.len() as u32, b.n_records);
        let mut cur = d.state;
        let mut s = b.start_sample;
        for (es, ev) in d.edges {
            assert!(es > s, "edges strictly after the block start / previous edge");
            out.extend(std::iter::repeat_n(cur, (es - s) as usize));
            cur = ev;
            s = es;
        }
        out.extend(std::iter::repeat_n(cur, (b.end_sample - s) as usize));
        next = b.end_sample;
    }
    out
}

fn encode_all(data: &[u8], splits: &[usize]) -> (Vec<archive::EncodedBlock>, u64) {
    let mut e = RawEncoder::new(0);
    let mut blocks = Vec::new();
    let mut at = 0usize;
    for &n in splits.iter().chain(std::iter::once(&usize::MAX)) {
        let end = at.saturating_add(n).min(data.len());
        blocks.extend(e.feed(&data[at..end]));
        at = end;
        if at == data.len() {
            break;
        }
    }
    let (last, end) = e.finish();
    blocks.extend(last);
    (blocks, end)
}

// ---------------------------------------------------------------------------------------------
// Raw codec

#[test]
fn raw_roundtrip_is_lossless_across_arbitrary_feed_splits() {
    let data = signal(300_000, 7);
    for splits in [vec![300_000], vec![1, 2, 3, 65_536, 7], vec![4096; 80]] {
        let (blocks, end) = encode_all(&data, &splits);
        assert_eq!(end, data.len() as u64);
        assert_eq!(reconstruct(&blocks, 0), data);
    }
}

#[test]
fn blocks_close_before_the_byte_that_would_exceed_records_or_span() {
    let toggles: Vec<u8> = (0..10_000u32).map(|i| (i & 1) as u8).collect();
    let (b, end) = encode_all(&toggles, &[10_000]);
    assert_eq!(end, 10_000);
    let shape: Vec<(u64, u64, u32, u8)> = b
        .iter()
        .map(|x| (x.start_sample, x.end_sample, x.n_records, x.state))
        .collect();
    assert_eq!(
        shape,
        vec![(0, 4097, 4096, 0), (4097, 8194, 4096, 1), (8194, 10_000, 1805, 0)]
    );
    let idle = vec![0x55u8; (BLOCK_MAX_SAMPLES + 5) as usize];
    let (b, _) = encode_all(&idle, &[1000, usize::MAX]);
    let shape: Vec<(u64, u64, u32)> = b
        .iter()
        .map(|x| (x.start_sample, x.end_sample, x.n_records))
        .collect();
    assert_eq!(
        shape,
        vec![
            (0, BLOCK_MAX_SAMPLES, 0),
            (BLOCK_MAX_SAMPLES, BLOCK_MAX_SAMPLES + 5, 0)
        ]
    );
    // nothing fed: no block, end = first sample; offsets preserved for a non-zero first sample
    assert_eq!(RawEncoder::new(42).finish(), (None, 42));
    let mut e = RawEncoder::new(1_000);
    assert!(e.feed(&[3, 3, 4]).is_empty());
    let (last, end) = e.finish();
    let last = last.unwrap();
    assert_eq!((last.start_sample, last.end_sample, end), (1_000, 1_003, 1_003));
    assert_eq!(reconstruct(&[last], 1_000), vec![3, 3, 4]);
}

#[test]
fn block_bytes_and_index_layout_are_exact() {
    let mut e = RawEncoder::new(10);
    let _ = e.feed(&[1, 1, 1, 1, 1, 0]); // transition at sample 15: delta 5
    let (b, _) = e.finish();
    let b = b.unwrap();
    let mut want = Vec::new();
    want.extend(10u64.to_le_bytes());
    want.push(1);
    want.extend(1u32.to_le_bytes());
    want.extend(2u32.to_le_bytes());
    want.extend([0x05, 0x00]);
    assert_eq!(b.bytes, want);
    // big delta → multi-byte LEB128
    let mut e = RawEncoder::new(0);
    let mut d = vec![0u8; 300];
    d.push(1);
    let _ = e.feed(&d);
    let (b, _) = e.finish();
    assert_eq!(
        &b.unwrap().bytes[archive::BLOCK_HEADER_BYTES..],
        &[0xAC, 0x02, 0x01]
    );
    let ent = RawIndexEntry {
        start_sample: 0x0102,
        offset: 8,
        state: 0xAB,
        n_records: 3,
    };
    let bytes = ent.to_bytes();
    let mut want = Vec::new();
    want.extend(0x0102u64.to_le_bytes());
    want.extend(8u64.to_le_bytes());
    want.push(0xAB);
    want.extend(3u32.to_le_bytes());
    want.extend([0, 0, 0]);
    assert_eq!(bytes.to_vec(), want);
    let ent2 = RawIndexEntry {
        start_sample: 0x0200,
        offset: 40,
        state: 0,
        n_records: 0,
    };
    let mut idx = [ent.to_bytes(), ent2.to_bytes()].concat();
    assert_eq!(parse_raw_index(&idx).unwrap(), (vec![ent, ent2], false));
    idx.extend([1, 2, 3, 4, 5]); // crash mid-append
    assert_eq!(parse_raw_index(&idx).unwrap(), (vec![ent, ent2], true));
    let bad = [ent2.to_bytes(), ent.to_bytes()].concat();
    assert_eq!(parse_raw_index(&bad).unwrap_err().code, ErrorCode::Internal);
}

#[test]
fn corrupt_blocks_are_errors_not_guesses() {
    let block = |n: u32, payload: &[u8]| {
        let mut v = Vec::new();
        v.extend(10u64.to_le_bytes());
        v.push(1);
        v.extend(n.to_le_bytes());
        v.extend((payload.len() as u32).to_le_bytes());
        v.extend(payload);
        v
    };
    let (ok, used) = decode_block(&block(1, &[0x05, 0x00])).unwrap();
    assert_eq!((ok.edges, used), (vec![(15, 0)], 19));
    for (name, bytes) in [
        ("delta 0", block(1, &[0x00, 0x00])),
        ("n mismatch", block(2, &[0x05, 0x00])),
        ("truncated payload", block(1, &[0x05, 0x00])[..18].to_vec()),
        ("truncated header", vec![1, 2, 3]),
        (
            "varint > 10 bytes",
            block(
                1,
                &[0x80; 11].iter().copied().chain([0x01, 0x00]).collect::<Vec<_>>(),
            ),
        ),
    ] {
        let e = decode_block(&bytes).unwrap_err();
        assert_eq!(e.code, ErrorCode::Internal, "{name}");
    }
}

// ---------------------------------------------------------------------------------------------
// Annotation lines, manifest, args

#[test]
fn annotation_lines_roundtrip_to_the_live_row_shape() {
    let a = parse_annotation("210-290 uart-1: 41");
    assert_eq!(
        archive::ann_line(&a),
        b"{\"s\":210,\"e\":290,\"d\":\"uart-1\",\"t\":\"41\"}\n".to_vec()
    );
    let raw = parse_annotation("garbage \"x\\y\u{1}");
    let line = archive::ann_line(&raw);
    assert_eq!(*line.last().unwrap(), b'\n');
    let line = String::from_utf8(line).unwrap();
    assert!(line.starts_with("{\"s\":null,\"e\":null,\"d\":null,\"t\":"));
    for (a, row) in [(a, 7u64), (raw, 8)] {
        let l = String::from_utf8(archive::ann_line(&a)).unwrap();
        let mut want = a.to_row(1_000_000);
        want["row"] = json!(row);
        want["source"] = json!("archive");
        assert_eq!(archive::ann_row(l.trim_end(), row, 1_000_000).unwrap(), want);
    }
    assert!(archive::ann_row("not json", 0, 1).is_none());
    assert!(archive::ann_row("{\"s\":1}", 0, 1).is_none());
}

fn manifest(id: &str, started: u64) -> Manifest {
    Manifest {
        format: FORMAT.into(),
        capture_id: id.into(),
        source: "synthetic".into(),
        samplerate: 1_000_000,
        decoders: vec![
            "uart:rx=D0:baudrate=115200:data_bits=8:parity=none:stop_bits=1:bit_order=lsb-first".into(),
        ],
        state: ArchiveState::Complete,
        started_wall_ns: started,
        updated_wall_ns: started,
        raw_first_sample: 0,
        raw_end_sample: 1000,
        acquired_samples: Some(1000),
        raw_bytes: 100,
        raw_blocks: 1,
        raw_truncated_at_sample: None,
        raw_error: None,
        ann_rows: 3,
        ann_bytes: 90,
        ann_truncated_at_row: None,
        ann_error: None,
        max_capture_bytes: 1 << 20,
    }
}

#[test]
fn manifest_roundtrip_and_completeness_rules() {
    let m = manifest("lc-1a-1", 5);
    let v = m.to_json();
    assert_eq!(v["format"], FORMAT);
    assert_eq!(v["raw"]["end_sample"], 1000);
    assert_eq!(v["annotations"]["truncated_at_row"], Value::Null);
    assert_eq!(Manifest::from_json(&v).unwrap(), m);
    assert!(m.complete());
    for bad in [
        Manifest {
            state: ArchiveState::Recording,
            ..m.clone()
        },
        Manifest {
            raw_truncated_at_sample: Some(900),
            raw_end_sample: 900,
            ..m.clone()
        },
        Manifest {
            ann_truncated_at_row: Some(1),
            ..m.clone()
        },
        Manifest {
            acquired_samples: Some(1001),
            ..m.clone()
        },
        Manifest {
            acquired_samples: None,
            ..m.clone()
        },
        Manifest {
            ann_error: Some("decoder consumed 10 of 1000 samples".into()),
            ..m.clone()
        },
        Manifest {
            raw_error: Some("x".into()),
            ..m.clone()
        },
    ] {
        assert!(!bad.complete(), "{bad:?}");
    }
    let mut w = v.clone();
    w["format"] = json!("other/1");
    assert!(Manifest::from_json(&w).is_err());
    let t = Tmp::new("manifest");
    m.write(&t.0).unwrap();
    assert!(!t.0.join("manifest.json.tmp").exists());
    assert_eq!(Manifest::read(&t.0).unwrap(), m);
}

#[test]
fn archive_args_from_config_and_wire() {
    let home = Path::new("/h");
    let a = ArchiveArgs::from_config(&json!({}), Some(home)).unwrap();
    let want = ArchiveConfig {
        root: PathBuf::from("/h/logic-archive"),
        max_capture_bytes: 256 << 20,
        max_total_bytes: 2048 << 20,
        max_age_s: 168 * 3600,
        max_captures: DEFAULT_MAX_CAPTURES,
    };
    assert_eq!(a, ArchiveArgs::Enabled(want.clone()));
    assert_eq!(ArchiveArgs::from_args(&a.to_args()).unwrap(), a);
    let a = ArchiveArgs::from_config(
        &json!({"archive_max_capture_mb": 8, "archive_max_captures": 2}),
        Some(home),
    )
    .unwrap();
    let ArchiveArgs::Enabled(c) = a else { panic!() };
    assert_eq!((c.max_capture_bytes, c.max_captures), (8 << 20, 2));
    assert_eq!(
        ArchiveArgs::from_config(&json!({"archive": false}), Some(home)).unwrap(),
        ArchiveArgs::Disabled {
            reason: "disabled by [logic] archive=false".into()
        }
    );
    assert_eq!(
        ArchiveArgs::from_config(&json!({}), None).unwrap(),
        ArchiveArgs::Disabled {
            reason: "daemon storage roots unavailable; logic archive is never written to a default home"
                .into()
        }
    );
    for bad in [
        json!({"archive_max_capture_mb": "5"}),
        json!({"archive_max_capture_mb": 0}),
        json!({"archive": "yes"}),
        json!({"archive_max_age_h": 87_601}),
    ] {
        let e = ArchiveArgs::from_config(&bad, Some(home)).unwrap_err();
        assert!(e.starts_with("[logic] archive"), "{e}");
    }
    assert_eq!(
        ArchiveArgs::from_args(&Value::Null).unwrap(),
        ArchiveArgs::Disabled {
            reason: "archive not configured".into()
        }
    );
    let d = ArchiveArgs::Disabled { reason: "x".into() };
    assert_eq!(ArchiveArgs::from_args(&d.to_args()).unwrap(), d);
    let mut rel = ArchiveArgs::Enabled(want).to_args();
    rel["root"] = json!("relative/dir");
    assert!(ArchiveArgs::from_args(&rel).is_err());
}

#[test]
fn capture_ids_are_path_safe() {
    for ok in ["lc-19a2b3c4d5e-1", "lc-0-18446744073709551615"] {
        assert!(valid_capture_id(ok), "{ok}");
    }
    for bad in [
        "",
        "lc-../x",
        "LC-1-1",
        "lc-1-",
        "lc-g-1",
        "lc-1-1/..",
        "lc-1-1 ",
        "lc-12345678901234567-1",
    ] {
        assert!(!valid_capture_id(bad), "{bad}");
    }
}

#[test]
fn read_and_export_requests_are_strict() {
    let r = ReadReq::from_params(&json!({"capture_id": "lc-1-1", "kind": "edges", "from_sample": 5, "to_sample": 9, "mask": 3, "limit": 10})).unwrap();
    assert_eq!(ReadReq::from_params(&r.to_params()).unwrap(), r);
    let r = ReadReq::from_params(&json!({"capture_id": "lc-1-1", "kind": "annotations", "after_row": 4}))
        .unwrap();
    assert_eq!(ReadReq::from_params(&r.to_params()).unwrap(), r);
    for bad in [
        json!({"capture_id": "lc-1-1"}),
        json!({"capture_id": "../x", "kind": "edges"}),
        json!({"capture_id": "lc-1-1", "kind": "edges", "x": 1}),
        json!({"capture_id": "lc-1-1", "kind": "edges", "from_sample": 9, "to_sample": 9}),
        json!({"capture_id": "lc-1-1", "kind": "edges", "limit": 0}),
        json!({"capture_id": "lc-1-1", "kind": "edges", "limit": 2049}),
        json!({"capture_id": "lc-1-1", "kind": "annotations", "limit": 1001}),
        json!({"capture_id": "lc-1-1", "kind": "edges", "mask": 0}),
        json!({"capture_id": "lc-1-1", "kind": "edges", "mask": 256}),
        json!({"capture_id": "lc-1-1", "kind": "annotations", "mask": 1}),
        json!({"capture_id": "lc-1-1", "kind": "edges", "after_row": 1}),
    ] {
        assert!(ReadReq::from_params(&bad).is_err(), "{bad}");
    }
    let e = ExportReq::from_params(&json!({"capture_id": "lc-1-1", "kind": "annotations", "format": "txt"}))
        .unwrap();
    assert_eq!(ExportReq::from_params(&e.to_params()).unwrap(), e);
    assert!(
        ExportReq::from_params(&json!({"capture_id": "lc-1-1", "kind": "edges", "format": "txt"})).is_err()
    );
    assert!(ExportReq::from_params(&json!({"capture_id": "lc-1-1", "kind": "edges"})).is_err());
}

// ---------------------------------------------------------------------------------------------
// Writer + readers on real files

fn cfg(root: &Path, max_capture_bytes: u64) -> ArchiveConfig {
    ArchiveConfig {
        root: root.to_owned(),
        max_capture_bytes,
        max_total_bytes: 1 << 30,
        max_age_s: 3600,
        max_captures: 8,
    }
}

fn meta(id: &str) -> ManifestMeta {
    ManifestMeta {
        capture_id: id.into(),
        source: "synthetic".into(),
        samplerate: 1_000_000,
        decoders: vec![],
        first_sample: 0,
    }
}

/// Record `data` + `anns` through a real sink; returns the final status object.
fn record(c: &ArchiveConfig, id: &str, data: &[u8], anns: &[Annotation]) -> Value {
    let sink = ArchiveSink::open(c, &meta(id), 1_000).unwrap();
    assert_eq!(sink.status()["state"], "recording");
    let tap = sink.tap();
    let mut enc = RawEncoder::new(0);
    for (i, chunk) in data.chunks(65_536).enumerate() {
        sink.raw(enc.feed(chunk));
        if i == 0 {
            for a in anns {
                tap.push(a);
            }
            tap.push(&Annotation::loss_marker(9)); // never archived
        }
    }
    drop(tap);
    let (last, end) = enc.finish();
    sink.finish(last, end, data.len() as u64)
}

fn ann(s: u64, text: &str) -> Annotation {
    parse_annotation(&format!("{s}-{} uart-1: {text}", s + 80))
}

fn read(root: &Path, p: Value) -> Value {
    archive::read(root, &ReadReq::from_params(&p).unwrap()).unwrap()
}

/// All chunks of a paged edges read, following `next_from`.
fn read_all_edges(root: &Path, id: &str, mask: u8, limit: usize) -> (Vec<Value>, Vec<Value>) {
    let (mut chunks, mut results) = (Vec::new(), Vec::new());
    let mut from = 0u64;
    loop {
        let r = read(
            root,
            json!({"capture_id": id, "kind": "edges", "from_sample": from, "mask": mask, "limit": limit}),
        );
        chunks.extend(r["chunks"].as_array().unwrap().clone());
        results.push(r.clone());
        match r["next_from"].as_u64() {
            Some(n) => from = n,
            None => return (chunks, results),
        }
        assert!(results.len() < 10_000);
    }
}

/// Expected masked transitions of `data`.
fn truth(data: &[u8], mask: u8) -> Vec<(u64, u8)> {
    let mut out = Vec::new();
    let mut cur = data[0] & mask;
    for (i, &b) in data.iter().enumerate().skip(1) {
        if b & mask != cur {
            cur = b & mask;
            out.push((i as u64, cur));
        }
    }
    out
}

#[test]
fn sink_records_everything_and_reads_page_without_holes() {
    let t = Tmp::new("sink");
    let c = cfg(&t.0, 64 << 20);
    let id = "lc-a1-1";
    let data = signal(400_000, 3);
    let anns: Vec<Annotation> = (0..5)
        .map(|i| ann(i * 100, &format!("4{i}")))
        .chain([parse_annotation("garbled")])
        .collect();
    let st = record(&c, id, &data, &anns);
    assert_eq!(st["state"], "complete");
    assert_eq!(
        (st["raw_end_sample"].as_u64(), st["annotation_rows"].as_u64()),
        (Some(400_000), Some(6))
    );
    assert_eq!(st["error"], Value::Null);
    let dir = t.0.join(id);
    for f in [MANIFEST, RAW_FILE, RAW_INDEX, ANN_FILE] {
        assert!(dir.join(f).is_file(), "{f}");
    }
    assert_eq!(std::fs::read(dir.join(RAW_FILE)).unwrap()[..8], RAW_HEADER);
    let m = Manifest::read(&dir).unwrap();
    assert!(m.complete());
    assert_eq!(m.ann_rows, 6);
    // list
    let l = archive::list(&t.0);
    assert_eq!(l["archives"][0]["capture_id"], id);
    assert_eq!(l["archives"][0]["complete"], true);
    // edges: paged, contiguous, equal to the truth, ≤ 1024 edges per chunk, live payload shape
    for (mask, limit) in [(0xFFu8, 2048usize), (0x01, 100)] {
        let (chunks, results) = read_all_edges(&t.0, id, mask, limit);
        let mut next = 0;
        let mut got = Vec::new();
        for ch in &chunks {
            assert_eq!(ch["sample_start"].as_u64(), Some(next));
            assert_eq!(ch["source"], "archive");
            assert_eq!(ch["seq"], Value::Null);
            assert_eq!(ch["dropped"], 0);
            let e = ch["edges"].as_array().unwrap();
            assert!(e.len() <= loom_logic::model::MAX_EDGES_PER_EVENT);
            got.extend(
                e.iter()
                    .map(|x| (x[0].as_u64().unwrap(), x[1].as_u64().unwrap() as u8)),
            );
            next = ch["sample_end"].as_u64().unwrap();
        }
        assert_eq!(next, 400_000);
        assert_eq!(chunks[0]["initial"].as_u64(), Some(u64::from(data[0] & mask)));
        assert_eq!(got, truth(&data, mask), "mask {mask:#x}");
        let last = results.last().unwrap();
        assert_eq!(last["complete"], true);
        assert!(
            results[..results.len() - 1]
                .iter()
                .all(|r| r["complete"] == false)
        );
        assert!(
            results
                .iter()
                .all(|r| serde_json::to_string(r).unwrap().len() < 60 * 1024)
        );
    }
    // sub-range: initial = true state at from
    let r = read(
        &t.0,
        json!({"capture_id": id, "kind": "edges", "from_sample": 12_345, "to_sample": 12_400}),
    );
    assert_eq!(r["chunks"][0]["initial"].as_u64(), Some(u64::from(data[12_345])));
    assert_eq!(r["range"]["returned"], json!([12_345, 12_400]));
    // annotations: paging by row and filtering by sample
    let r = read(&t.0, json!({"capture_id": id, "kind": "annotations", "limit": 2}));
    let texts: Vec<&str> = r["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["text"].as_str().unwrap())
        .collect();
    assert_eq!(
        (texts, r["next_row"].as_u64(), r["complete"].as_bool()),
        (vec!["40", "41"], Some(1), Some(false))
    );
    let r = read(
        &t.0,
        json!({"capture_id": id, "kind": "annotations", "after_row": 1}),
    );
    assert_eq!(r["rows"].as_array().unwrap().len(), 4); // 42 43 44 + unparsed
    assert_eq!(r["rows"][3]["class"], "_raw");
    assert_eq!(r["complete"], true);
    let r = read(
        &t.0,
        json!({"capture_id": id, "kind": "annotations", "from_sample": 150, "to_sample": 350}),
    );
    let texts: Vec<&str> = r["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["text"].as_str().unwrap())
        .collect();
    assert_eq!(texts, ["42", "43"]);
    // errors
    let e = archive::read(
        &t.0,
        &ReadReq::from_params(&json!({"capture_id": "lc-ff-9", "kind": "edges"})).unwrap(),
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::NotFound);
}

#[test]
fn quota_truncation_is_sticky_exact_and_never_complete() {
    let t = Tmp::new("quota");
    let c = cfg(&t.0, 20_000);
    let id = "lc-b2-1";
    let toggles: Vec<u8> = (0..200_000u32).map(|i| (i & 1) as u8).collect();
    let st = record(&c, id, &toggles, &[ann(0, "41")]);
    assert_eq!(st["state"], "partial");
    let at = st["raw_truncated_at_sample"].as_u64().unwrap();
    assert_eq!(st["raw_end_sample"].as_u64(), Some(at));
    assert!(at < 200_000);
    assert!(st["error"].as_str().unwrap().contains("archive quota"));
    let m = Manifest::read(&t.0.join(id)).unwrap();
    assert!(!m.complete());
    assert_eq!(m.state, ArchiveState::Partial);
    let on_disk: u64 = [RAW_FILE, RAW_INDEX, ANN_FILE, archive::ANN_INDEX]
        .iter()
        .map(|f| std::fs::metadata(t.0.join(id).join(f)).map_or(0, |m| m.len()))
        .sum();
    assert!(on_disk <= 20_000, "{on_disk}");
    // reading past the truncation returns nothing invented and says why
    let r = read(&t.0, json!({"capture_id": id, "kind": "edges", "from_sample": 0}));
    let end = r["chunks"]
        .as_array()
        .unwrap()
        .last()
        .map_or(0, |c| c["sample_end"].as_u64().unwrap());
    assert!(end <= at);
    assert_eq!(r["complete"], false);
    assert_eq!(r["truncated_at"].as_u64(), Some(at));
}

#[test]
fn prune_rotates_only_owned_archives_oldest_first() {
    let t = Tmp::new("prune");
    let mk = |id: &str, started: u64| {
        let d = t.0.join(id);
        std::fs::create_dir_all(&d).unwrap();
        manifest(id, started).write(&d).unwrap();
        std::fs::write(d.join(RAW_FILE), vec![0u8; 1000]).unwrap();
    };
    let hour = 3_600_000_000_000u64;
    let now = 100 * hour;
    mk("lc-1-1", now - 99 * hour); // older than max_age 48 h
    mk("lc-2-2", now - 3 * hour);
    mk("lc-3-3", now - 2 * hour);
    mk("lc-4-4", now - hour);
    std::fs::create_dir_all(t.0.join("notes")).unwrap(); // foreign: never touched
    std::fs::create_dir_all(t.0.join("lc-5-5")).unwrap(); // no manifest: never touched
    let c = ArchiveConfig {
        root: t.0.clone(),
        max_capture_bytes: 1 << 20,
        max_total_bytes: 1 << 30,
        max_age_s: 48 * 3600,
        max_captures: 3,
    };
    let r = archive::prune(&c, Some("lc-2-2"), now);
    // age removes lc-1-1; count: 3 left ≥ 3 → remove the oldest unprotected (lc-3-3), lc-2-2 is protected
    assert_eq!(r.removed, ["lc-1-1", "lc-3-3"]);
    for (id, exists) in [
        ("lc-1-1", false),
        ("lc-2-2", true),
        ("lc-3-3", false),
        ("lc-4-4", true),
        ("notes", true),
        ("lc-5-5", true),
    ] {
        assert_eq!(t.0.join(id).exists(), exists, "{id}");
    }
    // bytes (all files of candidate dirs, ≈ 1000 B raw + manifest each): total quota smaller than
    // used + one capture → oldest removed until it fits
    let used: u64 = ["lc-2-2", "lc-4-4"]
        .iter()
        .flat_map(|id| std::fs::read_dir(t.0.join(id)).unwrap())
        .map(|e| e.unwrap().metadata().unwrap().len())
        .sum();
    let one = used / 2;
    let c = ArchiveConfig {
        max_total_bytes: (1 << 20) + used - 1,
        max_captures: 100,
        max_age_s: 1 << 40,
        ..c
    };
    let r = archive::prune(&c, None, now);
    assert_eq!(r.removed, ["lc-2-2"]);
    assert!(r.kept_bytes <= one + 64, "{} vs {one}", r.kept_bytes);
}

#[test]
fn export_writes_metadata_and_escaped_csv_inside_the_capture_dir() {
    let t = Tmp::new("export");
    let c = cfg(&t.0, 64 << 20);
    let id = "lc-c3-1";
    let data = signal(5_000, 11);
    record(
        &c,
        id,
        &data,
        &[ann(10, "4C"), parse_annotation("100-180 uart-1: a,\"b\"")],
    );
    let req =
        ExportReq::from_params(&json!({"capture_id": id, "kind": "annotations", "format": "csv"})).unwrap();
    let r = archive::export(&t.0, &req, c.max_capture_bytes).unwrap();
    let path = PathBuf::from(r["path"].as_str().unwrap());
    assert!(path.is_absolute() && path.starts_with(t.0.join(id)));
    assert_eq!(
        (
            r["rows"].as_u64(),
            r["complete"].as_bool(),
            r["limit_hit"].as_bool()
        ),
        (Some(2), Some(true), Some(false))
    );
    let text = std::fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        &lines[..10],
        [
            "# loom logic export",
            &format!("# capture_id={id}"),
            "# kind=annotations",
            "# samplerate=1000000",
            "# from_sample=0",
            "# to_sample=5000",
            "# archive_state=complete",
            "# complete=true",
            "# truncated_at=none",
            "start_sample,end_sample,t_start_s,t_end_s,decoder,text",
        ]
    );
    assert_eq!(lines[10], "10,90,0.00001,0.00009,uart-1,4C");
    assert_eq!(lines[11], "100,180,0.0001,0.00018,uart-1,\"a,\"\"b\"\"\"");
    // a second export gets a new file, never overwrites
    let r2 = archive::export(&t.0, &req, c.max_capture_bytes).unwrap();
    assert_ne!(r2["path"], r["path"]);
    // edges csv: first row = state at from, then transitions
    let req = ExportReq::from_params(
        &json!({"capture_id": id, "kind": "edges", "format": "csv", "from_sample": 100, "to_sample": 200}),
    )
    .unwrap();
    let r = archive::export(&t.0, &req, c.max_capture_bytes).unwrap();
    let text = std::fs::read_to_string(r["path"].as_str().unwrap()).unwrap();
    let rows: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
    assert_eq!(rows[0], "sample,time_s,D0,D1,D2,D3,D4,D5,D6,D7");
    let bits = |v: u8| {
        (0..8)
            .map(|i| ((v >> i) & 1).to_string())
            .collect::<Vec<_>>()
            .join(",")
    };
    assert_eq!(rows[1], format!("100,0.0001,{}", bits(data[100])));
    assert_eq!(rows.len() - 2, truth(&data[100..200], 0xFF).len());
    // txt
    let req =
        ExportReq::from_params(&json!({"capture_id": id, "kind": "annotations", "format": "txt"})).unwrap();
    let r = archive::export(&t.0, &req, c.max_capture_bytes).unwrap();
    let text = std::fs::read_to_string(r["path"].as_str().unwrap()).unwrap();
    assert!(text.lines().any(|l| l == "0.00001 uart-1: 4C"));
    // quota applies to exports too
    let e = archive::export(&t.0, &req, 10).unwrap_err();
    assert_eq!(e.code, ErrorCode::Usage);
    assert!(e.message.contains("archive quota"));
}

// ---------------------------------------------------------------------------------------------
// B2a.1 amendment

#[test]
fn decoder_gap_makes_the_archive_partial_but_keeps_rows() {
    let t = Tmp::new("gap");
    let c = cfg(&t.0, 64 << 20);
    let id = "lc-d4-1";
    let data = signal(50_000, 5);
    let sink = ArchiveSink::open(&c, &meta(id), 1_000).unwrap();
    let tap = sink.tap();
    tap.push(&ann(0, "41"));
    tap.push(&ann(100, "42"));
    drop(tap);
    let mut enc = RawEncoder::new(0);
    sink.raw(enc.feed(&data));
    sink.mark_decode_gap("decoder consumed 20000 of 50000 samples; decoder did not exit cleanly");
    sink.mark_decode_gap("second text is ignored");
    let (last, end) = enc.finish();
    let st = sink.finish(last, end, 50_000);
    assert_eq!(st["state"], "partial");
    assert_eq!(
        st["error"],
        "decoder consumed 20000 of 50000 samples; decoder did not exit cleanly"
    );
    assert_eq!(
        (st["raw_end_sample"].as_u64(), st["annotation_rows"].as_u64()),
        (Some(50_000), Some(2))
    );
    assert_eq!(st["annotation_truncated_at_row"], Value::Null);
    let m = Manifest::read(&t.0.join(id)).unwrap();
    assert_eq!(
        m.ann_error.as_deref(),
        Some("decoder consumed 20000 of 50000 samples; decoder did not exit cleanly")
    );
    assert!(!m.complete());
    assert_eq!(archive::list(&t.0)["archives"][0]["complete"], false);
    let r = read(&t.0, json!({"capture_id": id, "kind": "annotations"}));
    assert_eq!(r["rows"].as_array().unwrap().len(), 2);
    assert_eq!(r["complete"], false);
    // raw part alone is still whole
    let (chunks, _) = read_all_edges(&t.0, id, 0xFF, 2048);
    assert_eq!(chunks.last().unwrap()["sample_end"].as_u64(), Some(50_000));
}

#[test]
fn export_streams_to_its_final_name_without_leftovers() {
    let t = Tmp::new("export-stream");
    let c = cfg(&t.0, 64 << 20);
    let id = "lc-e5-1";
    let anns: Vec<Annotation> = (0..3000).map(|i| ann(i * 100, "41")).collect();
    record(&c, id, &signal(400_000, 9), &anns);
    let req =
        ExportReq::from_params(&json!({"capture_id": id, "kind": "annotations", "format": "csv"})).unwrap();
    let r = archive::export(&t.0, &req, c.max_capture_bytes).unwrap();
    assert_eq!(
        (r["rows"].as_u64(), r["complete"].as_bool()),
        (Some(3000), Some(true))
    );
    let names: Vec<String> = std::fs::read_dir(t.0.join(id))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        names
            .iter()
            .all(|n| !n.ends_with(".part") && !n.ends_with(".tmp")),
        "{names:?}"
    );
    let text = std::fs::read_to_string(r["path"].as_str().unwrap()).unwrap();
    assert!(text.starts_with("# loom logic export\n"));
    assert_eq!(text.lines().filter(|l| l.ends_with(",uart-1,41")).count(), 3000);
    assert_eq!(r["bytes"].as_u64(), Some(text.len() as u64));
}

#[test]
fn reader_refuses_a_block_that_disagrees_with_its_index() {
    let t = Tmp::new("idxmismatch");
    let c = cfg(&t.0, 64 << 20);
    let id = "lc-f6-1";
    record(&c, id, &signal(300_000, 2), &[]);
    // corrupt: shift the start sample stored in the first block header (index still says 0)
    let raw = t.0.join(id).join(RAW_FILE);
    let mut b = std::fs::read(&raw).unwrap();
    b[8] ^= 0x01; // RAW_HEADER is 8 bytes; block header begins with start_sample LE
    std::fs::write(&raw, b).unwrap();
    let e = archive::read(
        &t.0,
        &ReadReq::from_params(&json!({"capture_id": id, "kind": "edges"})).unwrap(),
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::Internal);
}
