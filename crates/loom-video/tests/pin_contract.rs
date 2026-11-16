//! Operator upgrade B2 contract tests (work/reports/operator-upgrade/video/FUNCTIONS.md §Batch 2), Opus-authored.
//! Pure pin store + ring/pin lookup (V-03), RPC registration/risk (pin/unpin are R0 safe-direction memory ops),
//! CLI conversion, artifact `content-disposition` (V-06). No FFmpeg, no device. Sonnet must not edit this file.
#![allow(clippy::unwrap_used)]

use loom_cli_kit::DomainCli as _;
use loom_domain_api::{DomainModule as _, Registry, RoleSet, TargetRef, WriteHooks as _, WriteRequest};
use loom_proto::event::{Env, Origin, Risk};
use loom_proto::policy::WriteClass;
use loom_video::cli::VideoCli;
use loom_video::gateway::{Video, VideoHooks};
use loom_video::model::{
    self, Frame, MAX_FRAME_BYTES, MAX_USER_PINS, PIN_BYTES, PIN_TTL_DEFAULT_MS, PIN_TTL_MAX_MS, PinOwner,
    PinRefusal, PinStore, RING_FRAMES, Ring,
};
use serde_json::{Value, json};

const MS: u64 = 1_000_000;

fn frame(id: u64, len: usize) -> Frame {
    let mut v = vec![0xFF, 0xD8];
    v.extend(std::iter::repeat_n((id % 251) as u8, len.saturating_sub(4)));
    v.extend([0xFF, 0xD9]);
    Frame::new(id, 7, id * 33 * MS, 1_762_770_000_000_000_000 + id, v)
}

#[test]
fn pin_refresh_and_owners_share_one_copy() {
    let mut p = PinStore::default();
    let f = frame(10, 100);
    let a = p.pin(&f, PinOwner::User, 0, 5_000).unwrap();
    assert_eq!(
        (a.frame_id, a.size, a.expires_in_ms, a.refreshed),
        (10, 100, 5_000, false)
    );
    assert_eq!(a.sha256, f.sha256);
    // refreshing the same owner extends expiry, no extra bytes or slots
    let b = p.pin(&f, PinOwner::User, 1_000 * MS, 5_000).unwrap();
    assert_eq!((b.expires_in_ms, b.refreshed), (5_000, true));
    // an evidence owner on the same frame shares the copy and never consumes a user slot
    p.pin(&f, PinOwner::Evidence(1), 1_000 * MS, 9_000).unwrap();
    assert_eq!((p.len(), p.bytes(), p.user_count()), (1, 100, 1));
    assert_eq!(p.get(10).unwrap().sha256, f.sha256);
    assert!(!p.is_empty());
    assert_eq!(PinOwner::User.label(), "user");
    assert_eq!(PinOwner::Evidence(3).label(), "evidence-3");
}

#[test]
fn user_limit_and_byte_bound_refuse_explicitly() {
    let mut p = PinStore::default();
    for i in 1..=MAX_USER_PINS as u64 {
        p.pin(&frame(i, 10), PinOwner::User, 0, 1_000).unwrap();
    }
    let e = p.pin(&frame(99, 10), PinOwner::User, 0, 1_000).unwrap_err();
    assert_eq!(
        e,
        PinRefusal::UserLimit {
            pinned: MAX_USER_PINS
        }
    );
    assert_eq!(e.reason(), "pin_user_limit");
    // re-pinning an already user-pinned frame is a refresh, not a new slot
    assert!(p.pin(&frame(1, 10), PinOwner::User, 0, 1_000).unwrap().refreshed);
    // evidence pins are bounded by bytes only
    assert!(p.pin(&frame(99, 10), PinOwner::Evidence(1), 0, 1_000).is_ok());

    let mut p = PinStore::default();
    let n = PIN_BYTES / MAX_FRAME_BYTES;
    for i in 1..=n as u64 {
        p.pin(&frame(i, MAX_FRAME_BYTES), PinOwner::Evidence(1), 0, 1_000)
            .unwrap();
    }
    assert_eq!(p.bytes(), PIN_BYTES);
    let e = p
        .pin(&frame(1000, MAX_FRAME_BYTES), PinOwner::Evidence(1), 0, 1_000)
        .unwrap_err();
    assert_eq!(
        e,
        PinRefusal::Bytes {
            need: MAX_FRAME_BYTES,
            free: 0
        }
    );
    assert_eq!(e.reason(), "pin_bytes");
    // adding a second owner to a held frame needs no bytes
    assert!(
        p.pin(&frame(1, MAX_FRAME_BYTES), PinOwner::User, 0, 1_000)
            .is_ok()
    );
}

