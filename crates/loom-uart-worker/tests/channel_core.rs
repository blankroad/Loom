//! `ChannelCore` behaviour, driven by the sim devices' `Device` API (no transport): expect /
//! `send --expect` / run / read / lines, chunk boundaries, generations, losses, budgets.

use std::time::Duration;

use base64::Engine as _;
use loom_proto::error::ErrorCode;
use loom_uart_worker::bridge::{LostRange, RxChunk};
use loom_uart_worker::channel::{
    ChannelCore, ChannelError, EXPECT_BACKFILL_SCAN, Eol, ExpectFrom, Ingest, LinesQuery, MAX_READ_SCAN,
    MatchError, MatchHit, PatternKind, PatternSpec, Profile, ReadFrom, ReadMode, ReadQuery,
};
use loom_uart_worker::sim::{Binary, Bootshell, Device, LOGIN_PROMPT, SHELL_PROMPT};
use loom_uart_worker::tx::{TxChunk, TxLane, TxOrigin};
use serde_json::{Value, json};

/// A device wired straight into a `ChannelCore`.
struct Rig {
    dev: Box<dyn Device>,
    core: ChannelCore,
    now: Duration,
    seq: u64,
    off: u64,
    /// RX pieces cycle through these sizes (chunk-boundary coverage).
    split: Vec<usize>,
    split_i: usize,
    request: u64,
}

impl Rig {
    fn new(dev: Box<dyn Device>, profile: &str, split: &[usize]) -> Rig {
        let mut r = Rig {
            dev,
            core: ChannelCore::new("dut", Profile::named(profile).expect("built-in profile"), 1),
            now: Duration::ZERO,
            seq: 0,
            off: 0,
            split: split.to_vec(),
            split_i: 0,
            request: 0,
        };
        let mut out = Vec::new();
        r.dev.on_open(&mut out);
        r.deliver(&out);
        r
    }

    fn bootshell(split: &[usize]) -> Rig {
        Rig::new(Box::new(Bootshell::new(7)), "linux-shell", split)
    }

    /// Feed device output as RX chunks; returns every ingest.
    fn deliver(&mut self, bytes: &[u8]) -> Vec<Ingest> {
        let mut ings = Vec::new();
        let mut rest = bytes;
        while !rest.is_empty() {
            let n = self.split[self.split_i % self.split.len()].min(rest.len()).max(1);
            self.split_i += 1;
            ings.push(self.rx(&rest[..n]));
            rest = &rest[n..];
        }
        ings
    }

    fn rx(&mut self, bytes: &[u8]) -> Ingest {
        self.seq += 1;
        let c = RxChunk {
            source_seq: self.seq,
            rx_offset: self.off,
            mono_ns: self.seq * 1_000,
            wall_ns: 1_700_000_000_000_000_000 + self.seq * 1_000,
            bytes: bytes.to_vec(),
        };
        self.off += bytes.len() as u64;
        self.core.on_rx(&c)
    }

    /// Run device ticks until it idles (boot log).
    fn settle(&mut self) -> Vec<Ingest> {
        let mut out = Vec::new();
        for _ in 0..10_000 {
            match self.dev.tick(self.now, &mut out) {
                Some(d) => self.now += d,
                None => break,
            }
        }
        self.deliver(&out)
    }

    /// Agent TX: ring + device, then the device's answer as RX.
    fn send(&mut self, bytes: &[u8]) -> Vec<Ingest> {
        self.seq += 1;
        self.request += 1;
        let payload = self.core.on_tx(&TxChunk {
            source_seq: self.seq,
            lane: TxLane::Agent,
            origin: TxOrigin::Agent,
            client_id: "claude-7f3a".into(),
            request_id: self.request,
            mono_ns: self.seq * 1_000,
            bytes: bytes.to_vec(),
        });
        assert!(payload.get("rx_offset").is_none(), "TX payload has no rx_offset");
        let mut out = Vec::new();
        self.dev.on_bytes(bytes, self.now, &mut out);
        self.deliver(&out)
    }

    fn boot_to_login(split: &[usize]) -> Rig {
        let mut r = Rig::bootshell(split);
        r.settle();
        r
    }

    fn logged_in(split: &[usize]) -> Rig {
        let mut r = Rig::boot_to_login(split);
        r.send(b"root\r");
        r.send(b"secret\r");
        r
    }
}

fn hits(ings: &[Ingest]) -> Vec<MatchHit> {
    ings.iter().flat_map(|i| i.hits.clone()).collect()
}

fn n(v: &Value) -> u64 {
    v.as_str()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("decimal string expected, got {v}"))
}

fn texts(v: &Value) -> Vec<String> {
    v["lines"]
        .as_array()
        .expect("lines array")
        .iter()
        .map(|l| l["text"].as_str().expect("text").to_owned())
        .collect()
}

fn read(core: &mut ChannelCore, client: &str, q: ReadQuery) -> Value {
    core.read(client, &q).expect("read")
}

// ---------------------------------------------------------------------------------------------
// bootshell flows
// ---------------------------------------------------------------------------------------------

