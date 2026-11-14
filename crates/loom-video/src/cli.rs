//! `loom video` CLI fragment. Pure argument conversion; no FFmpeg, device or media access.

use std::{io, time::Duration};

use clap::{Arg, ArgAction, ArgMatches, Command, value_parser};
use loom_cli_kit::{DomainCli, RpcCall, UsageError, cursor_arg, render_common};
use loom_proto::{cursor::Cursor, envelope::Envelope};
use serde_json::{Value, json};

use crate::model::OpenConfig;

/// Video CLI fragment, registered through `CliOf<VideoCli>`.
#[derive(Debug, Default)]
pub struct VideoCli;

/// Parse `500ms`, `10s`, `2m`, bare seconds.
pub fn parse_duration(s: &str) -> Result<Duration, String> {
    let at = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let (digits, unit) = s.split_at(at);
    let n = digits.parse::<u64>().map_err(|_| format!("invalid duration {s:?}"))?;
    let f = match unit {
        "ms" => 1,
        "" | "s" => 1_000,
        "m" => 60_000,
        _ => return Err(format!("invalid unit in {s:?}")),
    };
    n.checked_mul(f)
        .map(Duration::from_millis)
        .ok_or_else(|| format!("duration {s:?} overflows"))
}

/// Parse `1280x720`.
pub fn parse_size(s: &str) -> Result<(u32, u32), String> {
    let (w, h) = s.split_once(['x', 'X']).ok_or_else(|| format!("expected WxH, got {s:?}"))?;
    Ok((
        w.parse().map_err(|_| format!("bad width in {s:?}"))?,
        h.parse().map_err(|_| format!("bad height in {s:?}"))?,
    ))
}

fn capture() -> Arg {
    Arg::new("capture").long("capture").value_name("ID").help("capture id (default: current)")
}

