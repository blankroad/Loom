docs/plan/02-core-principle.md 준수.

# UI R2 — ui-opus 교차 리뷰 (대상: `r1-sol.md` + `spikes/ui/sol/RESULTS.md`)

작성: ui-opus, 2026-11-10. 읽은 것: `r1-sol.md`(851줄), `spikes/ui/sol/RESULTS.md`·`results/run.log`·설치된 `node_modules` 소스 일부, `r1b-opus-xteam.md`, `_xteam/core-ui.md`(`## core 의견` — CORE Sol 초안), `_xteam/explore-ui.md`, `_xteam/ui-test.md`, `_xteam/core-test.md`, `_xteam/test-domains.md`, `test/plan.md` §3.1·§5·§6.4·§11.
총평: sol 초안은 **측정 정의의 정직성·근거 보존이 뛰어나고** 기술 선택 대부분에 동의한다. 문제는 (1) 사실 오류 2건, (2) 스파이크가 UI 수용 시험(UI-T03/T05/T10)을 대신할 수 없는 지점, (3) 패널 계약이 "패널은 선언한 것만 한다"를 강제하지 못하는 점, (4) 새 타팀 문서(TEST·EXPLORE·core-test)의 이름·상태값 미반영이다.

---

## 1. 틀린 사실

| # | sol 위치 | sol 주장 | 실제 (근거) | 영향·수정 |
|---|---|---|---|---|
| F1 | §5.2 | "xterm 6.0.0 소스는 OSC52 지원도 추가했다" | 설치된 `@xterm/xterm@6.0.0` `src/common/InputHandler.ts`가 등록하는 OSC 핸들러는 0·1·2·4·8·10·11·12·104·110·111·112뿐이다(292–331행). **OSC 52 핸들러는 core에 없다**(clipboard addon 영역). 직접 grep으로 확인 | OSC 52 위험은 addon을 로드할 때만 생긴다 → UART 명세대로 "clipboard addon 로드 금지"로 충분. 대신 core가 처리하는 **OSC 8(하이퍼링크)**과 **OSC 4/10/11/12 색 질의 응답**을 막아야 한다(F2) |
| F2 | §5.2·§9.2 | 자동 응답 차단 목록 = "DA/DSR/window report/DECRQSS" | 같은 파일의 `setOrReportIndexedColor`·`setOrReportFgColor/BgColor/CursorColor`(OSC 4/10/11/12)는 `?` 질의에 **보고 문자열을 생성**한다 — `onData` 경로로 장치에 갈 수 있는 자동 응답이다 | 차단 fixture에 OSC 4/10/11/12 `?` 질의 추가. 사용자 의도 게이트(UART 3중 방어 3번째)가 있으면 막히지만, 게이트 없는 parser hook만으로는 빠진다 |
| F3 | RESULTS 상단 | "30분 soak **진행 중 (720 s 표본까지)**" | `results/run.log` 끝: 750 s 표본 뒤 `EPERM: operation not permitted, open 'results/benchmark.json'`으로 **프로세스 종료**. 경로가 옛 Desktop 경로(재부팅 전)였다 | soak는 "진행 중"이 아니라 **750 s에서 중단, 미완**. 재부팅 공지의 "장시간 측정 ≤ 10분" 규칙상 30분 soak 재시도 대신 10분 압축 soak로 다시 정의해야 한다(§2.3) |
| F4 | §0.2 표 | CAN Trace "렌더 대기·제출 33 ms" + 본문 "일반 Trace는 20 Hz" | 20 Hz 스케줄러면 렌더 대기 최대 50 ms(평균 25 ms). 표와 본문이 서로 맞지 않는다 | CAN 게이트 p95 < 150 ms(CAN 명세·sol §0.2가 수용)에서 50 ms를 쓰면 여유가 줄어든다 → 쟁점 18 |
| F5 | §9.3 | 터미널 scrollback 5,000행 | UART 명세 `uart-ui.md` §4: scrollback **10,000줄(상한 50,000)**, 트레이스 링 20,000행 | 도메인 명세를 따른다(r1-opus와 r1b 동일) |
| F6 | §4.2 | 시간 동기 `core.time` "또는 `core.time.sync`" | CORE 두 초안 모두 `core.time`(core-opus §4.7, core-sol core-ui U4). `core.time.sync`는 어디에도 없음 | 별칭 삭제 |
| F7 | §15.1 마지막 줄 | CORE가 "정지 `core.stop_all`"로 통일 제안 | core-sol은 `core.stop_all`, **core-opus §4.7은 `core.stop{scope}` + `core.stop.release`** — CORE 안에서도 갈림 | "CORE 제안"으로 단정하지 말고 쟁점으로(쟁점 16) |
- 확인했고 맞는 것: `@xterm/xterm 6.0.0`, `@xterm/addon-webgl 0.19.0`, `react 19.3.1`, `uplot 1.6.32` 모두 스파이크 `node_modules`의 `package.json`과 일치. NTP식 오프셋 `theta=((d1−b0)+(d2−b3))/2`, `delta=(b3−b0)−(d2−d1)` 수식 맞음. rAF가 paint 전에 실행된다는 설명 맞음. `crossOriginIsolated=true`(COOP/COEP) 관측 기록 맞음.

