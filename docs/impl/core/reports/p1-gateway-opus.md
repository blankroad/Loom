# P1 gateway (opus) 보고서

작업 지시: `docs/impl/core/tasks/p1-gateway-opus.md` · 워크트리 `.worktrees/core` (`core/main`, 기준 632579b) · 커밋하지 않음(리드 커밋).

## G1 허브 — 완료

### 한 일
- `crates/loom-gateway/src/hub/mod.rs` — 실제 허브.
  - **전용 OS 스레드** `loom-hub`(tokio 밖)에서 시퀀서가 돈다. 입력은 `std::sync::mpsc::sync_channel` 하나(용량 = 데이터 16 384 + 예약 4 096).
  - **입력 예산**: 데이터 publish는 원자 카운터로 16k건 / 8 MiB(추정 바이트)까지만 받는다. 넘으면 `Err(recs)` → `Hub::count_drops`가 producer별로 `(first,last,count)`를 모으고, 허브가 다음 턴에 `display.gap{cause:"hub_overload", producer_range, count}`를 발행한다(producer별 공정 드롭 + gap). 우선 kind는 데이터 예산에 포함하지 않고 블로킹 send(채널 예약 칸) → 정상 경로에서 실패하지 않음.
  - **링**: 채널별 ≤ 4 MiB, 전체 ≤ 32 MiB(바이트 = 직렬화된 JSON 길이).
  - **한 번 직렬화**: `Published{rec, json: Arc<str>, prio, at}` — 링·모든 구독·snapshot이 같은 텍스트를 공유. `core.events` 프레임은 문자열 이어붙이기로 만든다(재직렬화 없음).
  - **리듀서**: `HubBuilder::reducer(domain, Box<dyn Reducer>)`(Registry `take_reducers()` 출력) + 코어 리듀서(latch, env, record_health, worker state, 진행 중 쓰기 ≤ 1024 — `final:true`·failed·outcome_unknown이면 제거). snapshot 첫 항목은 `core.snapshot`.
  - **구독**(01 §1.4–1.5): 등록과 snapshot 캡처(H, backlog, 리듀서 상태)를 허브 한 턴에서 원자적으로. `after_seq`+`stream_epoch` 재개 — 링 안이면 이어 보내기, 링 밖이면 `display.gap{cause:"resume_out_of_ring"}` + snapshot, epoch 다르면 `core.epoch_changed` + snapshot. `snapshot:true`면 항상 snapshot.
  - **우선 kind**: `is_priority_kind`이면 viewer 연결의 control 큐로만 한 번(데이터 구독에 중복 없음, Q10). 큐 256건/256 KiB 넘으면 `prio_overflow` → 연결이 `core.closed{slow_consumer, retry_after_ms:500}` 후 종료.
  - **데이터 큐**: 구독당 2 MiB(계약 "유입률×500 ms, 1–4 MiB"의 중간 고정값 — 아래 차이 참고), 전체 32 MiB. 넘치면 그 구독만 오래된 것 드롭 + 그 연결에만 `display.gap{cause:"slow_client", seq_range}`. lag > 2 s면 큐를 비우고 `display.gap{cause:"jump_to_live"}`.
  - **publish-before-reply**: `Hub::publish()`의 수신자는 모든 연결 큐에 넣은 뒤 풀린다. `Hub::barrier(deadline)`은 이전 명령이 모두 처리되면 `true`, deadline 넘으면 `false`(→ `E_PARTIAL{published:false}`의 근거).
  - **MappingSink** 트레이트(stream 범위 ↔ producer 범위, 같은 producer 연속 구간은 한 레코드로 합침) + `MemoryMapping`(최근 4096). 저널 worker가 `loom-journal` 구현을 꽂는다.
  - 상한: 연결 64, 연결당 구독 8, 전체 32. `stall_for_test`(숨김) — 허브 멈춤 시험용.
