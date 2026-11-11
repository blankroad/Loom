//! CLI piece (K3): `loom can send | trace | last | expect | status`.
//!
//! `to_call` is pure (no daemon) and checks frames, ids, cursors and bounds before anything is
//! sent. Human output uses the Trace line format
//! `812.004  tx AGT 0x300   1  01  sent` and ends with a summary line (rows, cursor, truncation);
//! `--json` prints the envelope unchanged (cli-kit).

use std::collections::HashMap;
use std::io;
use std::time::Duration;

use loom_cli_kit::{DomainCli, RpcCall, UsageError, cursor_arg, render_common};
use loom_proto::cursor::Cursor;
use loom_proto::envelope::Envelope;
use serde_json::{Map, Value, json};

use crate::schema::{MAX_EXPECT_MS, MAX_GAP_MS, MAX_SEND_COUNT};
use crate::signals::{SIG_MAX_WINDOW_MS, WAIT_MAX_MS, parse_condition};
use crate::{
    RPC_DB_INFO, RPC_DB_LOAD, RPC_EXPECT, RPC_ISOTP_ABORT, RPC_ISOTP_BARRIER, RPC_ISOTP_CANCEL,
    RPC_ISOTP_CLOSE, RPC_ISOTP_LIST, RPC_ISOTP_OPEN, RPC_ISOTP_RECV, RPC_ISOTP_SEND, RPC_LAST,
    RPC_PERIODIC_LIST, RPC_PERIODIC_RENEW, RPC_PERIODIC_START, RPC_PERIODIC_STOP, RPC_PERIODIC_TAKEOVER,
    RPC_PERIODIC_UPDATE, RPC_SEND, RPC_SIG_GET, RPC_SIG_SET, RPC_SIG_WAIT_FOR, RPC_STATUS, RPC_TRACE_QUERY,
};

/// The `can` CLI fragment.
#[derive(Debug, Default)]
pub struct CanCli;

/// Default `--within` of `send --expect` (ms).
pub const DEFAULT_SEND_EXPECT_MS: u32 = 100;
/// Extra time the CLI waits beyond a wait command's own deadline.
const WAIT_MARGIN: Duration = Duration::from_secs(2);

fn channel_arg() -> clap::Arg {
    clap::Arg::new("channel")
        .long("channel")
        .short('c')
        .value_name("CH")
        .help("Channel (default: the only open channel, else ch0)")
}

fn ms_arg(name: &'static str, max: u32, help: &'static str) -> clap::Arg {
    clap::Arg::new(name)
        .long(name)
        .value_name("MS")
        .value_parser(clap::value_parser!(u32).range(1..=i64::from(max)))
        .help(help)
}

fn parse_id(s: &str) -> Result<u32, String> {
    let r = match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(h) => u32::from_str_radix(h, 16),
        None => s.parse(),
    };
    match r {
        Ok(v) if v <= loom_can_frame::MAX_EXTENDED_ID => Ok(v),
        _ => Err(format!("bad CAN id {s:?} (0x300, 768, ≤ 0x1FFFFFFF)")),
    }
}

/// `0x300` or `0x7E0-0x7EF`, validated, kept as text for the RPC.
fn parse_id_range(s: &str) -> Result<String, String> {
    match s.split_once('-') {
        Some((a, b)) => {
            let (lo, hi) = (parse_id(a.trim())?, parse_id(b.trim())?);
            if lo > hi {
                return Err(format!("empty id range {s:?}"));
            }
            Ok(format!("{lo:#X}-{hi:#X}"))
        }
        None => Ok(format!("{:#X}", parse_id(s.trim())?)),
    }
}

fn parse_origin(s: &str) -> Result<String, String> {
    match s {
        "agent" | "user" | "device" | "script" => Ok(s.to_owned()),
        _ => Err(format!("origin must be agent|user|device|script, not {s:?}")),
    }
}

fn parse_hex_arg(s: &str) -> Result<String, String> {
    let clean: String = s.chars().filter(|c| !matches!(c, '.' | ' ' | ':')).collect();
    if clean.is_empty()
        || !clean.len().is_multiple_of(2)
        || clean.len() > 128
        || !clean.chars().all(|c| c.is_ascii_hexdigit())
    {
        return Err(format!("bad hex {s:?} (e.g. 01 or 01.FF)"));
    }
    Ok(clean.to_lowercase())
}

fn cursor_param(m: &clap::ArgMatches, name: &str) -> Option<String> {
    m.get_one::<Cursor>(name).map(ToString::to_string)
}

fn set(params: &mut Map<String, Value>, key: &str, v: Option<Value>) {
    if let Some(v) = v {
        params.insert(key.into(), v);
    }
}

fn decode_arg() -> clap::Arg {
    clap::Arg::new("decode")
        .long("decode")
        .action(clap::ArgAction::SetTrue)
        .help("Add DBC signal values to frame rows (needs `loom can db load`)")
}

/// `10` / `1.5` (seconds), `500ms`, `30s`, `2m` → milliseconds, 1 ms ..= 1 h.
fn parse_timeout(s: &str) -> Result<u32, String> {
    let t = s.trim();
    let (num, scale) = if let Some(v) = t.strip_suffix("ms") {
        (v, 1.0)
    } else if let Some(v) = t.strip_suffix('s') {
        (v, 1_000.0)
    } else if let Some(v) = t.strip_suffix('m') {
        (v, 60_000.0)
    } else {
        (t, 1_000.0)
    };
    let ms = num
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && *v > 0.0)
        .map(|v| (v * scale).ceil())
        .ok_or_else(|| format!("bad timeout {s:?} (e.g. 10, 1.5, 500ms, 2m)"))?;
    if ms > f64::from(WAIT_MAX_MS) {
        return Err(format!("timeout {s:?} is longer than 1 h"));
    }
    Ok(ms as u32)
}

/// `Sig=3000`, `EngineData.Gear=D` → (signal, number | label).
fn parse_assign(s: &str) -> Result<(String, Value), String> {
    let (k, v) = s
        .split_once('=')
        .ok_or_else(|| format!("expected Signal=VALUE, got {s:?}"))?;
    let (k, v) = (k.trim(), v.trim());
    if k.is_empty() || v.is_empty() {
        return Err(format!("expected Signal=VALUE, got {s:?}"));
    }
    let value = match v.parse::<f64>() {
        Ok(n) if n.is_finite() => json!(n),
        _ => json!(v.trim_matches('"')),
    };
    Ok((k.to_owned(), value))
}

/// Lifetime: `30` / `1.5` (s), `500ms`, `30s`, `10m`, `2h` → ms (1 ms ..= 24 h).
fn parse_life_ms(s: &str) -> Result<u64, String> {
    let t = s.trim();
    let (num, scale) = if let Some(v) = t.strip_suffix("ms") {
        (v, 1.0)
    } else if let Some(v) = t.strip_suffix('s') {
        (v, 1_000.0)
    } else if let Some(v) = t.strip_suffix('m') {
        (v, 60_000.0)
    } else if let Some(v) = t.strip_suffix('h') {
        (v, 3_600_000.0)
    } else {
        (t, 1_000.0)
    };
    let ms = num
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && *v > 0.0)
        .map(|v| (v * scale).ceil())
        .ok_or_else(|| format!("bad duration {s:?} (e.g. 30, 500ms, 10m, 2h)"))?;
    if ms > 86_400_000.0 {
        return Err(format!("{s:?} is longer than 24 h"));
    }
    Ok(ms as u64)
}

