# 03. CLI `loom`

## 3.1 공통 (D5, C1)
- 단일 Rust 바이너리 `loom`, 비대화형. 사람용 출력이 기본, `--json`이면 봉투.
- **무한 스트림 금지**: 대기형은 항상 timeout. `--follow` ≤ 600 s, 단일 결과 대기(`expect`, `wait-for`, 승인 대기) ≤ 3600 s. 무한 구독은 UI만.
- 출력 상한: 하드 1,000행 / 64 KiB. 기본은 도메인이 그 이하(UART·CAN 100행, UDS 40행). 넘으면 잘림 + `meta.next_cursor`.
- client id: 환경변수 **`LOOM_CLIENT`**(없으면 부모 셸 세션 + 사용자로 안정 id 생성).
- 쓰기 자동 재시도 금지. `--idempotency-key`로 상태를 조회한다(처리 중이면 같은 결과를 기다림, 결과 TTL 24 h, 만료면 `unknown` — 재송신하지 않음).
- `--dry-run` 공통: 분류·정책 판정·필요 확인 종류까지만.

## 3.2 봉투 (D5) — 타입 `loom_proto::envelope::Envelope<T>`
```json
{"schema_version":1, "ok":true, "cmd":"uart.send", "data":{…},
 "meta":{"truncated":false, "record_health":"ok", "env":"bench", "latch":false,
         "viewers":{"subscribed":1, "visible":1}, "stream_epoch":"3", "visibility":"visible"},
 "others_since_last":{"count":"0"}}
```
- **직렬화 규칙**: 선택 필드는 값이 없거나(`None`) 빈 배열·빈 맵이면 **출력하지 않는다**. 읽을 때는 `null`·`[]`·`{}`·생략을 모두 같은 것으로 받는다. 이벤트의 `payload`만은 항상 출력(빈 객체 `{}`).
- `meta.visibility` = `visible | not_visible | no_viewer`(경고 `no_visible_viewer`·`no_viewer_attached`의 근거).
- **`others_since_last`는 모든 결과에**(오류 포함). 기준 커서는 클라이언트 전역 `(LOOM_CLIENT)`이고 `by_channel` 내역을 함께 준다. 세는 것: user, 다른 actor의 script, 다른 agent client의 쓰기, `core.latch`, `core.policy_changed`. 자기 이벤트·device RX는 세지 않는다. 사람용 출력은 `NOTE:` 줄.
- 경고 코드: `no_viewer_attached`(구독 0), `no_visible_viewer`(보이는 패널 0), `daemon_autostarted`, `record_degraded`, `env_vehicle`, `daemon_tied_to_parent_job`.
- 증거: 대기·검증형 명령은 `data.evidence[]`(`07-test-session.md`).

## 3.3 종료 코드 (D5, E2, E3) — 매핑 정본 `loom_proto::error::ErrorCode::exit_code`
| 코드 | 의미 | 오류 코드 |
|---|---|---|
| 0 | 성공 | |
| 1 | 내부 오류 | `E_INTERNAL E_NOT_IMPLEMENTED` |
| 2 | 사용법·설정·DB | `E_USAGE E_AMBIGUOUS_CHANNEL E_CONFIG E_DB E_UNKNOWN_METHOD E_SCHEMA` |
| 3 | **조건 판정 불충족**: 대기 timeout + 즉시 판정 불일치 | `E_TIMEOUT E_NO_MATCH E_UNEXPECTED_MATCH E_COUNT_OUT_OF_RANGE` |
| 4 | 정책 거부·레이트·기록 불능·증거 거부 | `E_DENIED E_UNSUPPORTED E_RATE_LIMIT E_BUSY E_RECORDING_FAILED E_EVIDENCE_MISSING E_EVIDENCE_MISMATCH E_OUT_OF_WINDOW E_PLAN_MISMATCH` |
| 5 | 확인·승인 필요/대기/거부/만료(설정으로 켠 경우) | `E_CONFIRM_REQUIRED E_APPROVAL_REQUIRED E_NO_APPROVER E_APPROVAL_DENIED E_APPROVAL_EXPIRED` |
| 6 | 사용자·락 충돌, STOP, 세션 일시정지 | `E_USER_ACTIVE E_LOCKED E_LOCK_BROKEN E_HELD_BY_USER E_INTERRUPTED_BY_USER E_QUARANTINE E_STOPPED E_LEASE E_SESSION_ACTIVE E_SESSION_PAUSED` |
| 7 | 장치·전송 오류(쓰기는 `outcome_unknown` 확인) | `E_NOT_OPEN E_DISCONNECTED E_CONNECTION_CHANGED E_BUS_OFF E_NO_RESPONSE E_PORT_BUSY E_PERMISSION E_NOT_FOUND E_WORKER_DOWN E_WORKER_BUSY` |
| 8 | 부분 결과(gap·스캔 한도·미발행·inconclusive) | `E_CURSOR_GAP E_SCAN_LIMIT E_PARTIAL` |
| 9 | 데몬 연결 불가·프로토콜 불일치 | `E_DAEMON E_PROTO_MISMATCH` |
| 10 | 장치의 부정 응답 | `E_NRC`(+ `detail.nrc`) |
| 11 | 테스트 판정 failed(`test end`, `run --test`만) | `E_TEST_FAILED` |
- 장치 요청 무응답(UDS P2/P2* 초과 등)은 3이 아니라 **7 `E_NO_RESPONSE`**, 쓰기는 `outcome_unknown`.
- 오류 본문: `{code, message, hint?, retryable, request_id?, detail}`. 새 코드는 `loom-proto` PR + 묶음 지정 필수, enum은 추가만.

## 3.4 커서 (D5, A7, E1) — 타입 `loom_proto::cursor::Cursor`
`@last | now | seq:N | e<epoch>:seq:N | mark:X | -5s | -250ms | HH:MM:SS(.mmm) | ISO8601`, 도메인 접두 확장(UART `off:N`, `send:last`).
- `now` = 요청 접수 시 허브 high-water에 원자적으로 바인딩.
- `HH:MM`은 세션 시간대·날짜를 결과에 명시, 모호하면 exit 2. 보존 범위 밖 = exit 8 `E_CURSOR_GAP`.

## 3.5 자동 기동 (D8)
- 데몬이 없으면 CLI가 자동 기동(파일 락으로 동시 기동 1회, 대기 ≤ 2 s). `LOOM_NO_AUTOSTART=1`이면 exit 9. 사람용 stderr 한 줄 + `warnings:["daemon_autostarted"]`.
- 데몬은 장치를 자동으로 열지 않는다(`loom.toml`의 `autostart=true` 채널만).
- `loom daemon start|stop|status|logs`, `loom status`, `loom stop-all`, `loom events`, `loom mark`, `loom record …`, `loom confirm`, `loom approvals list|wait`(approve 동사 없음), `loom run …`, `loom test …`, `loom policy show|diff|propose`, `loom ui [open <panel>]`, `loom help --json`·`loom <domain> help --json`(C8 정본), `loom skills sync`, `loom doctor`.

## 3.6 성능 목표 (plan §0.4)
- CLI 고정 비용(생성→연결→hello→종료) p95 ≤ 10 ms(Windows ≤ 25 ms). R0 명령 왕복(데몬 실행 중) p95 ≤ 30 ms(Linux·macOS) / ≤ 60 ms(Windows). 자동 기동 포함 첫 호출 ≤ 1 s(Windows ≤ 1.5 s).
