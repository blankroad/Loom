# P1 M1 — `loom-journal` 1단계 보고 (CORE 워커)

작업 위치: `/Users/ctmctm/workspace/Loom/.worktrees/core` (브랜치 `core/main`). 커밋하지 않음.
수정 범위: 새 crate `crates/loom-journal/`만. `Cargo.lock`에 `loom-journal` 항목이 추가됨(workspace 자동 갱신, 이 파일은 core-opus 작업으로 이미 수정 상태였음). loom-gateway·loom-proto·루트 `Cargo.toml`은 건드리지 않음.

## 1. 만든 것

| 파일 | 내용 |
|---|---|
| `src/format.rs` | 헤더 `LOOMJ1\0\0 | ver | codec | checksum_alg | producer_id | epoch | wall_ns | clock_id`, 프레임 `u32 len | u32 crc32c(payload) | u8 rec_type | payload`(LE). codec 레지스트리 1=json/1, 2=msgpack/1(예약), 3=pcap-epb/1. rec_type 1=event, 2=index_hint, 3=marker, 4=mapping |
| `src/queue.rs` | 상한 있는 MPSC 큐(건수+바이트) + 프로세스 공유 `QueueBudget`(worker 합 16 MiB/32k). 넘칠 때 정책은 코드 doc에 적음: `try_push` 즉시 Full(데이터), `push_timeout` 최대 대기 후 Full(mapping) |
| `src/writer.rs` | `JournalWriter::spawn(cfg)` → 전용 OS 스레드. `JournalHandle::try_append(&SourceRecord)` / `try_append_raw(seq, mono, bytes)`(block 안 함). 10 ms 또는 64 KiB에 OS write, 그룹 `sync_data` ≤ 100 ms, seal 때 fsync. 64 MiB/60 s 회전. 워터마크 `written_through`/`durable_through`. 디스크 오류 → failed + `last_error`, 5 s마다 재시도, 회복 시 `GapReport` |
| `src/segment.rs` | 경로·목록, 프레임 반복자, `recover_segment/recover_producer_dir/recover_root`(crash tail), `verify_segment/verify_producer_dir` + `quarantine`, `rebuild_index/compute_index` |
| `src/index.rs` | sidecar `<seg>.idx`: `LOOMIX1\0` + 24 B 고정 레코드 `(key, mono_ns, offset)`. 4096건 또는 1 s 규칙(`IndexPolicy`), writer와 재구축이 같은 상태기계를 씀 |
| `src/reader.rs` | `scan(dir, ScanQuery{range: SourceSeq|Mono, limits{scan_bytes, result_bytes, deadline}, resume})` → `ScanResult{records, truncated, resume_from, corruptions, scanned_bytes}`. active 세그먼트의 written 구간도 읽음 |
| `src/mapping.rs` | `MappingLog`(rec_type 4, json, `<root>/_mapping/<stream_epoch>/`) + `lookup_mappings`/`lookup_seq` |
| `src/retain.rs` | `retain(root, policy, pins)` → `RetainReport{level: Ok/Warn/Prune/Full, deleted, …}`, `retention-manifest.jsonl` |
| `src/gap.rs` | `GapReport{producer_id, producer_epoch, from, to: Option, count: GapCount, cause, bytes}` + `to_gap_data()`(loom-proto `GapData`) |

의존성: loom-proto, serde, serde_json, crc32c, thiserror, tracing (dev: tempfile). 추가 의존성은 필요 없음. tokio도 쓰지 않음(std I/O 전용 스레드).

## 2. 시험 — 23개 통과

