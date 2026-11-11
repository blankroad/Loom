//! 외부 ps/lsof 측정. 권한 오류와 미지원 필드는 0 대신 None으로 남긴다.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read as _, Seek as _};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::metrics::Sample;

pub(super) fn output(command: &mut Command, scratch: &Path) -> Result<String, String> {
    let mut file = tempfile::tempfile_in(scratch).map_err(|e| e.to_string())?;
    let mut child = command
        .stdout(file.try_clone().map_err(|e| e.to_string())?)
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let end = Instant::now() + Duration::from_secs(2);
    loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) if status.success() => break,
            Some(status) => return Err(format!("외부 명령 실패: {status}")),
            None if Instant::now() >= end => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("외부 명령 2초 시간 초과".into());
            }
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    file.rewind().map_err(|e| e.to_string())?;
    let mut text = String::new();
    file.take(1024 * 1024)
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    Ok(text)
}

#[derive(Debug, Clone)]
struct Process {
    pid: u32,
    parent: u32,
    birth: String,
    name: String,
}

fn processes(text: &str) -> Vec<Process> {
    text.lines()
        .filter_map(|line| {
            let cols: Vec<_> = line.split_whitespace().collect();
            if cols.len() < 8 {
                return None;
            }
            Some(Process {
                pid: cols[0].parse().ok()?,
                parent: cols[1].parse().ok()?,
                birth: cols[2..7].join(" "),
                name: cols[7..].join(" "),
            })
        })
        .collect()
}

fn descendants(all: &[Process], pid: u32) -> Vec<Process> {
    let mut ids = BTreeSet::from([pid]);
    loop {
        let n = ids.len();
        for p in all {
            if ids.contains(&p.parent) {
                ids.insert(p.pid);
            }
        }
        if ids.len() == n {
            break;
        }
    }
    all.iter()
        .filter(|p| ids.contains(&p.pid))
        .take(32)
        .cloned()
        .collect()
}

#[derive(Debug, Default)]
pub(super) struct Sampler {
    pub errors: BTreeMap<String, u64>,
    last_fd: BTreeMap<String, f64>,
    pub discovery_complete: bool,
}

impl Sampler {
    fn record<T>(&mut self, label: &str, result: Result<T, String>) -> Option<T> {
        match result {
            Ok(v) => Some(v),
            Err(e) => {
                // 명령 종류별 오류 목록에도 상한을 두어 장시간 증가를 막는다.
                let key = format!("{label}: {}", e.chars().take(160).collect::<String>());
                if self.errors.len() < 32 || self.errors.contains_key(&key) {
                    *self.errors.entry(key).or_default() += 1;
                }
                None
            }
        }
    }

    pub fn sample(&mut self, root: u32, at: f64, scratch: &Path) -> Vec<Sample> {
        let discovered = self.record(
            "프로세스 탐색",
            output(
                Command::new("ps").args(["-axo", "pid=,ppid=,lstart=,comm="]),
                scratch,
            ),
        );
        let mut procs = discovered
            .as_deref()
            .map(processes)
            .map(|all| descendants(&all, root))
            .unwrap_or_default();
        self.discovery_complete =
            discovered.is_some() && procs.iter().any(|p| p.pid == root) && procs.len() < 32;
        if !procs.iter().any(|p| p.pid == root) {
            // 프로세스 목록이 잘려도 fallback을 포함한 추적 상한은 32개다.
            procs.truncate(31);
            procs.insert(
                0,
                Process {
                    pid: root,
                    parent: 0,
                    birth: "spawn".into(),
                    name: "loomd".into(),
                },
            );
        }
        let mut result = Vec::new();
        for p in procs {
            let lifetime = if p.pid == root {
                format!("{root}:spawn")
            } else {
                format!("{}:{}", p.pid, p.birth)
            };
            let rss = self.record(
                "RSS ps",
                output(
                    Command::new("ps").args(["-o", "rss=", "-p", &p.pid.to_string()]),
                    scratch,
                )
                .and_then(|s| {
                    s.trim()
                        .parse::<f64>()
                        .map(|v| v / 1024.0)
                        .map_err(|e| e.to_string())
                }),
            );
            let threads = if cfg!(target_os = "macos") {
                self.record(
                    "스레드 ps -M",
                    output(
                        Command::new("ps").args(["-M", "-p", &p.pid.to_string(), "-o", "pid="]),
                        scratch,
                    )
                    .and_then(|s| {
                        let n = s.lines().filter(|l| l.trim().parse::<u32>() == Ok(p.pid)).count() as u64;
                        if n == 0 {
                            Err("스레드 행 없음".into())
                        } else {
                            Ok(n)
                        }
                    }),
                )
            } else {
                self.record(
                    "스레드 ps nlwp",
                    output(
                        Command::new("ps").args(["-o", "nlwp=", "-p", &p.pid.to_string()]),
                        scratch,
                    )
                    .and_then(|s| s.trim().parse::<u64>().map_err(|e| e.to_string())),
                )
            };
            let fds = if cfg!(target_os = "linux") {
                self.record(
                    "FD /proc",
                    std::fs::read_dir(format!("/proc/{}/fd", p.pid))
                        .and_then(|mut d| d.try_fold(0u64, |n, entry| entry.map(|_| n + 1)))
                        .map_err(|e| e.to_string()),
                )
            } else if self.last_fd.get(&lifetime).is_none_or(|last| at - last >= 60.0) {
                self.last_fd.insert(lifetime.clone(), at);
                self.record(
                    "FD lsof",
                    output(
                        Command::new("lsof").args(["-nP", "-a", "-p", &p.pid.to_string(), "-F", "f"]),
                        scratch,
                    )
                    .and_then(|s| {
                        let count = s
                            .lines()
                            .filter(|l| {
                                l.strip_prefix('f')
                                    .is_some_and(|f| !f.is_empty() && f.bytes().all(|c| c.is_ascii_digit()))
                            })
                            .count() as u64;
                        if count == 0 {
                            Err("숫자 FD 행 없음".into())
                        } else {
                            Ok(count)
                        }
                    }),
                )
            } else {
                None
            };
            result.push(Sample {
                at,
                pid: p.pid,
                lifetime,
                name: p.name,
                rss,
                fds,
                threads,
            });
        }
        // 종료된 PID의 캐시를 제거하여 재시작이 반복되어도 증가하지 않는다.
        self.last_fd
            .retain(|key, _| result.iter().any(|s| &s.lifetime == key));
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn process_tree_excludes_unrelated_processes_and_keeps_birth_identity() {
        let p = processes(
            "1 0 Wed Nov 11 01:00:00 2026 /loomd\n2 1 Wed Nov 11 01:00:01 2026 /echo-worker\n3 2 Wed Nov 11 01:00:02 2026 /helper\n4 0 Wed Nov 11 01:00:03 2026 /other\n",
        );
        let children = descendants(&p, 1);
        assert_eq!(children.iter().map(|p| p.pid).collect::<Vec<_>>(), [1, 2, 3]);
        assert_ne!(children[0].birth, children[1].birth);
    }
}
