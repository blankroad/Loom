docs/plan/02-core-principle.md 준수

> **질문 창 금지**(playbook §6): 대화형 질문 창을 띄우지 않는다. 계약과 이 지시서로 판단해 진행하고 결정을 보고 파일에 적는다. 판단할 수 없으면 질문을 보고 파일에 적고 할 수 있는 부분까지 끝낸다.

# P1 M1 — core-opus: gateway에 저널 연결 · 정책 파일 · 지연 하네스

작업 위치: `/Users/ctmctm/workspace/Loom/.worktrees/core`(기준 `c993961`). 근거: `docs/contract/06-journal.md`, `04-safety.md` §4.7–4.8, `docs/plan/core/plan.md` §6.2·§9.4, 저널 보고 `/Users/ctmctm/workspace/Loom/docs/impl/core/reports/p1-journal.md`(API 요약 포함), crate `crates/loom-journal`(커밋됨 `1e0cba2`).
**범위 경계**: `crates/loom-gateway`, 새 crate `crates/loom-bench`. 리드 하위 작업자가 동시에 `crates/loom-worker-host`·`crates/loom-worker-sdk`를 고치고 있다 — 둘은 건드리지 않는다(IPC WorkerHost 연결은 그 작업이 끝난 뒤 따로). `loom-journal`·`loom-proto`·루트 `Cargo.toml`도 수정하지 않는다(버그·부족한 API는 보고). workspace에 `toml`이 추가됐다.

## J1. 저널 연결 (C13 gateway 쪽)
- `loomd` 기록 세션: `$LOOM_HOME/records/<YYYY-MM-DD>/<daemon_session>/` + `manifest.json`(OS·빌드·도메인·정책 hash·env).
- **gateway 이벤트 저널**: 허브가 발행하는 이벤트 중 producer = `gateway`인 것(쓰기 행·latch·policy·mark·test 마커·activity 등)과 **인프로세스 worker가 내는 원본**(지금은 echo가 인프로세스 — IPC worker가 붙으면 worker가 직접 쓰므로 그때는 gateway가 쓰지 않음; 이 분기를 `WorkerHost` 구현 종류로 나눠 둘 것)을 `loom-journal` writer에 `try_append`. `Full`/디스크 오류 → 그 producer `record.health=failed` 발행 + `record.gap` + 파이프라인의 기록 불능 차단(계약 04 §4.7, 이미 감사 실패 경로가 있으니 같은 플래그로).
- **mapping 로그**: 허브 `MappingSink`를 `loom-journal` mapping writer로 구현(버리지 않음 — append 실패 시 허브가 발행을 늦추는 계약; 지금 허브 구조에서 가능한 방식으로, 불가하면 보고).
- **질의**: `CallCtx::journal()`과 `core.events.query`가 링 밖이면 mapping + 저널 scan으로 내려간다(바이트·시간 상한, 넘으면 `E_SCAN_LIMIT` + `resume_from`, 보존 범위 밖 `E_CURSOR_GAP`).
- **보존**: 데몬 시작 시와 10분마다 `retain`(30일/20 GiB 기본, `loom.toml [record]`로 변경), 80/90/100 % 단계에 맞춰 `record.health` degraded/failed. 감사 디렉터리는 대상 아님.
- `--no-record` 채널은 M2(지금은 자리만).
- 시험: echo 송신 후 데몬 재기동 → 링 밖이 된 seq를 `core.events.query`로 다시 읽음(mapping 경유), 저널 디렉터리 쓰기 불가 → agent 쓰기 exit 4 + user 입력 허용 + `record.gap` 회복 보고, 질의 상한 → `E_SCAN_LIMIT`, GC가 pin(지금은 수동 pin API로 시험)을 남김.

## J2. 정책 파일 (계약 04 §4.8, F1)
- `toml`로 `$LOOM_HOME/loom.toml`(프로젝트 경로 인자 `--config`도) + `~/.loom/config/policy.toml` 병합(더 엄격한 쪽: deny 합집합). M1 지원 키: `[policy] deny = [...]`, `[policy.actions] "<method or glob>" = "auto"|"deny"`(confirm/approve 값은 파싱하되 M1에서는 **`E_NOT_IMPLEMENTED` 대신 경고 + auto로 동작하지 말고 `deny`로 처리**하고 경고 — 사용자가 켠 안전장치를 조용히 끄지 않기 위함), `[record] max_age_days, max_bytes`, `[env] env = "sim|bench|vehicle"`. 파싱 실패 = 직전 유효 정책 유지 + `core.policy_changed{ok:false}` 배너. 파일 변경 감시는 송신 전 mtime/hash 검사로(별도 감시 스레드 없이). 모든 변경 감사.

## J3. 지연 하네스 `crates/loom-bench` (C16, plan §9.4)
- bin `loom-bench`: (a) **코어 게이트** — 실제 `loomd`(임시 `LOOM_HOME`)에 echo로 N건을 보내고 Rust WS viewer 구독자가 받은 시각과 worker host-ingest `ts.mono_ns` 차이를 같은 호스트 monotonic으로 계산(p50/p95/p99/max, 표본·미수신 수). 프로파일: `idle`(1건씩), `burst`(1k ev/s 10 s). (b) **CLI 왕복** — `loom echo send` 프로세스 N회 p50/p95. 결과 JSON을 `bench/results/<scenario>-<os>-<date>.json`(저장소 `bench/` — `.gitignore` 대상 아님, 결과 파일은 커밋하지 않음: 리드가 판단).
- host monotonic: 지금 `SystemClock`은 프로세스 기준이므로, 같은 프로세스 안(인프로세스 echo)에서만 비교하거나 `Instant` 기준을 공유하는 방법으로. 다른 프로세스 간 비교는 IPC 이후 — 보고에 한계 명시.
- 한 번 실행 10분 이내(기본 1분 이내).

## 품질·보고
workspace 전체 fmt·clippy(`-D warnings`)·test(다른 작업자 crate가 잠시 깨져 있으면 `-p loom-gateway -p loom-bench`로 하고 보고). 커밋하지 않는다. 보고 `/Users/ctmctm/workspace/Loom/docs/impl/core/reports/p1-journal-wire-opus.md`(J1–J3, 시험 수, 하네스 수치, 계약과 다른 점). 화면에는 경로와 3줄 요약만. 10분 넘는 실행 금지.
