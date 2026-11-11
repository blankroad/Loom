# 01. 프로세스와 IPC

## 1.1 프로세스 (D1, C1, plan §3.1)
- 사용자·에이전트에게 보이는 데몬은 `loomd` 하나다(포트·URL·감사 하나).
- 내부 = gateway 프로세스 + 장치 worker 프로세스. 다음 중 하나라도 해당하면 **반드시 worker**: 벤더 네이티브 라이브러리 로드(vxlapi64·PCANBasic·seed-key DLL 등), ms 단위 마감 타이밍 스레드, 취소 불가 블로킹 호출, 독립 메모리 예산, 다른 언어(Python). 그 밖의 순수 Rust·소켓 기반 저속 도메인은 gateway 모듈로 둘 수 있다.
- 현재 배치: CAN = worker(`can-hw`·`can-sim`), UART = worker **포트당 1 프로세스**(최대 8), UDS 세션 관리자 = gateway 모듈(전용 런타임 스레드, 타이머 지터 p99 ≤ 2 ms 관문 — 넘으면 worker로 이동), seed-key DLL = helper worker, DoIP = OS TCP(C4.1), LA·디버그 프로브 = worker, CANoe COM = worker(Rust COM 먼저, 안 되면 Python — E11).
- worker는 같은 실행 파일을 `loomd worker <kind> --instance <id>`로 재실행한다. 벤더 DLL은 gateway 주소 공간에 올리지 않는다.
- **예외: eth worker는 별도 실행 파일 `loom-eth-worker`**(kind `eth`, PerChannel ≤ 4). Linux에서 사람이 이 파일에만 `cap_net_raw`를 주기 위함(최소 권한). 같은 worker 프로토콜을 쓴다.
- 물리 장치는 worker(또는 등록된 gateway 모듈)만 연다. CLI·UI가 장치를 여는 공개 경로는 없다.

## 1.2 외부 IPC (D2, E1, E8)
- **JSON-RPC 2.0 over WebSocket**, `127.0.0.1` 단일 포트. 경로: `/rpc`(WS), `/`(UI 정적 파일, `loomd`에 내장), `/api/v1/*`(대용량 HTTP).
- **UI 오리진 정본 = `http://localhost:<port>`**. `127.0.0.1:<port>` 접속은 localhost로 리디렉트. Origin/Host allowlist = `localhost:<port>`, `127.0.0.1:<port>`. 응답 헤더: CSP(엄격), `Permissions-Policy: serial=(), usb=(), hid=(), bluetooth=()`, `Referrer-Policy: no-referrer`.
- 메시지 상한: 클라이언트→서버 요청 1개 ≤ 64 KiB, `core.events` 배치 1개 ≤ 64 KiB. ping 20 s, 40 s 무응답이면 끊음.
- JSON-RPC 구현은 규격 오류 코드(−32700 parse, −32600 invalid request, −32601 method not found, −32602 invalid params, −32603 internal)를 지킨다. **batch 배열은 v1에서 거절**(−32600).
- **도메인 RPC 결과 와이어**: JSON-RPC `result` = CLI와 같은 **봉투 `Envelope` 전체**(`ok:false`도 `result`로, `others_since_last`는 오류에도). JSON-RPC `error`는 규격 오류와 코어 메서드 오류(−32000 + `data` = `ErrorBody`)에만 쓴다. 코어 메서드(`core.stop` 등)의 결과는 봉투가 아닌 일반 값.
- idempotency 키 와이어: `params._loom.idempotency_key`(핸들러에는 `_loom`을 뺀 params가 전달).
- stream_epoch = 데몬 시작 시각(unix 초) — 재기동마다 다르다.
- 발견 파일 `~/.loom/run/daemon.json` = `{port, pid, token, version, proto}`, 권한 0600. Windows는 사용자 프로필 아래 같은 경로, 현재 사용자 ACL.

