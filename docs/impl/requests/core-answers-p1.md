# CORE 답변 — P1 요청 일괄 판정 (core-lead, 2026-11-11)

대상: `uart-core.md` 2–8, `can-core.md` 1–9, `uds-core.md` 1–2, `eth-core.md` P1-1–15, `explore-core.md` X-1–7, `test-core.md` 1–4·추가, `ui-core.md` Q14–Q21·C-1–C-12·정적 자산.
기준: core/main `29f20c3` 시점. **[완료]** = 이미 core/main에 있음, **[R1]** = 지금 진행하는 CORE 라운드(P1 관문 전 반영), **[R2]** = 그다음 라운드(P1 안), **[P2]** = 뒤로. 타입·API 이름은 구현 때 바뀔 수 있으니 확정본은 계약 `docs/contract/CHANGELOG.md`와 core/main 커밋을 보라.

## 공통 (여러 팀)
| 주제 | 판정 |
|---|---|
| 실제 `loomd`가 echo를 **IPC worker**로 돌림 | **[완료]** `7c5f90f`. 정상 종료 시 worker 즉시 해제 `3cf1d50`. worker 쪽 저널(크레딧과 무관하게 원본 기록) `8ab86b2`. gateway 저널·`loom.toml` 정책 `490376a` |
| workspace 의존성 | **[완료]** `29f20c3`: `serde-saphyr 2.1.1`, `can-dbc 10.0.0`(encodings), `pest 2.9.6`, `proptest 1.12.0`, `pulldown-cmark 0.13.6`, `rust-embed 8.13.0`, `mime_guess 2.0.5`. 앞서 `toml 1.2.1`, `sha2 0.11.1`, `regex 1.13.1`, `etherparse`, `pcap`, `ipnet`. **YAML은 `serde-saphyr` 하나로**(순수 Rust, unsafe 없음) — TEST의 `serde_norway`도 이쪽으로 바꿔 달라 |
| 도메인 등록(`loomd`·`loom`에 feature로) | **[R1]** feature 이름 = 도메인 이름(`uart`, `can`, `uds`, `eth`, `psu`, `test`), 도메인 crate는 echo처럼 `gateway`/`worker`/`cli` feature로 조각 제공. CLI 사람용 렌더러는 도메인 crate의 `cli` 모듈 |
| PerChannel worker 인스턴스 (uart 5, eth P1-8) | **[R1]** `open`(채널을 만드는 명령) 때 gateway가 instance id = 채널 별칭으로 worker를 띄우고 그 worker에 Call을 보낸다. 이후 `params.channel`(또는 `ch`)로 라우팅. `CallCtx::worker_for(kind, channel)` 추가. 장치를 열지 않는 열거(`uart.ports`)는 실행 중인 아무 인스턴스 → 없으면 임시 `_enum` 인스턴스. `close` 후 worker는 포트를 닫고 종료해도 된다(채널 미러·커서는 gateway에 남음) |
| bounded wait API (can 1, eth P1-4, psu X-7) | **[R1]** `CallCtx::wait(WaitSpec{filter, after: Cursor, deadline, max_events}) -> WaitResult{events, cursor, timed_out, scanned_through}` — 허브 구독 기반. 폴링은 걷어내도 된다 |
| `Emitter` 확장 (can 4·5, eth P1-13·14, psu X-4, uart 7) | **[R1]** `Emitter::data(EventDraft)`(data lane에 dir·origin·purpose·quality·metrics·request_id 지정), `source_gap`에 `producer_range`·`detail`, 표준 gap 발행기(`display.gap`/`record.gap`/`source.gap`, domain·channel = 영향받은 스트림), **seq 예약** `reserve_seq()` + `*_with_seq`(발행 순서 = seq 순서는 도메인 책임 — 저널은 증가하지 않는 seq를 거절한다) |
| `SharedWorker` (uart 6 — M1 차단) | **[R1]** 기존 `Worker`(`&mut self`, 직렬)는 유지하고, **`SharedWorker`(`&self`, 콜백 동시 실행) + `run_shared_worker(Arc<W>)`**를 추가한다. `on_input`·`on_latch`는 긴 `expect`·`send` 대기 뒤에 줄 서지 않는다. CAN·UDS도 긴 대기가 있으면 이쪽을 권장 |
| 대화형 입력 (uart 2·8) | **[R1]** proto: `Input{conn_id?, channel?, input_seq?}`, `InputLease{conn_id?}`, G→W `InputLeaseEnd{lease_id, conn_id, cause:"conn_closed"|"released"}`, `Call{conn_id?}`(모두 선택 필드 — **Rust 리터럴은 새 필드 추가 필요**). gateway: `data_hex`를 `hex` 별칭으로, `input_seq` 전달, bytes에 0x03이 있으면 `priority=true`, `Registry::input_events(Owner::Domain)` 선언 시 core는 `<domain>.input` 이벤트를 내지 않는다(감사 `input.batch`는 계속 core). viewer WS가 끊기면 `InputLeaseEnd` |
| E7 해석 (can 6) | **맞다.** publish-before-reply는 "응답이 말하는 상태까지의 행"에 적용된다. `accepted`로 응답한 뒤 남은 행이 이어지고 마지막 행에 `payload.final:true`. 계약 02에 명시 |
| `range` 모양 (can 7) | **`{from, to}`(loom-proto `SeqRange`)가 정본.** 계약 02의 `range{seq_a, seq_b}` 표기를 고친다. 도메인은 `core.*` kind를 직접 발행하지 않는다 — 대기·활동은 **[R1]** `CallCtx::activity(action, range?, predicate?)`로 알리면 core가 `core.activity`(`ActivityPayload`에 `action, domain, channel` 선택 필드 추가)로 낸다 |
| 커서·봉투 (can 8) | `JournalQuery.from`은 **포함(inclusive)**. **[R1]** 도메인 결과 객체에 `_meta{next_cursor, truncated, range}`가 있으면 core가 봉투 `meta`로 올리고 `_meta`를 지운다. `@last`는 core가 `meta.next_cursor`(없으면 마지막 반환 seq)로 옮긴다. `ScanResult.scanned_through`(스캔한 high-water) 추가 |
| CallCtx 보강 (can 9) | **[R1]** `now_mono_ns()`, `spawn_blocking(f)`, `config()`(병합된 `loom.toml [<domain>]` 섹션 — psu X-6). 테스트 세션 DB pin은 **[P2]** |
| `dry_run` 미리보기 (eth P1-7, psu X-7) | **[R2]** `Registry::preview(method)` 선언 → `params.dry_run:true`면 core가 classify·정책만 하고 행·감사 없이 핸들러를 `_loom.preview=true`로 부른다(레이트 제한) |
| worker 별도 실행 파일 (eth P1-2) | **[R1]** `Registry::worker_program(kind, name)`(비파괴 추가). gateway가 `loomd`와 같은 디렉터리에서 찾는다 |
| 저널 꼬리 스캔 (eth P1-9) | **[R2]** `Cursor::Last` + `max_events=N` = 최근 N개 |
| gap 채널 (eth P1-10) | **[완료]** gap 이벤트 domain·channel = 영향받은 스트림이라 채널 필터 스캔에 포함된다. 계약 07에 "검증 입력에 창 안 gap·도메인 커버리지 이벤트 포함"(P1-11) 명시 **[R2]** |
| `filter.test_step` (eth P1-5) | **[R2]** |
| raw_ref HTTP·export job (eth P1-6·12) | **[P2]** |
| Health 어댑터 (eth P1-15) | **[완료]** `run_worker`가 Health를 낸다. capture seq ↔ SDK seq 좌표는 도메인 몫(seq 예약 API 사용 권장) |
| worker tick (psu X-1) | **[R1]** `Registry::worker_tick(kind, period)` + `SharedWorker::on_tick`(기본 no-op) |
| 다른 도메인 인터록 조회 (psu X-2) | **[P2]** core 파이프라인이 `Registry::interlock`을 이미 검사한다. 훅 안 조회는 P2 |
| `EmergencyOffSpec` 인자 (psu X-3) | core는 spec의 `method`를 spec의 고정 `params`로 부른다(R1에서 필드 확인·문서화) |
| `E_LIMIT` (psu X-5) | **[R1]** `ErrorCode::E_LIMIT`(exit 4) 추가 |
| `WriteState::Confirmed` 문서 (uds 2) | **[R1]** "backend가 정의한 확인(CAN = 버스 송신 확인, UDS = ECU 긍정 응답)"으로 정정. UDS의 SPRMIB·링크 끊김 결정은 계약과 충돌 없음 |
| CAN `dlc` (can 3) | 도메인 정의(DLC 코드 0..15, 바이트 수는 `len`) — CAN payload 스키마가 정본. FD 12바이트 fixture는 **[R2]** |
| TEST 경계 (test 1–4) | **동의.** `loom-test`가 `Registry::for_core("test")`를 써도 된다 — CORE가 `loomd`·`loom`에 등록한다. **[R1]** `loom-proto::canonical::{canonical_json, sha256_hex}` 정본 추가(TEST는 그쪽으로). TC-1 E2E 준비 상태는 CORE `STATUS.md`에 표시 |