---

## 2. 스파이크가 제품 수용 시험의 근거로 충분한가

### 2.1 UI-T03 (지연) — 부분 근거. 아래를 더 재야 함
| 스파이크가 보인 것 | 부족한 점 | 추가 측정 (≤ 10분 규칙 안) |
|---|---|---|
| synthetic 5필드 행, 10k/s, 생성→다음 rAF p95 ≈ 33 ms, JSON/열/msgpack 차이 미미 | 행 크기 **53 B/행**. 실제 D3 envelope는 CAN 계획 추정 ≈ 540 B/행 — 10배. JSON 디코드가 정말 문제없는지 미증명 | full D3 envelope(+CAN FD 64 B) JSON vs 열 블록, 10k/s 60 s |
| 디코드를 **메인 스레드**에서 수행 | r1-opus·sol 모두 Worker 디코드 설계. postMessage·transfer 비용·지연 미측정 | Worker 디코드 + transfer 경로로 같은 시험 |
| 생성기가 flush 때 행을 만든다(배치 축적 대기 제외) | 실제로는 장치 이벤트가 배치를 기다린다(UART 10 ms, CAN 40 ms) | 생성기를 "이벤트 시각 독립 발생 → 서버 적응형 배치"로 바꿔 장치시각→paint 측정 |
| 같은 호스트 epoch 시계(`timeOrigin+now`) | Rust `loomd` mono ↔ 브라우저 동기(`core.time` d1/d2)는 미검증 | CORE stub 또는 작은 Rust `core.time` 서버로 오프셋·오차폭 실측 |
| S1 단독(+S2 버스트 동시) | UART 폭주 + CAN 10k/s + UDS 진행 행 **동시**에 메인 스레드 하나 | 3패널 동시 부하에서 패널별 p95 |
| 사용자 입력 지연 없음 | keydown→`uart.input` 송신, Enter→`uds.tx`(UDS 명세 ≤ 50 ms) | Playwright `keyboard.press` 시각 → WS 송신 시각(서버 수신 기록) |
| 렌더 proxy(next rAF) | 실제 표시 시점과의 오차 | 고정 runner headed + CDP `Tracing`(frame 이벤트) 1회 보정 — sol §9.4 제안에 동의 |
- 결론: S1은 "브라우저 렌더 경로가 예산을 크게 먹지 않는다"의 **가능성 근거**로 충분하다. UI-T03 합격 근거로는 위 7항목 중 최소 1–3, 5–6을 M1 전에 다시 재야 한다.

### 2.2 UI-T05 (정직성) — 카운터 논리만 증명
| 보인 것 | 부족한 점 | 추가 |
|---|---|---|
| 클라이언트가 소비를 500 ms 멈추는 주입 → 생략 5,037행, first/last/count 일치, `LIVE-DEGRADED` 문구 | gap이 **클라이언트 자체 생성**(`injected_viewer_credit_exhaustion`). 제품은 서버 `display.gap` 이벤트가 정본 | mock/stub 서버가 `display.gap`·`record.gap`·`source.gap`을 각각 보내고 문구·색·배지가 섞이지 않는지 |
| STOP 버튼 클릭 → DOM ack | 데이터 포화 중 제어 이벤트가 묻히지 않는지 미측정(sol도 명시) | 데이터 큐 포화 + `core.latch`/승인/UDS 최종 응답 동시 주입 → 제어 표시 ≤ 1프레임 |
| — | `STALLED`(heartbeat 끊김), `LAG` 임계, 2 s 초과 "라이브로 점프", Fixed `count?` 표기, 숨김 탭 복귀 표시 | 각 1개 fixture. **숨김 탭 시험은 스파이크 플래그 `disable-background-timer-throttling`·`disable-renderer-backgrounding` 없이** 돌려야 한다(그 플래그가 실제 브라우저 동작을 감춘다) |
| — | 연결 끊김 중 쓰기 버튼 잠금, 재연결 후 `after_seq` 이음/링 밖 재snapshot | 재연결 fixture |