## 1.3 hello · role · rights (D2, E1, E9, F1)
- 연결 첫 메시지 `core.hello{proto:"loom/1.0", token?, client_id, want_role, client{kind, version, tty}}`. 서버가 role을 **부여**한다. 요청 필드의 origin은 무시하고 **origin은 연결 자격으로 결정**한다.

| 연결 자격 | role | origin |
|---|---|---|
| `daemon.json` 토큰 | `agent` | agent |
| 웹 bootstrap → HttpOnly·SameSite=Strict 쿠키 | `viewer` + rights | user |
| 러너·테스트 세션 내부 | `script` | script |
- viewer rights = `watch`(보기 + **STOP·비상 OFF**) / `operate`(입력·송신·STOP 해제·복구·worker 재시작) / `approve`(operate + 승인·정책 완화·판정 수정). STOP·비상 OFF는 안전 방향이고 에이전트도 `loom stop-all`로 할 수 있으므로 watch에서도 허용한다(UI Q3). TTY에서 실행한 `loom ui` bootstrap = operate+approve. `loom ui pair`(원격, TTY 필요) = watch 기본. **Playwright·자동화 브라우저 세션은 watch만**(bootstrap 쿠키 발급 안 함).
- bootstrap: `loom ui`가 OS 브라우저를 열 때 **URL fragment**로 일회성 코드를 전달하고, 페이지가 POST로 쿠키와 교환한다(60 s, 1회용). stdout이 TTY가 아니면 코드를 출력하지 않는다. 쿠키 없는 WS는 어떤 role도 받지 못한다(agent는 토큰).
- M1 viewer = 웹만. TTY viewer(`viewer-tty`)는 필요한 도메인이 생기면 M2에 추가한다(입력·hold·STOP만).
- 원격 보기: 기본 localhost 바인딩 + SSH 터널 또는 Tailscale. LAN 직접 노출은 선택·인증 필수(M2).

## 1.4 구독 (D2, A5, E1)
- `core.subscribe{filter{domains, channels, kinds, origins}, after_seq?, snapshot, backlog{max_events, max_bytes, max_age_ms}, max_batch_ms, encoding:"json"|"binary", coalesce_by?}` → `{sub_id, stream_epoch}` → `core.snapshot_part{n,N}`… → `core.snapshot_end{stream_seq:H}` → `core.events{sub, first, last, events[], display_gap, lag_ms}`.
- **순서 보장**(UI Q8): 서버는 `core.subscribe`의 RPC 응답 `{sub_id, stream_epoch}`을 **먼저** 보내고 그다음 snapshot 알림을 보낸다.
- `core.snapshot_part{sub, n, total, items:[{domain, channel?, kind:"<domain>.snapshot", state}], events?}` — `n`은 **1부터**(`n == total`이 마지막). 이 형식 하나만 쓴다(UI Q7)(state = 도메인 리듀서 출력, 구조는 도메인 스키마) → `core.snapshot_end{sub, stream_seq:H}`. 타입은 `core-p0-d5`에서 `loom-proto::rpc`에 추가한다.
- snapshot은 허브가 한 턴(H)에서 상태(도메인 리듀서)와 backlog를 원자적으로 캡처한다. snapshot 전송 중 live는 구독별 ≤ 1 MiB/2 s 보관, 넘으면 그 snapshot을 취소하고 gap + 재시도.
- 재연결은 `after_seq` + **`stream_epoch`**을 함께 보낸다. `snapshot:false`여도 서버가 판단한다(UI Q9): 링 안이면 이어 보내고, 링 밖이면 `display.gap{cause:"resume_out_of_ring"}` + snapshot, epoch가 다르면 `core.epoch_changed` + snapshot.
- **적응형 배치**: 조용한 구간 뒤 첫 이벤트는 즉시. 바쁠 때만 `max_batch_ms`(기본 40, 터미널 10)·200건·64 KiB 중 먼저 오는 조건으로 묶는다. worker→gateway도 같은 규칙(≤ 10 ms).
- 필터로 빠진 seq는 손실이 아니다. 배치의 seq가 불연속이면 각 이벤트의 `seq`로 판단한다.
- 상한: viewer 연결당 구독 ≤ 8, 전체 구독 ≤ 32, 구독 data 큐 = 유입률 × 500 ms(1–4 MiB), 전체 ≤ 32 MiB. 넘치면 **그 구독만** 오래된 것부터 드롭 + `display.gap{cause:"slow_client"}`. lag > 2 s면 backlog를 버리고 live로 점프 + `display.gap{cause:"jump_to_live"}`.
- `coalesce_by:"id"`(키별 최신값 화면)는 화면 전용 축약이며 `coalesced_count`를 싣는다. 기록·에이전트 결과에는 영향 없다.
- 고속 binary(`encoding:"binary"`, `loom.columns.v1`)는 M2 초반에 확정한다. M1 데이터는 JSON.