#[test]
fn ttl_clamp_expiry_and_release_counters() {
    let mut p = PinStore::default();
    assert_eq!(
        p.pin(&frame(1, 10), PinOwner::User, 0, 0).unwrap().expires_in_ms,
        1
    );
    assert_eq!(
        p.pin(&frame(2, 10), PinOwner::User, 0, u64::MAX)
            .unwrap()
            .expires_in_ms,
        PIN_TTL_MAX_MS
    );
    p.pin(&frame(3, 10), PinOwner::User, 0, 1_000).unwrap();
    assert_eq!(p.expire(999 * MS), 1, "frame 1 (1 ms) expired");
    assert!(p.get(1).is_none() && p.get(3).is_some());
    assert_eq!(p.expire(1_000 * MS), 1, "expiry is inclusive");
    assert!(p.get(3).is_none());
    assert_eq!((p.len(), p.bytes()), (1, 10));
    assert!(p.unpin(2, PinOwner::User));
    assert!(!p.unpin(2, PinOwner::User), "idempotent");
    assert!(p.is_empty() && p.bytes() == 0);
    // owner-wide release
    for i in 10..13 {
        p.pin(&frame(i, 10), PinOwner::Evidence(4), 0, 60_000).unwrap();
    }
    p.pin(&frame(10, 10), PinOwner::User, 0, 60_000).unwrap();
    assert_eq!(p.unpin_owner(PinOwner::Evidence(4)), 3);
    assert_eq!((p.len(), p.user_count()), (1, 1), "frame 10 still held by user");
    let j = p.json(0);
    assert_eq!(j["expired_total"], 2);
    assert_eq!(j["released_total"], 4);
}

#[test]
fn json_shapes_are_stable() {
    let mut p = PinStore::default();
    let f = frame(5, 64);
    p.pin(&f, PinOwner::Evidence(2), 0, 3_000).unwrap();
    p.pin(&f, PinOwner::User, 0, 2_000).unwrap();
    assert_eq!(
        p.entry_json(5, 500 * MS),
        json!({"frame_id": 5, "sha256": f.sha256, "size": 64,
               "owners": [{"owner": "user", "expires_in_ms": 1_500}, {"owner": "evidence-2", "expires_in_ms": 2_500}]})
    );
    assert_eq!(p.entry_json(6, 0), Value::Null);
    let j = p.json(500 * MS);
    assert_eq!(
        (j["count"].as_u64(), j["user_count"].as_u64(), j["bytes"].as_u64()),
        (Some(1), Some(1), Some(64))
    );
    assert_eq!(j["frames"], json!([p.entry_json(5, 500 * MS)]));
    assert_eq!(j["limits"], model::pin_limits());
    assert_eq!(
        model::pin_limits(),
        json!({"max_user_pins": MAX_USER_PINS, "pin_bytes": PIN_BYTES, "ttl_default_ms": PIN_TTL_DEFAULT_MS,
               "ttl_max_ms": PIN_TTL_MAX_MS, "released_on": ["unpin", "ttl", "next_open"], "memory_only": true})
    );
    assert_eq!(model::capabilities(&Value::Null)["pins"], model::pin_limits());
}

/// V-03 regression: the frozen frame leaves the ring while the user inspects it; a pin keeps the exact bytes.
#[test]
fn pinned_frame_outlives_ring_eviction() {
    let mut ring = Ring::default();
    let mut pins = PinStore::default();
    ring.push(frame(1, 10));
    ring.push(frame(2, 10));
    let f1 = ring.get(1).cloned().unwrap();
    pins.pin(&f1, PinOwner::User, 0, 60_000).unwrap();
    for i in 3..=(RING_FRAMES as u64 + 10) {
        ring.push(frame(i, 10));
    }
    assert!(
        ring.get(1).is_none() && ring.get(2).is_none(),
        "both evicted from the ring"
    );
    let (f, by) = model::lookup_frame(&ring, &pins, 1).unwrap();
    assert_eq!((f.id, f.sha256.as_str(), by), (1, f1.sha256.as_str(), "pin"));
    assert!(
        model::lookup_frame(&ring, &pins, 2).is_none(),
        "unpinned frame is gone"
    );
    let last = RING_FRAMES as u64 + 10;
    assert_eq!(
        model::lookup_frame(&ring, &pins, last).unwrap().1,
        "ring",
        "ring wins when both hold it"
    );
}

// ---- registration / risk ------------------------------------------------------------------------

fn registry() -> Registry {
    let mut r = Registry::new("video").unwrap();
    Video.register(&mut r).unwrap();
    r
}

