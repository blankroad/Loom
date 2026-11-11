# loom.viz 요구사항 (UART/UDS → CAN 팀)

각 팀은 `## <comp>` 절을 추가합니다. 소유: CAN 팀. 다른 팀 절은 수정하지 말고 덧붙이기만 합니다.

## uds

작성: uds-lead, 2026-11-09 (초안 v0.1 — R3 통합 후 갱신 가능)

### V-UDS-1. 진단 요청/응답 트레이스 표 (가장 중요)
- 행 = 요청/응답 한 쌍(트랜잭션). 열: 시각(wall, 상대 ms), origin(agent/user/script/keepalive), 대상 ECU(논리 주소 또는 CAN ID 쌍), 전송(DoIP/CAN-TP), 서비스명(예 `ReadDataByIdentifier`), 요약(예 `F190 VIN = WVW...`), 결과(+ / NRC 이름 예 `0x31 requestOutOfRange`), 응답시간(ms, P2 대비 색), 0x78 pending 횟수.
- 행 펼치기: 원시 요청/응답 hex, 디코드된 필드 트리, 위험 등급, 확인 토큰/승인자(감사 로그 연결).
- 필터: ECU, 서비스, 결과(+/NRC), origin. TesterPresent(3E 80) 행은 기본 접기.
- 라이브(데몬 이벤트 스트림 구독) + 파일(UDS 트랜잭션 JSONL) 둘 다.
- 입력 레코드 스키마(JSONL 한 줄 = 한 트랜잭션): 공통 이벤트 필드(`ts`, `domain="uds"`, `channel`, `dir`, `origin`, `payload`) + `payload` 안에 `{ecu, transport, sid, service, req_hex, resp_hex, nrc, nrc_name, decoded, t_req, t_resp, rtt_ms, pending_count, risk}`. 정확한 필드는 UDS plan.md §6에서 확정.

### V-UDS-2. DID 값 추이 플롯
- `loom uds poll F40D,F405 --period 200ms` 같은 폴링 결과(디코드된 스칼라, 단위 포함)를 시계열 플롯.
- 일반 "시계열 JSONL/CSV" 입력으로 충분: `{ts, series: "ECU1.F40D.vehicle_speed", value: 42.0, unit: "km/h"}` 형태(long format) 지원 요청. wide format(CSV 열=신호)도 OK.
- 비수치(문자열/enum) DID는 상태 표(마지막 값 + 변경 시각)로 표시.
- 라이브 스트림 + 파일 재생. 에이전트 한 줄 실행: `loom viz show uds_poll.jsonl --long --series series --y value` 수준.

### V-UDS-3. DTC 표
- 열: DTC(3바이트 hex + P/C/B/U 표기), 상태 바이트 8비트 분해(testFailed, confirmed 등), 설명(진단 DB 있으면), 최초/최근 발생, 스냅샷/확장 데이터 펼치기.
- 두 시점 비교(clear 전/후) diff 표시가 있으면 좋음 (선택).

### V-UDS-4. 세션/연결 상태 패널
- ECU별: 연결 상태, 활성 세션(0x01/0x02/0x03), 보안 레벨, TesterPresent 동작 여부, 마지막 응답 시각, P2/P2* 값.
- 진행률 표시: 시나리오 실행 단계 N/M, (플래시 범위에 포함 시) 다운로드 바이트/총량.

### V-UDS-5. CAN 트레이스 연동
- CAN 트레이스 창에서 ISO-TP 프레임(FF/CF/FC)을 **UDS 트랜잭션으로 묶어 보기**(심볼릭 해석) — CANoe의 진단 콘솔/트레이스 해석 수준. 최소: 트레이스 행에 `ISO-TP FF len=... (→ UDS 22 F190)` 주석 열.
- 공통 기록에서 UDS 트랜잭션 ↔ CAN 프레임을 같은 타임라인으로 정렬할 수 있게 `ts` 기준 통일.

### V-UDS-0. 핵심 원칙 요구 (02-core-principle.md, 모든 V-UDS 항목에 적용)
- **점진 갱신**: 트랜잭션 행은 완료 때 한 번에 나타나면 안 된다. `queued → tx → pending(0x78, N회) → rx/err` 이벤트마다 같은 행을 갱신(pending 동안 화면이 멈춘 것처럼 보이지 않게).
- **지연 목표**: 데몬이 ECU 응답을 받은 시각 → 화면 렌더 완료 p95 ≤ 100 ms, p99 ≤ 200 ms (정상 부하). 렌더러가 `lag`을 화면에 표시.
- **거짓말 금지**: 느린 구독자·고부하(빠른 폴링, 연속 0x78, TesterPresent 반복, 대량 0x36)에서는 UI를 20 Hz로 묶고 `gap{first_seq,last_seq,count}`/집계 카운터/`LIVE-DEGRADED`를 표시. 최종 응답·실패·승인·정지 이벤트는 집계하지 않는다. 데이터 자체는 기록에서 빠지지 않는다.
- **늦은 합류**: 뷰어를 언제 열어도 `snapshot(seq)` + 이후 라이브 이벤트로 이어 붙는다.
- **사용자 개입 입력**: 웹 화면에도 같은 스트림 위에서 사용자 요청 송신, 폴링 정지, 제어권 회수(take-control), 긴급 정지, 위험 동작 승인 입력이 필요(데몬 UDS RPC 호출, origin=user). M1은 UDS 팀 TUI가 이 역할을 하고, 웹은 viz 일정에 맞춘다.
- DTC P/C/B/U 표기는 형식이 맞을 때만, 아니면 3바이트 hex 유지.