fn ms(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

fn put_capture(p: &mut Value, s: &ArgMatches) {
    if let Some(c) = s.get_one::<String>("capture") {
        p["capture_id"] = json!(c);
    }
}

impl DomainCli for VideoCli {
    fn command() -> Command {
        let rec = Command::new("record")
            .about("Bounded MJPEG-copy Matroska recording of the current capture")
            .subcommand_required(true)
            .subcommand(Command::new("start").arg(capture()))
            .subcommand(Command::new("stop").arg(capture()));
        Command::new("video")
            .about("Video capture via a daemon-owned FFmpeg (metadata in the journal, frames on the media plane)")
            .subcommand_required(true)
            .subcommand(Command::new("capabilities").about("FFmpeg, implemented/verified sources, storage quota"))
            .subcommand(
                Command::new("sources")
                    .about("Source kinds and replay files; local devices only with --local")
                    .arg(
                        Arg::new("local")
                            .long("local")
                            .action(ArgAction::SetTrue)
                            .help("enumerate AVFoundation devices (may trigger the macOS camera permission prompt)"),
                    ),
            )
            .subcommand(
                Command::new("open")
                    .about("Open one explicitly selected source (no default device, no auto reconnect)")
                    .arg(
                        Arg::new("source")
                            .long("source")
                            .required(true)
                            .value_parser(["http-mjpeg", "replay", "avfoundation"]),
                    )
                    .arg(Arg::new("url").long("url").help("http-mjpeg: loopback http:// URL, no credentials"))
                    .arg(Arg::new("name").long("name").help("replay: file name in $LOOM_HOME/video/replay"))
                    .arg(Arg::new("device").long("device").help("avfoundation: exact device name"))
                    .arg(Arg::new("size").long("size").value_parser(parse_size).help("WxH, default 1280x720"))
                    .arg(Arg::new("fps").long("fps").value_parser(value_parser!(u32)).help("1..60, default 30"))
                    .arg(Arg::new("quality").long("quality").value_parser(value_parser!(u32)).help("JPEG q 2..31")),
            )
            .subcommand(Command::new("status").about("Current/last capture state, health and recording"))
            .subcommand(
                Command::new("frame")
                    .about("Metadata + sha256 of a ring frame (no image bytes)")
                    .arg(capture())
                    .arg(Arg::new("id").long("id").value_parser(value_parser!(u64))),
            )
            .subcommand(
                Command::new("snapshot")
                    .about("Save a ring frame (exact JPEG) as an artifact")
                    .arg(capture())
                    .arg(Arg::new("frame").long("frame").value_parser(value_parser!(u64))),
            )
            .subcommand(rec)
            .subcommand(Command::new("close").about("Stop acquisition and reap FFmpeg").arg(capture()))
            .subcommand(
                Command::new("read")
                    .about("Bounded read of video metadata events")
                    .arg(capture())
                    .arg(
                        Arg::new("kind")
                            .long("kind")
                            .value_parser(["state", "health", "snapshot", "record", "all"]),
                    )
                    .arg(Arg::new("tail").long("tail").value_parser(value_parser!(u64)))
                    .arg(cursor_arg("since"))
                    .arg(Arg::new("follow").long("follow").action(ArgAction::SetTrue))
                    .arg(Arg::new("wait").long("wait").value_parser(parse_duration)),
            )
            .subcommand(
                Command::new("wait")
                    .about("Wait (bounded) for a frame count and/or capture state")
                    .arg(capture())
                    .arg(Arg::new("frames").long("frames").value_parser(value_parser!(u64)))
                    .arg(
                        Arg::new("state")
                            .long("state")
                            .value_parser(["opening", "live", "stalled", "stopping", "closed", "failed"]),
                    )
                    .arg(Arg::new("timeout").long("timeout").value_parser(parse_duration)),
            )
    }

    fn to_call(m: &ArgMatches) -> Result<RpcCall, UsageError> {
        let Some((name, s)) = m.subcommand() else {
            return Err(UsageError::new("expected a subcommand").with_hint("loom video help --json"));
        };
        match name {
            "capabilities" => Ok(RpcCall::new("video.capabilities", json!({}))),
            "sources" => Ok(RpcCall::new("video.sources", json!({"local": s.get_flag("local")}))),
            "status" => Ok(RpcCall::new("video.status", json!({}))),
            "open" => {
                let mut p = json!({"source": s.get_one::<String>("source").cloned()});
                for k in ["url", "name", "device"] {
                    if let Some(v) = s.get_one::<String>(k) {
                        p[k] = json!(v);
                    }
                }
                if let Some((w, h)) = s.get_one::<(u32, u32)>("size") {
                    p["width"] = json!(w);
                    p["height"] = json!(h);
                }
                if let Some(f) = s.get_one::<u32>("fps") {
                    p["fps"] = json!(f);
                }
                if let Some(q) = s.get_one::<u32>("quality") {
                    p["quality"] = json!(q);
                }
                OpenConfig::from_params(&p).map_err(|e| UsageError::new(e.message))?;
                Ok(RpcCall::new("video.open", p))
            }
            "frame" | "snapshot" => {
                let mut p = json!({});
                put_capture(&mut p, s);
                let key = if name == "frame" { "id" } else { "frame" };
                if let Some(f) = s.get_one::<u64>(key) {
                    if *f == 0 {
                        return Err(UsageError::new(format!("--{key} must be ≥ 1")));
                    }
                    p["frame_id"] = json!(f);
                }
                Ok(RpcCall::new(format!("video.{name}"), p))
            }
            "record" => {
                let Some((sub, r)) = s.subcommand() else {
                    return Err(UsageError::new("expected record start|stop"));
                };
                let mut p = json!({});
                put_capture(&mut p, r);
                Ok(RpcCall::new(format!("video.record.{sub}"), p))
            }
            "close" => {
                let mut p = json!({});
                put_capture(&mut p, s);
                Ok(RpcCall::new("video.close", p))
            }
            "read" => {
                let mut p = json!({});
                put_capture(&mut p, s);
                if let Some(k) = s.get_one::<String>("kind") {
                    p["kind"] = json!(k);
                }
                if let Some(t) = s.get_one::<u64>("tail") {
                    if !(1..=1000).contains(t) {
                        return Err(UsageError::new("--tail must be 1..=1000"));
                    }
                    p["tail"] = json!(t);
                }
                if let Some(c) = s.get_one::<Cursor>("since") {
                    p["since"] = json!(c.to_string());
                }
                let follow = s.get_flag("follow");
                let wait = s.get_one::<Duration>("wait").copied();
                if wait.is_some() && !follow {
                    return Err(UsageError::new("--wait needs --follow"));
                }
                let mut c = RpcCall::new("video.read", p);
                if follow {
                    let w = wait.unwrap_or(Duration::from_secs(10));
                    if w.is_zero() || w > Duration::from_secs(30) {
                        return Err(UsageError::new("--wait must be 1ms..=30s"));
                    }
                    c.params["follow"] = json!(true);
                    c.params["wait_ms"] = json!(ms(w));
                    c.wait = Some(w + Duration::from_secs(2));
                }
                Ok(c)
            }
            "wait" => {
                let mut p = json!({});
                put_capture(&mut p, s);
                let frames = s.get_one::<u64>("frames").copied();
                let state = s.get_one::<String>("state").cloned();
                if frames.is_none() && state.is_none() {
                    return Err(UsageError::new("give --frames and/or --state"));
                }
                if let Some(f) = frames {
                    p["frames"] = json!(f);
                }
                if let Some(st) = state {
                    p["state"] = json!(st);
                }
                let d = s.get_one::<Duration>("timeout").copied().unwrap_or(Duration::from_secs(10));
                if d.is_zero() || d > Duration::from_secs(600) {
                    return Err(UsageError::new("--timeout must be 1ms..=600s"));
                }
                p["timeout_ms"] = json!(ms(d));
                let mut c = RpcCall::new("video.wait", p);
                c.wait = Some(d + Duration::from_secs(2));
                Ok(c)
            }
            o => Err(UsageError::new(format!("unknown subcommand {o}")).with_hint("loom video help --json")),
        }
    }

    fn render_human(call: &RpcCall, env: &Envelope<Value>, out: &mut dyn io::Write) -> io::Result<()> {
        if let Some(d) = &env.data {
            if call.method == "video.read" {
                for row in d["rows"].as_array().into_iter().flatten() {
                    writeln!(out, "{row}")?;
                }
                for f in ["complete", "rows_left_out", "next"] {
                    if let Some(v) = d.get(f).filter(|v| !v.is_null()) {
                        writeln!(out, "{f}: {v}")?;
                    }
                }
            } else {
                writeln!(out, "{}", serde_json::to_string_pretty(d).map_err(io::Error::other)?)?;
            }
        }
        render_common(env, out)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn call(args: &[&str]) -> Result<RpcCall, String> {
        let m = VideoCli::command()
            .try_get_matches_from(std::iter::once("video").chain(args.iter().copied()))
            .map_err(|e| e.to_string())?;
        VideoCli::to_call(&m).map_err(|e| e.to_string())
    }

    #[test]
    fn open_is_explicit_and_validated_locally() {
        assert!(call(&["open"]).is_err());
        assert!(call(&["open", "--source", "avfoundation"]).is_err());
        assert!(call(&["open", "--source", "avfoundation", "--device", "default"]).is_err());
        assert!(call(&["open", "--source", "http-mjpeg", "--url", "http://u:p@127.0.0.1:1/"]).is_err());
        assert!(call(&["open", "--source", "rtsp"]).is_err());
        let c = call(&[
            "open", "--source", "http-mjpeg", "--url", "http://127.0.0.1:8090/stream", "--size", "640x480", "--fps", "60",
        ])
        .unwrap();
        assert_eq!(c.method, "video.open");
        assert_eq!(
            c.params,
            json!({"source":"http-mjpeg","url":"http://127.0.0.1:8090/stream","width":640,"height":480,"fps":60})
        );
    }

    #[test]
    fn frame_snapshot_record_wait_read() {
        assert_eq!(call(&["frame", "--id", "7"]).unwrap().params, json!({"frame_id": 7}));
        assert!(call(&["frame", "--id", "0"]).is_err());
        assert_eq!(call(&["snapshot", "--frame", "9"]).unwrap().method, "video.snapshot");
        assert_eq!(call(&["record", "start"]).unwrap().method, "video.record.start");
        assert_eq!(call(&["record", "stop"]).unwrap().method, "video.record.stop");
        assert!(call(&["wait"]).is_err());
        let w = call(&["wait", "--frames", "30", "--timeout", "3s"]).unwrap();
        assert_eq!(w.params["timeout_ms"], 3000);
        assert_eq!(w.wait, Some(Duration::from_secs(5)));
        assert!(call(&["wait", "--frames", "1", "--timeout", "601s"]).is_err());
        assert!(call(&["read", "--wait", "1s"]).is_err());
        let r = call(&["read", "--follow", "--wait", "30s", "--kind", "health"]).unwrap();
        assert_eq!((r.params["wait_ms"].as_u64(), r.params["kind"].as_str()), (Some(30_000), Some("health")));
        assert_eq!(call(&["sources", "--local"]).unwrap().params, json!({"local": true}));
        assert_eq!(call(&["sources"]).unwrap().params, json!({"local": false}));
    }
}