- 단위 5: 헤더/프레임/hint 왕복·손상, 인덱스 규칙, 큐 상한·budget.
- `tests/roundtrip.rs` 6: JSON SourceRecord 1만 건 왕복(값 일치), 크기·시간 회전, 그룹 sync 워터마크 전진(written이 먼저, durable이 나중), json·pcap-epb 인덱스 재구축 결과 동일, scan 결과 상한·스캔 상한·deadline·resume·mono 범위, 처리량 smoke.
- `tests/recovery.rs` 4: tail 쓰레기 37 B → 정확히 37 B 잘림 + `crash_tail` gap(from = 마지막 seq+1), 마지막 프레임 일부 잘림, 마지막 프레임 CRC 손상(그 프레임 전체 길이만큼 잘림), 헤더만 있는 파일, sealed 중간 손상 → `corruption` + quarantine, 파일 길이 그대로, 읽기는 오류 지점 전까지(인덱스가 그 뒤로 점프해도).
- `tests/faults.rs` 5: 큐 가득 → `Full` + health failed + 정확한 `journal_queue_full` gap(9..99, 91건), 공유 budget, fault hook 쓰기 오류 → failed → 회복 시 `disk_error` gap(100..199, 100건) 그리고 scan에서도 구멍이 같음, 쓸 수 없는 root(파일) → 회복, sync 실패 → `sync_failed` gap(count unknown).
- `tests/mapping.rs` 2: 2000건 왕복·범위 조회·단건 조회·페이지 조회, 디스크 오류 중 `Backpressure`(약 50 ms block) 후 같은 항목 재시도 → **누락 0건**.
- `tests/retain.rs` 1: 나이 GC, quota 90 % 정리(오래된 것부터), pin·active 보존, 100 % → `Full`, manifest 줄 수 = 삭제 수.

전체 실행 약 3 s(debug). 3회 반복 모두 통과.
품질 검사: `cargo fmt -p loom-journal --check` 통과, `cargo clippy --workspace --all-targets -- -D warnings` 통과, `cargo check --workspace --all-targets` 통과(확인 시점 기준).
참고: `cargo fmt --all --check`는 **loom-gateway**(core-opus가 고치는 중) 파일 diff 때문에 실패한다. loom-journal은 깨끗하다.

## 3. 처리량 smoke (실패 기준 아님, macOS 이 기계)

| 빌드 | 10만 건 append | durable까지 | 속도 | 디스크 |
|---|---|---|---|---|
| release | 217 ms | 224 ms | **약 447k rec/s, 187 MB/s** | 41.9 MB (419 B/rec, 프레임 포함) |
| debug | 1.72 s | 1.73 s | 약 58k rec/s | 같음 |

`Full` 0건. JSON 인코딩은 호출 스레드에서 한다(큐 바이트 계산을 정확히 하려고). release에서는 이 인코딩이 병목이다.

## 4. 계약·지시와 다른 점, 결정한 것