### 2.3 UI-T10 (메모리) — 근거 아님. 측정 설계부터 다시
| 문제 | 이유 | 대안 |
|---|---|---|
| soak 미완(F3) | 750 s 중단 | — |
| 링을 48 MB로 **미리 할당** | 힙 증가가 작게 나오는 게 구조상 당연. 누수 후보(행 객체·리스너·xterm 버퍼·uPlot 배열·구독)는 대부분 시험 밖 | 실제 패널 구성(xterm + Trace + UDS 표 + 플롯)으로 |
| 강제 GC 뒤 JS heap만 | renderer RSS·GPU·네이티브(xterm WebGL 텍스처 아틀라스)·자연 GC 피크 누락 | CDP `SystemInfo.getProcessInfo` 또는 OS `ps`로 renderer RSS, `Memory` 도메인 |
| 정상 상태만 | 누수는 주로 **수명주기**에서 생긴다 | 10분 압축 soak: 10k/s + UART flood + 패널 열기/닫기 100회 + 재연결 20회 + 숨김/보임 전환, 시작·끝 RSS/heap/DOM/리스너 수 비교 |
| 24 h | 재부팅 공지로 로컬 장시간 금지 | 24 h는 전용 runner의 CORE soak 관문에 UI 탭 합류(r1-opus C-13 유지) |

### 2.4 sol 기술 선택 ↔ r1-opus 패널 프레임워크 맞물림
| sol 선택 | 맞물리는 r1-opus 부품 | 판정 | 조정할 점 |
|---|---|---|---|
| React 19 셸 + `useSyncExternalStore` | 셸·모달·승인 대기열, `ControlStore` | 맞음 | `ControlStore`·도메인 스토어를 external store로 노출. React state에 행 배열 금지(sol 규칙)를 lint로 |
| Worker가 WS·디코드·필터 projection 소유 | Net Worker(RpcClient·SubscriptionMux·ClockSync·제어 분류) | 맞음 | Worker 안에서 **제어 kind 분류 → 단건 즉시 postMessage**를 명시(sol은 "논리 큐 분리"까지만) |
| typed ring(Worker) + 메인에는 viewport projection | DomainStores(메인 스레드 링) | **차이** | 링을 Worker에 두면 패널 `store(key).range()`가 동기 접근 불가 → `StoreView`를 "projection 요청 + 최근 N행 미러"로 바꾼다. 대용량 원본은 Worker, 화면용 미러는 메인 |
| TanStack Virtual DOM viewport | 패널 렌더 + RenderScheduler | 맞음 | 가상 스크롤 갱신을 스케줄러 `onFrame(budgetMs)` 안에서만 |
| uPlot + 서버 min/max | `ctx.query`(64 KiB 초과 시 셸이 HTTP) | 맞음 | 질의에 `AbortSignal`(sol) 추가 — 줌 중 낡은 결과 취소 |
| xterm 6 + 공통 터미널 컴포넌트 | `uart.terminal`, `shell.console` 재사용 | 맞음 | 사용자 의도 게이트·접두키·질의 억제를 컴포넌트에. 패널은 `act('input')`만 |
| Dockview(M2) + 앱 버전 레이아웃 모델 | 레이아웃 JSON(`loom_layout:1`), `ui_focus` 토스트 | 맞음 | vendor `toJSON`은 `loom_layout` 안의 하위 필드로만 저장(이주 가능) |
| `RenderSample{stage}` | RenderScheduler 렌더 완료 기록 | 맞음 | 스케줄러가 패널 대신 stage 표본을 찍는다 → 패널마다 계측 코드 불필요 |
| data credit·ack | 숨김 탭·`LIVE-DEGRADED` | 맞음 | 크레딧 소진은 서버 `display.gap{cause:"slow_client"}`로 표시되어야 함(클라이언트 자체 생성 금지) |
| schemars → json-schema-to-typescript | `@loom/panel-sdk` 타입 | 맞음 | `TopicSpec.kinds`·`ActionSpec.rpc`를 생성 타입의 문자열 유니언으로 → 오타가 컴파일 오류 |

