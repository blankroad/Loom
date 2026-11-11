//! 부하의 성공 송신 집합과 viewer 수신 집합을 유한 비트 장부로 비교한다.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::rpc::number;

#[derive(Debug, Default)]
pub(super) struct View {
    pub connects: u64,
    pub reconnects: u64,
    pub duplicates: u64,
    pub last_seq: u64,
    pub gap_events: u64,
    pub gap_count: u64,
    pub unknown_gaps: u64,
    pub record_gaps: u64,
    pub batch_gap_count: u64,
    pub heartbeat_drops: u64,
    pub heartbeat_previous: u64,
    pub stop_pairs: u64,
}

#[derive(Debug)]
pub(super) struct State {
    pub agent: Vec<u8>,
    pub cli: Vec<u8>,
    pub views: [View; 2],
    pub writes: u64,
    pub write_failures: u64,
    pub idem_replays: u64,
    pub idem_failures: u64,
    pub cli_runs: u64,
    pub cli_failures: u64,
    pub query_runs: u64,
    pub query_failures: u64,
    pub journal_queries: u64,
    pub old_queries: u64,
    pub marks: u64,
    pub errors: BTreeMap<String, u64>,
}

impl State {
    pub fn new(seconds: f64) -> Self {
        Self {
            agent: vec![0; (seconds * 20.0).ceil() as usize + 64],
            cli: vec![0; (seconds / 10.0).ceil() as usize + 4],
            views: [View::default(), View::default()],
            writes: 0,
            write_failures: 0,
            idem_replays: 0,
            idem_failures: 0,
            cli_runs: 0,
            cli_failures: 0,
            query_runs: 0,
            query_failures: 0,
            journal_queries: 0,
            old_queries: 0,
            marks: 0,
            errors: BTreeMap::new(),
        }
    }

    pub fn error(&mut self, kind: &str, error: &str) {
        let key = format!("{kind}: {}", error.chars().take(160).collect::<String>());
        if self.errors.len() < 32 || self.errors.contains_key(&key) {
            *self.errors.entry(key).or_default() += 1;
        }
    }

    pub fn sent(&mut self, cli: bool, id: usize, ok: bool) {
        let rows = if cli { &mut self.cli } else { &mut self.agent };
        if let Some(row) = rows.get_mut(id)
            && ok
        {
            *row |= 1;
        }
        if cli {
            self.cli_runs += 1;
            self.cli_failures += u64::from(!ok);
        } else {
            self.writes += 1;
            self.write_failures += u64::from(!ok);
        }
    }

    pub fn observe(&mut self, view: usize, v: &Value) {
        if v["method"] == "core.heartbeat" {
            let dropped = v["params"]["sub_stats"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|s| number(&s["drops"]))
                .sum::<u64>();
            self.views[view].heartbeat_drops = self.views[view].heartbeat_drops.max(dropped);
        }
        if v["method"] != "core.events" && v["method"] != "core.snapshot_part" {
            return;
        }
        self.views[view].batch_gap_count += number(&v["params"]["display_gap"]["count"]).unwrap_or(0);
        for event in v["params"]["events"].as_array().into_iter().flatten() {
            if event["kind"] == "display.gap" {
                self.views[view].gap_events += 1;
                if let Some(count) = number(&event["payload"]["count"]) {
                    self.views[view].gap_count += count;
                } else {
                    self.views[view].unknown_gaps += 1;
                }
            }
            if event["kind"] == "record.gap" || event["kind"] == "source.gap" {
                self.views[view].record_gaps += 1;
            }
            if event["kind"] != "echo.data" {
                continue;
            }
            self.views[view].last_seq = self.views[view].last_seq.max(number(&event["seq"]).unwrap_or(0));
            let text = event["payload"]["text"].as_str().unwrap_or_default();
            let (cli, id) = if let Some(rest) = text.strip_prefix("soak-agent:") {
                (
                    false,
                    rest.split(':').next().and_then(|s| s.parse::<usize>().ok()),
                )
            } else if let Some(rest) = text.strip_prefix("soak-cli:") {
                (true, rest.parse::<usize>().ok())
            } else {
                continue;
            };
            let Some(id) = id else {
                continue;
            };
            let rows = if cli { &mut self.cli } else { &mut self.agent };
            if let Some(row) = rows.get_mut(id) {
                let mask = 2 << view;
                if *row & mask != 0 {
                    self.views[view].duplicates += 1;
                }
                *row |= mask;
            }
        }
    }

    pub fn json(&self) -> Value {
        let viewers:Vec<_>=self.views.iter().enumerate().map(|(i,v)| {
            let mask=2<<i;
            let all=self.agent.iter().chain(&self.cli);
            let expected=all.clone().filter(|f|**f&1!=0).count();
            let received=all.clone().filter(|f|**f&1!=0&&**f&mask!=0).count();
            let ambiguous=all.filter(|f|**f&1==0&&**f&mask!=0).count();
            let missing=expected-received;
            json!({"name":if i==0 {"normal"} else {"slow"},"connections":v.connects,"reconnections":v.reconnects,
                "expected_unique_echoes":expected,"received_unique_echoes":received,"missing":missing,
                "received_without_success_reply":ambiguous,"duplicates":v.duplicates,"last_stream_seq":v.last_seq,
                "display_gap_events":v.gap_events,"display_gap_count":v.gap_count,"unknown_gap_events":v.unknown_gaps,
                "record_or_source_gaps":v.record_gaps,"batch_gap_count_duplicate_summary":v.batch_gap_count,
                "heartbeat_drop_count":v.heartbeat_previous+v.heartbeat_drops,
                "missing_matches_gap_count":v.unknown_gaps==0&&missing as u64==v.gap_count,
                "unexplained_missing":(missing as u64).saturating_sub(v.gap_count),"stop_release_pairs":v.stop_pairs})
        }).collect();
        json!({"agent":{"attempts":self.writes,"failures":self.write_failures,"idempotency_replays":self.idem_replays,"idempotency_failures":self.idem_failures},
            "cli":{"runs":self.cli_runs,"failures":self.cli_failures},
            "queries":{"runs":self.query_runs,"failures":self.query_failures,"old_cursor_runs":self.old_queries,"journal_source_runs":self.journal_queries},
            "marks":self.marks,"viewers":viewers,"errors":self.errors})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn idempotent_replies_do_not_hide_missing_echoes_and_gap_summaries_are_not_double_counted() {
        let mut s = State::new(1.0);
        s.sent(false, 0, true);
        s.sent(false, 1, true);
        let e = json!({"method":"core.events","params":{"events":[{"kind":"echo.data","seq":"8","payload":{"text":"soak-agent:0:payload"}}]}});
        s.observe(0, &e);
        s.observe(0, &e);
        s.observe(0,&json!({"method":"core.events","params":{"display_gap":{"count":"1"},"events":[{"kind":"display.gap","payload":{"count":"1"}}]}}));
        let v = s.json();
        assert_eq!(v["viewers"][0]["missing"], 1);
        assert_eq!(v["viewers"][0]["duplicates"], 1);
        assert_eq!(v["viewers"][0]["display_gap_count"], 1);
        assert_eq!(v["viewers"][0]["missing_matches_gap_count"], true);
        assert_eq!(v["viewers"][1]["missing"], 2);
    }
}