1. **active/sealed 구분** = 파일 이름. 쓰는 중에는 `<seg>.lj.open`, seal(fsync → rename) 후 `<seg>.lj`. 계약의 "close → fsync → rename"을 이렇게 해석했다. 인덱스는 `<seg>.idx`, quarantine 표시는 `<seg>.lj.quarantine`(JSON: offset, reason).
2. **opaque codec(pcap-epb/msgpack)의 seq/mono**: payload를 해석하지 않으므로 `index_hint`(rec_type 2, 17 B: seq, mono, index_point flag) 프레임을 세그먼트 첫 레코드 앞, seq가 +1이 아닐 때, 인덱스 지점마다 쓴다. 나머지 레코드의 seq는 앞 seq+1로 계산한다. 이런 레코드는 mono를 저장하지 않는다(`mono_ns: None`). 그래서 opaque codec의 mono scan은 hint 간격 정밀도다. eth가 더 정밀하게 원하면 EPB 타임스탬프를 직접 해석해야 한다.
3. **CRC는 payload만** 덮는다(계약 그대로). `rec_type` 바이트 손상은 "알 수 없는 값"일 때만 검출된다 — 계약 보강 후보.
4. **health 의미**: 손실이 생기면 `failed`이고, 호출자가 `take_gaps()`로 gap을 가져가고 디스크도 정상이 될 때까지 유지된다. 큐가 50 % 이상 차면 `degraded`다. Full이 난 seq는 소비된 것으로 처리한다(같은 seq 재시도 → `OutOfOrder`). 큐 overflow gap은 다음 append가 성공할 때 닫힌다. 회복한 손실은 marker 프레임(rec_type 3, `{"marker":"gap", gap}`)으로 저널 안에도 남긴다.
5. **디스크 오류 중 데이터**: 큐를 계속 비우면서 버리고 개수를 센다(메모리 상한 유지). 그래서 그 구간의 `try_append`는 Ok를 돌려준다. 손실은 GapReport와 health로만 알 수 있다. mapping은 반대로 큐를 비우지 않아 backpressure가 걸린다.
6. **sync 실패**: `sync_failed` gap을 count `unknown`으로 낸다(durable+1 ~ written). `durable_through`는 sync되지 않은 데이터를 넘어 올라가지 않는다. 그 파일은 `.open`으로 남고, 다음 복구가 seal한다.
7. **scan 상한**: `byte_limit` 하나 대신 `scan_bytes`(256 MiB)와 `result_bytes`(64 KiB) 두 개를 두었고, deadline 기본값은 2 s(`ScanLimits::rpc_default()`)다. 결과가 비어 있으면 레코드 1건은 상한을 넘어도 돌려준다(진행 보장). `resume_from` = `Position{seg_no, offset, last_seq}`.
8. **quota 90 %** 정리 목표는 "< 90 %"(히스테리시스 없음). `_mapping`은 사용량에는 넣지만 지우지 않는다(정본). 나이 기준은 파일 mtime(= seal 시각).
9. **mapping의 `durable_through`**는 `seq_first` 기준이다(index key). 그 항목이 durable이면 `seq_last`까지 durable이다.
10. sparse 인덱스는 SQLite 대신 sidecar다(지시대로 M1). zstd는 넣지 않았다(M2).
11. 같은 producer epoch로 다시 spawn하면 남은 `.open`을 먼저 복구하고 seq 순서를 이어 간다. 복구가 IO 오류로 실패해도 spawn은 실패하지 않는다(`last_error`에 기록, 쓰기에서 failed).

## 5. gateway가 쓸 API 요약

```text
let w = JournalWriter::spawn(JournalConfig::new(root, "gateway", epoch))?; let h = w.handle();
h.try_append(&source_record)  -> Result<(), AppendError::{Full{source_seq}, OutOfOrder, …}>   // 안 막힘
h.status() -> JournalStatus{health, written_through, durable_through, queue_*, drops, last_error}; .to_proto() / .queue_stat()
h.take_gaps() -> Vec<GapReport>;  gap.to_gap_data() -> loom_proto GapData  (record.gap payload)
w.flush(timeout) / w.close() -> CloseReport{status, gaps}
let m = MappingLog::spawn(JournalConfig::mapping(root, stream_epoch))?;
m.append_mapping(&MappingEntry{..}) -> Err(MappingError::Backpressure)이면 발행을 늦추고 같은 항목 재시도
lookup_mappings(root, stream_epoch, seq_from, seq_to, &ScanLimits::rpc_default(), resume) / lookup_seq(root, epoch, seq)
scan(&producer_dir(root, pid, pepoch), &ScanQuery{range: ScanRange::SourceSeq{from,to}, limits, resume}) -> truncated면 E_SCAN_LIMIT + resume_from
recover_root(root, &IndexPolicy::default()) (기동 시 crash_tail) / verify_producer_dir / retain(root, &RetainPolicy::default(), &pins) -> level Full이면 record.health=failed
```

## 6. 기타
- 파일 읽기가 30초 넘게 멈춘 일은 없었다.
- 추가 의존성 요청 없음.
- 2단계(worker IPC)는 지시대로 시작하지 않았다.
