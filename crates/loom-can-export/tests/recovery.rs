#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use common::*;
use loom_can_export::*;
use serde_json::Value;
use std::fs;
use std::io::{self, Cursor, Seek, SeekFrom, Write};

#[test]
fn publish_finished_pair_and_refuse_existing_paths() {
    let dir = Directory::new();
    let path = dir.0.join("capture.blf");
    let paths = ExportPaths::new(&path).unwrap();
    let summary = export_frames(&path, Format::Blf, Options::default(), [frame(1)]).unwrap();
    assert_eq!(summary.frames, 1);
    assert!(paths.primary.is_file() && paths.sidecar.is_file());
    assert!(!paths.primary_partial.exists() && !paths.sidecar_partial.exists());
    let original = fs::read(&path).unwrap();
    let err = export_frames(&path, Format::Blf, Options::default(), [frame(2)]).unwrap_err();
    assert!(matches!(err.error, Error::AlreadyExists));
    assert_eq!(fs::read(&path).unwrap(), original);
    let side = fs::read_to_string(paths.sidecar).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(side.lines().last().unwrap()).unwrap()["kind"],
        "complete"
    );
}
#[test]
fn input_failure_retains_partials_with_last_staged_coordinates() {
    let dir = Directory::new();
    let path = dir.0.join("interrupted.asc");
    let mut f = frame(1);
    f.metadata.source = Some(SourceRef {
        producer_id: "producer".into(),
        producer_epoch: 11,
        source_seq: 9_007_199_254_740_993,
    });
    let result = export_file(
        &path,
        Format::Asc,
        Options::default(),
        [Ok(f.into()), Err(Error::Input("journal unavailable"))],
    )
    .unwrap_err();
    assert_eq!(result.staged.frames, 1);
    let paths = result.paths.unwrap();
    assert!(!paths.primary.exists() && !paths.sidecar.exists());
    assert!(paths.primary_partial.is_file() && paths.sidecar_partial.is_file());
    let side = fs::read_to_string(paths.sidecar_partial).unwrap();
    let rows: Vec<Value> = side.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert!(!rows.iter().any(|r| r["kind"] == "complete"));
    assert_eq!(rows.last().unwrap()["kind"], "failed");
    assert_eq!(
        rows.last().unwrap()["last_source"]["source_seq"],
        "9007199254740993"
    );
}
#[test]
fn existing_partial_is_never_modified() {
    let dir = Directory::new();
    let paths = ExportPaths::new(dir.0.join("old.blf")).unwrap();
    fs::write(&paths.sidecar_partial, b"old partial").unwrap();
    assert!(export_frames(&paths.primary, Format::Blf, Options::default(), [frame(1)]).is_err());
    assert_eq!(fs::read(paths.sidecar_partial).unwrap(), b"old partial");
    assert!(!paths.primary.exists());
}
#[test]
fn destination_created_during_input_prevents_publication() {
    let dir = Directory::new();
    let paths = ExportPaths::new(dir.0.join("race.blf")).unwrap();
    let records = std::iter::once_with(|| {
        fs::write(&paths.primary, b"other writer").unwrap();
        Ok(frame(1).into())
    });
    let failure = export_file(&paths.primary, Format::Blf, Options::default(), records).unwrap_err();
    assert!(matches!(failure.error, Error::AlreadyExists));
    assert_eq!(fs::read(&paths.primary).unwrap(), b"other writer");
    assert!(!paths.sidecar.exists());
    assert!(paths.primary_partial.exists() && paths.sidecar_partial.exists());
    let rows = fs::read_to_string(paths.sidecar_partial).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(rows.lines().last().unwrap()).unwrap()["kind"],
        "failed"
    );
}
#[test]
fn invalid_or_descending_records_poison_stream() {
    let mut invalid = frame(2);
    invalid.metadata.request_id = Some("a".repeat(MAX_LABEL_BYTES + 1));
    let mut channel = frame(2);
    channel.channel_index = u16::MAX;
    let cases = [
        invalid.into(),
        channel.into(),
        frame(0).into(),
        ExportRecord::Gap(Gap {
            ts_mono_ns: 2,
            scope: GapScope::Source,
            count: Some(0),
            reason: "bad count".into(),
            metadata: Metadata::default(),
        }),
        ExportRecord::Gap(Gap {
            ts_mono_ns: 2,
            scope: GapScope::Display,
            count: Some(1),
            reason: "a".repeat(257),
            metadata: Metadata::default(),
        }),
    ];
    for record in cases {
        let mut writer = Exporter::new(
            Cursor::new(Vec::new()),
            Vec::new(),
            Format::Blf,
            Options::default(),
        )
        .unwrap();
        writer.push(&frame(1).into()).unwrap();
        assert!(writer.push(&record).is_err());
        assert_eq!(writer.summary().records, 1);
        assert!(writer.push(&frame(3).into()).is_err());
        assert!(writer.finish().is_err());
    }
    let mut writer = Exporter::new(
        Cursor::new(Vec::new()),
        Vec::new(),
        Format::Asc,
        Options {
            anchor_wall_ns: Some(u64::MAX),
            ..Options::default()
        },
    )
    .unwrap();
    assert!(matches!(
        writer.push(&frame(1).into()),
        Err(Error::TimestampOverflow)
    ));
}
struct BrokenSidecar;
impl Write for BrokenSidecar {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("injected write failure"))
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
struct BrokenFinish {
    inner: Cursor<Vec<u8>>,
    seeks: usize,
}
impl Write for BrokenFinish {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.inner.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl Seek for BrokenFinish {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.seeks += 1;
        if self.seeks > 2 {
            Err(io::Error::other("injected finish seek failure"))
        } else {
            self.inner.seek(pos)
        }
    }
}
#[test]
fn io_and_finish_failures_are_errors() {
    assert!(
        Exporter::new(
            Cursor::new(Vec::new()),
            BrokenSidecar,
            Format::Blf,
            Options::default()
        )
        .is_err()
    );
    let mut writer = Exporter::new(
        BrokenFinish {
            inner: Cursor::new(Vec::new()),
            seeks: 0,
        },
        Vec::new(),
        Format::Blf,
        Options::default(),
    )
    .unwrap();
    writer.push(&frame(1).into()).unwrap();
    assert!(writer.finish().is_err());
}
#[test]
fn known_gap_does_not_manufacture_an_object() {
    let mut writer = Exporter::new(
        Cursor::new(Vec::new()),
        Vec::new(),
        Format::Blf,
        Options::default(),
    )
    .unwrap();
    for scope in [GapScope::Source, GapScope::Record, GapScope::Display] {
        writer
            .push(&ExportRecord::Gap(Gap {
                ts_mono_ns: 1,
                scope,
                count: Some(u64::MAX),
                reason: "loss".into(),
                metadata: Metadata::default(),
            }))
            .unwrap();
    }
    let (output, sidecar, summary) = writer.finish().unwrap();
    assert_eq!((summary.records, summary.gaps, summary.objects), (3, 3, 0));
    assert_eq!(output.into_inner().len(), 144);
    let text = String::from_utf8(sidecar).unwrap();
    assert!(text.contains("18446744073709551615"));
}