- `src/hub/stream.rs` — 연결 쪽 공통 로직(실제 gateway·replay 공용): `start_frames`(응답 **뒤**에 보낼 `snapshot_part{n:1..total}`·`snapshot_end`, 또는 재개 배치), `drain`(control 큐 전부 → 기한이 된 데이터 구독, 적응형 배치 200건/60 KiB/`max_batch_ms`), `heartbeat`(sub_stats 포함), `batch_frame`.
- `src/rpcutil.rs` — JSON-RPC 공통(파싱·64 KiB·batch 배열 거절·오류 본문).
- replay를 새 허브 위로 옮김: `replay/hub.rs` = 레코드 생성·latch 플래그만 가진 `ReplayHub` 래퍼, `replay/conn.rs`는 공통 stream 로직 사용. **replay 시험 8개 그대로 통과.**

### 시험
- `tests/hub.rs` 5개: snapshot 경계(200 구독 × 연속 publish, 경계 손실·중복 없음 = T1 허브 부분), 우선 kind control 단일 경로(T4 허브 부분), 과부하 드롭 집계 + 우선 예약 용량, barrier deadline(허브 멈춤), 재개/epoch 변경.
- `cargo test -p loom-gateway`: hub 5 + replay 8 통과. clippy `-D warnings` 통과, fmt 통과.

### 계약과의 차이 / 메모
- 구독 데이터 큐는 고정 2 MiB(유입률 기반 1–4 MiB 조정은 M1 이후). snapshot 중 live 보관 "≤ 1 MiB/2 s 넘으면 snapshot 취소 + 재시도"는 구독 큐 상한(2 MiB) + jump_to_live(2 s)로 대신한다 — 손실은 gap으로 보고됨.
- 링 상한은 이벤트 수가 아니라 JSON 바이트 기준.
- 필터 `ext`(도메인 push-down)는 허브가 무시(M1).

### 리드에게
- 의존성 추가: `loom-gateway`에 `loom-domain-api.workspace = true`(루트 `Cargo.toml`은 수정 안 함, 이미 workspace에 있음).

## G2 데몬·연결 — 완료

### 한 일
- `src/home.rs`: `LOOM_HOME`(없으면 `~/.loom`, Windows `%USERPROFILE%\.loom`), `run/`·`audit/` 0700, 단일 인스턴스 락 `run/loomd.lock`(std `File::try_lock`, 두 번째 기동 = `AddrInUse` → exit 9), `write_atomic`(임시 파일 + rename, 0600), `DaemonInfo{port,pid,token,version,proto}`, 토큰 = 256bit 랜덤 hex(`rand`), 상수 시간 비교.
- `src/auth.rs`: 토큰 = agent(`want_role:"script"`이면 script로만 좁힐 수 있음), bootstrap 코드(60 s, 1회용) → 쿠키 `loom_session`(HttpOnly·SameSite=Strict·Path=/) = viewer + rights. 세션 최대 256.
- `src/server.rs`: bind 127.0.0.1 — 기본 47123, 사용 중이면 OS 할당(`--port`로 지정, 0 = OS). 모든 요청 guard: Host는 `localhost:<port>`/`127.0.0.1:<port>`만(DNS rebinding 차단), Origin이 있으면 정본/127.0.0.1/`--allow-origin`만, `127.0.0.1`로 연 페이지는 `localhost`로 308, 보안 헤더(CSP·`Permissions-Policy: serial=(), usb=(), hid=(), bluetooth=()`·`Referrer-Policy: no-referrer`·nosniff·`X-Frame-Options: DENY`). `GET /api/v1/bootstrap/new`(Bearer 토큰, `?rights=watch|operate`로 낮추기만), `POST /api/v1/bootstrap {code}` → Set-Cookie, `POST /api/v1/stop`(토큰 또는 쿠키), `--ui-dir` 정적 서빙(경로 탈출 거절, SPA 폴백).
- `src/conn.rs`: Origin 있는(브라우저) WS는 쿠키 없으면 업그레이드 401. hello에서 role 부여(자격으로만, params의 origin 무시). 자격 없음/잘못된 토큰 → `E_DENIED` + `core.closed{auth_expired}`. ping 20 s, 40 s 무응답이면 끊음. 응답 전에 그 호출이 만든 control 이벤트를 먼저 보냄.
- `main.rs`: `loomd` / `loomd run`(`--port --ui-dir --env sim|bench|vehicle --rights --allow-origin`), stdout이 TTY면 `http://localhost:<port>/#bootstrap=<code>` 출력, Ctrl-C 시 `daemon.json` 삭제. `--fixture-replay`는 그대로.
- RPC: `core.hello`, `core.time`, `core.status`(버전·포트·epoch·hw·latch·record_health·env·viewers·연결/구독 수·도메인·worker 상태·정책·마지막 UI 지표), `core.viewer.presence`(visible → `meta.viewers.visible`/`visibility`), `core.metrics.report`(마지막 보고를 `core.status.ui_metrics`에).