**합친 패널 API 초안** (r1-opus §4.2를 대체할 제안 — sol 요소는 `// sol`)
```ts
interface PanelContext {
  readonly props: Readonly<Record<string, unknown>>;
  readonly rights: ReadonlySet<'watch' | 'operate' | 'approve'>;
  store<T>(topicKey: DeclaredTopic): StoreMirror<T>;            // 메인 미러(최근 N행) + projection 요청
  query<R>(rpc: DeclaredQuery, params: object, signal: AbortSignal): Promise<R>;   // sol: AbortSignal
  act(actionId: DeclaredAction, params: object): Promise<WriteHandle>;              // 셸 ActionGate 경유
  openSource(ref: EvidenceRef): void;                            // sol: TEST REF → 패널·seq 이동(= reveal)
  scheduler: { invalidate(): void; onFrame(cb: (budgetMs: number) => void): Unsubscribe };
  reportRender(sample: Omit<RenderSample, 'clock_error'>): void; // sol: stage 표본, 기본은 스케줄러가 자동 기록
  clock: { monoEst(): bigint; errMs(): number };
}
interface PanelManifest {
  id: string; version: number; milestone: 'M1a' | 'M1b' | 'M2' | 'M3';
  topics: TopicSpec[]; snapshotKind: string;                     // sol: snapshotKind
  queries: DeclaredQuery[]; actions: ActionSpec[];               // r1-opus: right·risk·confirm·safetyDirection
  maxResidentBytes: number;                                      // sol 명칭 채택
  render: { maxHz: number; budgetMs: number };
  shellSlots?: ('channel_bar' | 'approval_body' | 'test_evidence')[];  // r1b C-a·C-e
}
```

---

## 3. 빠진 것 (02 원칙·브리프·새 타팀 문서 기준)

| # | 빠진 것 | 출처 | 제안 |
|---|---|---|---|
| M1 | **패널 계약의 선언·강제**: sol `PanelContext.subscribe(spec)`·`invoke(method)`는 런타임에 임의 토픽·RPC를 허용. manifest의 `actions: string[]`에 자격·위험·확인 방식 없음 | 02 "숨은 경로 없음", r1-opus §4.2–4.3 | r1-opus manifest(`TopicSpec`·`ActionSpec{right,risk,confirm,safetyDirection}`) + sol의 장점(`AbortSignal` 질의, `openSource(ref)`, `reportRender(stage)`, `snapshotKind`, `maxResidentBytes`)을 합친다. 미선언 호출은 throw |
| M2 | **WebSerial/WebUSB 차단 수단**: "별도 연결 금지"만 있고 강제 방법 없음 | 02 ① | `Permissions-Policy`(r1-opus C-12) + 패널 lint 규칙 |
| M3 | **WebAuthn 사실**: §10.2·§14가 "미확인"으로 남김 | r1b §4 (출처 URL) | `127.0.0.1`은 RP ID 불가 → 오리진 `localhost` 정본, 확장 `chrome.debugger`가 WebAuthn 도메인 허용 → **등록 게이트 필수**. sol §6.3의 `http://127.0.0.1` 전제와 충돌 |
| M4 | **에이전트가 내 개입을 알았는지** 표시(`⇢ 전달됨` / UDS "에이전트 확인 위치 seq") | uds-ui, r1-opus §5.2.2 | `core.agent_cursor` 이벤트 요청 공동 제출 |
| M5 | 자격 단계(watch/operate/approve)와 "에이전트가 연 브라우저 = watch만" | r1b 쟁점 5, uart-ui Q8·Q12 | sol §10.2의 "정상 agent 세션에 viewer bootstrap 자동 전달 안 함"과 방향 같음 → 단계로 명문화 |
| M6 | TEST 판정 상태값 `done`(자극 전용, R-8), `incomplete`, 증거 검증 5상태 `verified/mismatch/not_found/uncertain/error`, 판정 `{proposed, effective, by, overrides}`, `agent_tx_only` 거부 사유 | test/plan §3.1·§6.4, core-test O4–O6 | 테스트 화면 아이콘·문구 표에 추가(sol §5.3은 exists/in_window/predicate만) |
| M7 | TEST 이벤트 이름 확정안 `test.session_started/ended`, `test.session_state`, `test.step_started/ended`, `test.mark`, `test.evidence_checked`, `test.verdict`, `test.note`, `test.ask`, `test.approval` (이벤트 2토큰, RPC는 3토큰) | core-test O6 | UI 구독·fixture를 이 이름으로 |
| M8 | EXPLORE M1 패널 3개의 구체 요구(아래 §3.1) | explore-ui | sol은 "후속 슬롯"으로만 언급 |
| M9 | 와이어프레임 최소(의도된 분담). 단 **STOP 후 상태·승인 모달**은 안전 UX라 공통 수용 시험이 필요 | r1-common §3 | r1-opus §5.1.5·§5.1.7을 공통안으로 |
| M10 | `coalesce_by` 화면 표기 문구 | core-sol U2 수용 | `coalesced N`과 `display.gap N` 문구 분리 — sol §15.1 정의에 동의, 화면 문구만 추가 |