#[test]
fn login_prompt_expect_backfills_and_moves_the_cursor() {
    let mut r = Rig::boot_to_login(&[512, 37, 4096]);
    let end = r.core.rx_end();
    // `expect 'login:'` after the prompt was already printed: found in the ring at once.
    let started = r.core.rx_end();
    let hs = r
        .core
        .expect_add(
            1,
            &[PatternSpec::literal("login: ")],
            ExpectFrom::Cursor("agent".into()),
        )
        .expect("expect");
    assert_eq!(hs.len(), 1);
    let h = &hs[0];
    assert_eq!(h.end_off, end);
    assert_eq!(h.end_off - h.start_off, LOGIN_PROMPT.len() as u64);
    let res = r.core.expect_result(h, started);
    assert_eq!(res["matched"], true);
    assert_eq!(res["pattern_index"], 0);
    assert_eq!(res["pattern"], "login: ");
    assert_eq!(res["text"], "login: ");
    assert_eq!(n(&res["rx_offset_end"]), end);
    assert_eq!(res["generation"], 1);
    let before = res["before"].as_str().expect("before");
    assert!(before.ends_with("login: "), "{before:?}");
    assert!(before.contains("Loom sim 1.0 sim ttyS0"), "{before:?}");
    assert!(res["before_lines"].as_u64().expect("count") <= 15);
    // Success moved the cursor to the match end: nothing new to read.
    let after = read(&mut r.core, "agent", ReadQuery::default());
    assert_eq!(texts(&after), Vec::<String>::new());
    assert_eq!(n(&after["range"]["from_rx"]), end);
    // The waiter is gone; removing it again is harmless.
    r.core.expect_remove(1);
    assert_eq!(r.core.status()["expect"]["pending"], 0);
}

#[test]
fn send_expect_from_now_ignores_old_output_and_sees_the_reply() {
    let mut r = Rig::boot_to_login(&[300]);
    // `send --expect 'login: '` registered now must not match the prompt already printed.
    assert!(
        r.core
            .expect_add(1, &[PatternSpec::literal("login: ")], ExpectFrom::Now)
            .expect("add")
            .is_empty()
    );
    r.core.expect_remove(1);

    let started = r.core.rx_end();
    assert!(
        r.core
            .expect_add(2, &[PatternSpec::literal("Password: ")], ExpectFrom::Now)
            .expect("add")
            .is_empty()
    );
    let ings = r.send(b"root\r");
    let hs = hits(&ings);
    assert_eq!(hs.len(), 1, "{hs:?}");
    assert_eq!(hs[0].id, 2);
    assert!(hs[0].start_off >= started);
    let res = r.core.expect_result(&hs[0], started);
    assert_eq!(res["text"], "Password: ");
    assert_eq!(n(&res["range"]["from_rx"]), started);

    // Shell prompt via the profile prompt (`--expect-prompt`); the arbiter hint fires too.
    assert!(
        r.core
            .expect_add(3, &[PatternSpec::prompt()], ExpectFrom::Now)
            .expect("add")
            .is_empty()
    );
    let ings = r.send(b"hunter2\r");
    let hs = hits(&ings);
    assert_eq!(hs.len(), 1);
    assert_eq!(hs[0].text, "# ");
    assert!(
        ings.last().expect("rx").prompt_seen,
        "prompt at the end of the RX stream"
    );
    assert_eq!(r.core.expect_result(&hs[0], started)["kind"], json!("prompt"));
    // The password itself never reached RX (no echo); TX is not RX text.
    let all = read(
        &mut r.core,
        "audit",
        ReadQuery {
            from: ReadFrom::RxOffset(0),
            mode: ReadMode::Peek,
            max_lines: Some(1000),
            max_bytes: Some(65536),
            ..ReadQuery::default()
        },
    );
    assert!(!texts(&all).iter().any(|t| t.contains("hunter2")));
}

#[test]
fn prompt_seen_only_when_the_prompt_is_last() {
    let mut r = Rig::logged_in(&[4096]);
    let ings = r.send(b"status\r");
    assert!(ings.last().expect("rx").prompt_seen);
    // Typing (echo) after the prompt: no longer at the end.
    let ings = r.send(b"ec");
    assert!(ings.iter().all(|i| !i.prompt_seen));
    let ings = r.send(b"ho hi\r");
    assert!(ings.last().expect("rx").prompt_seen);
    // Boot log lines are not prompts (`login: ` does not match `[#$] $`).
    let ings = r.send(b"reboot\r");
    assert!(ings.iter().all(|i| !i.prompt_seen));
    let ings = r.settle();
    assert!(ings.iter().all(|i| !i.prompt_seen));
}

