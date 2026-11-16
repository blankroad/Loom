//! Operator upgrade UDS batch 3 — pure/module contracts (Opus-authored specification).
//! U5 inline diagnostic DB (size limit, validation, decode with DB → ISO fallback, warnings).
//! U2 key provider (config lookup/validation, output parsing, bounded child process).
//! Contracts: work/reports/operator-upgrade/uds/FUNCTIONS.md (B3-*).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::{Duration, Instant};

use loom_proto::error::ErrorCode;
use loom_uds::diagdb::{
    MAX_DB_ERROR_LEN, MAX_INLINE_DB_BYTES, db_info, db_warnings, decode_dids_with, parse_inline_db,
};
use loom_uds::keyprovider::{
    MAX_ERROR_SNIPPET, MAX_PROVIDER_OUTPUT_BYTES, ProviderError, parse_key_output, provider_from_config,
    run_provider, sanitize_snippet, validate_provider,
};
use loom_uds::policy::KeyProviderConfig;
use loom_uds_codec::StandardDids;
use serde_json::{Value, json};

fn db() -> Value {
    json!({"loom_diag": 1, "ecu": "bench", "dids": [
        {"id": 0xF1A0, "name": "Supply voltage", "length": 1, "codec": {"type": "uint", "scale": 0.5, "unit": "V"}, "writable": true},
        {"id": 0x0101, "name": "Relay", "length": 1, "codec": {"type": "enum", "map": [{"value": 0, "name": "off"}, {"value": 1, "name": "on"}]}},
        {"id": 0xF18C, "name": "Bench serial", "codec": {"type": "ascii"}}
    ]})
}

// ------------------------------------------------------------------ U5 diag DB
#[test]
fn b3_d1_parse_inline_db() {
    let d = parse_inline_db(&db()).expect("valid");
    assert_eq!(d.db.dids.len(), 3);
    let e = parse_inline_db(&json!({"loom_diag": 2, "dids": []})).unwrap_err();
    assert!(e.starts_with("invalid loom_diag DB: "), "{e}");
    let dup = json!({"loom_diag": 1, "dids": [
        {"id": 1, "name": "a", "length": 1, "codec": {"type": "hex"}},
        {"id": 1, "name": "b", "length": 1, "codec": {"type": "hex"}}]});
    assert!(
        parse_inline_db(&dup).unwrap_err().contains("0x0001"),
        "duplicate DID named"
    );
    assert!(
        parse_inline_db(&json!({"loom_diag": 1, "dids": [], "risk": "R0"})).is_err(),
        "unknown fields (e.g. a risk override) rejected"
    );
    let long_name = "x".repeat(MAX_INLINE_DB_BYTES);
    let big = json!({"loom_diag": 1, "ecu": long_name, "dids": []});
    let e = parse_inline_db(&big).unwrap_err();
    assert!(
        e.contains(&MAX_INLINE_DB_BYTES.to_string()),
        "size checked before parsing: {e}"
    );
    let noisy = json!({"loom_diag": 1, "dids": [{"id": 1, "name": "y".repeat(4000), "length": 1, "codec": {"type": "hex"}}]});
    assert!(
        parse_inline_db(&noisy).unwrap_err().chars().count() <= MAX_DB_ERROR_LEN,
        "error text bounded"
    );
}

#[test]
fn b3_d2_d3_info_and_warnings() {
    let d = parse_inline_db(&db()).unwrap();
    let i = db_info(&d);
    assert_eq!(i.hash, d.hash_hex());
    assert_eq!(i.hash.len(), 16);
    assert_eq!(i.ecu.as_deref(), Some("bench"));
    assert_eq!(i.dids, 3);
    assert_eq!(
        db_warnings(&d),
        vec![
            "DID F18C overrides the ISO table entry ECUSerialNumber".to_string(),
            "writable flags are informational: write risk is decided from the bytes and never lowered by a DB".to_string(),
        ]
    );
    let plain = parse_inline_db(&json!({"loom_diag": 1, "dids": [{"id": 0x0101, "name": "r", "length": 1, "codec": {"type": "hex"}}]})).unwrap();
    assert!(db_warnings(&plain).is_empty());
}

