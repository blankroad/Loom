docs/plan/02-core-principle.md 준수


> **질문 창 금지**(playbook §6): 작업 중 사용자·리드에게 대화형 질문 창(선택지 묻기 등)을 띄우지 않는다. 계약(`docs/contract/`)과 이 지시서로 판단해 진행하고 그 결정을 보고 파일에 적는다. 판단할 수 없으면 질문을 보고 파일에 적고 할 수 있는 부분까지 끝낸다.

# P1 M1 — 저널 `crates/loom-journal` (+ 이후 worker IPC `crates/loom-worker-host`)

작업 위치: `/Users/ctmctm/workspace/Loom/.worktrees/core`(브랜치 `core/main`). 저장소 루트(main 체크아웃)에서는 코드를 고치지 않는다. 근거: `docs/contract/06-journal.md`, `05-worker-protocol.md`, `docs/plan/core/plan.md` §6.2·§9.2(Q2·Q18·Q25)·§3.3, 마스터 E5·E6.
**범위 경계**: `crates/loom-journal`(새로 만듦)만 수정한다(2단계에서 `crates/loom-worker-host` 새로 만듦, `crates/loom-worker-sdk`의 `run_worker` 구현). core-opus가 동시에 `crates/loom-gateway`를 고치므로 그 crate와 `crates/loom-proto`, 루트 `Cargo.toml`은 건드리지 않는다(필요 의존성은 보고). workspace에 이미 있는 것: serde/json, crc32c, sha2, tempfile, tokio, thiserror, tracing. 루트 workspace.dependencies에 `loom-journal`·`loom-worker-host` path 항목이 이미 있다.

## 1단계: `loom-journal` (먼저 끝낼 것)
Cargo.toml은 `crates/loom-proto/Cargo.toml` 형식을 따른다(`version.workspace = true`, `[lints] workspace = true`, 의존은 `.workspace = true`).
- **세그먼트 형식**(계약 06 §6.2): 경로 `<root>/<producer_id>/<producer_epoch>/<seg_no:06>.lj`, 헤더 `magic "LOOMJ1\0\0" | u16 format_ver=1 | u16 codec | u16 checksum_alg=1(crc32c) | producer_id(u16 길이+UTF-8) | producer_epoch u64 | created wall_ns u64 | clock_id(u16 길이+UTF-8)`, 레코드 `u32 len | u32 crc32c(payload) | u8 rec_type(1=event, 2=index_hint, 3=marker) | payload`. 모든 정수 LE. codec 레지스트리: 1 = `json/1`(M1), 2 = `msgpack/1`(예약), 3 = `pcap-epb/1`(eth, 바이트를 그대로 저장 — 이 crate는 payload를 해석하지 않는 경로도 지원).
- **Writer**(worker 프로세스 안 전용 OS 스레드): `JournalWriter::spawn(cfg) -> (JournalHandle, …)`. 입력 큐 bounded(채널 8 MiB·worker 합 16 MiB/32k건 — 설정), `try_append(SourceRecord 또는 raw bytes) -> Result<(), Full>`(block 안 함 — 넘치면 호출자가 `record.gap` 처리; 상태 `health()`가 `failed`로), OS write ≤ 10 ms 또는 64 KiB, `sync_data` 그룹 ≤ 100 ms, seal 시 fsync. **워터마크** `written_through`·`durable_through`(source_seq) 공개. 회전 64 MiB 또는 60 s. 디스크 오류 → health failed + 오류 원인, 5 s마다 재시도, 회복 시 손실 구간을 `GapReport{from, to, count|unknown, cause}`로 돌려준다.
- **복구**: 열린 세그먼트 재오픈 시 마지막 유효 CRC 레코드까지 잘라내고 잘라낸 바이트 수 보고(`cause:"crash_tail"`). sealed 세그먼트 중간 CRC 오류는 잘라내지 않고 `corruption`으로 보고 + 그 파일을 quarantine 표시(읽기는 오류 지점 전까지).
- **sparse 인덱스**(M1은 SQLite 대신 sidecar `<seg>.idx`): 4096건 또는 1 s마다 `(source_seq, mono_ns, file_offset)` 고정 크기 레코드. 세그먼트에서 재구축 가능(`rebuild_index`).
- **Reader**: `scan(range: SourceSeq 범위 | mono 범위, byte_limit, deadline) -> ScanResult{records, truncated, resume_from}` — 인덱스로 시작 위치를 찾고 순차 해제. 상한 넘으면 `truncated` + `resume_from`(호출자가 `E_SCAN_LIMIT`). active 세그먼트의 written 구간도 읽을 수 있어야 한다.
- **mapping 로그 writer**: gateway용 같은 형식(codec json, rec_type marker 또는 별도 rec_type 4=mapping)으로 `(stream_epoch, seq_first, seq_last, producer_id, producer_epoch, src_first, src_last, mono_first, mono_last)` 기록 + 조회(`stream seq → source ref 범위`). **버리지 않는다**: 큐가 차면 `append_mapping`이 짧게 block(≤ 50 ms) 후 오류를 돌려 호출자가 발행을 늦추게 한다.
- **보존 GC**: `retain(root, max_age, max_bytes, pins)` — sealed 세그먼트 단위, active·pinned 제외, 80/90/100 % 단계 반환값, 삭제 내역을 `retention-manifest.jsonl`에.
- 압축(zstd)은 M2 — 넣지 않는다.
- **시험**(`tests/`): 왕복(JSON SourceRecord 1만 건), 회전, 그룹 sync 워터마크 전진, 강제 tail 손상 후 복구(잘린 바이트 수 정확), sealed 중간 손상 → corruption, 인덱스 재구축 결과 동일, scan 상한·resume, 큐 가득 → `Full` + health failed, 디스크 쓰기 실패 주입(쓰기 불가 디렉터리 또는 fault hook) → failed → 회복 시 GapReport, mapping 왕복·조회, GC(pin 보존). 처리량 smoke(10만 건, 결과만 출력, 시험 실패 기준 아님). 전체 1분 이내.

## 2단계: worker IPC (1단계 보고 후 리드 지시가 있으면 진행 — 지금은 1단계만)

## 품질·보고
`cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test -p loom-journal`(그리고 workspace 전체가 깨지지 않는지 `cargo check --workspace`). `unsafe` 금지. 무한 큐 금지(상한·넘칠 때 정책을 코드 상수와 doc 주석으로). 커밋하지 않는다. 보고 `/Users/ctmctm/workspace/Loom/docs/impl/core/reports/p1-journal.md`(만든 것·시험 수·처리량 smoke 수치·계약과 다른 점·gateway가 쓸 API 요약 10줄). 10분 넘는 실행 금지. 파일 읽기가 30초 넘게 멈추면 보고에 적고 멈춘다.