#[test]
fn run_output_strips_echo_and_prompt() {
    for split in [vec![4096], vec![1], vec![3, 5, 64]] {
        let mut r = Rig::logged_in(&split);
        let started = r.core.rx_end();
        r.core
            .expect_add(10, &[PatternSpec::prompt()], ExpectFrom::Now)
            .expect("add");
        let cmd = b"uname -a\r";
        let hs = hits(&r.send(cmd));
        assert_eq!(hs.len(), 1, "split {split:?}");
        let out = r.core.run_output(cmd, &hs[0], started);
        assert_eq!(
            out["output"],
            "Linux sim 6.6.0-loom #1 SMP PREEMPT Thu Jan  1 00:00:00 UTC 1970 aarch64 GNU/Linux",
            "split {split:?}"
        );
        assert_eq!(out["echo_stripped"], true);
        assert_eq!(out["prompt"], SHELL_PROMPT);
        assert_eq!(out["matched"], true);
        assert_eq!(out["truncated"], false);

        let started = r.core.rx_end();
        r.core
            .expect_add(11, &[PatternSpec::prompt()], ExpectFrom::Now)
            .expect("add");
        let cmd = b"cat /proc/meminfo\r";
        let hs = hits(&r.send(cmd));
        let out = r.core.run_output(cmd, &hs[0], started);
        let text = out["output"].as_str().expect("output");
        assert_eq!(text.lines().count(), 10, "{text:?}");
        assert!(text.starts_with("MemTotal:"), "{text:?}");
        assert_eq!(out["lines"], 10);
    }
}

#[test]
fn read_summary_page_peek_and_tokens() {
    let mut r = Rig::boot_to_login(&[777]);
    // Summary: head + tail + omitted, cursor to the last complete line; `login: ` stays.
    let s = read(&mut r.core, "agent", ReadQuery::default());
    let lines = texts(&s);
    assert_eq!(lines.len(), 100);
    assert_eq!(s["truncated"], true);
    assert!(s["omitted"]["lines"].as_u64().expect("omitted") > 100);
    assert_eq!(lines[0], "", "on_open starts with CRLF");
    assert!(lines[1].contains("U-Boot SPL"), "{:?}", lines[1]);
    assert_eq!(lines.last().expect("last"), "login: ");
    assert_eq!(s["lines"][99]["complete"], false);
    assert!(
        lines
            .iter()
            .any(|l| l == "[FAILED] Failed to start Login Service."),
        "ANSI stripped"
    );
    let again = read(&mut r.core, "agent", ReadQuery::default());
    assert_eq!(
        texts(&again),
        vec!["login: ".to_owned()],
        "partial line is shown again, not consumed"
    );

    // Page through everything: no gaps, no duplicates, same lines as one big peek.
    let mut paged = Vec::new();
    let mut last_end = None;
    for _ in 0..20 {
        let p = read(
            &mut r.core,
            "pager",
            ReadQuery {
                mode: ReadMode::Page,
                max_lines: Some(40),
                ..ReadQuery::default()
            },
        );
        for l in p["lines"].as_array().expect("lines") {
            if l["complete"] == true {
                if let Some(e) = last_end {
                    assert_eq!(n(&l["rx_offset_start"]), e, "contiguous pages");
                }
                last_end = Some(n(&l["rx_offset_end"]));
                paged.push(l["text"].as_str().expect("text").to_owned());
            }
        }
        if p["more_available"] == false {
            break;
        }
    }
    let all = read(
        &mut r.core,
        "x",
        ReadQuery {
            from: ReadFrom::RxOffset(0),
            mode: ReadMode::Peek,
            max_lines: Some(1000),
            max_bytes: Some(65536),
            ..ReadQuery::default()
        },
    );
    let complete: Vec<String> = all["lines"]
        .as_array()
        .expect("lines")
        .iter()
        .filter(|l| l["complete"] == true)
        .map(|l| l["text"].as_str().expect("text").to_owned())
        .collect();
    assert_eq!(paged, complete);
    assert!(
        complete
            .iter()
            .any(|l| l == "fsck /dev/mmcblk0p2: [##########] 100%"),
        "progress bar collapses"
    );

    // Peek never moves; a token re-reads the same range (lost `--page` response).
    let p1 = read(
        &mut r.core,
        "peeker",
        ReadQuery {
            mode: ReadMode::Peek,
            max_lines: Some(5),
            ..ReadQuery::default()
        },
    );
    let p2 = read(
        &mut r.core,
        "peeker",
        ReadQuery {
            mode: ReadMode::Peek,
            max_lines: Some(5),
            ..ReadQuery::default()
        },
    );
    assert_eq!(p1["lines"], p2["lines"]);
    let start = read(
        &mut r.core,
        "tok",
        ReadQuery {
            mode: ReadMode::Peek,
            max_lines: Some(3),
            ..ReadQuery::default()
        },
    );
    let tok = start["next_cursor"].as_str().expect("token").to_owned();
    let a = read(
        &mut r.core,
        "tok",
        ReadQuery {
            from: ReadFrom::Token(tok.clone()),
            mode: ReadMode::Page,
            max_lines: Some(3),
            ..ReadQuery::default()
        },
    );
    let b = read(
        &mut r.core,
        "tok",
        ReadQuery {
            from: ReadFrom::Token(tok),
            mode: ReadMode::Page,
            max_lines: Some(3),
            ..ReadQuery::default()
        },
    );
    assert_eq!(a["lines"], b["lines"]);
    // A peek's `next_cursor` points past the peeked lines: the token page continues there.
    let p6 = read(
        &mut r.core,
        "peeker",
        ReadQuery {
            mode: ReadMode::Peek,
            max_lines: Some(6),
            ..ReadQuery::default()
        },
    );
    assert_eq!(texts(&a), texts(&p6)[3..6].to_vec());
    // Bad token → usage error.
    let e = r.core.read(
        "tok",
        &ReadQuery {
            from: ReadFrom::Token("c1-zz".into()),
            ..ReadQuery::default()
        },
    );
    assert!(matches!(e, Err(ChannelError::Usage(_))), "{e:?}");
}