fn periodic_command() -> clap::Command {
    let job = || clap::Arg::new("job").required(true).value_name("JOB");
    clap::Command::new("periodic")
        .about("Periodic sending (jobs). Remember the job id; stop with `periodic stop --mine` when done")
        .subcommand_required(true)
        .subcommand(
            clap::Command::new("start")
                .about("Start a periodic job: a raw FRAME, or --msg with --set Sig=V from the DBC")
                .arg(clap::Arg::new("frame").value_name("FRAME").conflicts_with("msg"))
                .arg(clap::Arg::new("msg").long("msg").value_name("MESSAGE"))
                .arg(
                    clap::Arg::new("set")
                        .long("set")
                        .value_name("Sig=V")
                        .action(clap::ArgAction::Append)
                        .value_parser(parse_assign)
                        .help("Signal value (number or label); others keep DBC start values"),
                )
                .arg(channel_arg())
                .arg(
                    clap::Arg::new("ext")
                        .long("ext")
                        .action(clap::ArgAction::SetTrue)
                        .help("29-bit id even if short"),
                )
                .arg(ms_arg(
                    "period",
                    60_000,
                    "Period (default for --msg: DBC GenMsgCycleTime)",
                ))
                .arg(
                    clap::Arg::new("offset")
                        .long("offset")
                        .value_name("MS")
                        .value_parser(clap::value_parser!(u32).range(0..=60_000)),
                )
                .arg(
                    clap::Arg::new("count")
                        .long("count")
                        .value_parser(clap::value_parser!(u32).range(1..))
                        .help("Stop after N frames"),
                )
                .arg(
                    clap::Arg::new("lease")
                        .long("lease")
                        .value_name("S")
                        .value_parser(parse_life_ms)
                        .help("Stops unless renewed within this time"),
                )
                .arg(
                    clap::Arg::new("max-duration")
                        .long("max-duration")
                        .value_name("S")
                        .value_parser(parse_life_ms)
                        .help("Hard lifetime (default 1 h)"),
                )
                .arg(
                    clap::Arg::new("owner")
                        .long("owner")
                        .help("Owner label shown to others"),
                )
                .arg(
                    clap::Arg::new("force-range")
                        .long("force-range")
                        .action(clap::ArgAction::SetTrue)
                        .help("Skip the DBC min/max check"),
                ),
        )
        .subcommand(
            clap::Command::new("update")
                .about("Change a job's bytes (compare-and-set on its revision)")
                .arg(job())
                .arg(
                    clap::Arg::new("rev")
                        .long("rev")
                        .required(true)
                        .value_parser(clap::value_parser!(u64))
                        .help("Revision from `periodic list`"),
                )
                .arg(clap::Arg::new("data").long("data").value_parser(parse_hex_arg))
                .arg(clap::Arg::new("mask").long("mask").value_parser(parse_hex_arg))
                .arg(clap::Arg::new("bytes").long("bytes").value_parser(parse_hex_arg)),
        )
        .subcommand(
            clap::Command::new("stop")
                .about("Stop a job, or all of yours with --mine")
                .arg(clap::Arg::new("job").value_name("JOB"))
                .arg(
                    clap::Arg::new("mine")
                        .long("mine")
                        .action(clap::ArgAction::SetTrue)
                        .conflicts_with("job"),
                ),
        )
        .subcommand(
            clap::Command::new("renew")
                .about("Renew a job's lease")
                .arg(job()),
        )
        .subcommand(
            clap::Command::new("takeover")
                .about("Become the owner of someone else's job")
                .arg(job()),
        )
        .subcommand(
            clap::Command::new("list")
                .about("Jobs, owners, revisions and timing stats")
                .arg(channel_arg()),
        )
}

fn isotp_command() -> clap::Command {
    let endpoint = || clap::Arg::new("endpoint").required(true).value_name("ENDPOINT");
    let flag = |name: &'static str, help: &'static str| {
        clap::Arg::new(name)
            .long(name)
            .action(clap::ArgAction::SetTrue)
            .help(help)
    };
    clap::Command::new("isotp")
        .about("ISO-TP endpoints for transport debugging (UDS diagnostics: use `loom uds`)")
        .subcommand_required(true)
        .subcommand(
            clap::Command::new("open")
                .about("Open an endpoint; its tx id is reserved (raw sends on it are refused)")
                .arg(
                    clap::Arg::new("tx")
                        .long("tx")
                        .required(true)
                        .value_name("ID")
                        .value_parser(parse_id),
                )
                .arg(
                    clap::Arg::new("rx")
                        .long("rx")
                        .required(true)
                        .value_name("ID")
                        .value_parser(parse_id),
                )
                .arg(channel_arg())
                .arg(flag("ext", "29-bit ids"))
                .arg(flag("fd", "CAN FD (default TX_DL 64)"))
                .arg(flag("brs", "FD bit rate switch"))
                .arg(
                    clap::Arg::new("tx-dl")
                        .long("tx-dl")
                        .value_parser(clap::value_parser!(u8))
                        .help("TX data length"),
                )
                .arg(
                    clap::Arg::new("padding")
                        .long("padding")
                        .value_name("HEX|none")
                        .help("Padding byte (default CC) or none"),
                )
                .arg(
                    clap::Arg::new("bs")
                        .long("bs")
                        .value_parser(clap::value_parser!(u8))
                        .help("Block size we ask for (default 8)"),
                )
                .arg(
                    clap::Arg::new("stmin")
                        .long("stmin")
                        .value_parser(clap::value_parser!(u8))
                        .help("STmin byte we ask for (default 0)"),
                )
                .arg(ms_arg(
                    "n-bs",
                    crate::isotp::MAX_TIMEOUT_MS,
                    "N_Bs (default 1000)",
                ))
                .arg(ms_arg(
                    "n-cr",
                    crate::isotp::MAX_TIMEOUT_MS,
                    "N_Cr (default 1000)",
                ))
                .arg(flag("server", "Sim ECU side (answers with --model)"))
                .arg(
                    clap::Arg::new("model")
                        .long("model")
                        .requires("server")
                        .help("Server model: vin_demo, echo"),
                )
                .arg(
                    clap::Arg::new("owner")
                        .long("owner")
                        .help("Owner label shown to others"),
                ),
        )
        .subcommand(
            clap::Command::new("send")
                .about("Send one PDU (hex)")
                .arg(endpoint())
                .arg(clap::Arg::new("pdu").required(true).value_name("PDU")),
        )
        .subcommand(
            clap::Command::new("abort")
                .about("Abort TX/RX in progress")
                .arg(endpoint()),
        )
        .subcommand(
            clap::Command::new("close")
                .about("Close (frees the tx id)")
                .arg(endpoint()),
        )
        .subcommand(
            clap::Command::new("list")
                .about("Open endpoints and counters")
                .arg(channel_arg()),
        )
        .subcommand(
            clap::Command::new("cancel")
                .about("Cancel one transfer (from the send result); RX on the endpoint keeps going")
                .arg(endpoint())
                .arg(clap::Arg::new("transfer").required(true).value_name("TRANSFER")),
        )
        .subcommand(
            clap::Command::new("recv")
                .about("Received PDUs after a cursor (waits up to --timeout for one)")
                .arg(endpoint())
                .arg(
                    clap::Arg::new("after")
                        .long("after")
                        .value_name("PDU_SEQ")
                        .value_parser(clap::value_parser!(u64))
                        .help("Only PDUs after this pdu_seq (default 0)"),
                )
                .arg(ms_arg("timeout", 60_000, "Wait for a PDU (default 0 = no wait)"))
                .arg(
                    clap::Arg::new("max")
                        .long("max")
                        .value_parser(clap::value_parser!(u64).range(1..=16))
                        .help("PDUs (default 16)"),
                ),
        )
        .subcommand(
            clap::Command::new("barrier")
                .about("Is ISO-TP processing complete up to a deadline? (UDS P2 drain check)")
                .arg(endpoint())
                .arg(
                    clap::Arg::new("epoch")
                        .long("epoch")
                        .required(true)
                        .value_parser(clap::value_parser!(u64)),
                )
                .arg(
                    clap::Arg::new("deadline-ns")
                        .long("deadline-ns")
                        .required(true)
                        .value_name("MONO_NS")
                        .value_parser(clap::value_parser!(u64)),
                )
                .arg(ms_arg(
                    "timeout",
                    60_000,
                    "Wait for completion (default 0 = no wait)",
                )),
        )
}

/// `CC` / `0xcc` / `none`.
fn padding_value(s: &str) -> Result<Value, UsageError> {
    if s.eq_ignore_ascii_case("none") {
        return Ok(Value::Null);
    }
    let h = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
    u8::from_str_radix(h, 16)
        .map(|b| json!(b))
        .map_err(|_| UsageError::new(format!("bad --padding {s:?} (a hex byte like CC, or none)")))
}

