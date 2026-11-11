//! PID 수명별 워밍업 제외와 회귀·중앙값 판정. 누락 표본으로 PASS를 만들지 않는다.

use std::collections::BTreeMap;

use serde_json::{Value, json};

#[derive(Debug, Clone)]
pub(super) struct Sample {
    pub at: f64,
    pub pid: u32,
    pub lifetime: String,
    pub name: String,
    pub rss: Option<f64>,
    pub fds: Option<u64>,
    pub threads: Option<u64>,
}

impl Sample {
    pub fn json(&self) -> Value {
        json!({"elapsed_s":self.at,"pid":self.pid,"lifetime":self.lifetime,"name":self.name,
            "rss_mib":self.rss,"fds":self.fds,"threads":self.threads})
    }
}

fn median(values: impl Iterator<Item = f64>) -> Option<f64> {
    let mut values: Vec<_> = values.filter(|v| v.is_finite()).collect();
    values.sort_by(f64::total_cmp);
    let n = values.len();
    if n == 0 {
        None
    } else {
        Some((values[(n - 1) / 2] + values[n / 2]) / 2.0)
    }
}

fn slope(points: &[(f64, f64)]) -> Option<f64> {
    if points.len() < 2 {
        return None;
    }
    let n = points.len() as f64;
    let x = points.iter().map(|(x, _)| x).sum::<f64>() / n;
    let y = points.iter().map(|(_, y)| y).sum::<f64>() / n;
    let variance = points.iter().map(|(t, _)| (t - x).powi(2)).sum::<f64>();
    (variance > 0.0).then(|| points.iter().map(|(t, v)| (t - x) * (v - y)).sum::<f64>() / variance * 3600.0)
}

fn trend(points: &[(f64, f64)]) -> Value {
    let (Some(first), Some(last)) = (points.first(), points.last()) else {
        return Value::Null;
    };
    let a = median(
        points
            .iter()
            .filter(|(t, _)| *t < first.0 + 300.0)
            .map(|(_, v)| *v),
    );
    let b = median(
        points
            .iter()
            .filter(|(t, _)| *t > last.0 - 300.0)
            .map(|(_, v)| *v),
    );
    let slope = slope(points);
    json!({"samples":points.len(),"first":first.1,"last":last.1,
        "minimum":points.iter().map(|(_,v)|*v).fold(f64::INFINITY,f64::min),
        "maximum":points.iter().map(|(_,v)|*v).fold(f64::NEG_INFINITY,f64::max),
        "slope_per_hour":slope,"increasing_trend":slope.map(|v|v>1e-6),
        "first_5m_median":a,"last_5m_median":b,
        "median_delta":a.zip(b).map(|(a,b)|b-a),"endpoint_delta":last.1-first.1,
        "window_span_s":last.0-first.0,"windows_overlap":last.0-first.0 < 600.0})
}

fn counter_trend(points: &[(f64, f64)]) -> Value {
    let mut value = trend(points);
    if let (Some(first), Some(last)) = (points.first(), points.last()) {
        value["last_within_baseline_plus_4"] = json!(last.1 <= first.1 + 4.0);
    }
    value
}