### 3.1 r1b 이후 반영해야 할 새 요구 목록 (EXPLORE·TEST·core-test)
| 출처 | 요구 | UI 반영 |
|---|---|---|
| explore-ui `dlt.trace` | M1. 트레이스 표 + 컨텍스트 트리(요청/보고/대기 3값), CTRL 행 A14 점진 갱신(`requested→sent→confirmed(ACK ms)→effective_verified`), 레벨 변경 R1 / 전체 레벨 R2(본인 확인 1회), 장치→화면 p95 ≤ 150 ms, CTRL·gap 드롭 금지, snapshot = 트리 + 최근 2k행 | 공통 행 갱신 표(RowStore) 재사용. 컨텍스트 트리는 새 컴포넌트 |
| explore-ui `la.capture` | M1. 진행 상태(`ARMED…READY/FAILED`) p95 ≤ 250 ms, "캡처 중 — 파형은 종료 후 표시" 배지, 캡처 창 음영, 종료→표시 ≤ 3 s, 전이 보존 레인(LTTB 금지), 디코드 표, `uart.tx`/`can.tx` 정렬선·오프셋 라벨(`host-approx`), `s`/`x` 키, 앱 직접 조작은 안 보인다는 고정 안내 | 레인 렌더러(전이 요약) 신규 — uPlot 비적합 가능, canvas 직접. M1 범위가 커지므로 쟁점 22 |
| explore-ui `psu.gauge` | M1(sim). 실측 시계열 60 s, `!` 비상 OFF(확인 없음), 출력 토글 R3, 값 편집 R2, p95 ≤ 100 ms, STOP 후 "출력 유지 중" | STOP 상태 패널의 "출력 유지 장치" 목록 데이터원. TEST TC-1이 PSU sim을 쓰므로 M1b 필수 |
| explore-ui 기타 | `shell.console` = `uart.terminal` 재사용, run 패널 = 테스트 화면 run 모드 | 공통 터미널 컴포넌트(r1b C-d) 확인 |
| explore-ui 이름 | `job.progress`, `la.capture.done`(3토큰), `core.marker` | CORE 규칙에 맞게 `core.job_progress`, `la.capture_done`, `core.mark`로 정리 요청 |
| ui-test `shell.test` | M1: 계획 검토 모달 + 일괄 승인, 단계 목록, 현재 단계, 실시간 증거(UI 링 2k행, 넘치면 `display.gap`), 판정 수정(사유 필수), 타임라인 단계 띠, 보고서. snapshot 200행. 상태 변경 ≤ 200 ms, 명령→현재 단계 ≤ 100 ms | 사용자 일시정지·건너뛰기·재시도·중단(확인 1회)·후보 확인·질문 답·복구 송신 |
| ui-test 이름 | `approval.grant`, `test.verdict{origin:user}`, `records.range`, `core.marker` | `core.approval.respond`(CORE), origin 필드는 요청에서 제거(서버 부여), 범위 조회는 `core.events.query`/HTTP, `core.mark` |
| ui-test 수용 | 단계 시작 → 200 ms 안 ▶ + 모든 레인 띠 / 지어낸 seq → 거부 사유 표시 / 늦은 합류 동일 상태 / STOP 시 finally 미실행 + 복구 키 / 에이전트의 판정 수정 거부 | UI-M1b 수용 시험으로 그대로 채택 |
| core-test O7 | 일괄 승인: `Approval{kind:TestPlan}`, 사용 시작 ≤ 10분, 세션 유효 `max_test_duration` 1 h(≤ 8 h), `max_uses` 잔량 원자 차감, 계획 변경·STOP·epoch 변경 시 무효, 계획 밖 쓰기는 개별 승인 + "계획 밖" 표시 | 상단 바에 개시 기한·세션 유효·잔량, 무효화 시 배너. 계획 밖 행 표식 |
| core-test 기타 | 세션 동시 1개(`E_SESSION_ACTIVE` exit 6), 평가 예산 초과 `inconclusive{cause:"eval_budget"}`, A12 기록 불능 시 PASS 판정 중단, 새 종료 코드 11 | 문구·배지 추가 |
| test/plan §6.6 | 보고서 증거 링크 `loom://records/…` | 브라우저가 못 여는 스킴 → `/ui/#/records/<session>?seq=a..b` (r1b §3) |

---

## 4. 더 나은 선택 (sol안 → 대안, 그리고 r1-opus가 틀린 곳)

