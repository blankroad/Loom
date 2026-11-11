//! Client cursors and bounded read results (plan §3.3, §3.6).

use loom_console::cursor::{
    CursorError, CursorStore, HARD_MAX_BYTES, HARD_MAX_LINES, MAX_CLIENTS, ReadFrom, ReadLimits, ReadMode,
    ReadResult, line_cost, read,
};
use loom_console::ring::{Chunk, Entry, GapLayer, GapMark, Ring, RingLimits};
use loom_console::text::TextOptions;
use loom_proto::event::{Dir, Origin};

struct Dev {
    ring: Ring,
    seq: u64,
}

impl Dev {
    fn new(limits: RingLimits) -> Self {
        Self {
            ring: Ring::new(limits),
            seq: 0,
        }
    }

    fn rx(&mut self, bytes: &[u8]) {
        self.seq += 1;
        let off = self.ring.rx_end();
        self.ring
            .push(Entry::Data(Chunk {
                source_seq: self.seq,
                dir: Dir::Rx,
                origin: Origin::Device,
                client_id: None,
                request_id: None,
                generation: 0,
                mono_ns: self.seq,
                wall_ns: self.seq,
                rx_offset: Some(off),
                bytes: bytes.to_vec(),
            }))
            .expect("push");
    }

    fn lines(&mut self, from: usize, n: usize) {
        let mut buf = Vec::new();
        for i in from..from + n {
            buf.extend_from_slice(format!("line {i}\r\n").as_bytes());
        }
        for c in buf.chunks(1000) {
            self.rx(c);
        }
    }

    fn read(
        &self,
        store: &mut CursorStore,
        client: &str,
        from: ReadFrom,
        mode: ReadMode,
        limits: ReadLimits,
    ) -> Result<ReadResult, CursorError> {
        read(
            &self.ring,
            store,
            client,
            from,
            mode,
            &limits,
            &TextOptions::default(),
        )
    }
}

fn texts(r: &ReadResult) -> Vec<&str> {
    r.lines.iter().map(|l| l.text.as_str()).collect()
}

fn lim(max_lines: u32) -> ReadLimits {
    ReadLimits {
        max_lines,
        ..ReadLimits::default()
    }
}

#[test]
fn summary_fits_and_partial_line_is_not_consumed() {
    let mut d = Dev::new(RingLimits::default());
    let mut s = CursorStore::new();
    d.rx(b"U-Boot\r\nDRAM: 512 MiB\r\nlogin: ");
    let r = d
        .read(&mut s, "a", ReadFrom::Last, ReadMode::Summary, lim(100))
        .expect("read");
    assert_eq!(texts(&r), ["U-Boot", "DRAM: 512 MiB", "login: "]);
    assert!(!r.lines[2].complete);
    assert!(!r.truncated && !r.more_available && r.omitted.is_none());
    assert_eq!(r.next_rx_offset, 23);
    assert_eq!(s.get("a").map(|p| (p.rx_offset, p.revision)), Some((23, 1)));

    // The prompt is shown again, now completed by the echo.
    d.rx(b"root\r\n");
    let r = d
        .read(&mut s, "a", ReadFrom::Last, ReadMode::Summary, lim(100))
        .expect("read");
    assert_eq!(texts(&r), ["login: root"]);
    assert!(r.lines[0].complete);
    assert_eq!(s.get("a").map(|p| p.revision), Some(2));
}

#[test]
fn summary_over_limit_gives_head_tail_and_moves_to_end() {
    let mut d = Dev::new(RingLimits::default());
    let mut s = CursorStore::new();
    d.lines(0, 500);
    let r = d
        .read(&mut s, "a", ReadFrom::Last, ReadMode::Summary, lim(100))
        .expect("read");
    assert_eq!(r.lines.len(), 100);
    assert_eq!(r.lines[0].text, "line 0");
    assert_eq!(r.lines[19].text, "line 19");
    assert_eq!(r.lines[20].text, "line 420");
    assert_eq!(r.lines[99].text, "line 499");
    let o = r.omitted.expect("omitted");
    assert_eq!((o.lines, o.at_line), (400, 20));
    assert_eq!(o.from_rx_offset, r.lines[19].rx_offset_end);
    assert_eq!(o.to_rx_offset, r.lines[20].rx_offset_start);
    assert_eq!(o.bytes, o.to_rx_offset - o.from_rx_offset);
    assert!(o.from_seq <= o.to_seq);
    assert!(r.truncated && !r.more_available);
    assert_eq!(r.next_rx_offset, d.ring.rx_end());

    // Cursor is at the end: the next summary is empty.
    let r = d
        .read(&mut s, "a", ReadFrom::Last, ReadMode::Summary, lim(100))
        .expect("read");
    assert!(r.lines.is_empty());
}

