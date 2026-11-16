//! RF-1 regression (ROOT-assigned shared test; Opus specification only — work/reports/operator-upgrade/video
//! FUNCTIONS.md amendment A8). Parses through the *integrated* `loom` command (all domains + global flags),
//! exactly as `loom_cli::run` does, without a daemon or any IO:
//! `loom [--json] video roi replace --rois <json>` must not panic, the root `--json` output flag must keep its
//! bool meaning, and the real video domain mapping must yield the exact `video.roi.replace` params.
//! Before the fix the payload flag was `--json`, which collided with the global flag and panicked
//! (`Mismatch between definition and access of json`) in `context()`'s `get_flag("json")`.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::{Value, json};

fn parse(args: &[&str]) -> Result<(bool, loom_cli_kit::RpcCall), String> {
    let domains = loom_cli::domains();
    let m = loom_cli::command(&domains)
        .try_get_matches_from(std::iter::once("loom").chain(args.iter().copied()))
        .map_err(|e| e.to_string())?;
    // what `context()` reads: the root output flag must stay a bool
    let json_out = m.get_flag("json");
    let (name, sub) = m.subcommand().expect("domain subcommand");
    let d = domains
        .iter()
        .find(|d| d.name() == name)
        .expect("domain registered");
    let call = d.to_call(sub).map_err(|u| u.message)?;
    Ok((json_out, call))
}

fn recipe() -> Value {
    json!([{"name": "lamp", "x": 0.125, "y": 0.25, "w": 0.25, "h": 0.5, "enter": 20}])
}

#[test]
fn roi_replace_parses_with_and_without_global_json() {
    let text = recipe().to_string();
    for (args, want_json) in [
        (
            vec!["--json", "video", "roi", "replace", "--rois", text.as_str()],
            true,
        ),
        (vec!["video", "roi", "replace", "--rois", text.as_str()], false),
        (
            vec!["video", "roi", "replace", "--rois", text.as_str(), "--json"],
            true,
        ),
    ] {
        let (json_out, call) = parse(&args).unwrap_or_else(|e| panic!("{args:?}: {e}"));
        assert_eq!(
            json_out, want_json,
            "{args:?}: global --json keeps its bool meaning"
        );
        assert_eq!(call.method, "video.roi.replace");
        assert_eq!(call.params, json!({"rois": recipe()}), "{args:?}");
    }
    // a saved `roi list` (object with "rois") + expect_rev, through the integrated parser
    let saved = json!({"config_rev": 2, "rois": recipe()}).to_string();
    let (_, call) = parse(&[
        "--json",
        "video",
        "roi",
        "replace",
        "--rois",
        &saved,
        "--expect-rev",
        "2",
    ])
    .unwrap();
    assert_eq!(call.params, json!({"rois": recipe(), "expect_rev": 2}));
    // empty clears; invalid input is a usage error, never a panic
    assert_eq!(
        parse(&["video", "roi", "replace", "--rois", "[]"])
            .unwrap()
            .1
            .params,
        json!({"rois": []})
    );
    assert!(parse(&["--json", "video", "roi", "replace", "--rois", "{nope"]).is_err());
    assert!(
        parse(&["--json", "video", "roi", "replace"]).is_err(),
        "--rois is required"
    );
}

#[test]
fn other_roi_commands_keep_global_json_meaning() {
    let (j, c) = parse(&["--json", "video", "roi", "list"]).unwrap();
    assert_eq!(
        (j, c.method.as_str(), c.params.clone()),
        (true, "video.roi.list", json!({}))
    );
    let (j, c) = parse(&[
        "--json",
        "video",
        "roi",
        "put",
        "--name",
        "lamp",
        "--rect",
        "0.125,0.25,0.25,0.5",
        "--enter",
        "20",
    ])
    .unwrap();
    assert!(j);
    assert_eq!(
        c.params,
        json!({"roi": {"name": "lamp", "x": 0.125, "y": 0.25, "w": 0.25, "h": 0.5, "enter": 20.0}})
    );
    let (j, c) = parse(&[
        "--json",
        "video",
        "wait",
        "--roi",
        "lamp",
        "--roi-state",
        "changed",
        "--new",
        "--timeout",
        "5s",
    ])
    .unwrap();
    assert!(j);
    assert_eq!(
        c.params,
        json!({"roi": "lamp", "roi_state": "changed", "new": true, "timeout_ms": 5000})
    );
}

/// Structural guard: no video subcommand may reuse a global bool/string flag id or long name.
/// (`--timeout` is tolerated by design: the root reads it with `try_get_one`, and `video wait` owns a duration.)
#[test]
fn video_args_never_shadow_global_flags() {
    let domains = loom_cli::domains();
    let video = domains.iter().find(|d| d.name() == "video").unwrap().command();
    let mut stack = vec![(String::from("video"), video)];
    let mut seen = 0;
    while let Some((path, cmd)) = stack.pop() {
        for a in cmd.get_arguments() {
            seen += 1;
            let id = a.get_id().as_str();
            let long = a.get_long().unwrap_or("");
            for bad in ["json", "idempotency_key", "idempotency-key"] {
                assert!(
                    id != bad && long != bad,
                    "{path}: arg {id:?}/--{long} shadows the global --{bad}"
                );
            }
        }
        for s in cmd.get_subcommands() {
            stack.push((format!("{path} {}", s.get_name()), s.clone()));
        }
    }
    assert!(seen > 20, "walked the video command tree ({seen} args)");
}