### V-UDS-6. R4 추가 (UDS plan.md 확정 반영)
- 결과 열 상태 추가: `may_have_executed`(실행 불확정, `⌛?`), `late`(quarantine 중 도착한 늦은 응답 — 기록·표시만), `ok_suppressed`(suppressPosRsp 무응답 정상).
- 승인 토큰 원문은 화면 데이터·로그에 내보내지 않는다(`approval_ref`만). 승인 입력은 포커스 시 `Y`/`N`, R3는 승인 코드 끝 2자 — 웹 승인 UI도 같은 규칙.
- 원시 24bit DTC 항상 보존, P/C/B/U 표기는 형식이 확인된 경우만(`19 02` 응답에는 형식 식별자가 없음).
- live 이벤트 스키마(queued/tx/pending/rx/timeout/late)와 완성 트랜잭션 파일(`record_type: txn`)을 구별해 받는다. 같은 `request_id`의 행 갱신.
- `active_overrides`(비기본 세션, IO 제어, 통신 차단, DTC 설정 off) 상태 패널 표시 + 사용자 "복구 송신" 입력 경로(`uds.recover`).

### 우선순위
V-UDS-1 > V-UDS-2 > V-UDS-4 > V-UDS-3 > V-UDS-5. 터미널(TUI) 트레이스는 UDS 팀이 자체로 최소 구현 가능하므로 viz는 웹 쪽 렌더러를 우선 요청.

## uart

작성: uart-lead, 2026-11-09 (초안 v0.1 — uart R3 통합 후 갱신 가능). docs/plan/02-core-principle.md 준수.

전제: **UART의 1차 실시간 표시는 viz가 아니라 `loom uart attach`(터미널 뷰어)** 다. viz는 UART 텍스트 로그에서 숫자를 뽑아 그리는 2차 도구이며 UART 일정상 후순위(M4)다. 단 아래 스키마는 지금 고정해 두어 UART 기록 포맷이 M1부터 source 참조를 예약할 수 있게 한다.

### V-UART-1. 텍스트 로그 → 숫자 시계열 (가장 중요)
- 장치 로그 줄(예 `temp=42.5 rpm=1200`)에서 **선언적 추출 규칙**(명명 그룹 정규식 + 단위 매핑, 또는 `k=v`/`k: v` 자동 파싱)으로 시계열 생성. 임의 Python eval 금지, 정규식 시간·줄 길이 상한(UART 쪽은 `regex` 패키지 timeout 사용 예정).
- 출력/입력 형식은 **UDS V-UDS-2와 같은 long format**: `{"ts":{"mono_ns":"…","wall_ns":"…"},"series":"dut.temp","value":42.5,"unit":"degC","quality":"valid","source":{"domain":"uart","channel":"dut","seq":"42","rx_offset":"90211"}}` (JSONL) / 같은 열의 CSV. wide CSV(`ts_wall,temp,rpm`)도 수용 요청.
- 추출 실패·재연결·gap 구간은 **선을 이어 그리지 않고** `quality`/gap 마커로 표시(NaN 보간 금지).
- 에이전트 한 줄 호출: `loom uart extract dut --re 'temp=(?P<temp>[-\d.]+)' --since mark:boot-1 -o temp.jsonl` → `loom viz show temp.jsonl --long --series series --y value`.

### V-UART-2. 라이브 소스
- viz가 UART 데몬 이벤트 스트림(`uart.subscribe`, 줄 단위 kind)에 붙어 추출 규칙을 viz 쪽에서 적용해 실시간 플롯. → CAN/UDS와 공통인 **"라이브 소스 플러그인" 인터페이스** 요청(구독 + 늦은 합류 snapshot(seq) + gap 이벤트).
- 브라우저/viz는 데몬 이벤트만 받고 **COM 포트를 다시 열지 않는다**.

### V-UART-3. 이벤트 오버레이
- 마크, 리셋/break/DTR·RTS 시그널, 재연결(generation 변경), 에이전트 송신을 시간축 수직선/마커로 표시. 입력: UART `events.jsonl` 또는 공통 이벤트 레코드 `kind ∈ {mark, signal, state, data(dir=tx)}`.

### V-UART-4. 원문 역참조
- 플롯 점 클릭/호버 → 원본 줄 텍스트, `source.seq`/`rx_offset`, 시각 표시. 가능하면 "UART 뷰어에서 이 위치 열기"(`loom uart attach dut --at off:N`).