### 결정(계약에 명시 없음 → 이렇게 함)
- **stream_epoch = 데몬 시작 unix 초**: 재기동마다 달라지고 따로 저장할 상태가 없음.
- viewer가 `core.viewer.presence`를 보내기 전에는 visible로 간주.
- `bootstrap/new`는 rights를 **낮추는 것만** 허용(`loom ui pair` = watch). 기본 = `--rights`(기본 approve = operate+approve).

## G3 디스패처·도메인·인프로세스 worker — 완료
- `src/daemon.rs` `Daemon::call`: 코어 메서드 표 → 없으면 `<domain>.<…>`를 Registry `CommandSpec`으로. `RoleSet` 검사(agent/script/viewer rights). 읽기 명령 = 핸들러 호출 + 봉투. 쓰기 = G4 파이프라인. job/interactive 클래스 명령은 `E_NOT_IMPLEMENTED`(M2).
- **도메인 RPC 결과 = JSON-RPC `result`에 봉투(`Envelope`) 전체**(ok:false 포함, `others_since_last`는 오류에도). JSON-RPC `error`는 규격 오류와 코어 메서드 오류(−32000 + `ErrorBody`)에만 씀. ← 계약 01/03에 와이어 표기가 없어 이렇게 정함. **확인 필요(리드/UI·CLI).**
- `src/domains.rs`: cargo feature `echo`(기본) → `loom_echo::Echo` 등록 + `EchoWorker` 인프로세스 기동. API 버전 불일치·등록 오류면 그 도메인만 끄고 로그.
- `CallCtx` 실제 서비스: `worker(kind)`(origin·client·request_id·deadline을 실어 `Call`/`Write` 생성), `publish`(도메인 소유 kind만, gateway producer), `journal()` = **허브 링 질의**(`Hub::query`, 최대 1000건·바이트 한도), `approvals()` = no-op(F1).
- `src/worker_host.rs` **`WorkerHost` trait**(call/write/job/input/latch/state) + `InProcessHost`(tokio 태스크, STOP 플래그는 공유 메모리로 먼저 세우고 큐 뒤에 줄 서지 않음). worker 이벤트는 반드시 `EmitSink`(= `HubSink`)를 지남: control lane = 우선(드롭 없음), data lane = 실패 시 `count_drops` → `display.gap{hub_overload}`. `PendingWrites`가 그 요청의 종단 행에 `payload.final:true`를 붙임.

### 리드(IPC 구현)에게 기대하는 것
- `loom-worker-host`가 같은 `WorkerHost`를 구현: `write`는 worker의 쓰기 상태 행이 sink에 들어간 **뒤** 완료, `latch`는 ack로 완료(gateway가 200 ms에서 끊고 `missing` 처리), 죽은 worker는 `E_WORKER_DOWN`, 수신 레코드는 `HubSink::try_push`로 넘김(종단 표시가 여기서 붙음).