#[test]
fn read_until_and_source_seq_starts() {
    let mut r = Rig::logged_in(&[64]);
    r.send(b"status\r");
    let all = read(
        &mut r.core,
        "x",
        ReadQuery {
            from: ReadFrom::RxOffset(0),
            mode: ReadMode::Peek,
            max_lines: Some(1000),
            max_bytes: Some(65536),
            ..ReadQuery::default()
        },
    );
    let line = all["lines"]
        .as_array()
        .expect("lines")
        .iter()
        .find(|l| l["text"] == "temp=41.5 rpm=1200")
        .expect("status line")
        .clone();
    let (s, e) = (n(&line["rx_offset_start"]), n(&line["rx_offset_end"]));
    // `until` stops right after the status line.
    let q = ReadQuery {
        from: ReadFrom::RxOffset(s),
        until: Some(e),
        mode: ReadMode::Page,
        ..ReadQuery::default()
    };
    let v = read(&mut r.core, "until", q);
    assert_eq!(texts(&v), vec!["temp=41.5 rpm=1200".to_owned()]);
    assert_eq!(n(&v["range"]["to_rx"]), e);
    // `src:N` = the chunk that holds the first byte of that line.
    let q = ReadQuery {
        from: ReadFrom::SourceSeq(n(&line["source_seq"])),
        mode: ReadMode::Peek,
        max_lines: Some(1),
        ..ReadQuery::default()
    };
    let v = read(&mut r.core, "seq", q);
    assert!(n(&v["range"]["from_rx"]) <= s);
}

#[test]
fn lines_history_rows_with_tx_and_no_cursor_change() {
    let mut r = Rig::boot_to_login(&[256]);
    r.send(b"root\r");
    let cursors = r.core.status()["cursors"].clone();
    // Newest rows: … `login: root` (echo completed the line), the TX row, `Password: `.
    let v = r
        .core
        .lines(&LinesQuery {
            limit: 4,
            ..LinesQuery::default()
        })
        .expect("lines");
    let rows = v["rows"].as_array().expect("rows");
    assert_eq!(rows.len(), 4);
    assert_eq!(v["more_before"], true);
    let tx = rows.iter().find(|r| r["dir"] == "tx").expect("tx row");
    assert_eq!(tx["text"], "root\\r");
    assert_eq!(tx["origin"], "agent");
    assert_eq!(tx["client_id"], "claude-7f3a");
    let last = rows.last().expect("row");
    assert_eq!(last["dir"], "rx");
    assert_eq!(last["text"], "Password: ");
    assert_eq!(last["complete"], false);
    assert!(rows.iter().any(|r| r["text"] == "login: root"));
    // Forward pages chain with `next_from`, RX only.
    let p1 = r
        .core
        .lines(&LinesQuery {
            from: Some(0),
            limit: 10,
            dir: Some(loom_proto::event::Dir::Rx),
            ..LinesQuery::default()
        })
        .expect("lines");
    assert_eq!(p1["rows"].as_array().expect("rows").len(), 10);
    assert_eq!(p1["more_after"], true);
    let next = n(&p1["next_from"]);
    let p2 = r
        .core
        .lines(&LinesQuery {
            from: Some(next),
            limit: 1,
            dir: Some(loom_proto::event::Dir::Rx),
            ..LinesQuery::default()
        })
        .expect("lines");
    assert_eq!(n(&p2["rows"][0]["rx_offset_start"]), next);
    assert!(
        p2["rows"]
            .as_array()
            .expect("rows")
            .iter()
            .all(|r| r["dir"] == "rx")
    );
    // TX only.
    let t = r
        .core
        .lines(&LinesQuery {
            dir: Some(loom_proto::event::Dir::Tx),
            ..LinesQuery::default()
        })
        .expect("lines");
    assert_eq!(t["rows"].as_array().expect("rows").len(), 1);
    assert_eq!(
        r.core.status()["cursors"],
        cursors,
        "lines never creates or moves cursors"
    );
}

