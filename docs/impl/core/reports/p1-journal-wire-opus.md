# P1 M1 저널 연결 · 정책 파일 · 지연 하네스 (opus) 보고서

작업 지시: `docs/impl/core/tasks/p1-journal-wire-opus.md` · 워크트리 `.worktrees/core`(기준 c993961) · 커밋하지 않음.
수정 범위: `crates/loom-gateway`, 새 crate `crates/loom-bench`. `loom-journal`·`loom-proto`·`loom-worker-*`·루트 `Cargo.toml`은 수정하지 않음.

> **주의(리드)**: 마무리 시점 `git status`에 **병합이 진행 중**이었다. `Cargo.lock` = `UU`(충돌 미해결)이고, 다른 팀 crate 다수가 staged 상태였다. 나는 git 명령을 쓰지 않았다. 다만 이 작업의 `cargo build/test`가 `Cargo.lock`을 다시 썼다(새 `loom-bench`, gateway의 `loom-journal`·`toml` 의존). 병합을 마칠 때 `Cargo.lock`을 확인해야 한다. 그 상태에서도 workspace 전체 시험은 통과했다.

## J1. 저널 연결 — 완료 (`src/journal.rs`, `daemon.rs`, `hub/mod.rs`, `worker_host.rs`)
- **세션 디렉터리**: `$LOOM_HOME/records/<YYYY-MM-DD>/s<stream_epoch>/`(daemon_session = `s<epoch>`). `manifest.json`에 OS·arch·loomd 버전·stream_epoch·env·정책 hash와 파일 목록·도메인/worker 상태·pid를 담는다. 정책이 바뀌면 다시 쓴다.
- **기동 시 복구**: 모든 세션 디렉터리에 `recover_root` → crash tail은 `record.gap` 발행.
- **gateway 이벤트 저널**: 허브에 새 **pre-publish 훅**을 두었다(허브 스레드, stream seq 부여 직전).
  - producer `gateway`/`gateway.<domain>` 레코드는 **허브가 발행 순서대로 producer별 source_seq를 다시 매긴다**. 이유: 저널은 seq가 엄격히 증가해야 하는데, 여러 태스크가 만든 레코드는 생성 순서와 발행 순서가 다를 수 있다. 같은 이유로 producer_epoch = daemon epoch로 둔다.
  - 그 뒤 `try_append`(non-blocking).
- **인프로세스 worker 원본**: `HubSink`가 화면 경로(허브)보다 **먼저** 저널에 쓴다(화면에서 드롭돼도 원본은 남음). 분기는 `HubSink::new(.., journal: Option)` + `WorkerHost::journals_itself()`(기본 false = gateway가 씀; IPC 호스트는 true를 돌려주고 `None` sink를 쓸 것).
- **producer id 변경**: `worker:echo` → `worker.echo`, `gateway:<domain>` → `gateway.<domain>`. 저널 디렉터리 이름이 producer id라 `:`가 Windows 파일명에서 불가하기 때문이다. **`loom-journal`이 producer id를 디렉터리 이름으로 쓸 때 정규화하는 것을 제안**한다.
- **mapping 로그**: 허브 `MappingSink` = `JournalMapping`(`MappingLog`).
  - 허브 mapping은 이제 **stream seq와 source seq가 둘 다 연속일 때만** 한 항목으로 합친다(1:1 대응 보장). `mono_first/last`도 싣는다(`MappingRecord`에 필드 추가).
  - 버리지 않는다: `Backpressure`면 허브 스레드가 같은 항목을 다시 시도한다(발행이 늦어짐).
  - **계약과 다른 점**: 2 s 넘게 밀리면 그 항목을 포기하고 `record.gap{cause:"mapping_backpressure", seq_range}` + 기록 불능(failed)으로 바꾼다. 디스크가 영구 고장일 때 허브가 무한정 멈추면 **STOP·라이브 화면까지 멈추기** 때문이다. 원본은 producer 저널에 남는다.
- **기록 불능**:
  - 판정: `record_health()` = 감사·저널(writer health + 손실 플래그)·보존 수준 중 가장 나쁜 것. `Full`/쓰기 오류/mapping 손실 → failed → agent·script R1+ 쓰기 `E_RECORDING_FAILED`(exit 4). user 입력·송신은 허용(기존 감사 경로와 같은 판정).
  - 1 s 주기 작업이 writer gap을 `record.gap`으로 발행하고, 전체 health가 바뀌면 `record.health{health, causes{audit, journal, retention}}`를 발행한다(회복 시 `ok`도). gap을 보고한 다음 주기에 손실 플래그가 풀린다.