#[test]
fn b3_d4_decode_with_db_then_iso_then_raw() {
    let d = parse_inline_db(&db()).unwrap();
    let v =
        serde_json::to_value(decode_dids_with(&d, &[0x22, 0xF1, 0xA0], &[0x62, 0xF1, 0xA0, 0x7B]).unwrap())
            .unwrap();
    assert_eq!(
        v,
        json!([{"did": "F1A0", "name": "Supply voltage", "value": 61.5, "unit": "V"}])
    );
    let v =
        serde_json::to_value(decode_dids_with(&d, &[0x22, 0x01, 0x01], &[0x62, 0x01, 0x01, 0x01]).unwrap())
            .unwrap();
    assert_eq!(v[0]["value"], "on");
    // not in the DB → ISO table (VIN)
    let mut resp = vec![0x62, 0xF1, 0x90];
    resp.extend_from_slice(b"LOOMSIM0000000001");
    let v = serde_json::to_value(decode_dids_with(&d, &[0x22, 0xF1, 0x90], &resp).unwrap()).unwrap();
    assert_eq!(
        (v[0]["name"].clone(), v[0]["value"].clone()),
        (json!("VIN"), json!("LOOMSIM0000000001"))
    );
    // unknown everywhere → raw hex, no name
    let v =
        serde_json::to_value(decode_dids_with(&d, &[0x22, 0xF4, 0x0D], &[0x62, 0xF4, 0x0D, 0x00]).unwrap())
            .unwrap();
    assert_eq!(
        v,
        json!([{"did": "F40D", "name": null, "value": "00", "unit": null}])
    );
    // same result as today's standard-only decoding when the lookup is the ISO table
    let a = decode_dids_with(&StandardDids, &[0x22, 0xF1, 0x90], &resp).unwrap();
    assert_eq!(a[0].name.as_deref(), Some("VIN"));
    assert!(
        decode_dids_with(&d, &[0x22, 0xF1, 0xA0], &[0x7F, 0x22, 0x31]).is_none(),
        "negative response does not decode"
    );
}

