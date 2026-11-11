//! Memory telemetry for the calling Loom process (never opens or enumerates other processes).

/// Current resident bytes on Linux/macOS (working set on Windows). Missing/zero samples remain
/// unavailable; callers must not interpret them as a measured zero-byte process.
pub fn process_rss_bytes() -> Option<u64> {
    memory_stats::memory_stats()
        .and_then(|m| u64::try_from(m.physical_mem).ok())
        .filter(|rss| *rss > 0)
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    fn current_process_has_a_real_resident_sample() {
        let rss = super::process_rss_bytes().expect("current process RSS must be measurable for P1");
        assert!(rss > 0);
        eprintln!("own-process RSS sample: {rss} bytes");
    }
}