#[test]
fn page_returns_up_to_limit_and_moves_only_that_far() {
    let mut d = Dev::new(RingLimits::default());
    let mut s = CursorStore::new();
    d.lines(0, 250);
    let mut seen = Vec::new();
    for round in 0..3 {
        let r = d
            .read(&mut s, "p", ReadFrom::Last, ReadMode::Page, lim(100))
            .expect("read");
        assert_eq!(r.more_available, round < 2, "round {round}");
        assert_eq!(r.truncated, round < 2);
        assert!(r.omitted.is_none());
        assert_eq!(r.next_rx_offset, r.lines.last().expect("lines").rx_offset_end);
        seen.extend(r.lines.iter().map(|l| l.text.clone()));
    }
    let want: Vec<String> = (0..250).map(|i| format!("line {i}")).collect();
    assert_eq!(seen, want);
}

#[test]
fn peek_does_not_move() {
    let mut d = Dev::new(RingLimits::default());
    let mut s = CursorStore::new();
    d.lines(0, 10);
    d.read(&mut s, "k", ReadFrom::Last, ReadMode::Page, lim(3))
        .expect("read");
    let before = s.get("k").expect("cursor");
    let r = d
        .read(&mut s, "k", ReadFrom::Last, ReadMode::Peek, lim(3))
        .expect("peek");
    assert_eq!(texts(&r), ["line 3", "line 4", "line 5"]);
    assert!(r.more_available);
    assert_eq!(s.get("k"), Some(before));
    assert_eq!(r.revision, before.revision);
    // Peek's token continues statelessly after the peeked lines.
    let r2 = d.read(
        &mut s,
        "other",
        ReadFrom::Token(r.next_cursor.clone()),
        ReadMode::Peek,
        lim(3),
    );
    // Token revision belongs to "k"; peeking never commits, so no conflict.
    assert_eq!(texts(&r2.expect("peek by token")), ["line 6", "line 7", "line 8"]);
}

#[test]
fn page_token_replay_returns_same_range() {
    let mut d = Dev::new(RingLimits::default());
    let mut s = CursorStore::new();
    d.lines(0, 30);
    let r1 = d
        .read(&mut s, "t", ReadFrom::Last, ReadMode::Page, lim(10))
        .expect("r1");
    let t1 = r1.next_cursor.clone();
    let r2 = d
        .read(&mut s, "t", ReadFrom::Token(t1.clone()), ReadMode::Page, lim(10))
        .expect("r2");
    assert_eq!(r2.lines[0].text, "line 10");
    // The r2 response is "lost": the client re-sends the same starting token.
    let r2b = d
        .read(&mut s, "t", ReadFrom::Token(t1), ReadMode::Page, lim(10))
        .expect("replay");
    assert_eq!(r2b.lines, r2.lines);
    assert_eq!(r2b.revision, r2.revision);
    let r3 = d
        .read(&mut s, "t", ReadFrom::Last, ReadMode::Page, lim(10))
        .expect("r3");
    assert_eq!(r3.lines[0].text, "line 20");
}