pub(super) fn analyze(samples: &[Sample], duration: f64, requested_minutes: f64) -> Value {
    let mut groups: BTreeMap<&str, Vec<&Sample>> = BTreeMap::new();
    for s in samples {
        groups.entry(&s.lifetime).or_default().push(s);
    }
    let mut lifetimes = Vec::new();
    let mut sufficient = !groups.is_empty();
    let mut rss_pass = true;
    for (key, group) in groups {
        let first = group[0];
        let warm_end = first.at + 120.0;
        let kept: Vec<_> = group.iter().filter(|s| s.at >= warm_end).collect();
        let rss: Vec<_> = kept.iter().filter_map(|s| s.rss.map(|v| (s.at, v))).collect();
        let fds: Vec<_> = kept
            .iter()
            .filter_map(|s| s.fds.map(|v| (s.at, v as f64)))
            .collect();
        let threads: Vec<_> = kept
            .iter()
            .filter_map(|s| s.threads.map(|v| (s.at, v as f64)))
            .collect();
        let rss_trend = trend(&rss);
        let span = rss_trend["window_span_s"].as_f64().unwrap_or(0.0);
        let complete = rss.len() >= kept.len().saturating_mul(9).div_ceil(10) && span >= 600.0;
        sufficient &= complete;
        rss_pass &= rss_trend["median_delta"].as_f64().is_some_and(|v| v <= 10.0);
        lifetimes.push(json!({"lifetime":key,"pid":first.pid,"name":first.name,
            "first_seen_s":first.at,"last_seen_s":group.last().map(|s|s.at),"samples":group.len(),
            "warmup_s":120,"post_warmup_samples":kept.len(),"rss_mib":rss_trend,
            "fds":counter_trend(&fds),"threads":counter_trend(&threads),"rss_gate_data_sufficient":complete}));
    }
    let gate = if requested_minutes < 30.0 || duration < 1799.0 || !sufficient {
        "판정 불가"
    } else if rss_pass {
        "통과"
    } else {
        "실패"
    };
    json!({"lifetimes":lifetimes,"p1_rss_gate":{"verdict":gate,"required_minutes":30,
        "maximum_median_delta_mib":10,"data_sufficient":sufficient,
        "reason":if requested_minutes<30.0 {"개발 실행: 30분 관문 미실행"} else if !sufficient {"외부 RSS 표본 또는 독립 5분 창 부족"} else {"PID 수명별 워밍업 제외 후 중앙값 차이"}}})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(at: u32, lifetime: &str, rss: Option<f64>) -> Sample {
        Sample {
            at: f64::from(at),
            pid: 1,
            lifetime: lifetime.into(),
            name: "loomd".into(),
            rss,
            fds: Some(8),
            threads: Some(4),
        }
    }

    #[test]
    fn regression_and_medians_do_not_count_warmup_or_join_pid_lifetimes() {
        let mut samples: Vec<_> = (0..1800)
            .map(|t| {
                sample(
                    t,
                    "old",
                    Some(if t < 120 {
                        999.0
                    } else {
                        20.0 + f64::from(t) / 3600.0
                    }),
                )
            })
            .collect();
        samples.extend((1800..2700).map(|t| sample(t, "reused-pid", Some(80.0))));
        let a = analyze(&samples, 2700.0, 45.0);
        assert_eq!(a["lifetimes"].as_array().map(Vec::len), Some(2));
        let rss = &a["lifetimes"][0]["rss_mib"];
        assert!((rss["slope_per_hour"].as_f64().expect("slope") - 1.0).abs() < 1e-8);
        assert!(rss["maximum"].as_f64().expect("max") < 21.0);
        assert_eq!(a["p1_rss_gate"]["verdict"], "통과");
    }

    #[test]
    fn missing_data_and_short_runs_cannot_pass_and_growth_fails() {
        let missing: Vec<_> = (0..1800).map(|t| sample(t, "p", None)).collect();
        assert_eq!(
            analyze(&missing, 1800.0, 30.0)["p1_rss_gate"]["verdict"],
            "판정 불가"
        );
        let growing: Vec<_> = (0..1800)
            .map(|t| sample(t, "p", Some(f64::from(t) / 30.0)))
            .collect();
        assert_eq!(analyze(&growing, 1800.0, 30.0)["p1_rss_gate"]["verdict"], "실패");
        assert_eq!(
            analyze(&growing[..600], 600.0, 10.0)["p1_rss_gate"]["verdict"],
            "판정 불가"
        );
    }

    #[test]
    fn flat_counters_and_overlapping_development_windows_are_explicit() {
        let s: Vec<_> = (0..600).map(|t| sample(t, "p", Some(20.0))).collect();
        let a = analyze(&s, 600.0, 10.0);
        assert_eq!(a["lifetimes"][0]["fds"]["slope_per_hour"], 0.0);
        assert_eq!(a["lifetimes"][0]["rss_mib"]["windows_overlap"], true);
    }
}
