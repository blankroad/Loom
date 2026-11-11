//! Cursor seams (plan §7.5 proptest 대상 "seq·cursor 이음매"): paging through a ring filled in
//! arbitrary chunks yields exactly the complete lines of a one-shot render, each once, in order,
//! and a summary read always ends where the last complete line ends.

use loom_console::cursor::{CursorStore, ReadFrom, ReadLimits, ReadMode, read};
use loom_console::ring::{Chunk, Entry, Ring, RingLimits};
use loom_console::text::{AnsiMode, TextOptions, render_lines};
use loom_proto::event::{Dir, Origin};
use proptest::prelude::*;

fn byte() -> impl Strategy<Value = u8> {
    prop_oneof![
        6 => prop::sample::select(b"abcdef 0123".to_vec()),
        2 => Just(b'\n'),
        1 => Just(b'\r'),
        1 => prop::sample::select(vec![0x1bu8, b'[', b'm', 0xc3, 0xa9, 0xff]),
    ]
}

fn ring_from(input: &[u8], cuts: &[usize]) -> Ring {
    let mut ring = Ring::new(RingLimits::default());
    let mut cuts: Vec<usize> = cuts.iter().map(|c| c % (input.len() + 1)).collect();
    cuts.sort_unstable();
    cuts.dedup();
    cuts.push(input.len());
    let mut pos = 0;
    for (i, c) in cuts.into_iter().enumerate() {
        if c <= pos {
            continue;
        }
        ring.push(Entry::Data(Chunk {
            source_seq: i as u64 + 1,
            dir: Dir::Rx,
            origin: Origin::Device,
            client_id: None,
            request_id: None,
            generation: 0,
            mono_ns: 0,
            wall_ns: 0,
            rx_offset: Some(pos as u64),
            bytes: input[pos..c].to_vec(),
        }))
        .expect("push");
        pos = c;
    }
    ring
}

proptest! {
    #![proptest_config(ProptestConfig { failure_persistence: None, ..ProptestConfig::with_cases(256) })]

    #[test]
    fn paging_reproduces_one_shot_lines(input in prop::collection::vec(byte(), 0..600),
                                        cuts in prop::collection::vec(any::<usize>(), 0..15),
                                        page in 1u32..7,
                                        ansi in prop::sample::select(vec![AnsiMode::Strip, AnsiMode::Keep, AnsiMode::Escape]),
                                        keep_cr in any::<bool>()) {
        let opts = TextOptions { ansi, keep_cr, ..TextOptions::default() };
        let want: Vec<(String, u64, u64)> = render_lines(opts, 0, &input)
            .into_iter()
            .filter(|l| l.complete)
            .map(|l| (l.text, l.rx_offset_start, l.rx_offset_end))
            .collect();
        let ring = ring_from(&input, &cuts);
        let mut store = CursorStore::new();
        let limits = ReadLimits { max_lines: page, ..ReadLimits::default() };
        let mut got = Vec::new();
        for _ in 0..input.len() + 2 {
            let r = read(&ring, &mut store, "p", ReadFrom::Last, ReadMode::Page, &limits,
                         &opts).expect("read");
            got.extend(r.lines.iter().filter(|l| l.complete)
                .map(|l| (l.text.clone(), l.rx_offset_start, l.rx_offset_end)));
            if !r.more_available {
                break;
            }
        }
        prop_assert_eq!(&got, &want);

        let mut store = CursorStore::new();
        let r = read(&ring, &mut store, "s", ReadFrom::Last, ReadMode::Summary, &limits,
                     &opts).expect("read");
        prop_assert_eq!(r.next_rx_offset, want.last().map_or(0, |l| l.2));
    }
}