### V-UART-5. 공통 시간축
- 여러 UART 포트와 CAN 신호·UDS DID를 **같은 시간축**에 겹치기. UART 시각은 `host_read_complete`(USB latency 수 ms 오차) 품질임을 레코드에 표시하므로, viz는 HW 타임스탬프(CAN)와 품질 차이를 범례/툴팁에 표시해 주면 좋다.

### V-UART-0. 핵심 원칙 요구 (모든 항목 공통)
- 규모: 1 kHz 이하 샘플 × 수 시간(~10^7 점) → 렌더층 다운샘플링(LTTB 등, min/max 보존), 원본은 유지.
- 라이브 플롯 지연: 데몬 수신 → 플롯 반영 p95 ≤ 200 ms. 고부하 시 다운샘플/드롭 사실과 `lag`을 화면에 표시(거짓말 금지).

### 우선순위
V-UART-1 > V-UART-4 > V-UART-3 > V-UART-2 > V-UART-5. 파일 기반(V-UART-1)만 있어도 UART 측 요구 대부분 충족.

## can (소유 팀 응답)

작성: can-lead, 2026-11-09. 상세는 `docs/plan/can/plan.md` §5.3, §6.4, §15.3.

**결론: uds·uart 요구를 모두 M3(웹 뷰어 + loom.viz) 범위로 수용한다.** 우선순위 V-UDS-1 > V-UART-1 > V-UDS-2 > V-UDS-4 > V-UART-4/3 > V-UDS-3 > V-UDS-5(최소 주석 열 M3, 심볼릭 묶음 M4) > V-UART-2/5. M1/M2에서 각 팀의 1차 실시간 표시는 자기 TUI가 맡는다(UART attach, UDS TUI, CAN monitor).

**표준 입력 계약 (V-UDS-2 · V-UART-1 공통)**
- long JSONL/CSV: `{"ts":<epoch s | ISO8601 | {"mono_ns","wall_ns"}>, "series":"…", "value":<num|str|bool>, "unit":"…", "quality":"valid|gap|…", "source":{"domain","channel","seq", …}}` — 요청 형식 그대로 수용. wide CSV(`ts,a,b`)도 수용.
- 메타(선택): `series_id, display_name, value_type(number|enum|bool|string|bytes), unit, enum_map, time_source, quality, source_domain, session_id, source_seq`.
- 공통 이벤트 envelope JSONL(`domain="uds"` 트랜잭션 등)은 도메인 어댑터로 자동 인식 → V-UDS-1 표.
- quality/gap 구간은 선을 잇지 않는다(NaN 보간 금지). 비수치는 State/표.

**플러그인 API (UART/UDS가 기여)**
- 라이브 소스: `register_source(domain, subscribe_fn)` — 데몬 이벤트 토픽 구독 + 늦은 합류 `snapshot(seq)` + `gap` 이벤트. 브라우저/viz는 장치를 다시 열지 않는다.
- 패널: `register_panel(kind, topics, component)` — UDS 트랜잭션 표·세션 상태, UART 원문 역참조 등. 해석(NRC 이름, DID 디코드, 정규식 추출)은 각 팀 payload를 그대로 표시하고 viz가 재구현하지 않는다.

**핵심 원칙 요구(V-UDS-0, V-UART-0) 수용**
- 점진 갱신: 같은 트랜잭션 행을 `queued→tx→pending(n)→rx/err` 이벤트로 갱신. 최종 응답·실패·승인·정지 이벤트는 화면 생략/집계 대상에서 제외.
- 지연: UDS 트랜잭션 → 화면 p95 ≤ 100 ms / p99 ≤ 200 ms, UART 추출값 → 플롯 p95 ≤ 200 ms를 M3 수용 기준에 포함. 화면에 `lag` 표시.
- 고부하: UI 갱신 20 Hz 상한, `gap{first_seq,last_seq,count}`·`LIVE-DEGRADED`·`downsampled N→M` 표시. 기본 다운샘플은 픽셀 버킷 min/max(극값 보존), 원본은 유지, 통계/커서 값은 원본으로 계산.
- 늦은 합류: 모든 패널은 `core.subscribe`의 원자적 snapshot + `after_seq` live로 이어 붙음.
- 웹 사용자 개입: UDS 패널의 요청 송신/폴링 정지/take-control/긴급 정지/승인 입력은 UDS 팀 RPC를 호출하는 패널 동작(origin=user는 서버가 viewer role로 부여)으로 제공.
- 공통 시간축(V-UART-5): 시각 품질(CAN HW/host, UART host_read_complete)을 범례·툴팁에 표시. 다른 어댑터·호스트 시계 간 정밀 동시성은 보장하지 않음을 표시.

**타팀에 요청**
- UDS: 트랜잭션 이벤트 스키마(`queued/tx/pending/rx/err`, 필드 목록)를 UDS plan.md §6에서 확정하면 그대로 렌더러 fixture로 쓴다.
- UART: 이벤트 오버레이 `kind` 목록과 `source.rx_offset` 의미를 UART plan.md에서 확정해 주면 역참조 링크(`loom uart attach --at off:N`)를 붙인다.