#[test]
fn expect_timeout_keeps_cursor_and_gives_resume_and_context() {
    let mut r = Rig::logged_in(&[4096]);
    let cur = read(&mut r.core, "agent", ReadQuery::default());
    assert_eq!(texts(&cur).last().map(String::as_str), Some(SHELL_PROMPT));
    let before = n(&read(
        &mut r.core,
        "agent",
        ReadQuery {
            mode: ReadMode::Peek,
            ..ReadQuery::default()
        },
    )["range"]["from_rx"]);
    let started = r.core.rx_end();
    assert!(
        r.core
            .expect_add(
                9,
                &[PatternSpec::literal("never!")],
                ExpectFrom::Cursor("agent".into())
            )
            .expect("add")
            .is_empty()
    );
    r.send(b"status\r");
    // Worker order: remove first, then build the timeout data.
    r.core.expect_remove(9);
    let t = r.core.expect_timeout(9, started);
    assert_eq!(t["matched"], false);
    assert_eq!(t["cause"], "timeout");
    assert_eq!(
        n(&t["resume_from"]),
        r.core.rx_end() - 5,
        "literal: scan end − (len − 1)"
    );
    let ctx = t["context"].as_str().expect("context");
    assert!(ctx.contains("temp=41.5 rpm=1200"), "{ctx:?}");
    assert!(ctx.ends_with(SHELL_PROMPT), "{ctx:?}");
    assert_eq!(t["patterns"][0]["pattern"], "never!");
    let after = read(
        &mut r.core,
        "agent",
        ReadQuery {
            mode: ReadMode::Peek,
            ..ReadQuery::default()
        },
    );
    assert_eq!(n(&after["range"]["from_rx"]), before, "timeout leaves the cursor");
    // Regex resume: window start.
    let now = r.core.rx_end();
    r.core
        .expect_add(12, &[PatternSpec::regex("nev[e]r")], ExpectFrom::Now)
        .expect("add");
    let t = r.core.expect_timeout(12, now);
    assert_eq!(n(&t["resume_from"]), now, "never before the waiter's start");
    r.send(b"uname -a\r");
    let t = r.core.expect_timeout(12, now);
    assert_eq!(
        n(&t["resume_from"]),
        now,
        "regex: window start, clamped to the start"
    );
}

#[test]
fn ingest_payloads_and_alt_screen() {
    let mut r = Rig::bootshell(&[4096]);
    let ing = r.rx(b"\x1b[?1049hfull screen");
    let p = &ing.data_payload;
    let data = base64::engine::general_purpose::STANDARD
        .decode(p["bytes"]["data"].as_str().expect("data"))
        .expect("base64");
    assert_eq!(data, b"\x1b[?1049hfull screen");
    assert_eq!(p["bytes"]["encoding"], "base64");
    assert_eq!(p["len"], data.len());
    assert!(n(&p["rx_offset"]) > 0);
    assert_eq!(ing.alt_screen, Some(true));
    assert_eq!(r.core.status()["alt_screen"], true);
    assert_eq!(r.rx(b"more").alt_screen, None);
    assert_eq!(r.rx(b"\x1b[?10").alt_screen, None);
    assert_eq!(r.rx(b"49l").alt_screen, Some(false));
}

#[test]
fn binary_hint_from_sim_binary() {
    let mut r = Rig::new(Box::new(Binary::new(40)), "generic", &[1024]);
    r.settle();
    let st = r.core.status();
    assert_eq!(st["binary_hint"], true);
    let v = read(
        &mut r.core,
        "agent",
        ReadQuery {
            mode: ReadMode::Peek,
            ..ReadQuery::default()
        },
    );
    assert_eq!(v["binary_hint"], true);
    let t = Rig::logged_in(&[4096]);
    assert_eq!(t.core.status()["binary_hint"], false);
}

// ---------------------------------------------------------------------------------------------
// boundaries: chunks, generations, losses
// ---------------------------------------------------------------------------------------------

#[test]
fn no_match_across_a_generation_change() {
    let mut r = Rig::bootshell(&[4096]);
    let start = r.core.rx_end();
    r.core
        .expect_add(1, &[PatternSpec::literal("login: ")], ExpectFrom::Now)
        .expect("add");
    r.core
        .expect_add(2, &[PatternSpec::regex("log.n: ")], ExpectFrom::Now)
        .expect("add");
    assert!(r.rx(b"old session logi").hits.is_empty());
    r.core.new_generation(2);
    assert_eq!(r.core.generation(), 2);
    assert!(r.rx(b"n: ").hits.is_empty(), "no match spanning generations");
    // A backfill from before the change does not join them either.
    let e = r
        .core
        .expect_add(3, &[PatternSpec::literal("login: ")], ExpectFrom::RxOffset(start))
        .expect("add");
    assert!(e.is_empty());
    // A real prompt in the new generation fires all three waiters.
    let hs = r.rx(b"\r\ndut login: ").hits;
    assert_eq!(hs.len(), 3, "{hs:?}");
    let res = r.core.expect_result(&hs[0], start);
    assert_eq!(res["generation"], 2);
    // Ring kept both generations.
    let all = read(
        &mut r.core,
        "x",
        ReadQuery {
            from: ReadFrom::RxOffset(start),
            mode: ReadMode::Peek,
            ..ReadQuery::default()
        },
    );
    let gens: Vec<u64> = all["lines"]
        .as_array()
        .expect("lines")
        .iter()
        .map(|l| l["generation"].as_u64().expect("gen"))
        .collect();
    assert_eq!(gens, vec![1, 2, 2]);
}