// ------------------------------------------------------------------ U2 key provider config
fn script(name: &str) -> String {
    format!("{}/tests/fixtures/keyprovider/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// An absolute command path in the host OS's own syntax (validation never checks existence).
/// Unix `/opt/loom/<n>`; Windows `C:\\loom\\<n>.exe` (`Path::is_absolute` needs a drive or UNC prefix there).
fn abs(n: &str) -> String {
    if cfg!(windows) {
        format!("C:\\loom\\{n}.exe")
    } else {
        format!("/opt/loom/{n}")
    }
}

fn provider(command: &str, timeout_ms: Option<u32>) -> KeyProviderConfig {
    KeyProviderConfig {
        name: "bench".into(),
        command: command.into(),
        args: vec![],
        timeout_ms,
        levels: vec![1, 3],
    }
}

#[test]
fn b3_k2_validate() {
    assert_eq!(validate_provider(&provider(&abs("seedkey"), None)), Ok(()));
    let bad = |c: KeyProviderConfig, needle: &str| match validate_provider(&c) {
        Err(ProviderError::BadConfig(m)) => assert!(m.contains(needle), "{m:?} should mention {needle:?}"),
        other => panic!("expected BadConfig({needle}), got {other:?}"),
    };
    bad(provider("seedkey", None), "absolute");
    bad(
        KeyProviderConfig {
            levels: vec![2],
            ..provider(&abs("x"), None)
        },
        "odd",
    );
    bad(
        KeyProviderConfig {
            levels: vec![0x7F],
            ..provider(&abs("x"), None)
        },
        "odd",
    );
    bad(
        KeyProviderConfig {
            levels: vec![],
            ..provider(&abs("x"), None)
        },
        "levels",
    );
    bad(provider(&abs("x"), Some(10_001)), "timeout_ms");
    bad(
        KeyProviderConfig {
            args: vec!["a".into(); 17],
            ..provider(&abs("x"), None)
        },
        "args",
    );
    bad(
        KeyProviderConfig {
            args: vec!["a".repeat(257)],
            ..provider(&abs("x"), None)
        },
        "args",
    );
    bad(
        KeyProviderConfig {
            name: "bad name".into(),
            ..provider(&abs("x"), None)
        },
        "name",
    );
    // `.bat` / `.cmd` would run through cmd.exe on Windows (a shell): refused on every OS, any case.
    for b in ["gen.bat", "gen.CMD", "gen.Bat"] {
        bad(
            provider(&format!("{}{b}", abs("dir").trim_end_matches(".exe")), None),
            "batch",
        );
    }
}

/// B3-K2: "absolute" is the host OS's `Path::is_absolute`, never a `starts_with('/')` test.
#[test]
fn b3_k2_absolute_path_is_os_native() {
    let ok = |c: &str| validate_provider(&provider(c, None)).is_ok();
    if cfg!(windows) {
        assert!(ok(r"C:\Program Files\Bench\seedkey.exe"), "drive path");
        assert!(ok(r"\\server\share\seedkey.exe"), "UNC path");
        assert!(
            ok("C:/Program Files/Bench/seedkey.exe"),
            "drive path with forward slashes"
        );
        assert!(
            !ok(r"\seedkey.exe"),
            "rooted without a drive is relative to the current drive"
        );
        assert!(!ok(r"C:seedkey.exe"), "drive-relative");
        assert!(!ok("/opt/loom/seedkey"), "a Unix path is not absolute on Windows");
    } else {
        assert!(ok("/opt/loom/seedkey"));
        assert!(
            !ok(r"C:\loom\seedkey.exe"),
            "a Windows drive path is relative on Unix"
        );
    }
    assert!(!ok("seedkey") && !ok("./seedkey") && !ok(""), "relative / empty");
}

#[test]
fn b3_k3_lookup_in_uds_section() {
    let cmd = abs("seedkey");
    let cfg = json!({"key_providers": [{"name": "bench", "command": cmd, "levels": [1]}]});
    let p = provider_from_config(&cfg, "bench", 1).unwrap();
    assert_eq!((p.command.as_str(), p.timeout_ms), (cmd.as_str(), None));
    assert_eq!(
        provider_from_config(&Value::Null, "bench", 1),
        Err(ProviderError::NotConfigured("bench".into()))
    );
    assert_eq!(
        provider_from_config(&cfg, "other", 1),
        Err(ProviderError::NotConfigured("other".into()))
    );
    assert_eq!(
        provider_from_config(&cfg, "bench", 3),
        Err(ProviderError::LevelNotAllowed {
            level: 3,
            allowed: vec![1]
        })
    );
    let typo = json!({"key_providers": [{"name": "bench", "command": cmd, "levels": [1], "shell": true}]});
    assert!(
        matches!(
            provider_from_config(&typo, "bench", 1),
            Err(ProviderError::BadConfig(_))
        ),
        "unknown keys refused"
    );
    let rel = json!({"key_providers": [{"name": "bench", "command": "seedkey", "levels": [1]}]});
    assert!(matches!(
        provider_from_config(&rel, "bench", 1),
        Err(ProviderError::BadConfig(_))
    ));
}

#[test]
fn b3_k4_parse_key_output() {
    assert_eq!(parse_key_output(b"1234ABCD\n"), Ok(vec![0x12, 0x34, 0xAB, 0xCD]));
    assert_eq!(parse_key_output(b"  0a0b \r\n"), Ok(vec![0x0A, 0x0B]));
    for bad in [
        &b""[..],
        b"\n",
        b"123",
        b"12 34",
        b"zz",
        b"key=1234\n",
        b"12\n34\n",
        b"\xff\xfe",
    ] {
        assert!(
            matches!(parse_key_output(bad), Err(ProviderError::BadKey(_))),
            "{bad:?}"
        );
    }
    assert!(
        matches!(
            parse_key_output("ab".repeat(513).as_bytes()),
            Err(ProviderError::BadKey(_))
        ),
        "over 1024 hex chars"
    );
    assert_eq!(
        parse_key_output(&vec![b'a'; MAX_PROVIDER_OUTPUT_BYTES + 1]),
        Err(ProviderError::OutputTooLarge)
    );
}

#[test]
fn b3_k5_snippet_and_k1_codes() {
    assert_eq!(sanitize_snippet(b"bad\n"), "bad");
    assert_eq!(sanitize_snippet(b"e\x1b[31mX\tY\n"), "e?[31mX?Y");
    assert_eq!(sanitize_snippet(&[b'a'; 300]).chars().count(), MAX_ERROR_SNIPPET);
    assert_eq!(sanitize_snippet(b"\xff\xfeok"), "\u{fffd}\u{fffd}ok");
    let cases = [
        (
            ProviderError::NotConfigured("x".into()),
            ErrorCode::Config,
            "key provider \"x\" is not configured in loom.toml [[uds.key_providers]]",
        ),
        (
            ProviderError::BadConfig("levels must be odd".into()),
            ErrorCode::Config,
            "key provider config: levels must be odd",
        ),
        (
            ProviderError::LevelNotAllowed {
                level: 3,
                allowed: vec![1, 5],
            },
            ErrorCode::Config,
            "key provider does not allow level 0x03 (allowed: 01, 05)",
        ),
        (
            ProviderError::Spawn("No such file".into()),
            ErrorCode::Config,
            "key provider could not start: No such file",
        ),
        (
            ProviderError::Timeout { ms: 300 },
            ErrorCode::Timeout,
            "key provider timed out after 300 ms (killed; no key sent)",
        ),
        (
            ProviderError::Exit {
                code: Some(3),
                stderr: "no license".into(),
            },
            ErrorCode::Config,
            "key provider exited with code 3: no license",
        ),
        (
            ProviderError::Exit {
                code: None,
                stderr: String::new(),
            },
            ErrorCode::Config,
            "key provider exited with a signal",
        ),
        (
            ProviderError::OutputTooLarge,
            ErrorCode::Limit,
            "key provider output exceeded 4096 bytes (killed; no key sent)",
        ),
        (
            ProviderError::BadKey("expected one hex line".into()),
            ErrorCode::Config,
            "key provider output is not a key: expected one hex line",
        ),
    ];
    for (e, code, msg) in cases {
        assert_eq!((e.code(), e.message()), (code, msg.to_string()), "{e:?}");
    }
}

// ------------------------------------------------------------------ U2 bounded child process
// Platform gate: these fixtures are POSIX `sh` scripts (tests/fixtures/keyprovider/*.sh) run directly
// through their shebang; Windows has no `sh` to exec them. The provider code itself is not
// Unix-specific — Windows coverage is `b3_k6_windows_system_executables` below.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn b3_k6_run_provider_bounded() {
    let ok = provider(&script("xor.sh"), None);
    assert_eq!(
        run_provider(&ok, 1, &[0x11, 0x22, 0x33, 0x44]).await,
        Ok(vec![0x03, 0x16, 0x65, 0x3C])
    );
    assert_eq!(
        run_provider(&ok, 1, &[0xAB, 0xCD, 0xEF, 0x01]).await,
        Ok(vec![0xB9, 0xF9, 0xB9, 0x79]),
        "seed hex upper-case on stdin"
    );
    assert_eq!(
        run_provider(&provider(&script("env.sh"), None), 3, &[1]).await,
        Ok(vec![3, 3]),
        "cleared env + LOOM_UDS_LEVEL"
    );

    let t = Instant::now();
    assert_eq!(
        run_provider(&provider(&script("slow.sh"), Some(300)), 1, &[1]).await,
        Err(ProviderError::Timeout { ms: 300 })
    );
    assert!(
        t.elapsed() < Duration::from_secs(2),
        "killed at the deadline, not after the script: {:?}",
        t.elapsed()
    );

    assert_eq!(
        run_provider(&provider(&script("big.sh"), None), 1, &[1]).await,
        Err(ProviderError::OutputTooLarge)
    );
    assert_eq!(
        run_provider(&provider(&script("fail.sh"), None), 1, &[1]).await,
        Err(ProviderError::Exit {
            code: Some(3),
            stderr: "no license".into()
        })
    );
    assert!(matches!(
        run_provider(&provider(&script("junk.sh"), None), 1, &[1]).await,
        Err(ProviderError::BadKey(_))
    ));
    assert!(matches!(
        run_provider(&provider(&abs("nonexistent-loom-seedkey"), None), 1, &[1]).await,
        Err(ProviderError::Spawn(_))
    ));
    assert!(
        matches!(
            run_provider(&provider("relative.sh", None), 1, &[1]).await,
            Err(ProviderError::BadConfig(_))
        ),
        "validated before spawn"
    );
    assert!(
        matches!(
            run_provider(&ok, 2, &[1]).await,
            Err(ProviderError::LevelNotAllowed { level: 2, .. })
        ),
        "level checked before spawn"
    );
}

// Platform gate: these fixtures are POSIX `sh` scripts (tests/fixtures/keyprovider/*.sh) run directly
// through their shebang; Windows has no `sh` to exec them. The provider code itself is not
// Unix-specific — Windows coverage is `b3_k6_windows_system_executables` below.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn b3_k6_dropping_the_call_kills_the_child() {
    let slow = provider(&script("slow.sh"), Some(10_000));
    let t = Instant::now();
    let r = tokio::time::timeout(Duration::from_millis(200), run_provider(&slow, 1, &[1])).await;
    assert!(r.is_err(), "caller gave up");
    assert!(t.elapsed() < Duration::from_secs(1));
    // kill_on_drop: no orphan keeps running; a fresh call still works
    assert!(
        run_provider(&provider(&script("xor.sh"), None), 1, &[0, 0, 0, 1])
            .await
            .is_ok()
    );
}