#[test]
fn same_name_concurrent_update_is_a_revision_conflict() {
    let mut d = Dev::new(RingLimits::default());
    let mut s = CursorStore::new();
    d.lines(0, 30);
    let r = d
        .read(&mut s, "dup", ReadFrom::Last, ReadMode::Page, lim(5))
        .expect("first");
    let token = r.next_cursor; // process A keeps this
    // Process B (same name) moves the cursor.
    d.read(&mut s, "dup", ReadFrom::Last, ReadMode::Page, lim(5))
        .expect("B");
    // Process A continues from its token with a different page size → conflict, no move.
    let before = s.get("dup");
    let err = d
        .read(&mut s, "dup", ReadFrom::Token(token), ReadMode::Page, lim(7))
        .expect_err("conflict");
    assert_eq!(
        err,
        CursorError::RevisionConflict {
            expected: 1,
            current: 2
        }
    );
    assert_eq!(s.get("dup"), before);

    // Store-level API.
    let cur = s.get("dup").expect("cursor");
    assert!(s.commit("dup", Some(cur.revision), cur.rx_offset + 1, 0).is_ok());
    assert!(matches!(
        s.commit("dup", Some(cur.revision), cur.rx_offset + 2, 0),
        Err(CursorError::RevisionConflict { .. })
    ));
}

#[test]
fn tokens_reject_garbage_and_tampering() {
    let s = CursorStore::with_token_key(1, 2);
    let pos = loom_console::cursor::CursorPos {
        rx_offset: 1234,
        generation: 3,
        revision: 9,
    };
    let t = s.encode_token(pos);
    assert_eq!(s.decode_token(&t), Ok(pos));
    assert_eq!(s.decode_token("nope"), Err(CursorError::BadToken));
    assert_eq!(s.decode_token(""), Err(CursorError::BadToken));
    // Flip one hex digit of the offset.
    let mut bad = t.clone().into_bytes();
    bad[5] = if bad[5] == b'0' { b'1' } else { b'0' };
    let bad = String::from_utf8(bad).expect("ascii");
    assert_eq!(s.decode_token(&bad), Err(CursorError::BadToken));
    // Truncated / extended.
    assert_eq!(s.decode_token(&t[..t.len() - 1]), Err(CursorError::BadToken));
    assert_eq!(s.decode_token(&format!("{t}0")), Err(CursorError::BadToken));
    // Another store (other key) does not accept it.
    let other = CursorStore::with_token_key(3, 4);
    assert_eq!(other.decode_token(&t), Err(CursorError::BadToken));
}

#[test]
fn cursor_outside_ring_reports_gap_and_reads_rest() {
    let mut d = Dev::new(RingLimits {
        max_payload: 2000,
        max_entries: 1000,
        max_meta: 1 << 20,
    });
    let mut s = CursorStore::new();
    d.lines(0, 5);
    d.read(&mut s, "old", ReadFrom::Last, ReadMode::Summary, lim(100))
        .expect("read");
    let at = s.get("old").expect("cursor").rx_offset;
    d.lines(5, 400); // pushes the cursor position out of the ring
    let retained = d.ring.retained_from_rx_offset();
    assert!(retained > at);
    let r = d
        .read(&mut s, "old", ReadFrom::Last, ReadMode::Page, lim(5))
        .expect("read");
    let g = r.cursor_gap.expect("gap");
    assert_eq!((g.lost_from, g.lost_to, g.exact), (at, retained, true));
    assert_eq!(r.from_rx_offset, retained);
    assert_eq!(r.lines.len(), 5);

    // A source_seq that was evicted: inexact gap.
    let r = d
        .read(&mut s, "x", ReadFrom::SourceSeq(1), ReadMode::Peek, lim(5))
        .expect("read");
    assert_eq!(r.cursor_gap.map(|g| g.exact), Some(false));
}