## UI (ui-core.md)
| # | 판정 |
|---|---|
| Q14 | `skew_err_ms`는 선택(`null`/생략 = 미동기) |
| Q15 | 패널별 여러 건 OK(≤ 16건/10 s) |
| Q16 | 탭 단위(channel 생략) OK — `no_visible_viewer`는 탭 단위로 판정 |
| Q17 | SSH 터널은 서버에서 로컬과 구별할 수 없다 → **[P2]** UI가 hello에 `client.remote`를 자가 신고하면 그대로 기록 |
| Q18 | `POST /api/v1/bootstrap {code}` → **204 + Set-Cookie**, 실패는 4xx(400/410, 정보 없음). 딥링크는 UI 형식 **`#boot=<code>&next=<route>`** 채택 — **[R1]** `loomd`·`loom ui`를 `#bootstrap=`에서 바꾼다 |
| Q19 | `ui.focus_request{panel, props, at, reason, actor}` — actor는 payload(이벤트 최상위 `actor_id`도 같이), `at` = 커서 문법, 서버도 클라이언트당 2 s에 1회 제한 **[R1]** |
| Q20 | **[R1]** 코어 리듀서 `core.channels`(domain·channel·state·generation)를 control snapshot에 넣고 `core.channels.list` RPC 추가 — 도메인 `<domain>.state` 이벤트에서 갱신 |
| Q21 | 쿠키 없는 WS 업그레이드 = **401**. 연결 후 만료 = `core.closed{reason:"auth_expired"}` 하나 |
| C-3 | 계약 01 §1.5(v1.2)에 이미 "control로만 한 번" — 실 `loomd`도 같다 |
| C-4·C-5·C-7·C-9·C-10 | **[R1]** replay를 계약에 맞춘다: gap domain = 영향받은 스트림, watch STOP 허용, `POST /api/v1/stop`, `snapshot:false`+`after_seq` 없음이면 snapshot 생략, `backlog` 필드 선택(기본값 문서화) |
| C-8 | **[R1]** `core.safe_state.list` 결과 = `{items:[{id, kind:"recover"|"emergency_off", domain, label, …}]}` |
| 정적 자산 | **[R1]** gateway Router에 `loom_ui_assets::router()`를 `merge`, Host allowlist·리디렉트는 gateway. `rust-embed`·`mime_guess` workspace 완료 |
