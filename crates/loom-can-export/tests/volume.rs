#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use common::*;
use loom_can_export::*;
use loom_can_frame::{CanFrame, CanId};
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::time::{Duration, Instant};

#[test]
fn hundred_thousand_frames_are_streamed_with_bounded_containers() {
    let dir = Directory::new();
    let count = std::env::var("LOOM_EXPORT_VOLUME_COUNT")
        .ok()
        .map(|s| s.parse::<u64>().unwrap())
        .unwrap_or(100_000);
    let begin = Instant::now();
    for format in [Format::Blf, Format::Asc] {
        let started = Instant::now();
        let path = dir.0.join(if format == Format::Blf {
            "large.blf"
        } else {
            "large.asc"
        });
        // The producer owns one frame at a time; neither producer nor consumer collects records.
        let frames = (0..count).map(|i| {
            let mut f = frame(i * 1_000_000);
            if i % 2 == 0 {
                f.frame =
                    CanFrame::fd(CanId::extended(0x18daf110).unwrap(), &[0x5a; 64], true, true).unwrap();
            }
            f.metadata.source = Some(SourceRef {
                producer_id: "volume".into(),
                producer_epoch: 1,
                source_seq: i,
            });
            f
        });
        let summary = export_frames(&path, format, Options::default(), frames).unwrap();
        assert_eq!(summary.frames, count);
        assert!(summary.max_sidecar_line <= MAX_SIDECAR_LINE);
        assert!(summary.max_asc_line <= MAX_ASC_LINE);
        let export_elapsed = started.elapsed();
        if format == Format::Blf {
            let reader = bounded_blf_reader(BufReader::new(File::open(&path).unwrap())).unwrap();
            assert_eq!(reader.header().object_count, u32::try_from(count).unwrap());
            for (i, object) in reader.enumerate() {
                assert_eq!(object.unwrap().timestamp(), i as u64 * 1_000_000);
            }
            // Independent container walk validates actual wire sizes, not just configured limits.
            let mut file = File::open(&path).unwrap();
            file.seek(SeekFrom::Start(144)).unwrap();
            let mut containers = 0;
            loop {
                let mut header = [0u8; 32];
                match file.read_exact(&mut header) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                    Err(e) => panic!("{e}"),
                }
                assert_eq!(&header[..4], b"LOBJ");
                let size = u32::from_le_bytes(header[8..12].try_into().unwrap());
                let raw = u32::from_le_bytes(header[24..28].try_into().unwrap());
                assert!(raw < CONTAINER_FLUSH_BYTES as u32);
                assert!(size <= CONTAINER_FLUSH_BYTES as u32 + 32);
                file.seek(SeekFrom::Current(i64::from(size - 32 + (size & 3))))
                    .unwrap();
                containers += 1;
            }
            assert!(containers > 0);
            if count >= 3000 {
                assert!(containers > 1);
            }
        } else {
            let mut frames = 0;
            // read_line reuses one bounded small buffer; no AscFile whole-file parser here.
            let mut reader = BufReader::new(File::open(&path).unwrap());
            let mut line = String::with_capacity(MAX_ASC_LINE);
            while reader.read_line(&mut line).unwrap() != 0 {
                assert!(line.len() <= MAX_ASC_LINE);
                if line.contains("CANFD") || line.contains(" Tx d ") {
                    frames += 1;
                }
                line.clear();
            }
            assert_eq!(frames, count);
        }
        let sidecar = ExportPaths::new(&path).unwrap().sidecar;
        let mut reader = BufReader::new(File::open(sidecar).unwrap());
        let mut line = Vec::with_capacity(MAX_SIDECAR_LINE);
        let mut rows = 0;
        while reader.read_until(b'\n', &mut line).unwrap() != 0 {
            assert!(line.len() <= MAX_SIDECAR_LINE);
            rows += 1;
            line.clear();
        }
        assert_eq!(rows, count + 2);
        eprintln!(
            "{format:?}: frames={count}, export={export_elapsed:?}, max_sidecar={}, max_asc={}",
            summary.max_sidecar_line, summary.max_asc_line
        );
    }
    eprintln!(
        "export and independent streaming verification total={:?}",
        begin.elapsed()
    );
    assert!(
        begin.elapsed() < Duration::from_secs(10),
        "volume export/verification exceeded 10s"
    );
}