#[test]
fn hard_limits_cap_output() {
    let mut d = Dev::new(RingLimits::default());
    let mut s = CursorStore::new();
    // 3000 lines of 300 chars: way above every hard limit.
    let line = format!("{}\n", "w".repeat(300));
    let blob = line.repeat(3000);
    for c in blob.as_bytes().chunks(16 * 1024) {
        d.rx(c);
    }
    let huge = ReadLimits {
        max_lines: 1_000_000,
        max_bytes: 100 << 20,
        hard_lines: 1_000_000,
        hard_bytes: 100 << 20,
        line_chars: 10_000,
    };
    let e = huge.effective();
    assert_eq!(
        (e.max_lines, e.max_bytes, e.line_chars),
        (HARD_MAX_LINES, HARD_MAX_BYTES, 512)
    );
    for mode in [ReadMode::Summary, ReadMode::Page, ReadMode::Peek] {
        let r = d
            .read(&mut s, "h", ReadFrom::RxOffset(0), mode, huge)
            .expect("read");
        let bytes: u32 = r.lines.iter().map(line_cost).sum();
        assert!(r.lines.len() as u32 <= HARD_MAX_LINES);
        assert!(bytes <= HARD_MAX_BYTES, "{mode:?}: {bytes}");
        assert!(r.truncated);
    }
    // Long lines are capped at 512 display chars.
    d.rx(format!("{}\n", "L".repeat(2000)).as_bytes());
    let r = d
        .read(&mut s, "h", ReadFrom::Last, ReadMode::Summary, huge)
        .expect("read");
    let last = r.lines.last().expect("line");
    assert_eq!(last.elided_chars, 2000 - 512);
}

#[test]
fn lru_evicts_least_recently_used_client() {
    let mut d = Dev::new(RingLimits::default());
    let mut s = CursorStore::new();
    d.lines(0, 1);
    for i in 0..MAX_CLIENTS {
        d.read(
            &mut s,
            &format!("c{i}"),
            ReadFrom::Last,
            ReadMode::Summary,
            lim(10),
        )
        .expect("read");
    }
    // Touch c0 so c1 becomes the oldest.
    d.lines(1, 1);
    d.read(&mut s, "c0", ReadFrom::Last, ReadMode::Summary, lim(10))
        .expect("read");
    let r = d
        .read(&mut s, "new", ReadFrom::Last, ReadMode::Summary, lim(10))
        .expect("read");
    assert_eq!(r.evicted_client.as_deref(), Some("c1"));
    assert_eq!(s.len(), MAX_CLIENTS);
    assert!(s.get("c0").is_some() && s.get("c1").is_none());
}

#[test]
fn gap_mark_cuts_the_line_and_names_are_validated() {
    let mut d = Dev::new(RingLimits::default());
    let mut s = CursorStore::new();
    d.rx(b"abc");
    d.ring
        .push(Entry::Gap(GapMark {
            layer: GapLayer::Record,
            cause: "reader_bridge_overflow".into(),
            rx_offset_from: Some(3),
            bytes: Some(100),
            source_seq_range: None,
            generation: 0,
            mono_ns: 0,
        }))
        .expect("gap");
    d.seq += 1;
    let off = 103;
    d.ring
        .push(Entry::Data(Chunk {
            source_seq: d.seq + 1,
            dir: Dir::Rx,
            origin: Origin::Device,
            client_id: None,
            request_id: None,
            generation: 0,
            mono_ns: 0,
            wall_ns: 0,
            rx_offset: Some(off),
            bytes: b"def\n".to_vec(),
        }))
        .expect("push");
    let r = d
        .read(&mut s, "g", ReadFrom::Last, ReadMode::Summary, lim(10))
        .expect("read");
    assert_eq!(texts(&r), ["abc", "def"]);
    assert!(!r.lines[0].complete && r.lines[1].complete);
    assert_eq!(r.lines[1].rx_offset_start, 103);

    assert_eq!(
        d.read(&mut s, "", ReadFrom::Last, ReadMode::Peek, lim(1))
            .map(|_| ()),
        Err(CursorError::EmptyClientName)
    );
    assert!(matches!(
        d.read(&mut s, &"n".repeat(129), ReadFrom::Last, ReadMode::Peek, lim(1)),
        Err(CursorError::ClientNameTooLong(129))
    ));
}

#[test]
fn binary_hint_is_reported() {
    let mut d = Dev::new(RingLimits::default());
    let mut s = CursorStore::new();
    let noise: Vec<u8> = (0..2048u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 11) as u8 | 0x80)
        .collect();
    d.rx(&noise);
    let r = d
        .read(&mut s, "b", ReadFrom::Last, ReadMode::Peek, lim(10))
        .expect("read");
    assert!(r.binary_hint);
}