- **질의**:
  - `core.events.query`:
    - 다른 epoch의 커서 `e<E>:seq:N` → 그 실행의 세션 디렉터리에서 mapping → 세그먼트 scan(`source:"journal"`, stream seq는 실행 당시 그대로).
    - 같은 epoch에서 시작이 링보다 오래되면 저널 구간 + 링 구간을 이어 붙인다(`source:"journal+ring"`).
    - 상한: 결과 64 KiB·1000행, 스캔 `Config.scan_bytes`(기본 256 MiB)·2 s. 넘으면 `E_SCAN_LIMIT`(exit 8) + `detail.resume_from`(커서) + `detail.partial`. mapping이 없으면 `E_CURSOR_GAP`.
    - 디스크 읽기는 `spawn_blocking`.
  - `CallCtx::journal().scan`: `Cursor::Seq`가 링 밖이거나 다른 epoch이면 같은 경로(`resume_from`·`complete`).
- **보존**: 시작 시 + 10분마다(`Config.retain_interval`) 실행한다. 정책은 `[record] max_age_days, max_bytes`(기본 30일/20 GiB).
  - `loom-journal::retain`은 세션 루트 하나만 다루므로 **세션 전체에 걸친 quota는 gateway가 조립**했다. 먼저 나이 기준으로 각 세션을 정리하고, 전체가 90 % 이상이면 오래된 세션부터 `max_bytes`를 계산해 넣어 < 90 %가 될 때까지 지운다.
  - 수준: 80 % 경고·90 % 정리 → degraded, 100 % → failed.
  - pin: `Journal::pin(session, SegmentId)`(M1 수동 API). active 세그먼트와 `_mapping`은 지우지 않는다. 감사 디렉터리는 대상이 아니다.
  - **리드에게**: `retain`이 여러 세션 루트(`records/` 전체)를 직접 다루는 API가 있으면 조립 코드를 없앨 수 있다.
- `--no-record`는 M2(자리 없음 — 채널 설정이 아직 없음).
- 종료: `loomd`가 `core.shutdown`/시그널을 받으면 `journal.close()`(flush·seal)를 호출한다.

### 시험 (`tests/journal.rs` 4개)
1. 재기동 후 이전 실행의 seq를 `core.events.query`로 다시 읽음: 같은 stream seq, `source:"journal"`. gateway 쓰기 행(requested ×3)도 기록됐는지, manifest를 확인한다. scan 예산 1 B → `E_SCAN_LIMIT` + `resume_from`, 모르는 epoch → `E_CURSOR_GAP`.
2. 저널 디스크 오류 주입(`Config.journal_fault`) → `record_health=failed` → agent `echo.send` `E_RECORDING_FAILED`, viewer 입력은 허용. 디스크 회복 → `record.gap` 보고 + `record.health` failed→ok, 이후 agent 쓰기 성공.
3. 보존: 40일 지난 sealed 세그먼트 2개 중 pin한 것만 남음. 작은 quota에서 pinned만 남으면 `Full`.
4. 정책 파일(J2).
- 같은 epoch에서 링 밖으로 내려가는 경로는 시험이 없다. 링 32 MiB를 넘기는 데이터를 만들어야 해서 링 크기 설정이 필요하다(후속).

## J2. 정책 파일 — 완료 (`src/policy.rs`, `toml`)
- 파일: 프로젝트 `--config <path>`(기본 `$LOOM_HOME/loom.toml`) + 사용자 `$LOOM_HOME/config/policy.toml`.
- 병합은 엄격한 쪽: deny 합집합, 같은 패턴은 더 엄격한 동작(`auto < confirm < approve < deny`), record 한계는 최솟값, env는 프로젝트 우선. 모르는 `[policy]` 키는 거절(`deny_unknown_fields`).
- 키: `[policy] deny = [...]`, `[policy.actions] "<method|glob>" = "auto"|"deny"|"confirm"|"approve"`, `[record] max_age_days, max_bytes`, `[env] env`.
- **confirm/approve = M1에서는 deny로 집행** + 경고. 거절 메시지에 "M2 전까지 deny로 처리, 사용자에게 물을 것"을 넣고, `detail.action`을 붙인다. 사용자가 켠 안전장치를 조용히 끄지 않기 위함이다.
- 변경 감지: 쓰기마다 두 파일의 mtime·크기를 비교하고, 바뀌면 다시 읽어 hash를 만든다(감시 스레드 없음).
  - 성공 → `core.policy_changed{ok:true, hash, deny, actions, warnings, env}` + 감사 `policy.changed` + manifest 갱신. env가 바뀌면 `core.env`.
  - 파싱 실패 → 직전 정책을 유지하고 `core.policy_changed{ok:false, error, kept_hash}` + 감사.
  - **기동 시 파싱 실패는 기동 거절**(모르는 정책으로 돌지 않음).
- env: `--env`(이제 선택 인자) > 정책 파일 `[env] env` > bench. `vehicle`이면 `env_vehicle` 경고.
- `core.status.policy` = mode·deny·actions·hash·warnings·files, `core.status.record` = 세션·health.
- 시험: 단위 2(파싱·병합·glob, 재로드 시 직전 유지) + 통합 1(위 4번: env 파일, approve→deny, deny, 깨진 파일 유지 + 배너, 프로젝트 ∪ 사용자, 기동 시 깨진 파일 거절).