### 4.1 sol보다 나은 대안
| 주제 | sol안 | 대안 | 이유 |
|---|---|---|---|
| 패널 쓰기 API | `invoke(method, params)` + `actions: string[]` | `act(actionId)` + `ActionSpec{rpc, class, right, risk, confirm, safetyDirection}` | 셸이 자격·확인 UI·안전 방향 규칙을 한 곳에서 집행. 패널마다 확인 UI를 다시 만들지 않음 |
| 구독 API | `subscribe(spec)` 런타임 자유 | manifest `topics` 선언 + `store(key)` 읽기 전용 뷰 | 구독 공유(refcount)·메모리 예산 할당·감사 가능성 |
| Trace 렌더 주기 | 항상 20 Hz | 정상 = rAF(≤ 60 Hz), 예산 초과 시 20 Hz + `LIVE-DEGRADED` | CAN 150 ms 게이트 여유(F4). viz 요구의 20 Hz는 "고부하 시" 규정 |
| 오리진 | `http://127.0.0.1`(§6.3) | `http://localhost`(바인딩은 127.0.0.1) | WebAuthn RP ID(r1b §4.1). COOP/COEP·보안 컨텍스트는 둘 다 성립 |
| 자기 승인 완화 | 별도 human viewer profile 권장, presence는 "선택·미확인" | 자격 단계 + WebAuthn(등록 게이트) 기본, 불가 호스트는 대역 외 코드 | 확장 기반 자동화는 사용자 실 프로필을 쓰므로 프로필 분리가 S2를 못 막는다(r1b §4.3) |
| soak 설계 | 단일 조합 30분, 사전 할당 링 | 10분 압축 + 수명주기 churn + RSS(§2.3) | 재부팅 규칙·누수 탐지력 |

### 4.2 r1-opus가 틀렸거나 sol이 더 나은 곳 (채택)
| 주제 | r1-opus | sol (채택) | 이유 |
|---|---|---|---|
| 프레임워크 | 미세 반응성 계열 성향 | **React 19 + external store**(`useSyncExternalStore`), 고빈도 경로는 프레임워크 밖 | 도킹·표·접근성 생태계, 번들 차이는 셸 수준에서 무의미. 내 `PanelInstance.mount(el)`가 명령형이라 React와 충돌 없음 — React 패널은 어댑터 `reactPanel(Component)`로 |
| 시계 동기 | 서버 mono 1개 | **d1(수신)·d2(송신) 두 값**, RTT 최소 표본, 오차 구간 보관 | 서버 처리 시간을 오프셋에서 뺀다. core-sol도 같은 안 |
| 렌더 계측 | `render_ack`(1 Hz, 단일 시각) | **`RenderSample{stage, seq 범위, clock_error}`** + 10 s 히스토그램 보고 | 단계별 proxy 의미가 다름을 데이터에 남김 |
| 메모리 예산 | "JS 힙 ≤ 300 MiB" | **데이터 예산 256 MiB(영역별 표) ≠ renderer RSS 512 MiB** | 힙과 RSS를 섞지 않음 |
| 서버 배압 | 언급 약함 | **data credit·ack, bounded writer, 제어 큐 포화 시 연결 해제·재snapshot** | WS 수신에 자동 배압 없음 — 맞는 지적 |
| WS 티켓 | 요청 | 불필요(1st-party 정적 번들 + lint) | r1b에서 이미 철회 제안, sol은 처음부터 없음 |
| 테스트 화면 일정 | UI-M3 → r1b에서 M2 | **M1b**(3도메인 M1a 뒤, 실HW M2 전) | TEST M1이 UI M1 셸 의존으로 명시. 실HW와 묶을 이유 없음 |
| 필터 seq 불연속 | 언급 없음 | "필터된 stream의 seq 건너뜀은 손실이 아니다, 서버 gap 메타로만 판정" | 거짓 gap 표시 방지. core-sol도 동일 |
| Fixed 키 | ID | `(channel, id, extended, frame_class)` + key-LRU 상한 | 29bit 공간·FD 의미 |

---

