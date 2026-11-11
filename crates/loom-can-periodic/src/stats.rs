/// Constant-size approximate latency facts. Quantiles are histogram upper bounds.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct StatsSnapshot {
    pub sent: u64,
    pub failed: u64,
    pub missed: u64,
    pub paused_skipped: u64,
    pub cancelled: u64,
    pub expired_submissions: u64,
    pub p50_lateness_ns: Option<u64>,
    pub p99_lateness_ns: Option<u64>,
    pub max_lateness_ns: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct Stats {
    pub(crate) sent: u64,
    pub(crate) failed: u64,
    pub(crate) missed: u64,
    pub(crate) paused_skipped: u64,
    pub(crate) cancelled: u64,
    pub(crate) expired_submissions: u64,
    bins: [u64; 128],
    samples: u64,
    max: u64,
}
impl Default for Stats {
    fn default() -> Self {
        Self {
            sent: 0,
            failed: 0,
            missed: 0,
            paused_skipped: 0,
            cancelled: 0,
            expired_submissions: 0,
            bins: [0; 128],
            samples: 0,
            max: 0,
        }
    }
}
fn upper(index: usize) -> u64 {
    if index <= 64 {
        index as u64 * 125_000
    } else {
        8_000_000u64.saturating_mul(1u64.checked_shl((index - 64) as u32).unwrap_or(u64::MAX))
    }
}
impl Stats {
    pub(crate) fn submitted(&mut self, lateness: u64) {
        self.sent = self.sent.saturating_add(1);
        self.samples = self.samples.saturating_add(1);
        self.max = self.max.max(lateness);
        if let Some(index) = (0..128).find(|&index| upper(index) >= lateness) {
            self.bins[index] = self.bins[index].saturating_add(1);
        }
    }
    fn quantile(&self, percent: u64) -> Option<u64> {
        if self.samples == 0 {
            return None;
        }
        let rank = (u128::from(self.samples) * u128::from(percent)).div_ceil(100);
        let mut total = 0u128;
        for (index, count) in self.bins.iter().enumerate() {
            total += u128::from(*count);
            if total >= rank {
                return Some(upper(index));
            }
        }
        Some(u64::MAX)
    }
    pub(crate) fn snapshot(&self) -> StatsSnapshot {
        StatsSnapshot {
            sent: self.sent,
            failed: self.failed,
            missed: self.missed,
            paused_skipped: self.paused_skipped,
            cancelled: self.cancelled,
            expired_submissions: self.expired_submissions,
            p50_lateness_ns: self.quantile(50),
            p99_lateness_ns: self.quantile(99),
            max_lateness_ns: self.max,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn counters_saturate_and_histogram_covers_u64() {
        let mut stats = Stats {
            sent: u64::MAX,
            samples: u64::MAX,
            ..Stats::default()
        };
        stats.submitted(u64::MAX);
        assert_eq!(stats.sent, u64::MAX);
        assert_eq!(stats.samples, u64::MAX);
        assert_eq!(stats.max, u64::MAX);
        assert!(stats.bins.contains(&1));
        let mut normal = Stats::default();
        normal.submitted(0);
        normal.submitted(4_530_000);
        assert_eq!(normal.snapshot().p50_lateness_ns, Some(0));
        assert_eq!(normal.snapshot().p99_lateness_ns, Some(4_625_000));
    }
}