fn hex_lower(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

impl DomainCli for CanCli {
    fn command() -> clap::Command {
        clap::Command::new("can")
            .about("CAN / CAN FD through the Loom daemon (M1: sim bus); the user sees it live in the Loom UI")
            .subcommand_required(true)
            .subcommand(
                clap::Command::new("send")
                    .about("Send a frame or a short burst (cansend syntax: 300#01, 123##1AABB, 300#R)")
                    .arg(clap::Arg::new("frame").required(true).value_name("FRAME"))
                    .arg(channel_arg())
                    .arg(
                        clap::Arg::new("ext")
                            .long("ext")
                            .action(clap::ArgAction::SetTrue)
                            .help("29-bit id even if short"),
                    )
                    .arg(
                        clap::Arg::new("count")
                            .long("count")
                            .value_parser(clap::value_parser!(u32).range(1..=i64::from(MAX_SEND_COUNT)))
                            .help("Frames to send (finite burst)"),
                    )
                    .arg(
                        clap::Arg::new("gap")
                            .long("gap")
                            .value_name("MS")
                            .value_parser(clap::value_parser!(u32).range(0..=i64::from(MAX_GAP_MS)))
                            .help("Gap between burst frames"),
                    )
                    .arg(
                        clap::Arg::new("expect")
                            .long("expect")
                            .value_name("ID")
                            .value_parser(parse_id)
                            .help("Wait for this response id (device frame) → effective_verified"),
                    )
                    .arg(ms_arg(
                        "within",
                        MAX_EXPECT_MS,
                        "Deadline for --expect (default 100)",
                    ))
                    .arg(
                        clap::Arg::new("dry-run")
                            .long("dry-run")
                            .action(clap::ArgAction::SetTrue)
                            .help("Show risk and bytes only; send nothing"),
                    ),
            )
            .subcommand(
                clap::Command::new("trace")
                    .about("Bounded trace read (frames and send rows)")
                    .arg(channel_arg())
                    .arg(cursor_arg("since"))
                    .arg(cursor_arg("until"))
                    .arg(
                        clap::Arg::new("ids")
                            .long("ids")
                            .value_delimiter(',')
                            .value_parser(parse_id_range)
                            .help("Ids or ranges: 0x300,0x7E0-0x7EF"),
                    )
                    .arg(
                        clap::Arg::new("origin")
                            .long("origin")
                            .value_delimiter(',')
                            .value_parser(parse_origin)
                            .help("agent,user,device,script"),
                    )
                    .arg(clap::Arg::new("dir").long("dir").value_parser(["rx", "tx"]))
                    .arg(
                        clap::Arg::new("max")
                            .long("max")
                            .value_parser(clap::value_parser!(u32).range(1..=1000))
                            .help("Rows (default 100)"),
                    )
                    .arg(
                        clap::Arg::new("peek")
                            .long("peek")
                            .action(clap::ArgAction::SetTrue)
                            .help("Do not advance @last"),
                    )
                    .arg(decode_arg()),
            )
            .subcommand(
                clap::Command::new("last")
                    .about("Last frame per id in a recent window (Fixed view)")
                    .arg(channel_arg())
                    .arg(clap::Arg::new("id").long("id").value_parser(parse_id_range))
                    .arg(ms_arg("window", 60_000, "Look-back window (default 10000)"))
                    .arg(decode_arg()),
            )
            .subcommand(
                clap::Command::new("expect")
                    .about("Wait for a matching frame; exit 3 on timeout. Result carries evidence refs")
                    .arg(channel_arg())
                    .arg(
                        clap::Arg::new("id")
                            .long("id")
                            .required(true)
                            .value_parser(parse_id),
                    )
                    .arg(clap::Arg::new("ext").long("ext").action(clap::ArgAction::SetTrue))
                    .arg(clap::Arg::new("data").long("data").value_parser(parse_hex_arg))
                    .arg(clap::Arg::new("mask").long("mask").value_parser(parse_hex_arg))
                    .arg(
                        clap::Arg::new("origin")
                            .long("origin")
                            .value_parser(parse_origin)
                            .help("Default device"),
                    )
                    .arg(cursor_arg("after"))
                    .arg(ms_arg("within", MAX_EXPECT_MS, "Deadline (required)").required(true)),
            )
            .subcommand(
                clap::Command::new("status")
                    .about("Channels, generation, drops, STOP latch, fake ECU nodes")
                    .arg(channel_arg()),
            )
            .subcommand(
                clap::Command::new("db")
                    .about("DBC of a channel (signals, decoding)")
                    .subcommand_required(true)
                    .subcommand(
                        clap::Command::new("load")
                            .about("Load a DBC file for a channel (replaces the channel's DBC)")
                            .arg(clap::Arg::new("path").required(true).value_name("FILE.dbc"))
                            .arg(channel_arg())
                            .arg(
                                clap::Arg::new("encoding")
                                    .long("encoding")
                                    .value_parser(["utf8", "windows1252", "auto"])
                                    .help("Text encoding (default auto: UTF-8, else Windows-1252)"),
                            ),
                    )
                    .subcommand(
                        clap::Command::new("info")
                            .about("Loaded DBC per channel (sha256, counts, warnings, blocked messages)")
                            .arg(channel_arg()),
                    ),
            )
            .subcommand(isotp_command())
            .subcommand(periodic_command())
            .subcommand(
                clap::Command::new("sig")
                    .about("DBC signal values (needs `loom can db load`)")
                    .subcommand_required(true)
                    .subcommand(
                        clap::Command::new("get")
                            .about("Newest value of a signal, with age and evidence ref (exit 3 if none)")
                            .arg(
                                clap::Arg::new("signal")
                                    .required(true)
                                    .value_name("Message.Signal"),
                            )
                            .arg(channel_arg())
                            .arg(ms_arg(
                                "window",
                                SIG_MAX_WINDOW_MS,
                                "Look-back window (default 10000)",
                            ))
                            .arg(
                                clap::Arg::new("origin")
                                    .long("origin")
                                    .value_parser(parse_origin)
                                    .help("Only frames of this origin (default any)"),
                            ),
                    )
                    .subcommand(
                        clap::Command::new("set")
                            .about("IG: change one signal of the running periodic job of its message")
                            .arg(
                                clap::Arg::new("assign")
                                    .required(true)
                                    .value_name("Msg.Sig=VALUE")
                                    .value_parser(parse_assign),
                            )
                            .arg(channel_arg())
                            .arg(
                                clap::Arg::new("job")
                                    .long("job")
                                    .help("Job to change (default: the message's job)"),
                            )
                            .arg(
                                clap::Arg::new("force-range")
                                    .long("force-range")
                                    .action(clap::ArgAction::SetTrue)
                                    .help("Skip the DBC min/max check"),
                            ),
                    )
                    .subcommand(
                        clap::Command::new("wait-for")
                            .about("Wait until a new sample satisfies the condition; exit 3 on timeout")
                            .arg(
                                clap::Arg::new("expr")
                                    .required(true)
                                    .value_name("EXPR")
                                    .help("`Sig > 3000` (> >= < <= == !=) or `Sig between 10 20`"),
                            )
                            .arg(channel_arg())
                            .arg(cursor_arg("after"))
                            .arg(
                                clap::Arg::new("timeout")
                                    .long("timeout")
                                    .required(true)
                                    .value_name("S")
                                    .value_parser(parse_timeout)
                                    .help("Deadline: seconds (`10`, `1.5`) or `500ms`, `2m`; at most 1 h"),
                            )
                            .arg(
                                clap::Arg::new("origin")
                                    .long("origin")
                                    .value_parser(parse_origin)
                                    .help("Default device"),
                            )
                            .arg(
                                clap::Arg::new("any-origin")
                                    .long("any-origin")
                                    .action(clap::ArgAction::SetTrue)
                                    .conflicts_with("origin")
                                    .help("Count frames of any origin (also your own)"),
                            ),
                    ),
            )
    }

    fn to_call(m: &clap::ArgMatches) -> Result<RpcCall, UsageError> {
        let mut p = Map::new();
        let Some((top, group)) = m.subcommand() else {
            return Err(UsageError::new("expected send|trace|last|expect|status|db|sig")
                .with_hint("loom can help --json"));
        };
        let (sub, s) = match top {
            "db" | "sig" | "isotp" | "periodic" => {
                let (leaf, ls) = group
                    .subcommand()
                    .ok_or_else(|| UsageError::new(format!("expected `loom can {top} <command>`")))?;
                (format!("{top} {leaf}"), ls)
            }
            _ => (top.to_owned(), group),
        };
        set(
            &mut p,
            "channel",
            s.try_get_one::<String>("channel")
                .ok()
                .flatten()
                .map(|c| json!(c)),
        );
        match sub.as_str() {
            "send" => {
                let frame: &String = s
                    .get_one("frame")
                    .ok_or_else(|| UsageError::new("missing FRAME"))?;
                let ext = s.get_flag("ext");
                loom_can_frame::parse_frame(frame, ext).map_err(|e| {
                    UsageError::new(e.to_string()).with_hint("examples: 300#01, 123##1AABB, 300#R")
                })?;
                p.insert("frame".into(), json!(frame));
                if ext {
                    p.insert("ext".into(), json!(true));
                }
                set(&mut p, "count", s.get_one::<u32>("count").map(|v| json!(v)));
                set(&mut p, "gap_ms", s.get_one::<u32>("gap").map(|v| json!(v)));
                let within = s.get_one::<u32>("within").copied();
                let mut wait = None;
                match (s.get_one::<u32>("expect"), within) {
                    (Some(id), w) => {
                        let w = w.unwrap_or(DEFAULT_SEND_EXPECT_MS);
                        p.insert(
                            "expect".into(),
                            json!({"id": id, "ext": *id > loom_can_frame::MAX_STANDARD_ID, "within_ms": w}),
                        );
                        wait = Some(Duration::from_millis(u64::from(w)) + WAIT_MARGIN);
                    }
                    (None, Some(_)) => return Err(UsageError::new("--within needs --expect ID")),
                    (None, None) => {}
                }
                if s.get_flag("dry-run") {
                    p.insert("dry_run".into(), json!(true));
                }
                let mut call = RpcCall::new(RPC_SEND, Value::Object(p));
                call.wait = wait;
                Ok(call)
            }
            "trace" => {
                set(&mut p, "since", cursor_param(s, "since").map(Value::from));
                set(&mut p, "until", cursor_param(s, "until").map(Value::from));
                if let Some(ids) = s.get_many::<String>("ids") {
                    p.insert("ids".into(), json!(ids.collect::<Vec<_>>()));
                }
                if let Some(o) = s.get_many::<String>("origin") {
                    p.insert("origins".into(), json!(o.collect::<Vec<_>>()));
                }
                set(&mut p, "dir", s.get_one::<String>("dir").map(|d| json!(d)));
                set(&mut p, "max", s.get_one::<u32>("max").map(|v| json!(v)));
                if s.get_flag("peek") {
                    p.insert("peek".into(), json!(true));
                }
                if s.get_flag("decode") {
                    p.insert("decode".into(), json!(true));
                }
                Ok(RpcCall::new(RPC_TRACE_QUERY, Value::Object(p)))
            }
            "last" => {
                set(&mut p, "id", s.get_one::<String>("id").map(|v| json!(v)));
                set(&mut p, "window_ms", s.get_one::<u32>("window").map(|v| json!(v)));
                if s.get_flag("decode") {
                    p.insert("decode".into(), json!(true));
                }
                Ok(RpcCall::new(RPC_LAST, Value::Object(p)))
            }
            "expect" => {
                let id = *s
                    .get_one::<u32>("id")
                    .ok_or_else(|| UsageError::new("--id is required"))?;
                let within = *s
                    .get_one::<u32>("within")
                    .ok_or_else(|| UsageError::new("--within is required"))?;
                p.insert("id".into(), json!(id));
                if s.get_flag("ext") {
                    p.insert("ext".into(), json!(true));
                }
                let data = s.get_one::<String>("data");
                let mask = s.get_one::<String>("mask");
                match (data, mask) {
                    (None, Some(_)) => return Err(UsageError::new("--mask needs --data")),
                    (Some(d), Some(m)) if d.len() != m.len() => {
                        return Err(UsageError::new("--mask must have the same length as --data"));
                    }
                    _ => {}
                }
                set(&mut p, "data", data.map(|v| json!(v)));
                set(&mut p, "mask", mask.map(|v| json!(v)));
                set(&mut p, "origin", s.get_one::<String>("origin").map(|v| json!(v)));
                set(&mut p, "after", cursor_param(s, "after").map(Value::from));
                p.insert("within_ms".into(), json!(within));
                let mut call = RpcCall::new(RPC_EXPECT, Value::Object(p));
                call.wait = Some(Duration::from_millis(u64::from(within)) + WAIT_MARGIN);
                Ok(call)
            }
            "status" => Ok(RpcCall::new(RPC_STATUS, Value::Object(p))),
            "db load" => {
                let path: &String = s
                    .get_one("path")
                    .ok_or_else(|| UsageError::new("missing FILE.dbc"))?;
                // The daemon has its own working directory: send an absolute path.
                let abs = std::path::absolute(path)
                    .map_err(|e| UsageError::new(format!("bad path {path:?}: {e}")))?;
                let abs = abs
                    .to_str()
                    .ok_or_else(|| UsageError::new(format!("path is not valid UTF-8: {path:?}")))?;
                p.insert("path".into(), json!(abs));
                set(
                    &mut p,
                    "encoding",
                    s.get_one::<String>("encoding").map(|v| json!(v)),
                );
                let mut call = RpcCall::new(RPC_DB_LOAD, Value::Object(p));
                call.wait = Some(Duration::from_secs(30) + WAIT_MARGIN);
                Ok(call)
            }
            "db info" => Ok(RpcCall::new(RPC_DB_INFO, Value::Object(p))),
            "sig get" => {
                let sig: &String = s
                    .get_one("signal")
                    .ok_or_else(|| UsageError::new("missing SIGNAL"))?;
                p.insert("signal".into(), json!(sig));
                set(&mut p, "window_ms", s.get_one::<u32>("window").map(|v| json!(v)));
                set(&mut p, "origin", s.get_one::<String>("origin").map(|v| json!(v)));
                Ok(RpcCall::new(RPC_SIG_GET, Value::Object(p)))
            }
            "isotp open" => {
                let id = |k: &str| s.get_one::<u32>(k).copied();
                p.insert("tx_id".into(), json!(id("tx")));
                p.insert("rx_id".into(), json!(id("rx")));
                for flag in ["ext", "fd", "brs"] {
                    if s.get_flag(flag) {
                        p.insert(flag.into(), json!(true));
                    }
                }
                set(&mut p, "tx_dl", s.get_one::<u8>("tx-dl").map(|v| json!(v)));
                if let Some(pad) = s.get_one::<String>("padding") {
                    p.insert("padding".into(), padding_value(pad)?);
                }
                set(&mut p, "block_size", s.get_one::<u8>("bs").map(|v| json!(v)));
                set(&mut p, "st_min", s.get_one::<u8>("stmin").map(|v| json!(v)));
                set(&mut p, "n_bs_ms", s.get_one::<u32>("n-bs").map(|v| json!(v)));
                set(&mut p, "n_cr_ms", s.get_one::<u32>("n-cr").map(|v| json!(v)));
                if s.get_flag("server") {
                    p.insert("role".into(), json!("server"));
                }
                set(&mut p, "model", s.get_one::<String>("model").map(|v| json!(v)));
                set(&mut p, "owner", s.get_one::<String>("owner").map(|v| json!(v)));
                Ok(RpcCall::new(RPC_ISOTP_OPEN, Value::Object(p)))
            }
            "isotp send" => {
                let ep = s
                    .get_one::<String>("endpoint")
                    .ok_or_else(|| UsageError::new("missing ENDPOINT"))?;
                let pdu = s
                    .get_one::<String>("pdu")
                    .ok_or_else(|| UsageError::new("missing PDU"))?;
                let bytes = crate::isotp::parse_pdu(pdu)
                    .map_err(|e| UsageError::new(e).with_hint("loom can isotp send tp1 22F190"))?;
                p.insert("endpoint".into(), json!(ep));
                p.insert("pdu".into(), json!(hex_lower(&bytes)));
                Ok(RpcCall::new(RPC_ISOTP_SEND, Value::Object(p)))
            }
            "isotp cancel" => {
                p.insert("endpoint".into(), json!(s.get_one::<String>("endpoint")));
                p.insert("transfer".into(), json!(s.get_one::<String>("transfer")));
                Ok(RpcCall::new(RPC_ISOTP_CANCEL, Value::Object(p)))
            }
            "isotp recv" | "isotp barrier" => {
                p.insert("endpoint".into(), json!(s.get_one::<String>("endpoint")));
                let timeout = s.get_one::<u32>("timeout").copied();
                set(&mut p, "timeout_ms", timeout.map(|v| json!(v)));
                let method = if sub == "isotp recv" {
                    set(
                        &mut p,
                        "after_pdu_seq",
                        s.get_one::<u64>("after").map(|v| json!(v.to_string())),
                    );
                    set(&mut p, "max_pdus", s.get_one::<u64>("max").map(|v| json!(v)));
                    RPC_ISOTP_RECV
                } else {
                    p.insert(
                        "epoch".into(),
                        json!(s.get_one::<u64>("epoch").map(u64::to_string)),
                    );
                    p.insert(
                        "deadline_mono_ns".into(),
                        json!(s.get_one::<u64>("deadline-ns").map(u64::to_string)),
                    );
                    RPC_ISOTP_BARRIER
                };
                let mut call = RpcCall::new(method, Value::Object(p));
                call.wait = Some(Duration::from_millis(u64::from(timeout.unwrap_or(0))) + WAIT_MARGIN);
                Ok(call)
            }
            "isotp abort" | "isotp close" => {
                let ep = s
                    .get_one::<String>("endpoint")
                    .ok_or_else(|| UsageError::new("missing ENDPOINT"))?;
                p.insert("endpoint".into(), json!(ep));
                let m = if sub == "isotp abort" {
                    RPC_ISOTP_ABORT
                } else {
                    RPC_ISOTP_CLOSE
                };
                Ok(RpcCall::new(m, Value::Object(p)))
            }
            "isotp list" => Ok(RpcCall::new(RPC_ISOTP_LIST, Value::Object(p))),
            "periodic start" => {
                let frame = s.get_one::<String>("frame");
                let msg = s.get_one::<String>("msg");
                let sets: Vec<&(String, Value)> =
                    s.get_many("set").map(Iterator::collect).unwrap_or_default();
                match (frame, msg) {
                    (Some(f), None) => {
                        if !sets.is_empty() {
                            return Err(UsageError::new("--set needs --msg"));
                        }
                        let ext = s.get_flag("ext");
                        loom_can_frame::parse_frame(f, ext).map_err(|e| {
                            UsageError::new(e.to_string()).with_hint("examples: 300#01, 123##1AABB")
                        })?;
                        if s.get_one::<u32>("period").is_none() {
                            return Err(UsageError::new("--period is required for a raw frame"));
                        }
                        p.insert("frame".into(), json!(f));
                        if ext {
                            p.insert("ext".into(), json!(true));
                        }
                    }
                    (None, Some(m)) => {
                        p.insert("msg".into(), json!(m));
                        let mut sig = Map::new();
                        for (k, v) in sets {
                            if sig.insert(k.clone(), v.clone()).is_some() {
                                return Err(UsageError::new(format!("--set {k} given twice")));
                            }
                        }
                        if !sig.is_empty() {
                            p.insert("signals".into(), Value::Object(sig));
                        }
                    }
                    _ => {
                        return Err(UsageError::new("give a FRAME or --msg MESSAGE")
                            .with_hint("loom can periodic start 300#01 --period 100"));
                    }
                }
                set(&mut p, "period_ms", s.get_one::<u32>("period").map(|v| json!(v)));
                set(&mut p, "offset_ms", s.get_one::<u32>("offset").map(|v| json!(v)));
                set(&mut p, "count", s.get_one::<u32>("count").map(|v| json!(v)));
                set(&mut p, "lease_ms", s.get_one::<u64>("lease").map(|v| json!(v)));
                set(
                    &mut p,
                    "max_duration_ms",
                    s.get_one::<u64>("max-duration").map(|v| json!(v)),
                );
                set(&mut p, "owner", s.get_one::<String>("owner").map(|v| json!(v)));
                if s.get_flag("force-range") {
                    p.insert("force_range".into(), json!(true));
                }
                Ok(RpcCall::new(RPC_PERIODIC_START, Value::Object(p)))
            }
            "periodic update" => {
                p.insert("job".into(), json!(s.get_one::<String>("job")));
                p.insert("expected_rev".into(), json!(s.get_one::<u64>("rev")));
                let patch = match (
                    s.get_one::<String>("data"),
                    s.get_one::<String>("mask"),
                    s.get_one::<String>("bytes"),
                ) {
                    (Some(d), None, None) => json!({"data": d}),
                    (None, Some(m), Some(b)) if m.len() == b.len() => json!({"mask": m, "bytes": b}),
                    (None, Some(_), Some(_)) => {
                        return Err(UsageError::new("--mask and --bytes must have the same length"));
                    }
                    _ => return Err(UsageError::new("give --data HEX, or --mask HEX with --bytes HEX")),
                };
                p.insert("patch".into(), patch);
                Ok(RpcCall::new(RPC_PERIODIC_UPDATE, Value::Object(p)))
            }
            "periodic stop" => {
                match (s.get_one::<String>("job"), s.get_flag("mine")) {
                    (Some(j), false) => p.insert("job".into(), json!(j)),
                    (None, true) => p.insert("all_mine".into(), json!(true)),
                    _ => return Err(UsageError::new("give a JOB or --mine")),
                };
                Ok(RpcCall::new(RPC_PERIODIC_STOP, Value::Object(p)))
            }
            "periodic renew" | "periodic takeover" => {
                p.insert("job".into(), json!(s.get_one::<String>("job")));
                let m = if sub == "periodic renew" {
                    RPC_PERIODIC_RENEW
                } else {
                    RPC_PERIODIC_TAKEOVER
                };
                Ok(RpcCall::new(m, Value::Object(p)))
            }
            "periodic list" => Ok(RpcCall::new(RPC_PERIODIC_LIST, Value::Object(p))),
            "sig set" => {
                let (sig, value) = s
                    .get_one::<(String, Value)>("assign")
                    .cloned()
                    .ok_or_else(|| UsageError::new("missing Msg.Sig=VALUE"))?;
                p.insert("signal".into(), json!(sig));
                p.insert("value".into(), value);
                set(&mut p, "job", s.get_one::<String>("job").map(|v| json!(v)));
                if s.get_flag("force-range") {
                    p.insert("force_range".into(), json!(true));
                }
                Ok(RpcCall::new(RPC_SIG_SET, Value::Object(p)))
            }
            "sig wait-for" => {
                let expr: &String = s.get_one("expr").ok_or_else(|| UsageError::new("missing EXPR"))?;
                let cond = parse_condition(expr).map_err(|e| {
                    UsageError::new(e).with_hint(r#"loom can sig wait-for "EngRPM > 3000" --timeout 10"#)
                })?;
                let within = *s
                    .get_one::<u32>("timeout")
                    .ok_or_else(|| UsageError::new("--timeout is required"))?;
                p.insert("expr".into(), json!(cond.text()));
                set(&mut p, "after", cursor_param(s, "after").map(Value::from));
                p.insert("within_ms".into(), json!(within));
                set(&mut p, "origin", s.get_one::<String>("origin").map(|v| json!(v)));
                if s.get_flag("any-origin") {
                    p.insert("any_origin".into(), json!(true));
                }
                let mut call = RpcCall::new(RPC_SIG_WAIT_FOR, Value::Object(p));
                call.wait = Some(Duration::from_millis(u64::from(within)) + WAIT_MARGIN);
                Ok(call)
            }
            other => Err(UsageError::new(format!("unknown subcommand {other}"))),
        }
    }

    fn render_human(call: &RpcCall, env: &Envelope<Value>, out: &mut dyn io::Write) -> io::Result<()> {
        if let Some(d) = &env.data {
            match call.method.as_str() {
                RPC_SEND => render_send(d, out)?,
                RPC_TRACE_QUERY => render_trace(d, out)?,
                RPC_LAST => render_last(d, out)?,
                RPC_EXPECT => render_expect(d, out)?,
                RPC_DB_LOAD | RPC_DB_INFO => render_db(d, out)?,
                RPC_SIG_GET => render_sig_get(d, out)?,
                RPC_SIG_WAIT_FOR => render_sig_wait(d, out)?,
                RPC_PERIODIC_START
                | RPC_PERIODIC_UPDATE
                | RPC_PERIODIC_STOP
                | RPC_PERIODIC_RENEW
                | RPC_PERIODIC_TAKEOVER
                | RPC_PERIODIC_LIST => render_periodic(call, d, out)?,
                RPC_SIG_SET => render_sig_set(d, out)?,
                RPC_ISOTP_OPEN | RPC_ISOTP_SEND | RPC_ISOTP_ABORT | RPC_ISOTP_CLOSE | RPC_ISOTP_LIST
                | RPC_ISOTP_CANCEL | RPC_ISOTP_RECV | RPC_ISOTP_BARRIER => {
                    render_isotp(call.method.as_str(), d, out)?
                }
                _ => writeln!(out, "{}", serde_json::to_string_pretty(d).unwrap_or_default())?,
            }
        }
        if let Some(e) = &env.error {
            // A wait timeout still cites what was scanned (contract 07 §7.2).
            for r in e.detail["refs"].as_array().into_iter().flatten() {
                writeln!(out, "scanned: {}", r.as_str().unwrap_or_default())?;
            }
            let last = &e.detail["last"];
            if last.is_object() {
                writeln!(
                    out,
                    "last: {} = {}  seq:{}",
                    last["name"].as_str().unwrap_or("?"),
                    sig_value(last),
                    last["seq"].as_str().unwrap_or("?")
                )?;
            }
        }
        render_common(env, out)
    }
}

// ---- human rendering ------------------------------------------------------------------------

fn origin_tag(rec: &Value) -> &'static str {
    if rec["external_actor"].is_string() {
        return "EXT";
    }
    match rec["origin"].as_str() {
        Some("agent") => "AGT",
        Some("user") => "USR",
        Some("device") => "DEV",
        Some("script") => "SCR",
        _ => "???",
    }
}

fn id_text(f: &Value) -> String {
    let id = f["id"].as_u64().unwrap_or(0);
    if f["ext"].as_bool().unwrap_or(false) {
        format!("0x{id:08X}")
    } else {
        format!("0x{id:03X}")
    }
}

fn data_text(f: &Value) -> String {
    if f["rtr"].as_bool().unwrap_or(false) {
        return "R".into();
    }
    let h = f["bytes"]["data"].as_str().unwrap_or_default().to_uppercase();
    h.as_bytes()
        .chunks(2)
        .map(|c| String::from_utf8_lossy(c).into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

fn secs(rec: &Value) -> String {
    let ns: u64 = rec["ts"]["mono_ns"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    format!("{}.{:03}", ns / 1_000_000_000, (ns / 1_000_000) % 1_000)
}

/// One Trace line: `812.004  tx AGT 0x300   1  01  sent`.
fn trace_line(rec: &Value, frame: &Value, state: Option<&str>) -> String {
    let dir = match (rec["dir"].as_str(), rec["kind"].as_str()) {
        (Some(d), _) => d,
        (None, Some("can.tx")) => "tx",
        _ => "--",
    };
    let fd = if frame["fd"].as_bool().unwrap_or(false) {
        " FD"
    } else {
        ""
    };
    let mut line = format!(
        "{:>10}  {dir} {} {:<10} {:>2}{fd}  {}",
        secs(rec),
        origin_tag(rec),
        id_text(frame),
        frame["len"].as_u64().unwrap_or(0),
        data_text(frame)
    );
    if let Some(s) = state {
        line.push_str("  ");
        line.push_str(s);
    }
    line.trim_end().to_owned()
}

fn render_trace(d: &Value, out: &mut dyn io::Write) -> io::Result<()> {
    let rows = d["rows"].as_array().cloned().unwrap_or_default();
    // Merge a send's write-state row into its echo frame line (one line per request, like the UI).
    let mut state: HashMap<String, String> = HashMap::new();
    let mut echoed: HashMap<String, bool> = HashMap::new();
    for r in &rows {
        if let Some(rid) = r["request_id"].as_str() {
            match r["kind"].as_str() {
                Some("can.tx") => {
                    if let Some(s) = r["payload"]["write_state"].as_str() {
                        state.insert(rid.to_owned(), s.to_owned());
                    }
                }
                Some("can.frame") => {
                    echoed.insert(rid.to_owned(), true);
                }
                _ => {}
            }
        }
    }
    let mut printed = 0usize;
    for r in &rows {
        let rid = r["request_id"].as_str();
        match r["kind"].as_str() {
            Some("can.frame") => {
                writeln!(
                    out,
                    "{}",
                    trace_line(
                        r,
                        &r["payload"],
                        rid.and_then(|k| state.get(k)).map(String::as_str)
                    )
                )?;
                decoded_line(&r["decoded"], out)?;
                printed += 1;
            }
            Some("can.tx") => {
                // Only requests whose echo is not in this page get their own line (last state).
                let Some(k) = rid else { continue };
                if echoed.get(k).copied().unwrap_or(false) {
                    continue;
                }
                let last = state.get(k).map(String::as_str);
                if r["payload"]["write_state"].as_str() != last {
                    continue;
                }
                writeln!(out, "{}", trace_line(r, &r["payload"]["frame"], last))?;
                echoed.insert(k.to_owned(), true);
                printed += 1;
            }
            _ => {}
        }
    }
    let mut summary = format!("-- {printed} rows");
    if let Some(c) = d["next_cursor"].as_str() {
        summary.push_str(&format!(" · next {c}"));
    }
    if d["truncated"].as_bool().unwrap_or(false) {
        summary.push_str(" · truncated (narrow --ids/--since or continue with the cursor)");
    }
    if d["complete"].as_bool() == Some(false) {
        summary.push_str(" · INCOMPLETE (gap in the range)");
    }
    writeln!(out, "{summary}")
}

fn render_send(d: &Value, out: &mut dyn io::Write) -> io::Result<()> {
    let risk = format!(
        "risk={} ({})",
        d["risk"].as_str().unwrap_or("?"),
        d["reason"].as_str().unwrap_or("")
    );
    let ch = d["channel"].as_str().unwrap_or("?");
    let frame = d["frame"].as_str().unwrap_or("?");
    if d["dry_run"].as_bool().unwrap_or(false) {
        return writeln!(
            out,
            "DRY-RUN {ch} {frame} ×{}  bytes {}  {risk}  policy={}  (nothing sent)",
            d["count"].as_u64().unwrap_or(1),
            d["bytes"].as_str().unwrap_or(""),
            d["policy"].as_str().unwrap_or("auto")
        );
    }
    let st = d["write_state"]
        .as_str()
        .or_else(|| d["state"].as_str())
        .unwrap_or("sent");
    writeln!(
        out,
        "{st} {ch} {frame}  request {}  {risk}",
        d["request_id"].as_str().unwrap_or("?")
    )?;
    let e = &d["expect"];
    if e.is_object() {
        if e["matched"].as_bool().unwrap_or(false) {
            let f = &e["frame"]["payload"];
            writeln!(
                out,
                "expect: {} {} [{}] {}  seq:{}",
                origin_tag(&e["frame"]),
                id_text(f),
                f["len"].as_u64().unwrap_or(0),
                data_text(f),
                e["frame"]["seq"].as_str().unwrap_or("?")
            )?;
        } else {
            writeln!(
                out,
                "expect: no response within {} ms → effect_unverified",
                e["within_ms"].as_u64().unwrap_or(0)
            )?;
        }
        for r in e["refs"].as_array().into_iter().flatten() {
            writeln!(out, "evidence: {}", r.as_str().unwrap_or_default())?;
        }
    }
    Ok(())
}

fn render_last(d: &Value, out: &mut dyn io::Write) -> io::Result<()> {
    let rows = d["rows"].as_array().cloned().unwrap_or_default();
    for r in &rows {
        let last = &r["last"];
        let f = &last["payload"];
        writeln!(
            out,
            "{:<10} {:>2}  {:<24} ×{:<6} {}  seq:{}",
            id_text(f),
            f["len"].as_u64().unwrap_or(0),
            data_text(f),
            r["count"].as_u64().unwrap_or(0),
            origin_tag(last),
            last["seq"].as_str().unwrap_or("?")
        )?;
        decoded_line(&last["decoded"], out)?;
    }
    let mut s = format!(
        "-- {} ids in the last {} ms",
        rows.len(),
        d["window_ms"].as_u64().unwrap_or(0)
    );
    if d["truncated"].as_bool().unwrap_or(false) {
        s.push_str(" · truncated");
    }
    if d["complete"].as_bool() == Some(false) {
        s.push_str(" · INCOMPLETE");
    }
    writeln!(out, "{s}")
}

fn render_expect(d: &Value, out: &mut dyn io::Write) -> io::Result<()> {
    let rec = &d["frame"];
    let f = &rec["payload"];
    writeln!(
        out,
        "MATCH {} {} [{}] {}  seq:{}",
        origin_tag(rec),
        id_text(f),
        f["len"].as_u64().unwrap_or(0),
        data_text(f),
        rec["seq"].as_str().unwrap_or("?")
    )?;
    for r in d["refs"].as_array().into_iter().flatten() {
        writeln!(out, "evidence: {}", r.as_str().unwrap_or_default())?;
    }
    Ok(())
}

// ---- signals --------------------------------------------------------------------------------

/// Display number: at most 6 decimals, no trailing zeros (JSON keeps the full value).
fn num_text(x: f64) -> String {
    let t = format!("{x:.6}");
    let t = t.trim_end_matches('0').trim_end_matches('.');
    if t == "-0" { "0".into() } else { t.to_owned() }
}

/// `37.25 km/h`, `2 "N"`.
fn sig_value(v: &Value) -> String {
    let mut s = v["value"].as_f64().map_or_else(|| "?".to_owned(), num_text);
    if let Some(u) = v["unit"].as_str().filter(|u| !u.is_empty()) {
        s.push(' ');
        s.push_str(u);
    }
    if let Some(l) = v["label"].as_str() {
        s.push_str(&format!(" \"{l}\""));
    }
    s
}

/// Indented line under a frame: `EngineData: EngRPM=3200 rpm, Gear=2 "N"`.
fn decoded_line(d: &Value, out: &mut dyn io::Write) -> io::Result<()> {
    if !d.is_object() {
        return Ok(());
    }
    let msg = d["message"].as_str().unwrap_or("?");
    if let Some(e) = d["error"].as_str() {
        return writeln!(out, "            {msg}: (not decoded: {e})");
    }
    let sigs: Vec<String> = d["signals"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|s| {
            let name = s["name"].as_str().unwrap_or("?");
            if s["unavailable"].is_string() {
                format!("{name}=n/a")
            } else {
                format!("{name}={}", sig_value(s))
            }
        })
        .collect();
    let mut line = format!("            {msg}: {}", sigs.join(", "));
    if d["length_mismatch"].is_object() {
        line.push_str("  (length differs from DBC)");
    }
    writeln!(out, "{line}")
}

fn render_db(d: &Value, out: &mut dyn io::Write) -> io::Result<()> {
    let list: Vec<&Value> = match d["channels"].as_array() {
        Some(a) => a.iter().collect(),
        None => vec![d],
    };
    if list.is_empty() {
        return writeln!(out, "no DBC loaded (loom can db load <file.dbc>)");
    }
    for c in list {
        let ch = c["channel"].as_str().unwrap_or("?");
        if c["loaded"].as_bool() != Some(true) {
            writeln!(out, "{ch}: no DBC loaded")?;
            continue;
        }
        let sha = c["db_sha256"].as_str().unwrap_or("");
        writeln!(
            out,
            "{ch}: {}  sha256 {}…  {} messages, {} signals{}",
            c["path"].as_str().unwrap_or("?"),
            sha.get(..12).unwrap_or(sha),
            c["messages"].as_u64().unwrap_or(0),
            c["signals"].as_u64().unwrap_or(0),
            if c["encoding_guessed"].as_bool() == Some(true) {
                format!("  (encoding guessed: {})", c["encoding"].as_str().unwrap_or("?"))
            } else {
                String::new()
            }
        )?;
        for w in c["warnings"].as_array().into_iter().flatten() {
            let line = w["line"]
                .as_u64()
                .map(|l| format!("line {l}: "))
                .unwrap_or_default();
            writeln!(
                out,
                "  warning: {line}{}: {}",
                w["kind"].as_str().unwrap_or("?"),
                w["detail"].as_str().unwrap_or("")
            )?;
        }
        let listed = c["warnings"].as_array().map_or(0, Vec::len) as u64;
        let total = c["warnings_total"].as_u64().unwrap_or(0);
        if total > listed {
            writeln!(out, "  … {} more warnings", total - listed)?;
        }
        for b in c["blocked"].as_array().into_iter().flatten() {
            let why = b["decode"]
                .as_str()
                .map(|r| format!("decode: {r}"))
                .unwrap_or_else(|| format!("encode: {}", b["encode"].as_str().unwrap_or("?")));
            writeln!(
                out,
                "  blocked: {} ({}) {why}",
                b["message"].as_str().unwrap_or("?"),
                b["id"].as_str().unwrap_or("?")
            )?;
        }
    }
    Ok(())
}

/// `VehicleSpeed = 37.25 km/h  (ESP_01 0x0FD, age 18 ms)`.
fn render_sig_get(d: &Value, out: &mut dyn io::Write) -> io::Result<()> {
    let age = d["age_ms"]
        .as_u64()
        .map(|a| format!(", age {a} ms"))
        .unwrap_or_default();
    writeln!(
        out,
        "{} = {}  ({} {}{age})",
        d["name"].as_str().unwrap_or("?"),
        sig_value(d),
        d["message"].as_str().unwrap_or("?"),
        d["id"].as_str().unwrap_or("?")
    )?;
    if d["complete"].as_bool() == Some(false) {
        writeln!(
            out,
            "warning: the window was not read completely; a newer value may exist"
        )?;
    }
    for r in d["refs"].as_array().into_iter().flatten() {
        writeln!(out, "evidence: {}", r.as_str().unwrap_or_default())?;
    }
    Ok(())
}

fn render_sig_wait(d: &Value, out: &mut dyn io::Write) -> io::Result<()> {
    let prev = if d["previous"].is_object() {
        format!(", was {}", sig_value(&d["previous"]))
    } else {
        String::new()
    };
    writeln!(
        out,
        "MATCH {} = {}  ({} {}, seq:{}{prev})",
        d["name"].as_str().unwrap_or("?"),
        sig_value(d),
        d["message"].as_str().unwrap_or("?"),
        d["id"].as_str().unwrap_or("?"),
        d["seq"].as_str().unwrap_or("?")
    )?;
    for r in d["refs"].as_array().into_iter().flatten() {
        writeln!(out, "evidence: {}", r.as_str().unwrap_or_default())?;
    }
    Ok(())
}

// ---- ISO-TP ---------------------------------------------------------------------------------

fn render_isotp(method: &str, d: &Value, out: &mut dyn io::Write) -> io::Result<()> {
    let ep = d["endpoint"].as_str().unwrap_or("?");
    match method {
        RPC_ISOTP_OPEN => writeln!(
            out,
            "{ep} open on {}: tx {} → rx {} ({}, epoch {})",
            d["channel"].as_str().unwrap_or("?"),
            d["tx_id"].as_str().unwrap_or("?"),
            d["rx_id"].as_str().unwrap_or("?"),
            d["role"].as_str().unwrap_or("?"),
            d["epoch"]
        ),
        RPC_ISOTP_SEND => writeln!(
            out,
            "accepted {ep} transfer {} ({} bytes)  request {}",
            d["transfer"].as_str().unwrap_or("?"),
            d["len"],
            d["request_id"].as_str().unwrap_or("?")
        ),
        RPC_ISOTP_ABORT => writeln!(
            out,
            "aborted {ep}: tx={} rx={} residual_tx_possible={} cancelled_frames={}",
            d["aborted_tx"], d["aborted_rx"], d["residual_tx_possible"], d["cancelled_frames"]
        ),
        RPC_ISOTP_CLOSE => writeln!(out, "closed {ep}"),
        RPC_ISOTP_CANCEL => writeln!(
            out,
            "cancelled {ep} transfer {} (residual_tx_possible {})",
            d["transfer"].as_str().unwrap_or("?"),
            d["residual_tx_possible"]
        ),
        RPC_ISOTP_RECV => {
            let pdus = d["pdus"].as_array().cloned().unwrap_or_default();
            for x in &pdus {
                writeln!(
                    out,
                    "#{} {ep} {} ({} B, rx {})",
                    x["pdu_seq"]
                        .as_str()
                        .map_or_else(|| x["pdu_seq"].to_string(), str::to_owned),
                    x["pdu"]["data"].as_str().unwrap_or(""),
                    x["len"],
                    x["rx_id"].as_str().unwrap_or("?")
                )?;
            }
            let text = |v: &Value| v.as_str().map_or_else(|| v.to_string(), str::to_owned);
            let mut line = format!("-- {} PDUs · next {}", pdus.len(), text(&d["next_pdu_seq"]));
            if d["dropped"].as_str().is_some_and(|x| x != "0") || d["dropped"].as_u64().is_some_and(|x| x > 0)
            {
                line.push_str(&format!(" · dropped {}", text(&d["dropped"])));
            }
            if d["cursor_gap"].as_bool() == Some(true) {
                line.push_str(" · CURSOR GAP (older PDUs were dropped)");
            }
            writeln!(out, "{line}")
        }
        RPC_ISOTP_BARRIER => {
            let text = |v: &Value| v.as_str().map_or_else(|| v.to_string(), str::to_owned);
            writeln!(
                out,
                "barrier {ep} epoch {}: {}  covered_ingest_seq={} transport_event_seq={}",
                text(&d["epoch"]),
                if d["complete"].as_bool() == Some(true) {
                    "complete"
                } else {
                    "NOT complete (uncertain)"
                },
                text(&d["covered_ingest_seq"]),
                text(&d["transport_event_seq"])
            )
        }
        _ => {
            let eps = d["endpoints"].as_array().cloned().unwrap_or_default();
            for e in &eps {
                let c = &e["counters"];
                writeln!(
                    out,
                    "{:<6} {:<5} {:<6} {} → {}  {:<7} tx_pdu={} rx_pdu={} tx_err={} rx_err={}  owner {}",
                    e["endpoint"].as_str().unwrap_or("?"),
                    e["channel"].as_str().unwrap_or("?"),
                    e["role"].as_str().unwrap_or("?"),
                    e["tx_id"].as_str().unwrap_or("?"),
                    e["rx_id"].as_str().unwrap_or("?"),
                    e["state"].as_str().unwrap_or("?"),
                    c["tx_pdu"],
                    c["rx_pdu"],
                    c["tx_err"],
                    c["rx_err"],
                    e["owner"].as_str().unwrap_or("-")
                )?;
            }
            writeln!(out, "-- {} ISO-TP endpoints", eps.len())
        }
    }
}

// ---- periodic / IG --------------------------------------------------------------------------

fn can_id_text(id: &Value, ext: &Value) -> String {
    let id = id.as_u64().unwrap_or(0);
    if ext.as_bool().unwrap_or(false) {
        format!("0x{id:08X}")
    } else {
        format!("0x{id:03X}")
    }
}

fn render_periodic(call: &RpcCall, d: &Value, out: &mut dyn io::Write) -> io::Result<()> {
    let job = d["job"].as_str().unwrap_or("?");
    match call.method.as_str() {
        RPC_PERIODIC_START => {
            let src = if d["period_from_dbc"].as_bool() == Some(true) {
                " (DBC cycle time)"
            } else {
                ""
            };
            writeln!(
                out,
                "started {job} on {}: {} every {} ms{src} (rev {})  risk={} ({})",
                d["channel"].as_str().unwrap_or("?"),
                d["frame"].as_str().unwrap_or("?"),
                d["period_ms"],
                d["revision"],
                d["risk"].as_str().unwrap_or("?"),
                d["reason"].as_str().unwrap_or("")
            )
        }
        RPC_PERIODIC_UPDATE => writeln!(out, "patched {job} → rev {}", d["revision"]),
        RPC_PERIODIC_STOP => match d["stopped"].as_array() {
            Some(list) => {
                let ids: Vec<&str> = list
                    .iter()
                    .filter_map(|j| j.as_str().or(j["job"].as_str()))
                    .collect();
                writeln!(out, "stopped {} jobs: {}", ids.len(), ids.join(", "))
            }
            None => writeln!(out, "stopped {job}"),
        },
        RPC_PERIODIC_RENEW => match d["lease_left_ms"].as_u64() {
            Some(ms) => writeln!(out, "renewed {job} (lease {ms} ms left)"),
            None => writeln!(out, "renewed {job}"),
        },
        RPC_PERIODIC_TAKEOVER => writeln!(
            out,
            "took over {} → job {job} (rev {})",
            call.params["job"].as_str().unwrap_or("?"),
            d["revision"]
        ),
        _ => {
            let jobs = d["jobs"].as_array().cloned().unwrap_or_default();
            for j in &jobs {
                writeln!(
                    out,
                    "{:<6} {:<5} {:<10} {:>6} ms  rev {:<3} {:<6} sent={} missed={} p99={}us  {:<8} owner {}",
                    j["job"].as_str().unwrap_or("?"),
                    j["channel"].as_str().unwrap_or("?"),
                    can_id_text(&j["id"], &j["ext"]),
                    // `Value` ignores width: pad the text form.
                    j["period_ms"].to_string(),
                    j["revision"].to_string(),
                    j["origin"].as_str().unwrap_or("?"),
                    j["sent"],
                    j["missed"],
                    j["lateness_p99_us"],
                    j["state"].as_str().unwrap_or("?"),
                    j["owner"].as_str().unwrap_or("-")
                )?;
            }
            writeln!(out, "-- {} periodic jobs", jobs.len())
        }
    }
}

/// `EngRPM = 3000 rpm → j3 rev 2  (bytes 0:00→b8 1:32→0b)`.
fn render_sig_set(d: &Value, out: &mut dyn io::Write) -> io::Result<()> {
    let changed: Vec<String> = d["changed"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| {
            format!(
                "{}:{}→{}",
                c["byte"],
                c["old"].as_str().unwrap_or("?"),
                c["new"].as_str().unwrap_or("?")
            )
        })
        .collect();
    let bytes = if changed.is_empty() {
        "no byte changed".to_owned()
    } else {
        format!("bytes {}", changed.join(" "))
    };
    // A label was given (`Gear=D`): show it as given.
    let shown = match d["value"].as_str() {
        Some(label) => format!("\"{label}\""),
        None => sig_value(d),
    };
    writeln!(
        out,
        "{} = {shown} → {} rev {}  ({bytes})  risk={}",
        d["name"].as_str().unwrap_or("?"),
        d["job"].as_str().unwrap_or("?"),
        d["revision"],
        d["risk"].as_str().unwrap_or("?")
    )?;
    if let Some(w) = d["warning"].as_str() {
        writeln!(out, "warning: {w}")?;
    }
    Ok(())
}