// Platform gate: uses executables that only exist on Windows (`%SystemRoot%\\System32\\cmd.exe`,
// `PING.EXE`). Same contract as the Unix tests: no shell is added by Loom (cmd.exe here is the
// configured executable itself, with fixed args), cleared env, bounded time, killed on timeout.
// NOT run on the macOS development host of this batch — see the UDS report's platform table.
#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn b3_k6_windows_system_executables() {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let sys = |exe: &str| format!(r"{root}\System32\{exe}");
    let with = |exe: &str, args: &[&str], timeout_ms: Option<u32>| KeyProviderConfig {
        args: args.iter().map(|a| (*a).to_string()).collect(),
        ..provider(&sys(exe), timeout_ms)
    };
    assert_eq!(
        run_provider(&with("cmd.exe", &["/D", "/C", "echo", "0102"], None), 1, &[9]).await,
        Ok(vec![1, 2])
    );
    assert_eq!(
        run_provider(&with("cmd.exe", &["/D", "/C", "exit", "3"], None), 1, &[9]).await,
        Err(ProviderError::Exit {
            code: Some(3),
            stderr: String::new()
        })
    );
    let t = Instant::now();
    assert_eq!(
        run_provider(&with("PING.EXE", &["-n", "6", "127.0.0.1"], Some(300)), 1, &[9]).await,
        Err(ProviderError::Timeout { ms: 300 })
    );
    assert!(
        t.elapsed() < Duration::from_secs(2),
        "killed at the deadline: {:?}",
        t.elapsed()
    );
    assert!(matches!(
        run_provider(&provider(&sys("loom-missing-seedkey.exe"), None), 1, &[9]).await,
        Err(ProviderError::Spawn(_))
    ));
}