## G4 쓰기 파이프라인 v0 + STOP + 감사 — 완료
- 순서: `requested` 행 → 전역 차단(STOP: agent·script, 복구 송신은 예외 / 감사 불능: agent·script) → `classify`(시간 측정, 1 ms 넘으면 경고) → 정책 v0(`loom.toml` `[policy] deny = [...]`, glob `*`; 거절 시 `core.policy_denied`) → idempotency(`_loom.idempotency_key`) → 감사 intent(R2+는 `sync_data` 후) → `accepted` → `presend_check` → 핸들러/worker → 종단 행(worker가 안 냈으면 코어가 냄) → 허브 barrier(기본 2 s) → 감사 result → 봉투. 거절된 쓰기도 `requested → failed(final)` 행으로 보임.
- barrier 시간 초과 = `E_PARTIAL{published:false, write_state}` + `data` 유지(장치 동작은 일어났으므로 failed로 바꾸지 않음).
- 봉투 `meta`: record_health, env, latch, viewers, stream_epoch, visibility + 경고 `no_viewer_attached`/`no_visible_viewer`/`env_vehicle`.
- `others_since_last`: 클라이언트 전역 커서(4096 LRU, 24 h). 세는 것 = 다른 클라이언트의 `requested` 행(요청당 1), `<domain>.input`, `core.latch`, `core.policy_changed`, user의 `core.mark`. 자기 이벤트·device RX 제외. `kinds`·`by_channel`(`<domain>:<channel>`)·`origins`·`from_cursor`. agent에게 1 이상 알리면 `core.agent_cursor` 발행.
- 대화형 입력 `<domain>.input`(viewer operate만, 도메인이 따로 등록하지 않아도 코어가 제공): worker로 바로, 연결+도메인당 lease 1회, `<domain>.input` 이벤트(origin user, `write_state:sent, final:true`), 감사 `input.batch`(줄바꿈 / 1 s / 4 KiB, hex).
- STOP: `core.stop`(모든 viewer·agent·script), `POST /api/v1/stop`. 순서 = 메모리 latch → worker latch(병렬, ≤ 200 ms, `acked`/`missing`) → `latch.json` 영속 → `core.latch` 발행 → 감사. **영속을 발행보다 먼저** 둠: payload의 `persisted`가 실제 값이 되도록(수 ms). 재기동 시 파일이 on·읽기 불가·손상이면 latched(`core.latch` 재발행). `core.stop.release` = viewer operate만. `core.safe_state.list/run`(Registry safe_state, 인터록·deny면 `finally_blocked`, origin user + latch 예외).
- 감사 `src/audit.rs`: `audit/audit-YYYY-MM.jsonl`, `h = sha256(prev ‖ canonical_json(h 뺀 레코드))`, 레코드에 `seq`·`wall_ns`·`prev`. 재기동·월 넘김에도 체인 이어짐. writer 스레드 1개, 큐 1024건/1 MiB, 가득 차면 ≤ 200 ms 대기 후 실패, 그룹 sync ≤ 100 ms. 실패 → `record.health{failed}` 1회 발행, agent·script 쓰기 `E_RECORDING_FAILED`(exit 4), user는 계속. 정규형이 정렬 맵인지 단위 시험으로 고정(`preserve_order`를 켜면 깨짐). `verify_file`로 변조 탐지.

