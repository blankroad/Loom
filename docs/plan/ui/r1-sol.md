# UI R1 기술 조사·실현 가능성 초안 — ui-sol

작성: 2026-11-10 KST. `docs/plan/02-core-principle.md` 준수.
상태: R1 제안. 제품 코드는 만들지 않았으며 스파이크만 `spikes/ui/sol/`에 둔다.
우선순위: 마스터 C1–C8 > B > A > D. 도메인의 이전 TUI 우선 일정은 C6로 대체한다.
표기: **확인**=레지스트리·문서·소스 확인, **측정**=이번 스파이크, **목표**=향후 수용 기준, **미확인**=시험 전.

## 0. 핵심 루프

| 요소 | UI의 구현 | 소유·불변식 | M1 증명 |
|---|---|---|---|
| 직접 접근 | UI는 장치를 열지 않는다. Rust `loomd`의 장치 worker가 유일한 owner다 | CLI·UI 모두 같은 gateway. Web Serial/WebUSB 별도 연결 금지 | sim open 결과가 기존 UI 세션에 나타남 |
| 읽기 | `core.subscribe`의 snapshot + live, 범위 query, 원문 참조 | 에이전트가 읽은 동일 이벤트·출처·seq를 보여 줌 | CLI의 request_id·응답 seq와 화면 행 대조 |
| 쓰기 | 터미널 입력·CAN 송신·UDS 요청을 같은 도메인 RPC로 호출 | origin은 viewer 자격으로 서버가 user 부여. 정책·감사는 CORE | 사용자 입력 뒤 에이전트 결과 `others_since_last` 확인 |
| 실시간 표시·개입 | UART 터미널, CAN Trace, UDS 점진 갱신 표 + 공통 STOP·승인 | 숨은 연결·숨은 토픽 없음. 늦게 열어도 동일 세션 | agent TX → device RX → 화면 → 사용자 입력을 한 시험으로 |

### 0.1 종단 간 시나리오 — M1의 최소 시연

1. 사용자: `loom ui`로 기존 `loomd`의 웹 UI를 연다(명령 표기는 CORE 합의 전 초안).
2. 에이전트: `loom uart open dut --sim bootshell` 후 `loom uart send dut "help\r" --json`.
3. gateway: 요청·정책·감사 → UART worker → `uart.tx`, `write_state=sent`, `origin=agent` 발행.
4. UI: Activity와 터미널 바의 `AGT help⏎`를 갱신한다. `sent`는 드라이버 접수이며 장치 실행 성공이 아니다.
5. sim 장치: `commands: help status reboot\r\n>` 응답 → RX 청크의 동일 seq가 에이전트 read와 UI에 전달된다.
6. 목표: UART host-read-complete → UI 표시 p95 ≤ 100 ms, p99 ≤ 200 ms.
7. 사용자: 터미널에 `status`를 치다가 Ctrl-C를 누른다. `uart.input` 우선 경로로 0x03을 보낸다.
8. 화면: `USR ^C`, 입력권·미완결 줄 상태를 표시한다. 에이전트 다음 결과에 사용자 개입이 포함된다.
9. 사용자: 전역 STOP. agent/script 새 쓰기·잡은 latch, 사용자 입력과 관찰은 계속한다.
10. 에이전트 CLI를 종료하고 UI를 새로 열어도 같은 channel·generation·backlog가 이어진다.

CAN 변형: `loom can send ch0 0x300#01` → Trace의 AGT TX·sim 응답 DEV RX → 사용자 송신줄에서 `0x301#02`.
UDS 변형: `loom uds read-did F190 -t ecu1` → 같은 request_id 행의 `queued→tx→rx` → 사용자 입력줄 `22 F190`.
위 CLI는 도메인 계획 예시를 따른다. 실제 인자 정본은 P0 `loom <domain> help --json`으로 고정한다.
`publish-before-reply`는 UI 스트림 발행 순서 보장이며 화면 픽셀이 먼저 켜졌다는 보장은 아니다.
표시 지연은 별도 브라우저 계측으로 검증해야 한다.

### 0.2 지연 예산 — 목표이며 측정값과 구분

| 경로 | worker→gateway·hub | 배치 대기 | WS 전달·스케줄 | 디코드 | 상태 반영 | 렌더 대기·제출 | 여유 | 종단 목표 p95 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| UART 정상 115200/921600 | 15 ms | 10 ms | 5 ms | 3 ms | 4 ms | 33 ms | 30 ms | 100 ms |
| CAN Trace 정상 5k/s | 20 ms | 40 ms | 10 ms | 8 ms | 8 ms | 33 ms | 81 ms | 200 ms |
| UDS 최종·진행 이벤트 | 15 ms | 5 ms | 5 ms | 3 ms | 4 ms | 33 ms | 35 ms | 100 ms |
| 다중 신호 플롯 | 25 ms | 40 ms | 10 ms | 10 ms | 25 ms | 50 ms | 90 ms | 250 ms |

- 위 열은 설계 예산 배분이다. 각 홉 p95를 더해 종단 p95를 증명할 수는 없다.
- CAN 도메인의 추가 요구 p95 < 150 ms / p99 < 300 ms도 정상 프로파일의 강화 목표로 수용 제안한다.
- UART 추출 플롯은 기존 V-UART-0의 p95 ≤ 200 ms를 별도 강화 목표로 둔다.
- 우선 제어 이벤트를 CAN의 40 ms 데이터 타이머에 기다리게 하지 않는다.
- 일반 Trace는 20 Hz, 터미널은 최대 60 Hz, UDS 진행 상태는 20 Hz·최종 상태는 다음 rAF로 갱신한다.
- 포커스·STOP·승인 UI는 데이터 렌더 타이머와 독립적이다.
- 물리 UART 선→USB→호스트 지연과 CAN HW timestamp→호스트 ingest 오차는 별도 표시한다.
- 숨겨진 탭은 rAF가 정지·지연되므로 실시간 표시 수용 대상에서 제외하고 presence=hidden으로 알린다.

### 0.3 고부하·메모리 불변식

- 데이터 폭주에도 제어 큐를 먼저 처리하며, 제어 큐 포화는 연결 실패·재snapshot으로 드러낸다.
- `display.gap`, `record.gap`, `source.gap` 세 층을 혼동하지 않는다.
- 표는 보이는 행만 그린다. 보관 행 수와 매 프레임 그리는 행 수는 다르다.
- 오래된 기록은 디스크에서 query한다. 브라우저에 24시간 원본을 누적하지 않는다.
- `LIVE/LAG/STALLED/LIVE-DEGRADED`와 idle·시각 불확실성을 별도로 표시한다.
- M1은 sim + 셸 + UART/CAN/UDS 최소 패널이다. 도킹 고급 기능·대규모 파일 형식은 이후다.

## 1. 범위 / 비범위

### 1.1 범위와 소유

| UI 팀 소유 | 도메인 팀 소유 | CORE 팀 소유 |
|---|---|---|
| SPA 셸·패널 수명주기·상태 저장 | 패널 내용·해석·RPC 명세 | 장치 worker·권한·정책·감사 |
| Trace·플롯·터미널 렌더러 | DBC/NRC/DID/정규식 해석 | 이벤트 허브·seq·snapshot·저널 |
| 공통 시간 커서·마커·딥링크 | 도메인 query·집계·min/max | 인증·전송·스키마 생성·정적 자산 서버 |
| TEST 실행 화면·증거 링크 | TEST의 TC/판정/보고서 내용 | TEST 세션·증거 존재·술어 검증 |

- C6에 따라 `loom.viz` 구현 소유를 CAN에서 UI로 이관한다.
- 도메인 명세 템플릿은 `_xteam/ui-panel-template.md`를 따른다.
- M1에 장치 직접 접근 브라우저 API나 별도 UI 전용 backend를 만들지 않는다.
- TUI는 SSH·터미널 사용자용 보조다. 웹과 경쟁하는 도킹·플롯 제품을 중복 구현하지 않는다.

### 1.2 후순위·비범위

| 기능 | 판단 | 이유 |
|---|---|---|
| Tauri 2 셸·OS 사용자 존재 확인 연동 | M3 이후 선택 | 웹만으로 핵심 루프 성립, OS별 추가 시험 필요 |
| 동적 외부 JS 패널·원격 CDN | 비범위 | 오프라인·검토 가능성·CSP·스키마 일치 |
| 10^7 원본 점을 브라우저에서 매번 전부 렌더 | 비범위 | query 기반 축소·극값 보존으로 해결 |
| LAN 직접 공개·클라우드 계정·다중 조직 | 후순위 | 초기 원격은 SSH 포워딩 |
| 드래그 애니메이션·고급 탭 검색·테마 편집기 | 후순위 | 장치 데이터 가시성과 직접 관련이 적음 |
| 임의 Python/JS 수식·HTML payload 실행 | 비범위 | 해석은 도메인의 제한된 선언형 엔진 |
| CANoe 모든 기능의 즉시 복제 | 비범위 | Trace/Graphics/IG/Data 작업 흐름을 단계별 제공 |

## 2. 사용 시나리오

### 2.1 에이전트 관점 — 세 가지 이상

| 시나리오 | 구체 행동 | 사용자가 동시에 보는 것 | 종료·경계 |
|---|---|---|---|
| UART 부팅 검사 | `loom uart send dut "reboot\r"`; `loom uart expect dut 'login:' --timeout 30` | AGT 송신·장치 부팅 로그·대기 남은 시간 | timeout·결과 seq·사용자 개입 |
| CAN 자극·응답 | `loom can send ch0 0x300#01`; bounded trace query | AGT/DEV/USR 구분·Fixed count·gap | 행·바이트 상한, next_cursor |
| UDS VIN 읽기 | `loom uds read-did F190 -t ecu1` | 완료 전부터 진행 행, RX 뒤 VIN·rtt | 같은 request_id·NRC·timeout |
| 데이터만으로 플롯 | `loom viz show live:can:ch0 --y EngRPM --layout analysis.json` | 동일 live 신호·원본 커서·축소 배지 | CLI는 view id 반환 후 종료 |
| 통합 TC 실행 | `loom test start tc/boot.tc.md --plan tc/boot.plan.yaml` | 현재 단계·실시간 증거·서버 검증 판정 | 단계별 증거·승인 범위·STOP |

### 2.2 사용자 관점 — 세 가지 이상