## 1.5 우선 제어 큐 (D2, E1)
- **viewer 연결마다** 셸 전역 제어 구독이 자동으로 1개 있다: 응답·쓰기 상태·승인·STOP·상태·오류·gap. 와이어 표기는 예약 sub id **`core.events{sub:"control", …}`**(`loom_proto` `CONTROL_SUB`). 우선 kind 판정은 `loom_proto`의 `is_priority_kind` 하나를 허브·UI가 같이 쓴다.
- 우선 이벤트는 **드롭하지 않는다**. 큐(256 KiB/256건)가 넘치면 그 연결을 끊고(`core.closed{reason:"slow_consumer"}`) 클라이언트는 재연결 + snapshot으로 회복한다. `core.closed`에 `retry_after_ms`(권장 재연결 지연)가 있을 수 있다(UI Q12). STOP 이벤트는 연결별 1칸 mailbox로 따로 보존한다.
- 데이터 배치보다 먼저 별도 프레임으로 보낸다. **우선 kind 이벤트는 control 구독으로만 한 번 온다** — 데이터 구독에 중복으로 실리지 않는다(UI Q10). UI는 kind로 패널에 라우팅한다.

## 1.6 publish-before-reply (D2, E7)
- 장치 이벤트(쓰기 결과 포함)를 허브가 **seq 부여 + 모든 viewer 우선 큐에 enqueue 완료**한 뒤에 RPC 응답을 보낸다. 브라우저 렌더 완료를 보장하지는 않는다(렌더 지연은 UI가 `core.metrics.report`로 보고).
- 허브 입력에는 우선 이벤트 예약 용량을 둬 정상 경로에서 enqueue가 실패하지 않게 한다. 허브가 멈춘 이상 상황에서 deadline 안에 발행하지 못하면 성공 응답 대신 **exit 8 `E_PARTIAL{published:false}`** — 장치 동작은 일어났으므로 `failed`로 바꾸지 않고 재시도를 권하지 않는다.

## 1.7 HTTP (A5, E8)
- `GET /api/v1/query/<id>`(64 KiB 넘는 질의 결과, 60 s), `GET /api/v1/raw/<raw_ref>`(서버 소유 opaque id), `GET /api/v1/report/<session>.{html,xml,json}`, **`POST /api/v1/stop`**(WS가 끊긴 중에도 STOP). 인증: agent = `Authorization: Bearer <token>`, viewer = 쿠키. 응답은 NDJSON 스트리밍, 상한 256 MiB, 동시 4.

## 1.8 이름 규칙 (D2, A16, E1)
- RPC: `<domain>.<verb>` 또는 `<domain>.<noun>.<verb>`, 토큰 `[a-z][a-z0-9_]*`, **2–3토큰**(`loom_proto::rpc::validate_method`). 예: `uart.send`, `can.isotp.open`, `core.subscribe`.
- 이벤트 kind: `<domain>.<event>` 정확히 2토큰(snake). 예: `can.isotp_rx_start`, `test.step_started`.
- 예약 domain(CORE): `core, record, display, source, run, test, ui, adapter`.
- 코어 RPC 목록은 `docs/plan/core/plan.md` §4.7. 시계 RPC `core.time` → `{recv_mono_ns, send_mono_ns, wall_ns, clock_id}`(수신 즉시 우선 처리).
