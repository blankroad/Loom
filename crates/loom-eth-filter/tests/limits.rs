mod common;

use common::{Fx, Rng, decoded};
use loom_eth_filter::{Filter, MAX_DEPTH, MAX_FILTER_LEN, MAX_NODES, MAX_REGEX_LEN, MAX_SET_ITEMS};

#[test]
fn length_limit() {
    let long = format!("doip.vin == \"{}\"", "a".repeat(MAX_FILTER_LEN));
    let e = Filter::compile(&long).expect_err("too long");
    assert_eq!(e.pos, MAX_FILTER_LEN);
}

#[test]
fn paren_depth_limit() {
    let ok = format!("{}doip{}", "(".repeat(MAX_DEPTH - 2), ")".repeat(MAX_DEPTH - 2));
    assert!(Filter::compile(&ok).is_ok());
    let deep = format!("{}doip{}", "(".repeat(70), ")".repeat(70));
    let e = Filter::compile(&deep).expect_err("too deep");
    assert_eq!(e.pos, 64);
    assert!(e.msg.contains("nesting"));
}

#[test]
fn not_depth_limit() {
    let deep = format!("{}doip", "not ".repeat(70));
    let e = Filter::compile(&deep).expect_err("too deep");
    assert_eq!(e.pos, 63 * 4);
}

#[test]
fn node_limit() {
    // Set items count as nodes: 5 sets x 230 single-digit items stay under MAX_FILTER_LEN.
    let set = format!("ip.ttl in {{{}}}", vec!["1"; 230].join(" "));
    let many = vec![set; 5].join(" or ");
    assert!(many.len() < MAX_FILTER_LEN);
    const { assert!(5 * 231 > MAX_NODES) };
    let e = Filter::compile(&many).expect_err("too many nodes");
    assert!(e.msg.contains("nodes"), "{e}");
    let fine = vec!["doip"; 200].join(" or ");
    assert!(Filter::compile(&fine).is_ok());
}

#[test]
fn set_limit() {
    let items: Vec<String> = (0..=MAX_SET_ITEMS).map(|i| i.to_string()).collect();
    let e = Filter::compile(&format!("doip.type in {{{}}}", items.join(" "))).expect_err("too many items");
    assert!(e.msg.contains("set items"), "{e}");
    let ok: Vec<String> = (0..MAX_SET_ITEMS).map(|i| i.to_string()).collect();
    assert!(Filter::compile(&format!("doip.type in {{{}}}", ok.join(" "))).is_ok());
}

#[test]
fn regex_limits() {
    let long = format!("doip.vin matches \"{}\"", "a".repeat(MAX_REGEX_LEN + 1));
    assert_eq!(Filter::compile(&long).expect_err("long regex").pos, 17);
    // A pattern that compiles to more than REGEX_SIZE_LIMIT is rejected, not built.
    let huge = "doip.vin matches \"(\\\\w{100}){100}\"";
    assert!(Filter::compile(huge).is_err());
}

#[test]
fn random_filter_text_100k() {
    // Token-ish alphabet so the parser and checker see deep paths, plus raw random bytes.
    const PIECES: &[&str] = &[
        "doip",
        "doip.type",
        "ip.addr",
        "eth.src",
        "uds.sid",
        "tcp.port",
        "frame",
        "loom.seq",
        "frame.time_relative",
        "==",
        "!=",
        "<",
        ">=",
        "eq",
        "contains",
        "matches",
        "~",
        "in",
        "{",
        "}",
        "[",
        "]",
        "(",
        ")",
        ",",
        "..",
        "and",
        "or",
        "not",
        "!",
        "&&",
        "||",
        "0x8001",
        "13400",
        "192.0.2.0/24",
        "fd00::1",
        "02:00:00",
        "\"a.*\"",
        "\"x",
        "0:3",
        "-1:1",
        "4-5",
        "1.5",
        "true",
        "#",
        "&",
        "$",
        "'",
        "===",
        ";",
        " ",
        "  ",
    ];
    let fixtures = [decoded(Fx::Req), decoded(Fx::Sd), decoded(Fx::Arp)];
    let mut rng = Rng(0xC0FF_EE00_DEAD_BEEF);
    let mut compiled = 0usize;
    for i in 0..100_000u32 {
        let mut s = String::new();
        if i % 5 == 0 {
            let n = rng.below(64) as usize;
            for _ in 0..n {
                s.push(char::from_u32(rng.below(0x250) as u32).unwrap_or('?'));
            }
        } else {
            let n = 1 + rng.below(12) as usize;
            for _ in 0..n {
                s.push_str(PIECES[rng.below(PIECES.len() as u64) as usize]);
                if rng.below(2) == 0 {
                    s.push(' ');
                }
            }
        }
        match Filter::compile(&s) {
            Ok(f) => {
                compiled += 1;
                for d in &fixtures {
                    let _ = f.matches(d);
                }
            }
            Err(e) => assert!(e.pos <= s.len(), "error offset {} past end for `{s}`", e.pos),
        }
    }
    assert!(compiled > 1000, "random generator too weak: {compiled} compiled");
}