## 5. 합의 가능한 부분 (두 초안 같은 결론)
- 장치 직접 접근 없음, 탭당 WS 1개 다중화, 서버가 role·origin 부여, 요청 origin 필드 무시.
- Web Worker가 WS·디코드 소유, 메인은 스토어·렌더, 제어/데이터 논리 경로 분리, 숨김 탭은 렌더 중지 + presence 보고.
- M1 = JSON-RPC·JSON 이벤트, 고속 data 구독만 M2 전 binary opt-in(capability), 64 KiB는 wire frame 전체, permessage-deflate off.
- 스키마: Rust serde + schemars → JSON Schema 2020-12 → json-schema-to-typescript, u64 decimal string, generated drift CI, TS 타입 ≠ 런타임 검증.
- xterm.js 6 + WebGL addon(실패 시 DOM), canvas addon 제외, Ctrl-C = 0x03(복사 Ctrl-Shift-C), 자동 응답 차단 + fixture 시험, 브라우저 예약 키는 팔레트로.
- Trace: DOM 가상화(TanStack) 먼저, canvas는 교체 가능한 렌더러. S1이 canvas 우위를 보이지 않음.
- 플롯: uPlot + 서버 min/max(극값 보존), gap에서 선 끊기, 커서·통계는 원본.
- 도킹 M2(Dockview 우선, 앱의 버전 있는 레이아웃 모델로 감싸기), M1은 고정 split.
- 정적 자산 `loomd` 내장, CDN 0, CSP, 원격은 SSH 포워딩, LAN 직접 공개 비범위.
- 쓰기 단계 표시: `sent` ≠ 성공, `outcome_unknown`·`effect_unverified`·`late`·`ok_suppressed` 별도 라벨, mutation 자동 재전송 금지.
- STOP: agent·script latch, 사용자 입력 허용, 출력 유지 표시, finally 자동 실행 금지 + 복구 송신, 비상 OFF와 일반 복구의 인터록 규칙 구분(CORE 확정 요청).
- TEST: 증거 없는 PASS는 서버가 거부, UI는 검증기 결과만 확정 표시, 원판정·사용자 수정 병기.
- 일반 CI 3 OS는 논리·정직성, 지연 합격은 고정 runner. DOM의 `data-request-id`로 확인, WS 수신만으로 합격 처리 금지.

---

## 6. 쟁점 의견 (r1b §5 1–15 + 신규)

CORE 의견(`core-ui.md` `## core 의견`, core-sol)과 다른 곳은 **굵게** 표시.