## J3. 지연 하네스 `crates/loom-bench` — 완료
- `loom-bench [--scenario idle|burst|cli|all] [--n 200] [--rate 1000] [--secs 10] [--cli-runs 30] [--out-dir bench/results]`. 기본 `all` 약 15 s.
- **코어 게이트**: 같은 프로세스에서 `loomd`를 임시 `LOOM_HOME`·포트 0으로 띄운다.
  - 하네스 시계를 `Config.clock`으로 주입해 인프로세스 echo worker가 그 시계로 `ts.mono_ns`를 찍는다(host ingest). Rust WS viewer(쿠키, `echo.data` 구독, `max_batch_ms` 1)가 받은 순간을 같은 `Instant` 기준으로 잰다 → 프로세스 간 시계 정렬 불필요.
  - 송신은 `Daemon::call`(agent, 쓰기 파이프라인 전체: 감사 intent 포함).
  - idle = 1건씩 5 ms 간격, burst = 1000 ev/s × 10 s(태스크 동시).
- **CLI 왕복**: `loom --json echo send`를 별도 프로세스로 N회 실행한다(워밍업 1회 제외, `LOOM_NO_AUTOSTART=1`). `loom`은 `loom-bench` 옆이나 `LOOM_BIN`에서 찾는다.
- 결과 JSON: `bench/results/<scenario>-<os>-<date>.json`(os·arch·cpus·profile·method 포함). 이번 실행분 `bench/results/all-macos-2026-11-11.json`이 작업 트리에 있다(커밋 여부는 리드 판단).

### 수치 (macOS, release, 부하 평균 약 10–22 — 다른 에이전트 다수 실행 중, **기준 머신 아님**)
| 프로파일 | p50 | p95 | p99 | max | 표본/누락 |
|---|---|---|---|---|---|
| core idle (worker ingest → viewer) | 0.13–0.16 ms | 0.28–1.9 ms | 0.6–5.3 ms | 0.7–9.6 ms | 200 / 0 |
| core burst 1k ev/s 10 s | 1.8–2.1 ms | 4.6–16 ms | 13–30 ms | 31–101 ms | 10 000 / 0 |
| CLI `echo send` 왕복 | 12–13 ms | 52–84 ms | 60–106 ms | | 30 / 0 실패 |
- (release 3회 범위. debug 짧은 실행: idle p95 0.74 ms, burst p95 2.05 ms.) burst의 꼬리와 CLI 꼬리는 부하에 크게 흔들린다 → 유휴 기준 머신에서 다시 재야 한다. 계약 목표(코어 우선 ≤ 5 ms, UART ≤ 25 ms, CLI 왕복 p95 ≤ 30 ms)와는 아직 판단하지 않는다.
- **한계**: 인프로세스 echo만 잰다(worker IPC·프로세스 간 시계는 IPC 호스트 이후). viewer는 Rust 클라이언트라 브라우저 렌더는 포함되지 않는다. burst 송신은 쓰기 파이프라인을 거치므로 감사 쓰기 비용이 포함된다.

## 품질
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`(약 84 s) 모두 통과.
- 시험 출력에 보이는 `loom-worker-host/src/bin/loom-test-worker.rs` panic 한 줄은 다른 작업자 crate의 시험용 worker가 내는 것이고, 그 시험도 통과다.
- `loom-gateway` 시험: 단위 6 + daemon 9 + hub 5 + **journal 4** + replay 8 = 32.

## 리드에게 / 의존성
- 루트 `Cargo.toml` 변경 없음. gateway에 `loom-journal`·`toml` 추가(workspace에 있음). `loom-bench` 의존은 loom-gateway·loom-proto·loom-worker-sdk·tokio·tungstenite·serde_json·clap·tempfile(모두 workspace).
- `loom-journal`에 바라는 것:
  - (1) 여러 세션 루트를 한 번에 다루는 `retain`.
  - (2) producer id → 디렉터리 이름 정규화(`:` 등).
  - (3) `MappingLog::take_gaps` 또는 health 회복 규칙 명시. 시험에서는 디스크 회복 후 ok로 돌아오는 것을 확인했다.
- IPC worker 호스트를 붙일 때: `WorkerHost::journals_itself() -> true`, 그 호스트의 sink는 `HubSink::new(hub, pending, None)`. worker 프로세스가 같은 세션 디렉터리(`records/<date>/s<epoch>/`)에 쓰도록 경로를 넘겨야 한다(현재 `Journal::root`).
- 남은 것: 링 크기 설정(같은 epoch 저널 경로 시험), `--no-record`(M2), 테스트 증거 자동 pin(M2), 기준 머신 하네스 측정.