#[test]
fn literal_split_into_single_bytes_is_found() {
    let mut r = Rig::logged_in(&[1]);
    r.core
        .expect_add(1, &[PatternSpec::literal("rpm=1200")], ExpectFrom::Now)
        .expect("add");
    r.core
        .expect_add(2, &[PatternSpec::regex(r"temp=[0-9.]+")], ExpectFrom::Now)
        .expect("add");
    let hs = hits(&r.send(b"status\r"));
    let mut ids: Vec<u64> = hs.iter().map(|h| h.id).collect();
    ids.sort_unstable();
    assert_eq!(ids, vec![1, 2]);
    let regex = hs.iter().find(|h| h.id == 2).expect("regex hit");
    assert_eq!(
        regex.text, "temp=4",
        "regex fires as soon as a match exists (no lookahead)"
    );
}

#[test]
fn lost_range_is_a_gap_in_reads_and_breaks_matches() {
    let mut r = Rig::bootshell(&[4096]);
    r.rx(b"line one\r\nabc");
    r.core
        .expect_add(1, &[PatternSpec::literal("abcdef")], ExpectFrom::RxOffset(0))
        .expect("add");
    let from = r.off;
    let lost = LostRange {
        rx_from: from,
        rx_to: from + 100,
        bytes: 100,
        seq_from: r.seq + 1,
        seq_to: r.seq + 3,
        chunks: 3,
        cause: "reader_bridge_overflow",
    };
    r.seq += 3;
    r.off += 100;
    let g = r.core.on_lost(&lost);
    assert_eq!(g["count"], "3");
    assert_eq!(g["cause"], "reader_bridge_overflow");
    assert_eq!(g["bytes"], "100");
    assert_eq!(g["producer_range"]["from"], lost.seq_from.to_string());
    assert_eq!(g["producer_range"]["to"], lost.seq_to.to_string());
    assert!(g["producer_range"].get("producer_id").is_none());
    assert_eq!(n(&g["detail"]["rx_from"]), from);
    assert_eq!(
        r.core.rx_end(),
        from + 100,
        "lost bytes count in the offset space"
    );
    assert!(
        r.rx(b"def\r\nline two\r\n").hits.is_empty(),
        "no match across the gap"
    );

    let v = read(&mut r.core, "agent", ReadQuery::default());
    let t = texts(&v);
    assert_eq!(t[t.len() - 4..], ["line one", "abc", "def", "line two"]);
    assert_eq!(v["lines"][t.len() - 3]["complete"], false, "line cut by the gap");
    let gaps = v["gaps"].as_array().expect("gaps");
    assert_eq!(gaps.len(), 1);
    assert_eq!(gaps[0]["cause"], "reader_bridge_overflow");
    assert_eq!(gaps[0]["layer"], "record");
    assert_eq!(n(&gaps[0]["rx_offset"]), from);
    assert_eq!(n(&gaps[0]["bytes"]), 100);
    r.rx(b"line three\r\n");
    let v2 = read(&mut r.core, "agent", ReadQuery::default());
    assert_eq!(texts(&v2), vec!["line three"]);
    assert!(v2.get("gaps").is_none(), "a gap is reported once");
    let l = r.core.lines(&LinesQuery::default()).expect("lines");
    assert_eq!(l["gaps"].as_array().expect("gaps").len(), 1);
    let st = r.core.status();
    assert_eq!(st["lost"]["bytes"], "100");
    assert_eq!(st["lost"]["chunks"], "3");

    let mut c = ChannelCore::new("dut", Profile::default(), 1);
    c.set_producer("uart-worker", 4);
    let g = c.on_lost(&lost);
    assert_eq!(g["producer_range"]["producer_id"], "uart-worker");
    assert_eq!(g["producer_range"]["producer_epoch"], "4");
}

// ---------------------------------------------------------------------------------------------
// budgets and errors
// ---------------------------------------------------------------------------------------------

fn filler(n: usize) -> Vec<u8> {
    let mut v = Vec::with_capacity(n + 64);
    let mut i = 0;
    while v.len() < n {
        v.extend_from_slice(format!("filler line {i:06} ..............................\r\n").as_bytes());
        i += 1;
    }
    v.truncate(n);
    v
}

#[test]
fn backfill_scan_limit_resumes_until_found() {
    for pat in [
        PatternSpec::literal("MAGIC-42"),
        PatternSpec::regex(r"MAGIC-[0-9]+"),
    ] {
        let mut r = Rig::bootshell(&[4096]);
        let magic_at = {
            r.deliver(&filler(700 << 10));
            let at = r.off;
            r.deliver(b"MAGIC-42\r\n");
            r.deliver(&filler(10 << 10));
            at
        };
        let mut from = 0;
        let mut tries = 0;
        let hit = loop {
            tries += 1;
            assert!(tries < 30, "no progress");
            match r
                .core
                .expect_add(tries, std::slice::from_ref(&pat), ExpectFrom::RxOffset(from))
            {
                Ok(hs) => break hs.into_iter().next().expect("found in the ring"),
                Err(ChannelError::ScanLimit { resume_from }) => {
                    assert!(resume_from > from, "progress");
                    assert!(resume_from - from <= EXPECT_BACKFILL_SCAN);
                    assert_eq!(
                        ChannelError::ScanLimit { resume_from }.code(),
                        ErrorCode::ScanLimit
                    );
                    from = resume_from;
                }
                Err(e) => panic!("{e:?}"),
            }
        };
        assert_eq!(hit.start_off, magic_at);
        assert!(tries >= 3, "{:?} tries {tries}", pat.kind);
        assert_eq!(
            r.core.status()["expect"]["pending"],
            0,
            "failed attempts left nothing behind"
        );
    }
}

