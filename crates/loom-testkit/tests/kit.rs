//! Self-tests of the kit's fixtures and assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use loom_proto::num::U64;
use loom_testkit::asserts::{check_no_gap_lies, check_publish_before_reply, check_write_rows};
use loom_testkit::{TraceItem, fixtures};
use serde_json::json;

#[test]
fn fixtures_load() {
    assert_eq!(fixtures::ALL.len(), 15);
    for (name, _) in fixtures::ALL {
        assert!(fixtures::json(name).is_some(), "{name}");
        if name.starts_with("event_") {
            assert!(!fixtures::events(name).is_empty(), "{name}");
        }
    }
}

#[test]
fn write_rows_rules() {
    let rows = fixtures::events("event_uart_write_rows");
    check_write_rows(&rows).unwrap();
    let uds = fixtures::events("event_uds_rows");
    check_write_rows(&uds[..2]).unwrap();

    let mut back = rows.clone();
    back.swap(1, 2);
    back[1].seq = U64(rows[1].seq.get());
    back[2].seq = U64(rows[2].seq.get());
    assert!(check_write_rows(&back).unwrap_err().contains("backwards"));

    let mut after_terminal = rows.clone();
    after_terminal[1].payload = json!({"write_state": "failed"});
    assert!(
        check_write_rows(&after_terminal)
            .unwrap_err()
            .contains("after terminal")
    );

    let mut other = rows.clone();
    other[2].request_id = Some("q-other".into());
    assert!(check_write_rows(&other).is_err());

    let mut missing = rows.clone();
    missing[0].payload = json!({});
    assert!(check_write_rows(&missing).unwrap_err().contains("write_state"));
    assert!(check_write_rows(&[]).is_err());
}

#[test]
fn publish_before_reply_shape() {
    let rows = fixtures::events("event_uart_write_rows");
    let mut trace: Vec<TraceItem> = rows
        .iter()
        .cloned()
        .map(|r| TraceItem::Event(Box::new(r)))
        .collect();
    trace.push(TraceItem::Reply {
        request_id: "q-01JC7".into(),
    });
    check_publish_before_reply(&trace).unwrap();

    let late: Vec<TraceItem> = std::iter::once(TraceItem::Reply {
        request_id: "q-01JC7".into(),
    })
    .chain(rows.iter().cloned().map(|r| TraceItem::Event(Box::new(r))))
    .collect();
    assert!(check_publish_before_reply(&late).is_err());

    let mut straggler = trace.clone();
    straggler.push(TraceItem::Event(Box::new(rows[2].clone())));
    assert!(
        check_publish_before_reply(&straggler)
            .unwrap_err()
            .contains("after its reply")
    );
}

#[test]
fn gap_honesty() {
    let mut evs = Vec::new();
    for n in ["event_display_gap", "event_record_gap", "event_source_gap"] {
        evs.extend(fixtures::events(n));
    }
    check_no_gap_lies(&evs).unwrap();

    let mut lie = fixtures::events("event_display_gap");
    lie[0].payload["count"] = json!("100000");
    assert!(check_no_gap_lies(&lie).unwrap_err().contains("exceeds"));
    let mut no_cause = fixtures::events("event_record_gap");
    no_cause[0].payload["cause"] = json!("");
    assert!(check_no_gap_lies(&no_cause).is_err());
    let mut no_range = fixtures::events("event_display_gap");
    no_range[0].payload.as_object_mut().unwrap().remove("seq_range");
    assert!(check_no_gap_lies(&no_range).unwrap_err().contains("no range"));
    let mut order = evs.clone();
    order.swap(0, 2);
    assert!(check_no_gap_lies(&order).is_err());
}