| 시나리오 | 입력·제어 | 같은 스트림에서 결과 |
|---|---|---|
| 에이전트 UART 명령에 개입 | 터미널 포커스 후 일반 키·Ctrl-C | user lease와 origin=user TX, agent는 개입 인지 |
| CAN 분석 중 수동 자극 | Trace 송신줄·IG 값 변경·takeover | 바이트 preview·write_state·owner/revision 표시 |
| UDS 느린 응답 관찰 | pending 행 펼침·abort·poll stop | 0x78 누계·P2/P2*·이미 보낸 요청의 불확정 상태 |
| 위험 요청 승인 | 셸 승인 목록에서 대상·diff·위험 검토·코드 입력 | approval_ref만 기록, 토큰 원문 미노출 |
| 과거 증거 확인 | TEST 증거 seq 클릭 → Trace/원문 동시 이동 | LIVE와 REPLAY 명시, replay 쓰기 금지 |
| 늦게 합류 | 이미 작업 중인 채널의 패널 열기 | 장치 재연결 없이 snapshot+live |

- 사용자 개입으로 기대 결과가 바뀌면 TEST는 해당 출처와 판정 제외·포함 규칙을 보여 준다.
- TEST UI는 에이전트 문장을 PASS로 바꾸지 않는다. 검증기 결과만 확정 판정으로 표시한다.
- 장치가 조용하면 idle이며, heartbeat가 끊기면 STALLED다. 둘을 같은 상태로 표시하지 않는다.

## 3. 아키텍처

### 3.1 프로세스·데이터 흐름

```text
agent loom CLI ───────────────┐
                             v
장치 <-> Rust worker <-> loomd gateway / hub / policy / journal
                             | 동일 origin HTTP + JSON-RPC WS
                             v
               Browser ConnectionManager (viewer credential)
                    | 제어                | 데이터
                    v                     v
           ShellControlStore       Decode Worker / bounded ring
            STOP·승인·env          viewport selector / query cache
                    |                     |
                    +---- PanelHost ------+
                         UART / CAN / UDS / viz / TEST
```

