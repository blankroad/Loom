#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;
use autors_asc::{AscFile, AscRecord};
use autors_blf::objects::{BlfObject, CanFdFlags, CanFlags, ObjectFlags};
use common::*;
use loom_can_export::*;
use loom_can_frame::{CanFrame, CanId, DeviceTimestamp, Direction, TimestampSource};
use serde_json::Value;
use std::io::Cursor;

fn encode(format: Format, options: Options, records: &[ExportRecord]) -> (Vec<u8>, Vec<Value>, Summary) {
    let mut writer = Exporter::new(Cursor::new(Vec::new()), Vec::new(), format, options).unwrap();
    for record in records {
        writer.push(record).unwrap();
    }
    let (output, sidecar, summary) = writer.finish().unwrap();
    let rows = std::str::from_utf8(&sidecar)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    (output.into_inner(), rows, summary)
}
fn fixtures() -> Vec<ExportRecord> {
    let frames = [
        CanFrame::classic(CanId::standard(0x123).unwrap(), &[1, 2]).unwrap(),
        CanFrame::classic(CanId::extended(0x1abcde).unwrap(), &[255; 8]).unwrap(),
        CanFrame::remote(CanId::standard(0x7ff).unwrap(), 8).unwrap(),
        CanFrame::remote(CanId::extended(0x123).unwrap(), 3).unwrap(),
        CanFrame::fd(CanId::extended(0x1fffffff).unwrap(), &[0xab; 64], true, true).unwrap(),
        CanFrame::fd(CanId::standard(1).unwrap(), &[0xcd; 12], false, false).unwrap(),
        CanFrame::classic(CanId::standard(0).unwrap(), &[]).unwrap(),
    ];
    let mut records: Vec<_> = frames
        .into_iter()
        .enumerate()
        .map(|(i, f)| {
            ExportRecord::Frame(ExportFrame::new(
                (i as u64 + 1) * 1000,
                (i % 3) as u16,
                if i % 2 == 0 { Direction::Tx } else { Direction::Rx },
                f,
                Origin::Device,
            ))
        })
        .collect();
    records.push(ExportRecord::Error(ErrorFrame {
        ts_mono_ns: 8000,
        wall_ns: None,
        channel_index: 2,
        dir: Direction::Tx,
        origin: Origin::User,
        description: "driver bus error".into(),
        metadata: Metadata::default(),
    }));
    records.push(ExportRecord::Gap(Gap {
        ts_mono_ns: 9000,
        scope: GapScope::Record,
        count: None,
        reason: "unknown loss".into(),
        metadata: Metadata::default(),
    }));
    records
}
#[test]
fn independent_blf_reader_preserves_all_frame_fields() {
    let records = fixtures();
    for compressed in [false, true] {
        let (bytes, rows, summary) = encode(
            Format::Blf,
            Options {
                compressed,
                ..Options::default()
            },
            &records,
        );
        let reader = bounded_blf_reader(Cursor::new(bytes)).unwrap();
        assert_eq!(reader.header().object_count, 8);
        assert_eq!((summary.frames, summary.errors, summary.gaps), (7, 1, 1));
        let objects: Vec<_> = reader.map(Result::unwrap).collect();
        for (record, object) in records.iter().zip(&objects) {
            assert_eq!(
                object.timestamp(),
                match record {
                    ExportRecord::Frame(f) => f.ts_mono_ns,
                    ExportRecord::Error(e) => e.ts_mono_ns,
                    _ => unreachable!(),
                }
            );
            assert_eq!(object.header().object_flags, ObjectFlags::TIME_ONE_NANS);
            match (record, object) {
                (ExportRecord::Frame(f), BlfObject::CanMessage(o)) => {
                    assert_eq!(o.channel, f.channel_index + 1);
                    assert_eq!(
                        o.id,
                        f.frame.id().raw() | if f.frame.id().is_extended() { 0x80000000 } else { 0 }
                    );
                    assert_eq!(o.dlc, f.frame.dlc());
                    assert_eq!(o.flags.0 & CanFlags::RTR.0 != 0, f.frame.is_remote());
                    assert_eq!(o.flags.0 & CanFlags::TX.0 != 0, f.dir == Direction::Tx);
                    assert_eq!(&o.data[..f.frame.data().len()], f.frame.data());
                }
                (ExportRecord::Frame(f), BlfObject::CanFdMessage(o)) => {
                    assert_eq!(o.channel, f.channel_index + 1);
                    assert_eq!(
                        o.id,
                        f.frame.id().raw() | if f.frame.id().is_extended() { 0x80000000 } else { 0 }
                    );
                    assert_eq!(o.dlc, f.frame.dlc());
                    assert_eq!(usize::from(o.valid_data_bytes), f.frame.len());
                    assert_eq!(&o.data[..f.frame.len()], f.frame.data());
                    assert_eq!(o.flags.0 & CanFlags::TX.0 != 0, f.dir == Direction::Tx);
                    let expected = if f.frame.len() == 64 {
                        CanFdFlags::EDL.0 | CanFdFlags::BRS.0 | CanFdFlags::ESI.0
                    } else {
                        CanFdFlags::EDL.0
                    };
                    assert_eq!(o.fd_flags.0, expected);
                }
                (ExportRecord::Error(_), BlfObject::CanError(o)) => assert_eq!(o.channel, 3),
                _ => panic!("wrong object kind"),
            }
        }
        assert_eq!(rows[8]["dir"], "tx");
        assert_eq!(rows[9]["count"], Value::Null);
        assert_eq!(rows[9]["object_index"], Value::Null);
    }
}
#[test]
fn independent_asc_reader_and_hand_checked_lines() {
    let records = fixtures();
    let (bytes, _, summary) = encode(Format::Asc, Options::default(), &records);
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.starts_with("date Thu Jan 01 00:00:00.000 1970\nbase hex timestamps absolute\n"));
    assert!(text.contains(" 0.000001000 1 123 Tx d 2 01 02\n"));
    assert!(text.contains(" 0.000003000 3 7FF Tx r 8\n"));
    assert!(text.contains(" 0.000004000 1 123x Rx r 3\n"));
    assert!(text.contains("CANFD 2 Tx 1FFFFFFFx 1 1 F 64 AB AB"));
    assert!(text.contains(" 0 0 7000 0 0 0 0 0\n"));
    assert!(text.contains(" 0.000008000 3 ErrorFrame\n"));
    let parsed = AscFile::parse(&text).unwrap();
    let messages: Vec<_> = parsed
        .records
        .iter()
        .filter_map(|r| {
            if let AscRecord::Message(m) = r {
                Some(m)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(messages.len(), 7);
    for (record, m) in records.iter().zip(messages) {
        let ExportRecord::Frame(f) = record else {
            panic!("frame")
        };
        assert_eq!(m.channel, u32::from(f.channel_index) + 1);
        assert_eq!(
            m.id,
            f.frame.id().raw() | if f.frame.id().is_extended() { 0x80000000 } else { 0 }
        );
        assert_eq!(m.dlc, f.frame.dlc());
        assert_eq!(m.data, f.frame.data());
        assert_eq!(m.is_remote, f.frame.is_remote());
        assert_eq!(m.direction == autors_asc::Direction::Tx, f.dir == Direction::Tx);
        assert_eq!(m.timestamp.as_nanos(), u128::from(f.ts_mono_ns));
    }
    assert!(summary.max_asc_line <= MAX_ASC_LINE);
}
#[test]
fn hand_checked_blf_header_container_and_classic_object() {
    let (bytes, _, _) = encode(Format::Blf, Options::default(), &[frame(123456789).into()]);
    assert_eq!(&bytes[..8], b"LOGG\x90\0\0\0");
    assert_eq!(u64::from_le_bytes(bytes[16..24].try_into().unwrap()), 224);
    assert_eq!(&bytes[32..36], &1u32.to_le_bytes());
    // SYSTEMTIME: 1970-01-01 Thu 00:00:00.000; end truncates to .123 ms.
    assert_eq!(
        &bytes[40..56],
        &[0xb2, 7, 1, 0, 4, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    );
    assert_eq!(
        &bytes[56..72],
        &[0xb2, 7, 1, 0, 4, 0, 1, 0, 0, 0, 0, 0, 0, 0, 123, 0]
    );
    // Container has a base header (version 0); the inner CAN object uses version 1.
    assert_eq!(&bytes[144..160], b"LOBJ\x10\0\0\0\x50\0\0\0\x0a\0\0\0");
    assert_eq!(&bytes[168..172], &48u32.to_le_bytes());
    // v1 header, CAN_MESSAGE type 1, TIME_ONE_NANS=2, 123456789ns,
    // channel 1, Tx=1, DLC 2, id 0x123, payload 01 02 then zero fill.
    let golden = [
        0x4c, 0x4f, 0x42, 0x4a, 32, 0, 1, 0, 48, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0x15, 0xcd,
        0x5b, 7, 0, 0, 0, 0, 1, 0, 1, 2, 0x23, 1, 0, 0, 1, 2, 0, 0, 0, 0, 0, 0,
    ];
    assert_eq!(&bytes[176..224], &golden);
}
#[test]
fn exact_anchor_across_midnight_and_provenance() {
    let wall = 86_399_999_999_500;
    let options = Options {
        anchor_mono_ns: 1000,
        anchor_wall_ns: Some(wall),
        compressed: false,
    };
    let mut f = frame(1000);
    f.wall_ns = Some(wall + 7); // Observation remains independent of anchor projection.
    f.device_ts = Some(DeviceTimestamp {
        value_ns: u64::MAX,
        source: TimestampSource::Hardware,
        clock_id: 7,
    });
    f.metadata = Metadata {
        request_id: Some("req\n\"한글".into()),
        source: Some(SourceRef {
            producer_id: "can-worker".into(),
            producer_epoch: u64::MAX,
            source_seq: u64::MAX,
        }),
        time_quality: TimeQuality::Unknown,
    };
    let records = [f.into(), frame(2000).into()];
    let (bytes, rows, _) = encode(Format::Blf, options, &records);
    let reader = bounded_blf_reader(Cursor::new(bytes)).unwrap();
    assert_eq!((reader.header().start.day, reader.header().end.day), (1, 2));
    assert_eq!(
        (
            reader.header().end.hour,
            reader.header().end.second,
            reader.header().end.milliseconds
        ),
        (0, 0, 0)
    );
    let stamps: Vec<_> = reader.map(|o| o.unwrap().timestamp()).collect();
    assert_eq!(stamps, [999500, 1000500]);
    assert_eq!(rows[1]["wall_ns"], (wall + 7).to_string());
    assert_eq!(rows[1]["source"]["source_seq"], u64::MAX.to_string());
    assert_eq!(rows[1]["device_ts"]["value_ns"], u64::MAX.to_string());
    assert_eq!(rows[1]["time_quality"], "unknown");
    let (asc, _, _) = encode(Format::Asc, options, &records);
    let asc = String::from_utf8(asc).unwrap();
    assert!(asc.contains("Thu Jan 01 23:59:59.999 1970"));
    assert!(asc.contains(" 0.000999500 1 123"));
    assert!(asc.contains(" 0.001000500 1 123"));
}
#[test]
fn empty_input_and_zero_length_fd() {
    let dir = Directory::new();
    for format in [Format::Blf, Format::Asc] {
        export_frames(
            dir.0.join(format!("empty-{format:?}")),
            format,
            Options::default(),
            [],
        )
        .unwrap();
        let (bytes, rows, summary) = encode(format, Options::default(), &[]);
        assert_eq!(summary.records, 0);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["wall_anchor_known"], false);
        assert_eq!(rows[1]["kind"], "complete");
        if format == Format::Blf {
            assert_eq!(bytes.len(), 144);
            assert_eq!(bounded_blf_reader(Cursor::new(bytes)).unwrap().count(), 0);
        }
    }
    let mut f = frame(1);
    f.frame = CanFrame::fd(CanId::standard(0).unwrap(), &[], false, true).unwrap();
    let (bytes, _, _) = encode(Format::Blf, Options::default(), &[f.clone().into()]);
    let BlfObject::CanFdMessage(o) = bounded_blf_reader(Cursor::new(bytes))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
    else {
        panic!("FD")
    };
    assert_eq!((o.dlc, o.valid_data_bytes, o.fd_flags.0), (0, 0, 5));
    let (asc, _, _) = encode(Format::Asc, Options::default(), &[f.into()]);
    // Minimal independent token check: upstream ASC reader mislabels FD length 0 as RTR.
    let text = String::from_utf8(asc).unwrap();
    let columns: Vec<_> = text
        .lines()
        .find(|l| l.contains("CANFD"))
        .unwrap()
        .split_whitespace()
        .collect();
    assert_eq!(
        &columns[..9],
        &["0.000000001", "CANFD", "1", "Tx", "0", "0", "1", "0", "0"]
    );
}
#[test]
fn bounded_reader_rejects_header_container_and_object_claims() {
    let (original, _, _) = encode(Format::Blf, Options::default(), &[frame(1).into()]);
    let mut bytes = original.clone();
    bytes[4..8].copy_from_slice(&((MAX_HEADER_BYTES + 1) as u32).to_le_bytes());
    assert!(bounded_blf_reader(Cursor::new(bytes)).is_err());
    for (offset, size) in [
        (152, MAX_CONTAINER_BYTES + 1),
        (168, MAX_CONTAINER_BYTES + 1),
        (184, MAX_OBJECT_BYTES + 1),
    ] {
        let mut bytes = original.clone();
        bytes[offset..offset + 4].copy_from_slice(&(size as u32).to_le_bytes());
        assert!(
            bounded_blf_reader(Cursor::new(bytes))
                .unwrap()
                .next()
                .unwrap()
                .is_err()
        );
    }
    for length in [0, 7, 100, 150, 170, 223] {
        match bounded_blf_reader(Cursor::new(&original[..length])) {
            Err(_) => {}
            Ok(mut reader) => assert!(reader.next().unwrap().is_err()),
        }
    }
}

#[test]
fn maximum_escaped_metadata_stays_bounded() {
    let meta = Metadata {
        request_id: Some("\0".repeat(MAX_LABEL_BYTES)),
        source: Some(SourceRef {
            producer_id: "\0".repeat(MAX_LABEL_BYTES),
            producer_epoch: u64::MAX,
            source_seq: u64::MAX,
        }),
        time_quality: TimeQuality::Estimated,
    };
    let records = [
        ExportRecord::Error(ErrorFrame {
            ts_mono_ns: 1,
            wall_ns: Some(u64::MAX),
            channel_index: u16::MAX - 1,
            dir: Direction::Tx,
            origin: Origin::Script,
            description: "\0".repeat(256),
            metadata: meta.clone(),
        }),
        ExportRecord::Gap(Gap {
            ts_mono_ns: 1,
            scope: GapScope::Source,
            count: Some(u64::MAX),
            reason: "\0".repeat(256),
            metadata: meta,
        }),
    ];
    for format in [Format::Blf, Format::Asc] {
        let (_, rows, summary) = encode(format, Options::default(), &records);
        assert_eq!(rows[1]["channel_index"], 65534);
        assert_eq!(rows[1]["time_quality"], "estimated");
        assert_eq!(rows[2]["count"], u64::MAX.to_string());
        assert!(summary.max_sidecar_line <= MAX_SIDECAR_LINE);
    }
}