#[test]
fn page_read_stops_at_the_scan_budget_and_resumes() {
    let mut r = Rig::bootshell(&[4096]);
    let base = r.off;
    // 100 lines of 4 KiB escape sequences + short text: a page hits the scan budget long
    // before its 1 000-line / 64 KiB caps.
    let mut data = Vec::new();
    for i in 0..100 {
        for _ in 0..1000 {
            data.extend_from_slice(b"\x1b[0m");
        }
        data.extend_from_slice(format!("visible {i}\r\n").as_bytes());
    }
    r.deliver(&data);
    let want: Vec<String> = (0..100).map(|i| format!("visible {i}")).collect();
    let mut seen = Vec::new();
    let mut from = base;
    let mut pages = 0;
    for _ in 0..10 {
        let v = read(
            &mut r.core,
            "pg",
            ReadQuery {
                from: ReadFrom::RxOffset(from),
                mode: ReadMode::Page,
                ..ReadQuery::default()
            },
        );
        pages += 1;
        assert!(texts(&v).len() <= 32, "a page holds ≤ budget / 4 KiB lines");
        seen.extend(texts(&v));
        if v["more_available"] == false {
            break;
        }
        let resume = n(&v["scan_limit"]["resume_from"]);
        assert!(resume > from && resume - from <= MAX_READ_SCAN);
        from = resume;
    }
    assert_eq!(seen, want);
    assert!(pages >= 4, "{pages}");
}

#[test]
fn summary_over_the_budget_skips_to_the_newest_part() {
    let mut r = Rig::bootshell(&[4096]);
    r.deliver(b"first\r\n");
    r.deliver(&filler(1 << 20));
    r.deliver(b"\r\nnewest\r\n");
    let v = read(&mut r.core, "agent", ReadQuery::default());
    let s = &v["skipped"];
    assert_eq!(n(&s["from_rx"]), 0);
    assert!(r.core.rx_end() - n(&s["to_rx"]) <= MAX_READ_SCAN);
    assert_eq!(n(&v["scan_limit"]["resume_from"]), 0);
    assert_eq!(v["truncated"], true);
    assert_eq!(texts(&v).last().map(String::as_str), Some("newest"));
    assert!(
        texts(&v)[0].starts_with("filler line"),
        "window starts at a line start"
    );
    // The cursor reached the end; the skipped part is still readable by offset.
    assert!(texts(&read(&mut r.core, "agent", ReadQuery::default())).is_empty());
    let p = read(
        &mut r.core,
        "agent",
        ReadQuery {
            from: ReadFrom::RxOffset(0),
            mode: ReadMode::Peek,
            max_lines: Some(1),
            ..ReadQuery::default()
        },
    );
    assert_eq!(n(&p["range"]["from_rx"]), 0);
    assert_eq!(texts(&p).len(), 1);
}

#[test]
fn cursor_gap_when_the_start_left_the_ring() {
    let mut r = Rig::bootshell(&[8]);
    // > 32 768 entries evict the oldest ones.
    r.deliver(&filler(40_000 * 8));
    let retained = n(&r.core.status()["retained_from"]);
    assert!(retained > 0);
    let e = r
        .core
        .expect_add(1, &[PatternSpec::literal("x")], ExpectFrom::RxOffset(0));
    assert_eq!(e, Err(ChannelError::CursorGap { earliest: retained }));
    assert_eq!(
        ChannelError::CursorGap { earliest: retained }.code(),
        ErrorCode::CursorGap
    );
    let v = read(
        &mut r.core,
        "agent",
        ReadQuery {
            from: ReadFrom::RxOffset(0),
            mode: ReadMode::Peek,
            ..ReadQuery::default()
        },
    );
    assert_eq!(n(&v["cursor_gap"]["earliest"]), retained);
    assert_eq!(n(&v["cursor_gap"]["lost_from"]), 0);
    let l = r
        .core
        .lines(&LinesQuery {
            from: Some(0),
            limit: 1,
            ..LinesQuery::default()
        })
        .expect("lines");
    assert_eq!(n(&l["cursor_gap"]["earliest"]), retained);
}