- 외부 WS는 `_xteam/core-ui.md` U1에 맞춰 탭당 하나로 시작한다.
- 제어·데이터는 논리 큐를 분리한다. 이미 TCP에 쌓인 대용량 데이터보다 제어를 앞세울 수는 없다.
- 따라서 서버 writer는 작은 배치 사이마다 제어 큐를 확인하고, application credit으로 미전송 데이터를 제한한다.
- 클라이언트의 `WebSocket.bufferedAmount`는 수신 backlog 계측값이 아니다.
- 표준 WebSocket 수신에는 자동 backpressure가 없으므로 서버측 credit·ack가 중요하다. [MDN](https://developer.mozilla.org/en-US/docs/Web/API/WebSocket)
- 서버 기록·ISO-TP 제어를 브라우저 처리 속도로 늦추면 안 된다.
- 제어 연결의 응답 부재에는 "STOP 요청 중/미확인"을 표시한다. 클릭만으로 정지 성공을 칠하지 않는다.

### 3.2 상태 관리·패널 수명주기

| 상태 종류 | 저장 위치 | 갱신·상한 |
|---|---|---|
| STOP·승인·env·record.health·연결 | 셸의 작은 external store | 즉시 반영, 요청 ID별 갱신 |
| 원본 CAN·UART 청크 | Worker의 typed ring | 고정 byte 상한, schema별 보유량 |
| UDS 거래 | request_id map + 완료 LRU | 진행 건수·완료 행 수 모두 상한 |
| 화면 선택·필터·폭·커서 | 패널 로컬 상태 | 저장 가능한 선언형 설정 |
| 과거 query | 별도 byte-LRU | 64 MiB 예산, 취소·세대 검사 |
| layout | 버전 있는 JSON | 1 MiB 상한·허용 패널 ID 검증 |

- React 상태에 프레임마다 객체 배열을 append하지 않는다.
- React에는 viewport revision·선택·표시용 작은 projection만 알린다.
- external store 구독은 `useSyncExternalStore`로 묶고 표시 갱신을 rAF/20 Hz로 제한한다. [React API](https://react.dev/reference/react/useSyncExternalStore)
- 패널 닫기 → refcount 감소 → unsubscribe → Worker 포트·observer·timer·GPU 자원 해제.
- 같은 토픽을 여러 패널이 쓰면 가능한 한 구독·원본 링을 공유한다.
- 숨겨진 탭은 화면 데이터 credit을 줄이되 STOP·승인·연결 상태 구독은 유지한다.
- 다시 보이면 backlog 전체 재생보다 최신 snapshot+gap 표시를 우선한다.

### 3.3 패널 등록 계약 초안

```ts
interface PanelSpec {
  id: string; version: number; title: string;
  topics: TopicFilter[]; snapshotKind: string;
  maxResidentBytes: number;
  queries: string[]; actions: string[];
  viewKinds: ('terminal'|'trace'|'timeseries'|'state'|'custom')[];
}
interface PanelContext {
  subscribe(spec: TopicFilter): Subscription;
  query(method: string, params: unknown, signal: AbortSignal): Promise<unknown>;
  invoke(method: string, params: unknown): Promise<WriteReceipt>;
  openSource(ref: SourceRef): void;
  reportRender(sample: RenderSample): void;
}
```

- 실제 타입은 Rust→Schema→TS 산출물로 교체한다. 위는 역할 설명용이다.
- 정적 manifest에서 허용된 panel ID를 로컬 chunk로 lazy import한다.
- 외부 데이터가 component URL·코드·HTML을 지정할 수 없다.
- 도메인은 템플릿의 목적·snapshot·kinds·RPC·입력·지연·gap·수용 시험을 채운다.
- UI는 도메인 `decoded`, `risk`, `approval.display`를 렌더하고 의미 해석을 복제하지 않는다.
- 공통 RowStore는 키별 갱신·가상화·선택을 공유한다. 모든 도메인을 같은 행 스키마로 강제하지 않는다.

## 4. CLI / API 명세 초안

### 4.1 UI 진입·배치·딥링크

| 명령 초안 | 동작 | 주의 |
|---|---|---|
| `loom ui [--layout NAME] [--no-open]` | bootstrap URL 출력·브라우저 열기 | 일회성 자격, 실제 발급 권한은 CORE 합의 |
| `loom ui open can/trace --channel ch0 --at seq:1204` | 기존 viewer에 focus 요청·URL 제공 | 사용자 자동 포커스 허용 설정 존중 |
| `loom viz show SRC --kind auto --x ts --y a,b` | dataset 등록·view 생성·종료 | 대용량 ingest는 job |
| `loom viz summary VIEW --json` | 원본 기반 요약·next_cursor | 화면 축소 데이터로 계산 금지 |
| `loom stop-all` | 공통 latch 요청 | 출력 유지·사용자 입력 허용 |

딥링크: `/#/can/trace?ch=ch0&since=mark:s2`, `/#/uds/console?target=ecu1&request=r7`.
딥링크에는 자격·원시 승인 토큰을 넣지 않는다. 사용자 문자열은 출력·링크에서 escape한다.

### 4.2 RPC·구독

| 기능 | 계약 | 처리 |
|---|---|---|
| handshake | `hello{proto, client_id, want_role, capabilities}` | cookie/token으로 서버가 role 부여 |
| 구독 | `core.subscribe{filter, snapshot, after_seq, max_batch_ms, encoding}` | 구독별 id·epoch·watermark |
| 초기 상태 | `snapshot_part n/N` → `snapshot_end{stream_seq}` | live 이음매까지 원자적 세대 처리 |
| 이벤트 | JSON-RPC `core.events` | ≤200건·≤64 KiB·≤40 ms, UART 10 ms |
| 우선 이벤트 | 응답·승인·오류·STOP·state·gap | 데이터 표시 예산·필터와 독립 |
| 시간 동기 | `core.time` 또는 `core.time.sync` | 수신·송신 daemon mono 둘 다 필요 |
| 제어 | `core.stop_all`, `core.approval.respond` 등 | CORE 초안의 `core.stop`과 이름 합의 필요 |
| 대용량 | query 결과의 `raw_ref`/동일 origin HTTP GET | cookie 검사·cursor·byte 상한·취소 |
| 렌더 보고 | `ui.latency_sample`/`core.metrics.report` | 단일 공통 schema로 정리 요청 |

- RPC 이름은 초안 간 불일치가 있다. 별칭을 UI가 임의 구현하지 않고 P0 스키마에서 확정한다.
- `origin` 요청 필드는 권한 근거가 아니다. 서버가 actor·role에서 파생한다.
- 재접속 시 조회는 재시도 가능하나 mutation은 자동 재전송하지 않는다.
- 응답 유실 후 `request_id`/`idempotency_key` 상태를 조회하고 `outcome_unknown`을 유지한다.
- snapshot 총량 2 MiB 목표, 64 KiB 이하 part, 중간 수신만으로 LIVE 배지를 켜지 않는다.
- `stream_epoch` 변경·유실 cursor는 resnapshot + 명시 gap을 만든다.
- 필터된 stream에서 전역 seq가 건너뛰는 것은 손실 증거가 아니다.
- 서버 gap 메타 또는 subscription batch ordinal로 손실을 판정한다.

### 4.3 입력·쓰기 결과

| 입력 | 예시 RPC | UI 결과 |
|---|---|---|
| UART 문자·Ctrl-C | `uart.input{channel, lease, bytes}` | 즉시 요청 표시·server ack·입력권 |
| UART hold·yield·clear | 도메인 viewer 전용 메서드 | 채널 상태를 정본으로 표시 |
| CAN 단발 | `can.tx.send` | requested→accepted→sent→confirmed/unknown |
| UDS raw·DID | `uds.request`/도메인 명령 | 같은 request_id 행을 점진 갱신 |
| 승인 | `core.approval.respond{approval_id, decision, challenge}` | 서버 검증 후 대기 해제 |
| 복구 | `uds.recover`/core safe_state 동작 | STOP 예외 범위와 interlock 결과 |

- UI의 disabled 버튼은 설명용이다. 서버 정책 검사를 대체하지 않는다.
- `sent`는 UART의 정상 종단일 수 있으며 `confirmed`로 자동 승격하지 않는다.
- `effect_unverified`, `outcome_unknown`, `late`, `ok_suppressed`는 각각 다른 라벨을 둔다.

## 5. 사용자 뷰어 UX

### 5.1 최소 셸 — 와이어프레임은 하나로 제한

```text
+ Loom / bench / channel dut,ch0,ecu1 | REC ok | LIVE lag 24ms | 승인 1 | STOP +
| UART terminal              | CAN Trace (Chrono / Fixed)                     |
| device output...           | time ch dir AGT/USR/DEV id bytes state         |
| AGT help<CR> / input:user   | ... display.gap N (record status separately)   |
| raw keys / Ctrl-C          | TX [0x300#01____________________] [Send]       |
+----------------------------+-----------------------------------------------+
| UDS transactions: request r7  queued -> tx -> pending x3 -> rx              |
| target/session/security/lease | request [22 F190_______] [Send] [Abort]      |
+ Activity / test step / markers / recovery actions / connection heartbeat --+
```

- env=vehicle은 폭이 좁아도 숨기지 않는다. vehicle 정책은 bench와 동일하다(B4).
- STOP: "agent/script 쓰기 정지 — 출력 유지 중". 재개는 별도 명시 사용자 동작이다.
- 비상 OFF: 출력 유지 장치별 대상·현재 상태와 함께 노출, 확인 없음·정책 판정 결과 표시.
- 일반 safe_state 복구 송신은 interlock·deny를 통과한다(A15).
- A3의 비상 OFF는 interlock 경고 처리 예외와 구분해 CORE에 확정을 요청한다.
- 화면 Pause는 수집·기록을 멈추지 않으며 새 데이터 수와 재개 시 gap을 표시한다.

### 5.2 UART — xterm.js 통합의 핵심

- xterm.js 6.0.0 + WebGL addon 0.19.0, 실패·context lost 시 DOM fallback을 추천한다.
- 독립 xterm DOM 컨테이너 밖에 상태·승인·출처 UI를 둔다. VT가 셸 상태줄을 지우지 못한다.
- UART §5.2의 정제 원칙은 유지하되 TUI의 rows−1 clamp를 웹에 그대로 옮길 필요는 없다.
- 서버 `loom-console`의 청크 경계 보존 sanitizer가 위험 escape·단말 질의를 제거한다.
- 브라우저도 parser hook allowlist로 DA/DSR/window report/DECRQSS 등의 자동 응답 경로를 차단한다.
- xterm `onData`에는 사용자 입력과 에뮬레이터 응답이 함께 나올 수 있다. 그대로 UART에 연결하면 안 된다.
- 확인 소스: 설치된 `@xterm/xterm/src/common/InputHandler.ts`의 `triggerDataEvent`, `deviceStatus`, `requestStatusString`.
- [xterm 6.0.0 소스](https://github.com/xtermjs/xterm.js/blob/6.0.0/src/common/InputHandler.ts)는 OSC52 지원도 추가했다. 기본 안전을 가정하지 않는다.
- parser hook 차단은 ANSI 질의별 fixture로 검증한다. 사용자 키 패턴만 비교해 응답을 추측 제거하지 않는다.
- readonly: 입력 송신 금지 + 자동 응답 송신도 0건이어야 한다.
- 원시 Ctrl-C는 selection copy와 충돌한다. 터미널 포커스에서는 0x03 송신, 복사는 메뉴·Ctrl-Shift-C를 제공한다.
- `Ctrl-] x` STOP, `Ctrl-] Ctrl-]` 리터럴 0x1D, 나머지 접두키는 UART 명세를 따른다.
- Ctrl-W/T/N 등 브라우저 예약 단축키는 웹에서 모두 탈취할 수 없다. 화면 키 팔레트·바이트 송신 버튼을 제공한다.
- IME·붙여넣기·Alt/Meta·비영문 키보드를 분리 시험한다. keyCode만으로 바이트를 만들지 않는다.
- 붙여넣기 4 KiB 초과는 preview, bracketed paste 제어와 내용은 구분한다.
- terminal.write callback은 파싱 처리 완료다. 실제 화면 표시 완료는 별도 렌더 계측이다. [Flow control](https://xtermjs.org/docs/guides/flowcontrol/)
- write backlog를 byte 상한으로 제어하고, 넘치면 화면만 drop+gap+parser/SGR 재동기화한다.
- UART 수신 reader·저널을 xterm의 write callback까지 기다리게 하지 않는다.
- InputArbiter의 owner·lease·input_dirty·hold·agent lock은 서버 상태다.
- 미완결 줄은 idle timeout으로 깨끗하다고 간주하지 않는다. 브라우저 종료 후에도 input_dirty는 남는다.
- 최신 UART U3의 다중 viewer 표현과 기존 §3.7의 선점 규칙이 다르므로 서버 계약 하나로 합의해야 한다.

### 5.3 CAN·UDS·기타 1차 패널

| 패널 | 구현·입력 | 정직성 |
|---|---|---|
| CAN Trace | Chrono 가상화·Fixed 최신값, raw TX, query 기반 과거 탐색 | coalesced와 display.gap 구분·count? |
| CAN Graphics | ≥16 신호·최대4 축 그룹·동기 커서·줌 | min/max N→M·원본 커서·시각 품질 |
| CAN IG/Data | 바이트 preview·job owner/revision/lease·takeover | user 시작 R3도 서버 승인 정책 적용 |
| UDS console | request_id별 guard/queued/tx/pending/rx/timeout/verify | pending 중 계속 갱신·late 오결합 금지 |
| UDS 값·DTC | decoded 필드·상태 비트·age·출처 | DTC format 확인 시만 P/C/B/U 표시 |
| LA lanes | 캡처 상태 실시간, 파형은 종료 후 | "캡처 후 표시"·capture_gap·정렬 오차 |
| DLT trace | ECU/APID/CTID/level·필터 pushdown·컨텍스트 | display/record/source 손실 구분·MCNT unknown |
| loom.viz | 파일·live 어댑터·metrics 자동 렌더 | numeric/enum/bool/string·gap·source 역참조 |

- CAN Fixed 키는 단순 ID가 아니라 `(channel, id, extended, frame_class)`로 잡는다.
- 다른 데이터 길이·FD flag의 의미를 도메인 규칙으로 결정한다. 매 프레임 count와 최신값은 별개다.
- 사용자 입력·승인·마커를 Trace 필터로 숨겨도 셸 제어·Activity에서는 확인할 수 있다.
- LA의 전이 보존 요약에 일반 LTTB를 쓰지 않는다.
- TEST 화면은 현재 단계·원문/계획·검증 exists/in_window/predicate·증거 seq·질문·판정 이력을 제공한다.
- TEST의 pause는 스케줄링 정지이며 장비 시계·진행 중 요청이 멈춘다고 표시하지 않는다.
- TEST 원판정과 사용자 수정 판정·사유를 모두 보존하며 증거 없는 PASS는 서버가 거부한다.
- TEST UI는 TEST 계획 §5·새로 전달된 `_xteam/ui-test.md`의 shell.test 명세를 따른다.

## 6. 데이터 / 기록 포맷

### 6.1 정본·브라우저 표현

- 정본은 Rust serde 타입과 CORE 저널. UI 메모리는 조회·표시용 캐시다.
- D3의 session/channel/kind/origin/actor/request/job/generation/risk/purpose를 손실 없이 매핑한다.
- uint64 ns·seq는 JSON decimal string이다. JSON number로 바꾸지 않는다.
- 브라우저에서는 epoch 기준 차이를 BigInt로 뺀 뒤 안전 범위의 ms만 Number로 변환한다.
- origin enum은 agent/user/device/script 네 종류를 유지한다. EXT는 external_actor 등 부가 메타 배지다.
- 출처를 수신 에코·바이트 값으로 추측하지 않는다.
- 승인 원시 토큰·UDS key는 렌더 데이터·로그·URL에 넣지 않는다.
- `raw_ref`는 파일시스템 절대 경로가 아니라 권한 검사되는 불투명 artifact ID다.
- export/공유 마스킹과 로컬 표시 정책은 에이전트와 UI가 동일한 결과를 보도록 적용한다.

### 6.2 전송 비교·D2 공존 제안

| 형식 | 브라우저 비용·장점 | 단점·사용처 |
|---|---|---|
| JSON | 내장 parser·debug·기존 D2와 동일 | 문자열·행 객체·hex 변환·중복 키; M1 기본 |
| MessagePack | 바이트 payload·숫자형을 직접 표현 | decode 뒤 행 객체 생성, u64 의미·JS BigInt 옵션 필요 |
| Arrow IPC | typed column·schema·nullable·파일/분석 연계 | IPC metadata·dictionary·청크 수명·JS 의존성 비용 |
| 자체 열 프레임 | 고정 CAN 필드·TypedArray 직접 접근 | schema evolution·정렬·검증·fuzz·Rust/TS 이중 구현 부담 |

- M1은 JSON-RPC 텍스트·core.events를 유지한다. MessagePack을 RPC 전체에 강제하지 않는다.
- M2 전 CAN 요구의 바이너리는 capability 협상 후 데이터 subscription만 opt-in으로 제안한다.
- JSON fallback은 같은 의미의 필드를 제공해야 하며 바이너리에서 origin·seq·quality를 생략하면 안 된다.
- 자체 열 형식 초안: magic/version/schema_hash/sub_id/batch_no/epoch/row_count/column_offsets + 열 버퍼.
- 공통 값(session/channel 등)은 배치 사전으로 옮길 수 있으나 canonical envelope로 복원 가능해야 한다.
- uint64는 LE u64, 바이트 payload는 offsets+values, nullable은 validity bitmap으로 정의한다.
- 열 offset 정렬·길이 합·count·최대 메모리를 검사한 뒤 view를 만든다. 64 KiB 합산 상한을 지킨다.
- 하나의 논리 배치를 여러 WS 메시지로 보낼 경우 part 식별·총량·최대 동시 조립 수가 필요하다.
- snapshot과 바이너리 live의 watermark를 연결한다. 제어 우선 재정렬은 batch fencing으로 감지한다.
- 단일 전역 stream_seq만으로 부분 토픽·복수 큐의 연속성을 추정하지 않는다.
- Arrow IPC는 대용량 과거 query·artifact에 우선 검토한다. 라이브 작은 배치에 자동 채택하지 않는다.
- Arrow가 columnar라고 브라우저 전체 경로가 무복사라는 뜻은 아니다. 정렬·변환·전송 소유권에 따라 복사된다. [Arrow 형식](https://arrow.apache.org/docs/format/Columnar.html)
- WS permessage-deflate는 loopback 기본 off를 제안한다. 압축 CPU·지터·메모리는 따로 측정한다.
- 원격에서는 TLS와 압축이 다른 문제다. TLS는 인증·기밀성, 압축은 대역폭의 선택이다.

### 6.3 Worker·SharedArrayBuffer

- Web Worker가 WS·decode·필터 projection을 소유하고 main에는 viewport 데이터·작은 상태만 전달한다.
- ArrayBuffer transferable을 기본으로 삼고, 소유권 이동 뒤 detached buffer 재사용을 막는다.
- 재활용 pool도 byte cap을 갖는다. Worker message backlog는 credit으로 제한한다.
- SAB는 COOP `same-origin` + COEP `require-corp` 또는 `credentialless`와 secure context가 필요하다. [MDN isolation](https://developer.mozilla.org/en-US/docs/Web/API/Window/crossOriginIsolated)
- loopback `http://127.0.0.1`은 potentially trustworthy origin이므로 HTTPS 없이도 조건을 충족할 수 있다. [MDN secure contexts](https://developer.mozilla.org/en-US/docs/Web/Security/Defenses/Secure_Contexts)
- 실제 스파이크는 같은 origin에 COOP/COEP를 설정하고 `crossOriginIsolated`를 기록한다.
- 초기 구현은 SAB 없이 동작해야 한다. iframe·원격 embedding·WebView 지원 차이를 fallback으로 흡수한다.
- SAB 도입 시 producer/consumer index·Atomics·wrap·overwrite gap·reader 생존을 명시한다.
- main thread의 blocking Atomics.wait는 사용하지 않는다. SAB가 데이터 경합을 자동 해결하지 않는다.
- COOP로 popup opener가 끊길 수 있으므로 multiwindow 도킹은 별도 수용 시험이 필요하다.

### 6.4 viz 데이터·재생

- long JSONL/CSV: `ts,series,value,unit,quality,source{domain,channel,seq,rx_offset}`.
- wide CSV는 시간 열·수치 열을 명시/탐지하고, 애매하면 선택 UI를 띄운다.
- 숫자가 아닌 DID·enum·bool은 State/표로 보내며 부적절한 연속선을 그리지 않는다.
- `quality=gap/invalid`·generation 변경·추출 실패 구간은 선을 끊는다.
- source seq·UART rx_offset·UDS request_id를 끝까지 보존해 원문으로 돌아갈 수 있게 한다.
- 재생은 `replay:<session>` 네임스페이스·원래 origin·원래 timestamp를 보존한다.
- replay 화면에서 쓰기 RPC는 서버가 거부한다. 과거 TX를 현재 물리 송신으로 오인시키지 않는다.
- 레이아웃 저장에는 패널 설정·시간 커서만 포함하고 자격·민감 payload는 제외한다.

## 7. 라이브러리 선택과 근거

### 7.1 조사 방법·재현 가능한 근거

- 조회일: 2026-11-10 KST. npm `dist-tags.latest`의 안정판을 조회했으며 beta/canary와 구분했다.
- `spikes/ui/sol/npm-evidence.json`: 버전·라이선스·stable 게시일·저장소·unpackedSize.
- `compat-evidence.json`: 게시된 버전의 dependencies·peerDependencies. README main과 배포판 차이를 확인한다.
- `additional-evidence.json`: dockview-react·기타 플롯 후보. `crates-evidence.json`: Rust 후보.
- `source-evidence.json`, `evidence/`: 확인한 1차 소스 URL·HTTP 상태·문서 사본.
- unpackedSize는 gzip 번들 크기가 아니다. 아래 번들 수치는 직접 빌드해 따로 측정했다.
- stable 게시일은 유지보수의 한 신호일 뿐이다. 오래된 stable만으로 프로젝트가 중단됐다고 판정하지 않는다.
- exact dependency와 integrity는 `package-lock.json`에 보존한다.

### 7.2 프레임워크 — React 권고, 고빈도 경로는 외부 store

| 후보 | 확인 버전·라이선스 | 갱신 모델·고빈도 유의점 | 생태계·유지 부담 | 판단 |
|---|---|---|---|---|
| React / react-dom | 19.3.1 / MIT | 렌더·reconcile 비용, 프레임마다 state 배열 append 금지 | TanStack·dockview-react·FlexLayout 직접 연계 | 셸·폼·접근성 기본 권고 |
| Preact | 11.2.0 / MIT | 작은 VDOM, compat 경로는 별도 검증 | React peer를 만족한다고 자동 간주 불가 | 작은 독립 뷰 대안 |
| SolidJS | 1.9.18 / MIT | signal 기반 세밀한 갱신, 큰 객체를 전부 반응형으로 만들지 않음 | TanStack 지원, 도킹은 core imperative 연계 | 기술상 유력, 인력·통합 비용 추가 |
| Svelte 5 | 5.57.3 / MIT | runes·컴파일 기반, 대형 deep reactive 배열 주의 | TanStack 어댑터, xterm·uPlot imperative 래핑 | UI 팀 숙련 시 대안 |

출처: 각 패키지 [React](https://registry.npmjs.org/react/latest), [Preact](https://registry.npmjs.org/preact/latest), [Solid](https://registry.npmjs.org/solid-js/latest), [Svelte](https://registry.npmjs.org/svelte/latest).
모델 확인: [React external store](https://react.dev/reference/react/useSyncExternalStore), [Solid fine-grained reactivity](https://docs.solidjs.com/advanced-concepts/fine-grained-reactivity), [Svelte $state](https://svelte.dev/docs/svelte/$state).

| 직접 측정한 최소 counter | minified ESM byte | gzip byte | 해석 |
|---|---:|---:|---|
| React + DOM | 222,826 | 69,102 | 셸 생태계에 지불하는 기본 비용 |
| Preact + hooks | 13,138 | 5,562 | compat·실제 패널 의존성은 제외 |
| Solid + web | 11,328 | 4,491 | JSX 없이 동등한 DOM counter |
| Svelte 5 compiled | 44,857 | 16,875 | compiler 자체는 런타임 번들에서 제외 |

- 측정법: `node bundle-sizes.mjs`, esbuild 0.28.2, production·minify·browser condition, gzip 기본값.
- 이 수치는 전체 Loom 번들이나 프레임워크 성능 순위가 아니다. startup·memory 비교도 아니다.
- S1은 프레임워크 없는 DOM/canvas 경로다. React·Solid·Svelte의 10k/s 실측으로 주장하지 않는다.
- React를 고르는 근거는 직접 연결되는 도킹·폼·접근성·표 생태계와 유지 인력의 위험 감소다.
- 10k/s 원본 처리는 framework 밖에서 수행하고 셸 갱신량을 제한하면 번들 차이와 별개로 제어 가능하다.
- xterm/uPlot은 작은 lifecycle adapter를 직접 소유해 wrapper 버전 지연을 줄인다.
- React Compiler·특정 상태 라이브러리를 M1 필수 의존성으로 두지 않는다.

### 7.3 고속 Trace — DOM 가상화부터, canvas는 전환 가능한 renderer

| 후보 | 버전·라이선스 | 장점 | 한계·gate |
|---|---|---|---|
| TanStack Virtual | react 3.14.26 / core 3.19.11, MIT | 행·열 가상화, DOM 접근성·선택·상세 쉬움 | 데이터 저장·정렬·필터·byte cap은 앱 책임 |
| Glide Data Grid | 6.0.3, MIT | canvas·대규모 행·편집·열 조작 | stable peer는 React 16–18, React 19와 미합치 |
| 자체 canvas | 별도 라이브러리 없음 | batch draw·고정 열 제어·typed ring 직결 | 선택·복사·IME·접근성·hit-test·고DPI 직접 구현 |

출처: [TanStack 도입](https://tanstack.com/virtual/latest/docs/introduction), [배포 peer](https://registry.npmjs.org/@tanstack%2Freact-virtual/latest), [Glide 배포판](https://registry.npmjs.org/@glideapps%2Fglide-data-grid/latest), [Glide 소스](https://github.com/glideapps/glide-data-grid).

- Glide main README의 React 19 지원 문구와 npm 6.0.3 peer 범위는 다르다. `--force` 설치로 해결했다고 하지 않는다.
- M1: TanStack DOM viewport·고정 행높이·row detail 별도 drawer. M2의 실측에 따라 renderer를 교체한다.
- S1의 pooled DOM은 TanStack 자체 benchmark가 아니다. canvas 역시 Glide benchmark가 아니다.
- 수백만 행을 하나의 거대 CSS spacer로 만들지 않고 logical cursor + 유한 페이지 viewport로 나눈다.
- 브라우저별 최대 scroll height·float 정밀도는 실제 버전에서 별도 측정한다. 고정 한계값을 일반화하지 않는다.
- 최근 원본 링 기본 100k 행, UI 전체 byte cap 안에서 2m까지 선택 확장, 오래된 것은 query page로 이동한다.
- 정상 유입 10k/s 전체를 ring에 받는 것과 화면에서 10k 행을 사람이 읽는 것은 다르다.
- viewport 밖은 렌더 생략이지만 query 가능한 저장 행은 보존한다. 정책상 전체 행을 버리면 display.gap을 만든다.
- Chrono 예산 2k 행/s는 CAN 명세의 표시 정책이다. raw 수신 처리량과 별도 카운터다.
- Fixed: 최신값·누계·직전 timestamp를 ring append 없이 key별 갱신, dirty key만 10–20 Hz projection한다.
- Extended CAN ID 전체 공간을 배열로 미리 할당하지 않는다. key-LRU·상한·퇴출 수를 표시한다.
- gap 이후 count·period는 불완전(`?`), 마지막 값 age와 분리한다.
- 정렬·filter 변경은 Worker 또는 서버 query에서 실행하고 취소·generation 검사로 낡은 결과 덮어쓰기를 막는다.
- canvas 선택 시 키보드 표·접근 가능한 detail·copy/export 경로를 M1부터 별도로 제공해야 한다.

### 7.4 플롯 — uPlot + 서버 min/max 우선

| 후보 | 확인 버전·라이선스 | 다중 신호·커서 | 10^7점 처리 판단 |
|---|---|---|---|
| uPlot | 1.6.32, MIT | 다축·live setData·커서 동기·missing data | 원본 전체를 매 frame 공급하지 않음 |
| webgl-plot | 1.1.2, MIT | GPU line·rolling waveform, 축·커서 통합은 추가 | GPU도 원본 메모리·전송·float 정밀도 문제 남음 |
| regl | 2.1.1, MIT | 저수준 WebGL 구성 | plot 엔진·축·picking·복구를 자체 구현해야 함 |
| ECharts | 6.1.0, Apache-2.0 | 범용 차트·interaction 풍부 | 패키지·기능 범위 증가, 같은 부하 미실측 |
| Plotly dist-min | 4.2.0, MIT | 분석·WebGL trace 가능 | 벤치 패널 기본으로는 기능·의존성 큼, 미실측 |
| TimeChart | 0.5.2, MIT | WebGL 시계열 후보 | npm stable 2020-12-07, 현재 유지 상태 추가 확인 필요 |

출처: [uPlot](https://github.com/leeoniya/uPlot), [webgl-plot](https://github.com/danchitnis/webgl-plot), [regl](https://github.com/regl-project/regl), 추가 후보 버전은 `additional-evidence.json`의 npm 조회.

- uPlot stable 게시일 2025-03-14, webgl-plot 2026-02-16, regl 2024-11-12를 확인했다.
- 위 날짜는 해당 stable 게시일이다. 현재 branch 활동·장기 유지보수 약속은 미확인이다.
- uPlot은 Canvas2D 기반이며 일반 데이터 파싱·통계·집계를 대신 해 주지 않는다.
- webgl-plot의 README 수치·벤더 데모는 이번 Loom 측정으로 옮기지 않는다.
- 원본 10^7점 × (Float64 timestamp 8 + Float64 value 8) = 신호 하나에 약 160 MB 산술 추정이다.
- 16 신호별로 같은 원본을 복제하면 GB 단위가 된다. 원본은 데몬 저널·query 계층에 둔다.
- `signal.query{t0,t1,px,mode:minmax}` → series당 ≤min(4×px,20000), first/last/min/max를 시각 순서로 반환한다.
- 확대 시 재질의, 원본 source 범위·유효성·원본 개수·반환 개수를 함께 받는다.
- spike·극값 보존이 기본이다. LTTB를 선택하면 모든 극값 보존을 약속하지 않는다.
- gap·quality 비정상 구간은 null/segment로 끊고 spanGaps=false에 해당하는 동작을 시험한다.
- 시계가 다른 비동기 신호를 무조건 union timestamp로 펼쳐 큰 희소 배열을 만들지 않는다.
- uPlot aligned dataset이 필요한 그룹은 의미가 맞는 시간축으로 query하고, 임의 보간은 하지 않는다.
- cursor·alarm·TEST 판정·summary는 원본 query 결과로 계산한다.
- live scroll은 t0/t1 window 이동·20 Hz 갱신, pause 후에는 과거 구간이 움직이지 않게 한다.
- 동기화된 두 cursor·Δ·단위별4축 그룹은 UI adapter 책임, 16신호에서 pan/zoom ≥30fps를 M2 gate로 둔다.
- WebGL 전환은 동일 시나리오에서 uPlot 예산 초과가 재현될 때 결정한다.
- GPU context lost·devicePixelRatio·다중 plot context 수·드라이버 차이를 비용에 포함한다.

### 7.5 터미널·도킹

| 패키지 | 확인 안정판·라이선스 | 결정 |
|---|---|---|
| @xterm/xterm | 6.0.0 / MIT | 원시 키·VT·scrollback 기반 |
| @xterm/addon-webgl | 0.19.0 / MIT | 선택 가속, DOM fallback |
| @xterm/addon-canvas | 0.7.0 / MIT, peer xterm ^5.0.0 | xterm 6 조합에서 제외 |
| dockview / dockview-core / dockview-react | 8.9.0 / MIT | M2 도킹 우선 후보, React용 패키지 구분 |
| flexlayout-react | 0.11.1 / MIT | React 18/19 peer, 유력 대안 |
| golden-layout | 2.6.0 / MIT | 기본 기능 가능, stable 2022-09-26로 API·유지 확인 추가 |

출처: [xterm 6 release](https://github.com/xtermjs/xterm.js/releases/tag/6.0.0), [Dockview 설치](https://dockview.dev/docs/overview/installation), [Dockview 라이선스](https://dockview.dev/docs/overview/licence), [FlexLayout](https://github.com/caplin/FlexLayout), [Golden Layout](https://github.com/golden-layout/golden-layout).

- xterm 6 release의 #5105는 Canvas renderer 제거를 명시한다. 오래된 addon이 npm에 남아 있다고 지원 조합은 아니다.
- xterm WebGL 성공·실패와 context loss를 별도로 기록한다. throughput 수치는 §9와 RESULTS에 둔다.
- Dockview의 `toJSON/fromJSON`, lifecycle·resizing·framework-independent core가 패널 계약과 잘 맞는다.
- Dockview free MIT 범위에서 split·tab·floating·save/restore·기본 접근성을 제공한다.
- 고급 탭 검색·pinned tabs·layout history·keyboard docking 등 Enterprise 기능은 무료라고 약속하지 않는다.
- FlexLayout은 React 전용이지만 JSON model·factory·popout·변경 action 경로가 명확하다.
- M1은 단순 고정 split으로 먼저 완성한다. M2에서 Dockview/FlexLayout lifecycle·resize 비용을 비교한다.
- 도킹 중 xterm resize는 debounce, uPlot setSize는 한 rAF당 한 번, hidden panel은 draw를 멈춘다.
- 저장 JSON은 앱의 versioned layout model로 감싸 vendor serialization 변경을 migration한다.
- popout은 새 viewer 자격·presence·입력권·COOP 상호작용을 시험한 뒤 허용한다.

### 7.6 Rust 타입 → JSON Schema → TS

| 후보 | 확인 버전·라이선스 | 장점 | 판단 |
|---|---|---|---|
| schemars | 1.2.3 / MIT | serde 속성 반영·JSON Schema 출력 | C3 정본 경로 |
| json-schema-to-typescript | 16.0.0 / MIT | schema에서 TS·주석 생성 | UI 코드 생성 후보 |
| ts-rs | 12.0.1 / MIT | Rust derive에서 TS 직접 출력·serde 호환 | drift 교차 검사용 대안, 정본 경로 대체 안 함 |
| specta | stable 1.0.5, 최신 2.0.0-rc.28 / MIT | 타입 export 생태계·Tauri 연계 | stable/RC API 차이·지원 범위 확인 필요 |
| typeshare | 1.0.5 / MIT OR Apache-2.0 | 다언어 생성·선택 타입 표시 | 전체 RPC·runtime validation은 별도 |

출처: [schemars](https://docs.rs/schemars/1.2.3/schemars/), [TS generator](https://github.com/bcherny/json-schema-to-typescript), [ts-rs](https://github.com/Aleph-Alpha/ts-rs), [specta](https://crates.io/crates/specta), [typeshare](https://github.com/1Password/typeshare).

- `Serialize`/`Deserialize` + `JsonSchema`를 가진 wire DTO를 내부 장치 타입과 분리한다.
- C3에 따라 Rust→Schema→TS를 기본으로 하고 Python 예외 어댑터도 같은 schema를 사용한다.
- ts-rs 직접 경로는 도구 수가 줄지만 JSON Schema가 필요한 C3·다언어 검증을 없애지는 못한다.
- specta의 stable 1.x와 2.x RC 문서를 섞어 API가 있다고 주장하지 않는다.
- `u64/i64`는 wire DTO에서 decimal-string newtype으로 표현하고 schema `type:string`·패턴을 고정한다.
- ts-rs 기본 large integer `bigint`는 JSON decimal string 계약과 다르다. 명시 override 없이 직결하지 않는다.
- 태그 enum·flatten·optional/null·skip_serializing_if·byte encoding을 fixture로 검증한다.
- TS 타입 생성은 runtime validation이 아니다. handshake·제어·snapshot·binary header를 별도로 검증한다.
- Schema draft 2020-12와 generator 지원 keyword subset을 P0에서 실제 compile fixture로 확인한다.
- generated 파일 수동 수정 금지, schema hash·변경 diff·Rust serialize→JS validate roundtrip을 CI gate로 둔다.
- 추가 직접 검증: [`schema-probe/README.md`](../../../spikes/ui/sol/schema-probe/README.md).
- schemars1.2.3의 draft2020-12 → generator16.0.0 → TypeScript7.0.2를 실제 실행하고 Ajv8.20.0 strict로 검증했다.
- tagged enum·decimal-string newtype·optional 필드에서 u64 최대값·2^53+1을 정확히 보존했다.
- number seq·알 수 없는 tag·필수 필드 누락은 거부했다. 숫자 문자열 문법·u64 범위 자체는 별도 검증이 필요하다.
- direct ts-rs12.0.1은 `skip_serializing_if=Option::is_none`의 note를 required로 생성해 실제 JSON fixture가 tsc에서 실패했다.
- schema 경로는 `note?: string|null`로 생성해 통과했다. 직접 경로는 `#[ts(optional=nullable)]` 같은 명시 규약·fixture가 필요하다.
- ts-rs의 serde(transparent) 경고와 예상 tsc 실패를 보존했다. 모든 serde 속성 지원을 주장하지 않는다.

### 7.7 내장 배포·Tauri

| 후보 | 확인 버전·라이선스 | 쓰임·차이 |
|---|---|---|
| rust-embed | 8.13.0 / MIT | 자산 lookup·빌드 내장, debug/release 동작·feature 확인 |
| include_dir | 0.7.4 / MIT | 정적 디렉터리 내장, 단순·예측 가능한 구성 |
| Tauri | 2.12.5 / MIT OR Apache-2.0 | 선택 데스크톱 셸, OS WebView와 배포 의존성 |

출처: [rust-embed](https://docs.rs/rust-embed/8.13.0/rust_embed/), [include_dir](https://docs.rs/include_dir/0.7.4/include_dir/), [Tauri process model](https://v2.tauri.app/concept/process-model/), [Tauri prerequisites](https://tauri.app/start/prerequisites/).

- 초기 권고: `include_dir` 또는 CORE 표준 하나로 UI dist를 release 바이너리에 내장한다.
- byte 크기·precompressed gzip/br·ETag·MIME·해시 manifest·SPA fallback은 CORE와 합의한다.
- CDN·원격 폰트·외부 icon fetch 없이 빈 캐시·네트워크 차단 상태에서도 부팅해야 한다.
- bootstrap·RPC·raw_ref 응답은 static asset cache 정책과 분리한다.
- 외부 dev proxy는 개발 모드 전용·origin allowlist이며 release 기본 동작으로 두지 않는다.
- Tauri 장점: 독립 창·예약키 대체 UI·OS presence·프로필 자격 분리·설치 패키지.
- 비용: Windows WebView2 런타임, Linux WebKitGTK/GTK·GPU 환경, macOS WKWebView 차이.
- Tauri가 같은 Chromium을 세 OS에 묶어 주는 것은 아니다. WebView2와 WebKitGTK 렌더 성능은 미측정이다.
- 이번 macOS headless Chromium 수치로 Linux WebKitGTK·Windows WebView2 성능을 보장하지 않는다.
- 오프라인 Windows 설치는 WebView2 런타임 배포·정책을 별도 확인해야 한다.
- Tauri의 native command로 장치를 다시 열지 않는다. 같은 loomd·권한·감사를 사용한다.
- 초기에는 웹을 정본으로 유지하고 Tauri에서만 가능한 기능은 capability로 선언한다.

## 8. HW / OS 지원 매트릭스

| 환경 | M1 UI | 실HW 연결 책임 | 시험 상태·한계 |
|---|---|---|---|
| Windows x64 + Chromium/Edge | 지원 목표 | Rust UART·PCAN FD·Vector worker | UI 성능·WebGL·키보드 미실측 |
| Linux x64 + Chromium/Firefox | 지원 목표 | Rust UART·SocketCAN FD 등 | CI 기능시험·전용 runner 지연 예정 |
| macOS arm64 + Chromium | 개발·sim·원격 보기 | sim 또는 지원되는 도메인 backend | 이번 S1/S2 측정 환경 |
| macOS Safari/WebKit | 기능 지원 목표 | 동일 loopback daemon | terminal 키·COOP·Worker·canvas 별도 gate |
| Tauri Windows WebView2 | 후순위 | 기존 loomd | OS WebView 실측 필요 |
| Tauri Linux WebKitGTK | 후순위 | 기존 loomd | 드라이버·배포판·font·GPU 차이 미확인 |
| 원격 macOS→Windows/Linux | SSH local forwarding 기본 | 장치는 벤치 PC worker | 동일 origin·별도 bootstrap·시계 오프셋 |

- PCAN/VN5640 소유권·CANoe 공존·CAN FD·DoIP 전송은 각 도메인의 실HW gate다.
- UI는 raw ID/bytes뿐 아니라 backend·env·time source·generation·관측 가능 여부를 표시한다.
- 브라우저가 지원해도 OS가 장치 backend를 지원하는 것은 아니다.
- 원격 RTT·추정 오프셋 불확실성은 local 성능 목표와 따로 보고한다.
- 원격 direct LAN bind는 기본 off. SSH 터널·Host allowlist·viewer cookie로 시작한다.
- LAN 직접 노출을 추가하면 TLS/WSS·pairing·credential lifetime·접속 감사 계약이 선행한다.

## 9. 시뮬레이션 & 테스트 전략

### 9.1 이번 스파이크 — 완료 결과는 RESULTS가 정본

- S1: Node `ws` 생성기 → loopback WS → Chromium headless → 2m-row typed ring → pooled DOM/canvas.
- 10,000 frames/s, ≤200 rows/message, ≤64 KiB, 압축 off, 각 조합 3 s warmup + 60 s 측정.
- 조합: JSON/DOM, JSON/canvas, column/DOM, column/canvas, MessagePack/canvas, column/Fixed.
- history ring은 seed 데이터로 미리 채운다. Fixed만 2,048 slot으로 작게 할당한다.
- 렌더는 최신28행 viewport다. 모든 유입 프레임의 개별 픽셀 표시나 과거 scroll UX를 증명하지 않는다.
- 지연은 batch 생성 시각 → 해당 draw 뒤 다음 rAF의 **proxy**다. compositor presentation ACK가 아니다.
- 생성기는 flush 때 행을 만들므로 독립 장치 이벤트의 배치 축적 대기시간도 이 수치에 포함되지 않는다.
- 각 배치의 모든 행이 보였다는 뜻이 아니다. 최신 화면 갱신 지연과 ingest 손실을 분리해 보고한다.
- batch descriptor가 추적한 행이 viewport에 있는지 검증한 sample은 아니다. 특히 Fixed의 일부 ID만 화면에 보인다.
- JSON decode 시간과 column DataView 생성 시간만 비교하면 불공정하다. ingest 시간을 함께 확인한다.
- 소형 5-field synthetic row이며 D3 전체 envelope·64-byte CAN FD·Rust 서버는 이번 측정에 포함되지 않는다.
- S2: xterm 6에서 DOM/WebGL 각각 4.2 MB 출력의 write callback까지 처리량·이후2rAF 시간을 측정한다.
- S2는 sanitizer·실UART·키→장치 지연 시험이 아니다. WebGL 초기화 성공을 실제 GPU 가속 증명으로 보지 않는다.
- gap 주입: viewer consume을 500 ms 멈추고 생략 count·seq 범위와 LIVE-DEGRADED 문구를 대조한다.
- STOP 시험은 브라우저 버튼 이벤트 처리만 확인한다. loomd·worker latch의 실측이 아니다.
- 30분 soak는 column/canvas 한 조합에서 수행, 30 s마다 강제GC 뒤 JS heap·ArrayBuffer·DOM 수를 기록한다.
- 강제GC 표본은 자연GC peak·전체 renderer RSS·GPU 메모리와 다르다. 24 h C2 통과를 의미하지 않는다.
- 상세 수치·환경·명령·한계: [`spikes/ui/sol/RESULTS.md`](../../../spikes/ui/sol/RESULTS.md).

| 실측 경로 | 생성→다음rAF p50 / p95 / p99 | draw p95 | 해석 |
|---|---|---|---|
| JSON / pooled DOM | 25.417 / 33.124 / 34.513 ms | 0.510 ms | 원본2m-row 링·viewport28행 |
| JSON / canvas | 25.502 / 33.293 / 34.668 ms | 0.520 ms | DOM 대비 우위 입증 안 됨 |
| column / canvas | 25.375 / 33.073 / 34.244 ms | 0.645 ms | decode+ingest·전송 byte를 따로 평가 |
| column / Fixed | 25.040 / 32.814 / 33.894 ms | 0.395 ms | 최신ID2,048 slot |

- 환경: Apple M1·RAM16 GiB·Darwin27 arm64·Node25.6.1·Chromium158.0.8127.18 headless.
- 정상6조합 모두 약10k/s·seq gap0·측정 descriptor drop0, rAF interval p95 16.670 ms.
- synthetic 전송은 JSON 약53.21 B/행, column 약24.15 B/행. full D3 대역폭으로 환산하지 않는다.
- S2 write callback 처리량 DOM7.001 / WebGL7.019 MiB/s. 렌더·키 지연 수치와 구분한다.
- 30분 soak 최종값은 RESULTS의 완료 표본과 함께 확정한다. 24 h 제품 gate는 미실행이다.

### 9.2 HW 없는 CI — 제품 구현 때의 필수 시험

| 계층 | 입력·결함 | 검증 |
|---|---|---|
| codec | JSON/MP/column 동일 golden fixture | canonical 필드·u64·payload·null·endianness 동일 |
| transport | 잘린 frame·oversize·unknown schema·malformed offsets | 상한 내 reject·재동기화·크래시 없음 |
| snapshot | join 중 RX·part 중 disconnect·epoch 변경 | 중복/누락 없음 또는 명시 gap |
| renderer | 10k/s·2m history·16 signals·UART flood | DOM·canvas·terminal별 지연·노드·heap |
| 우선 제어 | data queue 포화 중 UDS final·승인·STOP | 묻힘·drop 없음, 포화 disconnect면 명시 |
| loss honesty | display/record/source 손실을 각각 주입 | 정확한 배지·range·unknown·record 문구 |
| 키·입력권 | 미완결 줄·Ctrl-C·다중 viewer·disconnect | InputArbiter 규칙·priority·origin 검증 |
| 보안 | DA/DSR/OSC52/DCS·XSS payload·origin 위조 | 의도 없는 TX 0건·권한 우회 없음 |
| TEST | 없는 seq·창 밖 증거·gap 내 부재 주장 | PASS 거부·inconclusive·사용자 수정 감사 |
| lifecycle | 패널100회 열기/닫기·도킹·hidden→visible | 구독·listener·Worker·GPU 자원 누수 없음 |
| 배포 | 네트워크 차단·빈 cache·단일 바이너리 | 모든 자산 로컬, bootstrap·쿠키·raw_ref 검사 |

- Playwright는 실제 DOM의 data-request-id·출처·행 상태를 검사한다. WS 메시지만 보고 성공 처리하지 않는다.
- canvas는 screenshot·선택 detail DOM·렌더 sample을 함께 검증한다. invisible test hook 하나로 화면을 대체하지 않는다.
- 일반 CI 3 OS는 논리·순서·정직성 중심, 지연 수용은 고정 머신·전용 runner에서 판정한다.
- Chromium/Firefox/WebKit 기능시험과 OS WebView 자체 시험을 구분한다. Playwright WebKit은 Tauri WebKitGTK 실측 대체가 아니다.
- 실제 장치 timing gate는 UART 115200/921600, PCAN FD, VN5640·ISO-TP·DoIP와 병행한다.
- sim은 같은 스키마·쓰기 경로·안전 gate를 사용한다. 특별한 숨은 UI용 장치 연결을 만들지 않는다.
- seed·rate·payload·CPU 제한·WS 배치·browser flags·백그라운드 여부를 artifact에 기록한다.
- 정상 프로파일은 drop 0이 필수다. 계속 버리고 gap만 정직하게 보여 주는 것은 성능 합격이 아니다.

### 9.3 C2 브라우저 메모리·24 h soak 목표

| 자원 | 초기 제안 상한 | 초과 정책 |
|---|---|---|
| live raw ring | 탭 전체 128 MiB | 오래된 것 퇴출·과거 query·범위 안내 |
| decode in-flight | 8 MiB / 128 batches 중 먼저 | data credit 축소·local display.gap |
| control projection | 4,096 items / 4 MiB | 조용히 drop 금지, resnapshot·STALLED |
| plot query/cache | 64 MiB | byte-LRU·취소·세대 검사 |
| terminal write pending | 1 MiB/channel, 탭 합4 MiB | 화면 생략·sanitizer 재동기화·gap |
| terminal scrollback | 5,000행, 열수·cell 메모리 예산 별도 | 오래된 행 퇴출·원문 query |
| UDS 완료 거래 | 10k rows / 32 MiB 중 먼저 | 과거 page query |
| total UI data budget | 256 MiB | 패널별 admission·자동 최신 모드 제안 |

- 표의 byte cap은 데이터 예산이다. JS 엔진·DOM·font·GPU를 포함한 전체 탭 RSS 상한과 같지 않다.
- 24 h: CAN 고부하 + UART flood + UDS polling + 패널 전환·join·재접속·기록 장애 주입.
- 목표: 1 h warmup 뒤 renderer RSS 증가 ≤64 MiB/23 h, post-GC heap 증가 ≤16 MiB, 새는 DOM/listener 없음.
- 절대 RSS 예산 초기안 512 MiB/주요 workspace, 3 OS 기준 머신으로 M0에서 보정한다.
- peak·post-GC·ArrayBuffer·renderer RSS·GPU를 각각 기록하고 signed delta·slope·최댓값을 보고한다.
- 초기화·lazy chunk·font atlas 성장은 warmup과 구분한다. 끝점 두 개만으로 leak 없음을 확정하지 않는다.
- 탭을 숨겨 놓고 rAF가 돌지 않은 것을 낮은 CPU/지연으로 보고하지 않는다.

### 9.4 브라우저 ↔ 데몬 monotonic 정렬·lag

1. 브라우저가 `b0=performance.now()`로 time RPC를 보낸다.
2. 데몬은 `d1=receive_mono_ns`, `d2=send_mono_ns`, `clock_id`, `stream_epoch`를 응답한다.
3. 브라우저가 `b3=performance.now()`를 얻는다. 동일 단위 ms로 안전하게 변환한다.
4. `theta=((d1-b0)+(d2-b3))/2`: daemon mono ≈ browser perf + theta.
5. `delta=(b3-b0)-(d2-d1)`; 비대칭 오차를 포함한 offset 구간·대략 ±delta/2 불확실성을 함께 보관한다.
6. 첫8회 중 최소 delta 표본을 기준으로 삼고, 이후5 s 주기·가시성 복귀·재연결 때 재측정한다.
7. drift 추정은 천천히 적용하되 큰 불연속·clock_id/epoch 변경·sleep 복귀는 기존 매핑을 폐기한다.
8. event time을 `b_event=d_event-theta`로 매핑하고 `b_render_proxy-b_event`를 샘플링한다.
9. epoch 기준 BigInt ns 차이를 먼저 구한 뒤 Number ms로 바꿔 2^53 초과 정밀도 손실을 막는다.
10. daemon이 프로세스 상대 Instant를 쓰면 worker clock을 CORE가 daemon clock으로 정규화해야 한다.
11. 장치 HW clock·원격 ECU clock을 이 공식으로 브라우저에 바로 맞추지 않는다. CORE의 time_source·quality를 따른다.
12. 왕복 지연의 비대칭은 완전히 제거할 수 없다. 불확실성이 목표보다 크면 "동기 불확실"로 보고한다.

- rAF callback은 paint **전**에 실행된다. 다음 rAF도 완전한 실제 화면 표시 증명은 아니다.
- xterm onRender/onWriteParsed·React commit·uPlot draw 완료도 API별 의미가 다르다.
- 따라서 RenderSample에는 `stage=receive|decode|commit|draw|next_raf|presented`·seq 범위·clock_error를 명시한다.
- 고정 runner의 headed Chromium trace·frame presentation 관측 또는 고속 화면 캡처로 proxy 오차를 보정한다.
- `lag`는 알려진 미렌더 이벤트 중 가장 오래된 age이며, 서버 queue age와 client queue age를 분해한다.
- 미렌더 이벤트가 없으면 lag=0 + idle 시간. heartbeat 마지막 수신과 마지막 장치 RX를 따로 저장한다.
- 대기 queue가 없더라도 상류 watermark가 오래됐으면 transport lag/unknown 상태를 표시한다.
- 상태: LIVE=신선·무손실, LAG=경로 목표 초과, STALLED=heartbeat3회 없음, LIVE-DEGRADED=drop/축소 중.
- STALLED는 다른 배지보다 우선, record/source loss는 상태와 독립 경고로 계속 보인다.
- UDS 기존 초안의 wall clock 뺄셈만으로는 host clock jump·sleep에 취약하므로 monotonic RPC로 통일한다.
- `ui.latency_sample`은 표본1/50 또는 주기1 s 통계로 제한하고 측정 자체가 고부하를 만들지 않게 한다.

## 10. 안전 / 가드레일

### 10.1 인증·로컬/원격

- 일회성 bootstrap → HttpOnly·SameSite=Strict cookie → viewer 연결 자격을 기본으로 한다(D2).
- HTTPS에서는 Secure cookie를 사용하고, loopback HTTP cookie 정책은 실제 browser별로 검증한다.
- bootstrap 소비 뒤 즉시 redirect·토큰 폐기·Referrer-Policy no-referrer·URL/로그 redaction을 적용한다.
- agent token은 viewer cookie로 승격되지 않는다. want_role=user/viewer 요청 필드를 믿지 않는다.
- Host·Origin allowlist를 static·WS upgrade·HTTP artifact에 적용한다. loopback도 CSRF·DNS rebinding을 고려한다.
- 쿠키가 있어도 모든 query·artifact는 세션 권한을 검사하며 임의 로컬 파일 경로를 열지 않는다.
- 초기 원격은 SSH forwarding + 로컬 동일 origin. 직접 LAN은 TLS/WSS·pairing 설계 후 추가한다.
- CSP는 script/style/font/connect 범위를 자체 자산에 제한하고 CSP 위반을 검토한다.

### 10.2 에이전트 자기 승인 — viewer role만으로 해결되지 않음

| 완화 | 효과 | 남는 한계 |
|---|---|---|
| agent token·viewer cookie 분리, cookie HttpOnly | RPC 역할 혼동·JS token 유출 감소 | 같은 브라우저 자동화는 cookie를 읽지 않고도 버튼 클릭 가능 |
| 승인 코드·대상·digest·변경 diff·1 s focus gate | 오타·잘못된 대상·우발 클릭 감소 | agent가 화면·DOM을 읽으면 코드도 입력 가능 |
| 별도 human viewer profile·명시 UI 진입 | 일상 작업의 자동화 세션과 분리 | 동일 OS 사용자가 의도적으로 접근하면 강한 경계 아님 |
| R3/R4 OS/WebAuthn user verification 선택 | 사용자 존재에 더 강한 증거 가능 | 플랫폼·보안키·Tauri 연동은 미확인, M1 필수 아님 |
| 승인 요청·승인자·origin·plan hash 감사 | 사고 추적·재사용 방지 | 승인 자체의 사람 여부를 소급 보장하지 못함 |

- `event.isTrusted`, TTY 존재, 대문자 Y, 길게 누르기, 코드2자 입력만으로 사람임을 증명하지 않는다.
- 브라우저 자동화로 실제 trusted input이 생성될 수 있다. UI 도구를 가진 agent의 자기 승인 가능성을 명시한다.
- 스킬에는 승인·정책 완화 UI 자동화 금지를 적고, 정상 agent 세션에는 viewer bootstrap을 자동 전달하지 않는다.
- 사람 전용 viewer 진입·기존 별도 세션을 기본 완화로 제안하되 악의적인 같은 OS 사용자 방어라고 주장하지 않는다.
- bootstrap TTY 제한도 우발 발급 방지 수준이며 에이전트가 pseudo-TTY를 만들 수 있다는 한계를 남긴다.
- 강한 경계가 필요하면 별도 OS 계정·하드웨어 사용자 확인을 선택하고 CORE·사용자와 범위를 확정한다.
- D6의 목표는 실수 방지·사용자 통제권이다. 불가능한 브라우저 자동화 식별 기능을 약속하지 않는다.

### 10.3 쓰기·STOP·복구

- R0–R4·R2 경로·R3/R4 viewer 승인·digest/epoch/profile/만료 바인딩은 서버 정본이다.
- sim R2 auto, bench/vehicle 도메인 기본 경로, 실제 CAN viewer 기본 등 A8/B4를 그대로 따른다.
- 사용자 본인 R3 요청도 위험 계획을 보여 주고 승인 객체를 서버가 생성·검증해야 한다.
- 사용자 viewer라는 이유로 `immutable_deny`·하드 deny·rate 제한을 건너뛰지 않는다.
- STOP은 agent/script 새 쓰기·잡을 latch하며 사용자 입력은 계속 허용한다.
- 이미 드라이버에 넘긴 송신은 취소 성공으로 표시하지 않는다. UART 청크 중단 바이트 수·UDS outcome_unknown을 남긴다.
- 기록·감사 실패는 agent/script 새 R1+만 차단하고 사용자 입력은 허용 + REC FAILED 배너(A12).
- 사용자 STOP 뒤 finally는 자동 실행하지 않는다. 복구 송신을 user 동작으로 제시한다(A15).
- 비상 OFF와 일반 복구의 interlock 예외 차이는 CORE가 명시해야 한다. UI가 자체로 판단해 해제하지 않는다.
- 자동 reconnect는 관찰 복구만 한다. mutation·주기 송신·승인·잡을 자동 재개하지 않는다.

## 11. 마일스톤

| 단계 | 산출물 | 수용 기준 |
|---|---|---|
| M0 / P0 첫 주 | wire schema·TS 생성·가짜 gateway·렌더 계측 계약 | uint64·snapshot seam·origin·loss fixture, UI와 도메인 병렬 착수 |
| M1 걷는 뼈대 | 정적 split 셸 + UART 터미널·CAN raw Trace·UDS 점진 표 | sim에서 agent 송신→같은 데이터 화면→사용자 입력·STOP·R2, 늦은 합류 |
| M1 성능·정직성 | 브라우저 E2E·flood·렌더 proxy·memory budget | UART100/CAN200/UDS100 ms p95, UDS p99 200 ms, 정상 drop0·주입 gap 정확 |
| M1b TEST 걷는 뼈대 | shell.test 최소 화면·계획/일괄승인·증거표·단계띠·PSU sim 상태/OFF | TEST TC-1의 개입·없는seq PASS거부·판정수정·late join, TEST/CORE schema 의존 |
| M2 실HW·고속 경로 | PCAN FD·UART·UDS 도메인 연결, Fixed·16신호·도킹·binary opt-in | full D3/FD10k/s 비교·Ctrl-C flood100 ms·schema roundtrip·3 OS 기능 |
| M2 C2 gate | 동시 24 h soak·panel lifecycle·기록 장애 | §9.3 heap/RSS/queue cap, source/record/display 손실 정확 |
| M3 분석·통합 | loom.viz·원문 참조·TEST 화면 고도화·기록 탐색 | 원본 근거 cursor·TEST 다중증거 탐색·LA 캡처 후 표시·DLT |
| M4 선택 | Tauri·원격 TLS·고급 레이아웃·export 확장 | WebView2/WebKitGTK·offline 설치·사용자 확인 gate 별도 통과 |

- M1은 TUI만으로 완료 처리하지 않는다(C6). 시각 디자인·도킹 고급 기능은 M1 수용에 끼워 넣지 않는다.
- M1a는 세 도메인 핵심 루프, M1b는 TEST의 최소 TC-1 통합으로 분리한다. TEST 화면 전체를 M3까지 미루지 않는다.
- CORE·도메인 M1이 모두 완성될 때까지 기다리지 않도록 schema fixture·stub로 병렬 진행한다.
- 이 문서의 S1/S2는 M0 근거이며 제품 M1 성능·안전 합격을 대신하지 않는다.
- 강화 목표 CAN150 ms·UART 추출plot200 ms는 팀 합의 뒤 gate로 승격한다.

## 12. 구현 단계 작업 분해

| ID | 작업 | 추천 담당 | 의존 | 완료 증거 |
|---|---|---|---|---|
| UI-00 | CORE DTO·RPC·snapshot·clock·loss fixture 합의 | Sol + CORE | P0 | schema hash·roundtrip |
| UI-01 | shell·env·STOP·승인·connection store | Opus | UI-00 stub | 클릭·키보드·ACK 상태 E2E |
| UI-02 | static PanelHost·lifecycle·subscription refcount | Sol | UI-00 | open/close100회 자원 안정 |
| UI-03 | UART xterm·sanitizer·raw keys·lease 표시 | Sol + UART | UI-02 | DA/DSR TX0·Ctrl-C·dirty line |
| UI-04 | CAN DOM viewport·ring·query·raw TX | Sol + CAN | UI-02 | 동일 seq·10k/s·gap·origin |
| UI-05 | UDS request_id RowStore·console·diff card | Opus + UDS | UI-01/02 | pending→rx·late·unknown E2E |
| UI-06 | latency harness·clock RPC adapter·CI3OS | Sol | UI-00/03–05 | p50/p95/p99·proxy stage |
| UI-07 | M1 통합 시나리오·사용자 개입 수용 | Opus + TEST | UI-01–06 | 원칙4요소·late join |
| UI-08 | full-envelope·CAN FD·binary/JSON 비교 | Sol + CORE | UI-04/06 | canonical equivalence·CPU·heap |
| UI-09 | Fixed·plot min/max·cursor·source 링크 | Sol + CAN/UART/UDS | UI-08 |16신호·극값·gap·원본 cursor |
| UI-10 | docking·layout version/migration·resize | Opus | UI-02/07 | restore·hidden·multiwindow |
| UI-11 | TEST M1b 최소 화면·마커·증거·판정 수정 | Opus + TEST | test schema/UI-07 | 증거 없는 PASS 거부 |
| UI-12 | LA/DLT·viz file/live adapters | Sol + EXPLORE | UI-09 | 캡처 후표시·MCNT·byte cap |
| UI-13 | 24 h soak·RSS·실HW 키 지연·fallback | Sol | UI-08–12 | 상한·queue·재연결 artifact |
| UI-14 | 내장 배포·offline·auth·원격 tunnel | Sol + CORE | UI-07 | 빈cache·noCDN·role 검사 |

- UI-03/04/05는 PanelHost·schema stub 뒤 병렬 가능한 작업 단위다.
- Opus는 셸·상호작용·승인·TEST 화면, Sol은 데이터·렌더·계측·전송에 무게를 둔다.
- 이 분해는 구현 단계의 담당 제안이다. 이번 R1에서 제품 구현이나 타팀 파일 수정은 하지 않았다.

## 13. 리스크

| 리스크 | 영향 | 완화·검증 |
|---|---|---|
| 단순 synthetic 성능의 과잉 일반화 | full D3·CAN FD·다중 패널에서 실패 | 실제 envelope·FD64B·16신호·3OS 재측정 |
| rAF를 실제 paint로 오인 | 종단 지연 과소평가 | stage 구분·headed trace·clock uncertainty |
| 수신 WS backlog·숨긴 탭 | 메모리 무한 증가 | credit·server cap·visibility·resnapshot |
| React 행 객체 누적 | GC·렌더 stall | typed ring·external projection·byte cap |
| canvas 접근성·선택 구현 비용 | 사용자 개입·복사 불편 | DOM 우선·detail DOM·키보드 gate |
| Glide stable peer React19 미지원 | dependency 강제 설치·runtime 오류 | 당장 채택 보류·정식 호환판 실측 |
| xterm 자동 응답·OSC52 | 의도 없는 UART 쓰기·클립보드 영향 | 서버 sanitizer+parser hook·악성 fixture |
| 브라우저 예약키·IME | 원시 키 일부 송신 불가 | 명시 제약·키 팔레트·3OS 키 시험 |
| viewer cookie 자동화 접근 | 에이전트 자기 승인 | 자격 분리·사용자 확인·D6 한계 명시 |
| 제어 큐 무한·TCP HoL | STOP/승인 지연 | queue cap·작은 batch·credit·disconnect |
| 다중 신호 원본 복제 | GB 메모리 | query min/max·공통 원본·cache LRU |
| Tauri WebView 엔진 차이 | WebGL·성능·키·COOP 차이 | 실제 OS WebView 수용·웹 기본 유지 |
| 라이선스·Enterprise 혼동 | 무료 기능 약속 위반 | 배포버전 license·feature 표·lockfile |
| loss 이후 count·통계 오판 | 사용자·TEST의 거짓 판단 | count?·quality·원본 증거·inconclusive |
| schema/codec drift | 잘못된 출처·u64·승인 바인딩 | generated hash·golden fixture·fuzz |

## 14. 사용자에게 물어볼 질문 — 답이 없어도 진행할 기본값

| 질문 | 기본값 | 재결정 시점 |
|---|---|---|
| 주력 벤치 브라우저는 Chromium/Edge인가? | Windows/Linux Chromium을 성능 기준, macOS 개발 | M0 기준 머신 선정 |
| 기본 workspace의 동시 채널·패널 수는? | UART2·CAN2·UDS20target·plot16signals | M0 부하 프로파일 |
| UI 장기 메모리 허용 예산은? | data256 MiB, workspace renderer512 MiB 초기 목표 | 3OS M0 측정 |
| 강한 사람 존재 확인이 필요한가? | D6 실수 방지 + 별도 viewer, R3/R4 presence 옵션 조사 | 실HW 위험 동작 전 |
| 오프라인 설치 PC에 WebView2가 있는가? | 우선 기존 브라우저·내장 SPA | Tauri 착수 전 |
| 원격에서 쓰기·승인도 필요한가? | SSH 터널 위 동일 viewer 정책, 직접LAN off | 원격 실벤치 수용 |

이 질문들은 R1 산출물을 막지 않는다. 기존 사용자 결정 B4(vehicle=bench), C1(Rust), C6(웹 M1)는 다시 묻지 않는다.

## 15. 공통 계약 변경 요청 / 타팀 의존성

### 15.1 CORE에 요청 — 확정 계약을 조용히 바꾸지 않음

| ID | 요청 | 근거·기본값 | 시점 |
|---|---|---|---|
| CR-UI-01 | JSON-RPC 유지 + data-only binary capability | S1은 가능성 근거, M1 JSON·M2 full-schema 재측정 | P0 capability 예약 |
| CR-UI-02 | 서버 data credit·queue telemetry·bounded writer | WS receive backpressure 없음·제어 HoL | M1 최소 cap, M2 credit |
| CR-UI-03 | d1/d2 mono·clock_id/epoch 시간 RPC | wall clock·RTT 하나로 정확한 지연 불가 | P0 |
| CR-UI-04 | RenderSample stage·clock_error·seq 범위 schema | ui.latency_sample/core.metrics.report 이름 불일치 | P0 |
| CR-UI-05 | control queue 포화 시 disconnect/resnapshot | 무한 queue 없이 drop 금지·정직 상태 | M1 |
| CR-UI-06 | replay write를 서버에서 거부 | `기본 off`만으로 사고 방지 불충분 | replay 도입 전 |
| CR-UI-07 | viewer 발급·presence·강한 승인 훅 | browser automation 자기 승인·숨은 탭 | M1 기본분리·M2 확장 |
| CR-UI-08 | STOP·비상 OFF·safe_state 예외를 각각 명시 | A3/A15 interlock 예외 혼동 방지 | P0 정책 표 |
| CR-UI-09 | bootstrap/stop/time/query/RPC 이름 단일화 | CORE·도메인 초안 차이 | schema v1 |
| CR-UI-10 | schema→TS 생성·offline dist embedding | C3/C6·release 단일 배포 | P0+M1 |

- `_xteam/core-ui.md` U1의 단일 WS와 공존한다. 전송 채널을 임의로 추가하지 않는다.
- 진행 중 CORE R1 Sol 답변도 JSON 기본·binary opt-in·credit/유한 큐·decimal-string·렌더 proxy를 제안했다.
- CORE 제안의 시간 이름 `core.time`, 보고 `core.metrics.report`, 정지 `core.stop_all`로 P0에서 통일하는 방향에 동의한다.
- CORE의 연결당/구독당 상한은 UI 내부 cache 예산과 다른 계층이다. full-schema 부하로 함께 보정한다.
- 같은 TCP 연결이 실제로 막히면 제어 메시지도 못 간다. 네트워크 단절 때 무조건 STOP 지연 보장을 약속하지 않는다.
- `coalesce_by`는 화면용 최신값 projection + 집계 count로 정의하고 원본 손실 display.gap과 구분한다.
- drop 금지 제어를 표시하지 못한 채 연결이 끊기면 서버 state snapshot·감사/저널에서 복원한다.

### 15.2 도메인·TEST와 합의할 내용

| 상대 | 받은 요구·제안한 응답 | 미확정 |
|---|---|---|
| CAN | U-CAN-1~10:100k 보존·2k 표시·Fixed·16신호·딥링크·test marker 수용 | binary canonical schema·150ms 강화 gate·user R3 |
| UART | xterm6+WebGL/DOM·자동 응답 차단·raw Ctrl-C·input_dirty·byte cap | 다중 viewer 선점 규칙·reserved keys·sanitizer ownership |
| UDS | request_id RowStore·도메인 승인 display slot·공통 렌더 계측 제공 | wall 비교를 monotonic RPC로 교체·M1 R3 범위 |
| EXPLORE | LA 캡처 후 레인·전이 요약·DLT source.gap·run 복구 | capture artifact·time alignment·decode table schema |
| TEST | shell.test 명세·실행 단계·검증·판정 수정·모든 레인 marker 수용 | event enum·사용자 개입 증거 처리·승인 RPC 이름 |

- 내용 명세는 각 팀, UI 프레임워크·렌더러는 UI 팀이다. UART 팀에 UI 코드를 중복 소유시키지 않는다.
- TEST 신규 명세의 evidence ring2k행·snapshot200행·test.* 우선 큐·viewer-only 판정 수정을 수용한다.
- TEST가 M1 TC-1에 요구한 shell.test 최소판을 UI M1b로 연결하고, 풍부한 분석 기능은 M3에 둔다.
- `approval.grant`, `core.marker` 등 새 명세 이름은 CORE `core.approval.respond`/`core.mark`와 P0에서 정리한다.
- TEST의 origin:user 요청 필드는 자격 증명이 아니다. viewer role 검증 뒤 서버가 origin을 부여한다.
- EXPLORE 신규 명세의 dlt.trace·la.capture·psu.gauge는 M1 도메인 최소 패널로 추가 요구됐다.
- UI M1 필수 UART/CAN/UDS와 병렬 가능한 후속 슬롯으로 계획하되 TEST TC-1의 PSU sim은 셸 최소 상태·OFF를 우선한다.
- `_xteam/` 변경은 리드가 수행한다. 본 초안은 요청만 기록한다.

### 15.3 원칙 자체 점검

- [x] §0에 직접 접근·읽기·쓰기·실시간 표시의 UI 역할·owner 표.
- [x] §0.1에 구체 명령·장치 응답·사용자 개입·late join 시나리오.
- [x] §0.2·§9.4에 지연 예산·시계 정렬·측정 한계·lag.
- [x] §3·§6·§9에 bounded queue/ring·control priority·3층 gap·24 h gate.
- [x] §11의 M1은 sim + 최소 웹 UART/CAN/UDS + 사용자 개입.
- [x] 에이전트 전용 숨은 데이터·별도 UI 장치 연결 금지.
- [x] 루프 밖 기능은 §1.2의 후순위·비범위로 표시.
- [x] 버전·라이선스·소스 근거와 실측 artifact를 별도 보존.
- [x] 제품 구현 없이 자기 파일·스파이크 경로만 작성.