### 계약과 다른 점 / 리드에게
- **`payload.final`의 성공 종단**: `CommandSpec`에 필드가 없어 임시 규칙(`uart`·`echo` = `sent`, 그 외 `confirmed`). → **`loom-domain-api` `CommandSpec.success_terminal: WriteState` 추가 요청**.
- **쓰기 행 kind**: 도메인 event kind 중 `/payload/write_state` 포인터를 가진 것(echo = `echo.write`), 없으면 `<domain>.write`. → 등록에 명시 필드가 있으면 좋음.
- **`<domain>.input` 이벤트 kind**: 코어가 발행하지만 도메인 등록 kind가 아님. 도메인이 등록하도록 템플릿(echo)에 추가할지 결정 필요.
- **echo E2E 행 수**: echo의 성공 종단이 `sent`라 행은 `requested·accepted·sent(final)` **3개** + rx(작업 지시의 "4개"는 replay의 confirmed 행 기준으로 보임). 시험은 3개로 고정.
- idempotency 키 와이어: params 안 `_loom.idempotency_key`(핸들러에는 빠진 채 전달). CLI 쪽과 맞춰야 함.
- `loom.toml`은 `$LOOM_HOME/loom.toml`에서 `[policy] deny`만 손 파서로 읽음. 전체 설정에는 workspace에 **`toml` 크레이트 추가 필요**.
- 코어 메서드(`core.stop` 등)의 결과는 봉투가 아닌 일반 값.

## G5 마커·활동 — 완료
- `core.mark{label, note?}` → `core.mark`(origin = 연결 자격), `test.step.begin/end{step|name, note?, verdict?, test_session?}` → `test.step_started/ended`. 결과 `{seq, stream_epoch}`(publish 후 응답).
- `core.activity`: agent RPC를 (client, method)별로 모아 1 Hz로 발행(`summary: "N call(s) in the last second"`). 읽은 범위 `range`는 아직 비어 있음(도메인 읽기 결과에서 범위를 받는 경로가 필요 — M1 후반).
- `core.agent_cursor`: G4 참고.

## 시험 (전체)
- `tests/daemon.rs` 9개(실제 WS/HTTP, 시험마다 임시 `LOOM_HOME`, 포트 0): echo E2E, T2(최종 행이 응답보다 늦지 않게 도착 — 50 ms 허용 오차, 허브 정지 → `E_PARTIAL{published:false}`), T3+T4(안 읽는 viewer만 gap, 읽는 viewer는 2000건 전부 + 폭주 중 STOP 수신), T5(위조 origin 무시, 외부 Origin/Host 거절, 쿠키 없는 브라우저 WS·자격 없는 hello 거절, 308, 보안 헤더, HTTP STOP), T6(agent의 release·watch의 입력/해제 거절, STOP 중 agent 쓰기 `E_STOPPED`, user 쓰기 허용), T8(감사 불능 → agent `E_RECORDING_FAILED`, user 입력·송신 허용, `record.health` 발행), T9(latch 영속·재기동·손상 → latched·두 번째 인스턴스 거절), T10(같은 키 동시 2요청 = worker 쓰기 1회), T11(체인 검증·변조 탐지).
- `tests/hub.rs` 5, `tests/replay.rs` 8, 단위 5(audit 2, state 2, auth 1). `loom-gateway` 합계 27개, 약 7 s. 3회 반복 모두 통과.
- workspace 전체: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` 통과. `--no-default-features`(echo 없음) 빌드도 통과.

## 남은 것 / 범위 밖
- safe_state 실행 경로는 구현했지만 echo에 safe_state가 없어 시험 없음(UART/CAN 도메인이 붙을 때).
- 구독 큐 유입률 기반 크기, snapshot 중 live 1 MiB 취소·재시도, binary 인코딩, 저널 연결(`MappingSink`·`JournalReader`), IPC worker host, 승인·확인(M2), `core.activity.range`.
- 의존성(루트 `Cargo.toml` 변경 없음, 모두 workspace에 이미 있음): `loom-gateway`에 `loom-domain-api`, `loom-worker-sdk`, `loom-echo`(optional, feature `echo`), `sha2`, `rand`, `hex`, dev `tempfile`. 새로 필요: `toml`(설정 전체 파싱 시).
- `loom-proto` 변경 없음.