#[test]
fn expect_admission_errors() {
    let mut r = Rig::boot_to_login(&[4096]);
    let e = r
        .core
        .expect_add(1, &[PatternSpec::regex("(a")], ExpectFrom::Now)
        .expect_err("syntax");
    assert!(matches!(e, ChannelError::Pattern(MatchError::Syntax { .. })));
    assert_eq!(e.code(), ErrorCode::Usage);
    let e = r
        .core
        .expect_add(1, &[PatternSpec::regex(r"(a)\1")], ExpectFrom::Now)
        .expect_err("backref");
    assert_eq!(e.code(), ErrorCode::Usage);
    let e = r.core.expect_add(1, &[], ExpectFrom::Now).expect_err("empty");
    assert_eq!(e, ChannelError::Pattern(MatchError::EmptyPattern));
    for i in 0..8 {
        r.core
            .expect_add(100 + i, &[PatternSpec::literal("zz")], ExpectFrom::Now)
            .expect("admit");
    }
    let e = r
        .core
        .expect_add(200, &[PatternSpec::literal("zz")], ExpectFrom::Now)
        .expect_err("full");
    assert_eq!(e.code(), ErrorCode::ScanLimit);
    let e = r
        .core
        .expect_add(100, &[PatternSpec::literal("zz")], ExpectFrom::Now)
        .expect_err("dup");
    assert!(matches!(
        e,
        ChannelError::Pattern(MatchError::DuplicateWaiter | MatchError::TooManyWaiters)
    ));
    for i in 0..8 {
        r.core.expect_remove(100 + i);
    }
    let long = "x".repeat(200);
    let e = r
        .core
        .expect_add(1, &[PatternSpec::literal("a")], ExpectFrom::Cursor(long))
        .expect_err("name");
    assert_eq!(e.code(), ErrorCode::Usage);
    // A profile without a prompt cannot serve `--expect-prompt`.
    let mut c = ChannelCore::new(
        "raw",
        Profile {
            name: "raw".into(),
            prompt: None,
            line_kill_ctrl_u: false,
            eol: Eol::None,
        },
        1,
    );
    let e = c
        .expect_add(1, &[PatternSpec::prompt()], ExpectFrom::Now)
        .expect_err("no prompt");
    assert_eq!(e.code(), ErrorCode::Usage);
    assert!(c.status()["profile"]["prompt"].is_null());
}

// ---------------------------------------------------------------------------------------------
// profiles and params
// ---------------------------------------------------------------------------------------------

#[test]
fn profiles_and_eol() {
    let g = Profile::named("generic").expect("generic");
    assert_eq!(g.prompt.as_deref(), Some(r"[#$>] ?$"));
    assert!(!g.line_kill_ctrl_u);
    let l = Profile::named("linux-shell").expect("linux");
    assert_eq!(l.prompt.as_deref(), Some(r"[#$] $"));
    assert!(l.line_kill_ctrl_u);
    let u = Profile::named("uboot").expect("uboot");
    assert_eq!(u.prompt.as_deref(), Some("=> $"));
    assert!(u.line_kill_ctrl_u);
    assert!(Profile::named("nope").is_none());
    assert_eq!(Eol::parse("crlf").map(|e| e.bytes()), Some(&b"\r\n"[..]));
    assert_eq!(Eol::parse("cr"), Some(Eol::Cr));
    assert_eq!(Eol::None.bytes(), b"");
    assert_eq!(Eol::parse("CR"), None);
    // The U-Boot prompt is detected for the arbiter.
    let mut c = ChannelCore::new("u", u, 1);
    let ing = c.on_rx(&RxChunk {
        source_seq: 1,
        rx_offset: 0,
        mono_ns: 1,
        wall_ns: 1,
        bytes: b"Hit any key\r\n=> ".to_vec(),
    });
    assert!(ing.prompt_seen);
}

#[test]
fn rpc_params_parse() {
    let q = ReadQuery::from_params(&json!({"channel": "dut", "from": "off:42", "mode": "page", "max_lines": 7, "ansi": "escape", "until": "off:99"}))
        .expect("parse");
    assert_eq!(q.from, ReadFrom::RxOffset(42));
    assert_eq!(q.mode, ReadMode::Page);
    assert_eq!(q.max_lines, Some(7));
    assert_eq!(q.until, Some(99));
    let q = ReadQuery::from_params(&json!({"cursor_token": "c1-abc"})).expect("token");
    assert_eq!(q.from, ReadFrom::Token("c1-abc".into()));
    assert!(
        ReadQuery::from_params(&json!({"from": "seq:5"})).is_err(),
        "stream seq needs the gateway"
    );
    assert!(ReadQuery::from_params(&json!({"mode": "all"})).is_err());
    let l = LinesQuery::from_params(&json!({"from": "off:5", "limit": 2000, "dir": "tx"})).expect("lines");
    assert_eq!((l.from, l.limit), (Some(5), 2000));
    assert!(LinesQuery::from_params(&json!({"limit": 2001})).is_err());
    assert_eq!(
        ExpectFrom::from_spec(None, "a").expect("default"),
        ExpectFrom::Cursor("a".into())
    );
    assert_eq!(
        ExpectFrom::from_spec(Some("now"), "a").expect("now"),
        ExpectFrom::Now
    );
    assert_eq!(
        ExpectFrom::from_spec(Some("off:9"), "a").expect("off"),
        ExpectFrom::RxOffset(9)
    );
    let p: Vec<PatternSpec> = serde_json::from_value(
        json!([{"pattern": "login:"}, {"kind": "regex", "pattern": "a+"}, {"kind": "prompt"}]),
    )
    .expect("patterns");
    assert_eq!(p[0], PatternSpec::literal("login:"));
    assert_eq!(p[1].kind, PatternKind::Regex);
    assert_eq!(p[2], PatternSpec::prompt());
}