| # | 쟁점 | 의견 | 한 줄 |
|---|---|---|---|
| 1 | 출처 색 | 동의(청록/노랑) | sol도 문자 배지 병기 — 색만 CAN과 합의 |
| 2 | 승인 입력 규칙 | 동의(1 s, `Y`/버튼만, 기본 포커스 거부) | sol·core-sol의 "1 s focus gate·재확인"과 같음 |
| 3 | M1 승인 범위 | 동의(M1 R2 카드, R3/R4 M2) | sol M1 수용에도 R2 포함 |
| 4 | 사용자 본인 R3/R4 | 동의 + 수정: core-sol의 "본인 확인 1회로 같은 승인 객체 소비" 채택, **존재 확인은 승인과 같은 정책 적용** | core-sol은 존재 확인을 사람 증명으로 쓰지 않겠다는 입장 — 쟁점 6과 같이 결정 |
| 5 | 웹 자격 발급 | 유지(에이전트가 연 브라우저 = watch, operate 승격 = 프로필당 1회 사람 행동). **core-sol·core-opus는 단계 없이 viewer 하나** | 단계가 없으면 `loom ui open`이 곧 쓰기 자격 발급이 된다 |
| 6 | 존재 확인 형식 | 유지 + 수정: `presence{method, proof}`, webauthn은 **등록 게이트 필수**. **core-sol: "challenge 수용, 사람 존재 증명 아님, 강한 경계는 별도 principal"** | 동의하는 부분: 악의적 우회 방어가 아님. 반대하는 부분: 등록 게이트 있는 WebAuthn은 "도움 되려는 자동화"(D6의 대상)를 실제로 막는다(r1b §4.2–4.3) — 옵션으로라도 계약에 자리 필요 |
| 7 | 지연 보고 RPC | **수정: core-sol 이름 `core.metrics.report`(10 s 히스토그램, stage·coverage·clock error) + `core.viewer.presence{visible, rendered_through_seq, oldest_pending_age_ms}` 채택**, r1-opus `render_ack` 폐기 | 두 개로 표본·집계를 모두 덮는다. 시험 하네스의 요청별 판정은 DOM `data-request-id` + 브라우저 시각으로 |
| 8 | 시계 RPC | 수정: `core.time` + **d1/d2(수신·송신 mono)** | ui-sol·core-sol 일치. core-opus의 단일 `mono_ns`는 보완 필요 |
| 9 | 가리키기 | 동의(`loom ui open <panel-ref>`, 묻기 토스트). 이벤트 이름 **core-opus `core.ui_focus` vs core-sol `ui.focus_request`** — CORE가 하나로 | UI는 이름만 받으면 됨. `ui` 예약 domain이면 `ui.focus_request`가 규칙에 맞다 |
| 10 | 쓰기 행 키 | 동의(`request_id` + `payload.write_state` 스키마 필수) | DLT CTRL 행도 같은 규칙으로 들어옴 |
| 11 | 테스트 이벤트 이름 | **수정: core-test O6 채택**(`test.step_started/ended` 등, RPC 3토큰). r1b의 `test.step_begin` 폐기 | TEST·CORE가 이미 합의 |
| 12 | 증거 REF | 해결: `test-domains.md`에서 UART도 TEST 형식(`uart:dut@seq:a..b`) 채택 | 닫음 |
| 13 | 복구 송신 API | 유지(`core.safe_state.list` + `core.safe_state.run{ids[]}` 항목별 결과) | core-sol도 "safe_state 복구 버튼, 비상 OFF와 인터록 구분" — 묶음 실행 RPC만 추가 요청 |
| 14 | 테스트 화면 일정 | **수정: sol의 M1b 채택**(r1b의 M2 철회) | TEST M1 정의와 정확히 맞음 |
| 15 | 오리진 | 유지(`localhost` 정본, 경로는 CORE `/ui/*`·`/api/v1/*`) | sol §6.3의 127.0.0.1 전제를 바꿔야 함 |
| 16 (신규) | STOP RPC 이름 | `core.stop_all`(core-sol·ui-sol·스킬 `loom stop-all`과 일치) vs `core.stop{scope}`(core-opus). 추천 **`core.stop{scope:"all"}` + CLI `loom stop-all`** — 범위 확장 여지. 어느 쪽이든 agent도 STOP 가능·해제는 viewer만에는 세 초안 동의 | 이름만 CORE가 확정 |
| 17 (신규) | `STALLED` 임계 | core-opus 1.5 s / CAN "heartbeat 1 s 끊김" / UDS·ui-sol·r1-opus 3 s. 추천 **heartbeat 1 s, 3 s 무수신 = STALLED, 1.5 s = 회색 경고점** | 탭 throttle·GC 순간 정지로 깜빡임 방지 |
| 18 (신규) | Trace 렌더 주기 | 정상 rAF, 초과 시 20 Hz(§4.1) | CAN 150 ms 게이트 |
| 19 (신규) | 터미널 scrollback·UI 메모리 모델 | scrollback = UART 명세 10k(상한 50k), 예산 모델 = sol(데이터 256 MiB ≠ RSS 512 MiB) | F5, §4.2 |
| 20 (신규) | 타팀 명세 RPC 이름 정리 | `approval.grant`→`core.approval.respond`, `core.marker`→`core.mark`, `records.range`→`core.events.query`/HTTP, `job.progress`→`core.job_progress`, `la.capture.done`→`la.capture_done`, 요청의 `origin:user` 필드 삭제 | CORE §4.7 2토큰 규칙·origin 서버 부여 |
| 21 (신규) | TEST 상태값 렌더 | `done`·`incomplete`·검증 5상태·`agent_tx_only`·`eval_budget`을 아이콘·문구 표로 고정(M6) | 판정이 "PASS/FAIL"보다 많아짐 — 오해 방지 |
| 22 (신규) | EXPLORE M1 패널 범위 | `psu.gauge`는 **M1b 필수**(TEST TC-1·STOP 출력 유지), `dlt.trace`는 RowStore 재사용이라 M1b, `la.capture` 레인 렌더러는 **M2**(M1b에는 상태 배지·디코드 표만) | 걷는 뼈대를 키우지 않으면서 TC-1을 닫는다 |
| 23 (신규) | 스파이크 측정 규칙 | 제품 정직성 시험은 백그라운드 throttle 끄는 플래그 없이, 장시간 측정은 10분 압축 + 전용 runner | §2.2–2.3, 재부팅 공지 |

---

## 7. R3 통합 전에 할 일 (요약)
1. sol: F1–F7 정정, RESULTS의 soak 상태를 "750 s 중단(EPERM)"으로 고치고 10분 압축 soak로 재정의.
2. 공동: §2.1 추가 측정 1–3·5–6(full D3, Worker 디코드, 독립 이벤트 배치, Rust `core.time`, 3패널 동시, 입력 지연)을 M1 전 스파이크로.
3. 공동: §2.4 합친 패널 API를 `_xteam/ui-panel-template.md` v0.2에 반영 요청(리드).
4. 리드: 쟁점 5·6(자격 단계·존재 확인)을 CORE와 한 번에 결정 — 둘이 엮여 있다.