#[test]
fn pin_rpcs_are_safe_direction_memory_writes() {
    let r = registry();
    for m in ["video.pin", "video.unpin"] {
        let c = r
            .commands()
            .iter()
            .find(|c| c.method == m)
            .unwrap_or_else(|| panic!("{m} registered"));
        assert_eq!(c.write_class, Some(WriteClass::Request), "{m}");
        assert_eq!(
            c.roles,
            RoleSet::READ,
            "{m}: watchers may pin (freeze) like close"
        );
        let row = r
            .risk_table_ref()
            .unwrap()
            .rows
            .iter()
            .find(|x| x.method == m)
            .unwrap();
        assert_eq!(row.risk, Risk::R0, "{m}");
        let schema = serde_json::to_value(&c.params).unwrap();
        assert!(schema["properties"]["frame_id"].is_object(), "{m}: {schema}");
    }
    let pin = r.commands().iter().find(|c| c.method == "video.pin").unwrap();
    assert!(serde_json::to_value(&pin.params).unwrap()["properties"]["ttl_ms"].is_object());
    let hooks = VideoHooks;
    let target = TargetRef {
        channel: None,
        target: None,
        generation: None,
    };
    let args = json!({});
    for (m, word) in [("video.pin", "memory"), ("video.unpin", "releases")] {
        let c = hooks.classify(&WriteRequest {
            domain: "video",
            method: m,
            target: &target,
            bytes: None,
            args: &args,
            origin: Origin::Agent,
            client_id: "c",
            env: Env::Sim,
            write_class: WriteClass::Request,
            job: None,
        });
        assert_eq!(c.risk, Risk::R0, "{m}");
        assert!(c.reason.contains(word), "{m}: {}", c.reason);
    }
    // unchanged contracts
    let open = r.commands().iter().find(|c| c.method == "video.open").unwrap();
    assert_eq!(open.roles, RoleSet::WRITE);
}

// ---- CLI ------------------------------------------------------------------------------------------

fn call(args: &[&str]) -> Result<loom_cli_kit::RpcCall, String> {
    let m = VideoCli::command()
        .try_get_matches_from(std::iter::once("video").chain(args.iter().copied()))
        .map_err(|e| e.to_string())?;
    VideoCli::to_call(&m).map_err(|e| e.to_string())
}

#[test]
fn cli_pin_unpin() {
    let c = call(&["pin", "--frame", "7", "--ttl", "2m"]).unwrap();
    assert_eq!(
        (c.method.as_str(), c.params.clone()),
        ("video.pin", json!({"frame_id": 7, "ttl_ms": 120_000}))
    );
    assert_eq!(
        call(&["pin"]).unwrap().params,
        json!({}),
        "latest frame, default TTL"
    );
    assert_eq!(
        call(&["pin", "--capture", "v0123456789ab"]).unwrap().params,
        json!({"capture_id": "v0123456789ab"})
    );
    for bad in [
        &["pin", "--frame", "0"][..],
        &["pin", "--ttl", "601s"],
        &["pin", "--ttl", "0s"],
    ] {
        assert!(call(bad).is_err(), "{bad:?}");
    }
    assert_eq!(call(&["unpin"]).unwrap().method, "video.unpin");
    assert_eq!(
        call(&["unpin"]).unwrap().params,
        json!({}),
        "all of this caller's user pins"
    );
    assert_eq!(
        call(&["unpin", "--frame", "3"]).unwrap().params,
        json!({"frame_id": 3})
    );
    assert!(call(&["unpin", "--frame", "0"]).is_err());
}

// ---- media: artifacts download instead of replacing the app (V-06) --------------------------------

#[test]
fn artifact_replies_carry_attachment_disposition() {
    let cap = "v0123456789ab";
    let session = std::env::temp_dir().join(format!("loom-video-pin-{}", std::process::id()));
    let dir = model::capture_dir(&session, cap);
    std::fs::create_dir_all(&dir).unwrap();
    let id = format!("snap-{cap}-1");
    std::fs::write(dir.join(format!("{id}.jpg")), [0xFF, 0xD8, 0xFF, 0xD9]).unwrap();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let r = rt.block_on(loom_video::media::artifact(&session, &id));
    assert_eq!((r.status, r.content_type), (200, "image/jpeg"));
    let h = |r: &loom_video::media::Reply, k: &str| {
        r.headers.iter().find(|(n, _)| *n == k).map(|(_, v)| v.clone())
    };
    assert_eq!(
        h(&r, "content-disposition").as_deref(),
        Some(format!("attachment; filename=\"{id}.jpg\"").as_str())
    );
    assert_eq!(h(&r, "cache-control").as_deref(), Some("no-store"));
    let missing = rt.block_on(loom_video::media::artifact(&session, &format!("snap-{cap}-2")));
    assert_eq!(missing.status, 404);
    assert!(
        h(&missing, "content-disposition").is_none(),
        "errors are JSON, not attachments"
    );
    let _ = std::fs::remove_dir_all(&session);
}
