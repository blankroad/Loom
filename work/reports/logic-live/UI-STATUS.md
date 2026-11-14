# logic UI — UI Opus 반환 (2026-11-14)

기준: main d382628 working tree, `CONTRACT.md` §8, §8 packing 보충, §9 backend 확정 shape, `REVIEW-SOL.md` R1–R6 + B2.
범위: `ui/src/panels/logic/**`, `ui/src/panels/index.ts`, `ui/src/shell/routes.ts`(preset), `ui/src/store/channels.ts`(explorer), `ui/src/shell/dock/activeWorkspace.ts`(history retention), 관련 tests. backend 파일·Git index·브라우저는 건드리지 않았다. 하위 에이전트는 쓰지 않았다.

## 1. 구현 (실제 native 패널, static mock 아님)

`logic.analyzer` — `#/p/logic/analyzer` (단독 workbench) 또는 `#/p/logic/analyzer?channel=<capture_id>` (캡처 고정).

| 파일 | 내용 |
|---|---|
| `ui/src/panels/logic/logicModel.ts` | wire 파싱(capture_state / edges / annotation·annotations / §9 snapshot / logic.status), 캡처별 보관(CaptureData), stale seq guard(CaptureBook), start config ⇄ §9 params, CLI 한 줄 |
| `ui/src/panels/logic/waveform.ts` | 순수 lane geometry(`laneRuns`: level 0/1 · dense · unknown), 디코드 overlap 조회, 시간축 tick, canvas painter |
| `ui/src/panels/logic/logicPanel.ts` | 패널: 상태줄 · 카운터 · STOP 래치 · 설정 폼 · 보기 막대 · 파형 · hover readout · 디코드 테이블 · 해석 불가 줄 |
| `ui/src/panels/logic/logic.css` | panel-scoped Bench Light (`.lg-panel` 아래만, 전역 shell 무변경) |
| `ui/src/panels/logic/logic.test.ts`, `ui/src/shell/logicWiring.test.ts` | 아래 §4 |

사용자 정보:
- 상태줄: 캡처 선택(최신 따라가기 / 개별 capture_id), 상태 chip(시작 중/캡처 중/정지 중/정지됨/실패), source badge(`sigrok demo 드라이버` / `실제 장비` / `SYNTHETIC 생성 파형` 점선), driver·rate·채널 수·디코더·상한·시작 origin(AGT/USR), `■ 캡처 정지`.
- 카운터: 샘플(+sample clock 시간), 전이, 디코드, 전이 생략(dropped_edges), 디코드 손실, 잘린 줄, 해석 못 한 줄(unparsed/_raw), 형식 오류(malformed edge + invalid annotation + overlap chunk), 스트림 손실(gap), 수신 지연(host ts 기준 — 신호 타이밍 아님). 손실 > 0이면 빨강.
- 정지/실패: 사유(user_stop/duration/samples/eof/stop_latch/error), §9 `exit{acquisition,decoder}` (code/signal/stop_style), error, stderr tail(접힘). "기록은 이 패널에 남아 있습니다".
- 정직성 노트: SYNTHETIC(원시는 생성 이상파형, 디코드는 실제 libsigrok), sigrok-demo(경로 실제, 내용 무의미), 보관 상한 밖 구간.
- 파형: 채널별 디지털 step lane, 시간축은 sample/samplerate만 사용. 1 px 안 전이 ≥2 → dense 블록, 데이터 없는 구간(이벤트 간 불연속, cap 이후, malformed 이후, 스트림 손실, 보관 밖) → 빗금 unknown(이전 레벨로 잇지 않음). 디코드 트랙은 decoder id(`uart-1` 등 원문)별 별도 행, 상자로만 표시(원시처럼 그리지 않음). 범례로 raw/dense/unknown/decoded 구분.
- 조작: 휠 확대/축소(커서 기준), Shift+휠·가로휠·끌기 이동, ←/→ +/- 0(전체) F/End(라이브 따라가기) Home, 버튼 −/+/전체/●라이브. 이동하면 follow 해제. hover readout: sample · 시각 · 레인별 레벨(데이터 없으면 `?`) · 그 지점 디코드.
- 디코드 테이블(VirtualTable): sample 시작–끝 · 시각 · 길이 · 디코더 · class(null → `—`, 생성하지 않음) · 내용. 행 클릭 → 파형 중앙 정렬·선택 강조. `_raw`(sample null) 줄은 테이블/파형 대신 "해석 불가 줄" 접힘 목록.
- 설정 폼: source(synthetic / sigrok-demo / sigrok-device), driver(fx2lafw만 선택 가능, saleae-logic16 / logic-pro는 disabled + "미지원, 실물 미검증"), conn, demo 패턴(“지정 안 함” 포함; 지정 시 rate 200 kHz 고정·select 비활성), 합성 텍스트(`{n}`, `\r\n` 이스케이프) + 프레임 주기, 샘플레이트 1 kHz–10 MHz, 디코더 UART(baud, RX) / SPI(CLK/MOSI/MISO/CS#) / 없음, 캡처 시간. 유효성 문제는 시작 버튼 옆 목록. 같은 캡처의 `loom logic start …` 한 줄 표시. 설정 열 때 `logic.status`(엔진 경로/버전/오류, 구현 소스·디코더·장비 드라이버, device_verified, compiled 드라이버 수 "지원 의미 아님").
- 수집은 항상 D0..D7, 레인 체크박스는 표시 선택이며 start의 `channels`(표시 선택, §9)로도 전달된다.

## 2. 계약 정렬 (§8, §9, backend 실제 코드 대조)

- `logic.start` params: `startParams()`는 `source, driver?, conn?, samplerate, channels, decoders(0..1), duration_ms, pattern?, text?, frame_period_ms?` 만 보낸다. `crates/loom-logic/src/model.rs` START_KEYS 집합 밖 키 없음(테스트로 고정). demo는 driver 생략, 패턴 지정 시 samplerate=200000. sigrok-device는 fx2lafw만. synthetic text는 `\r\n` 이스케이프를 실제 CR LF로 변환. `max_samples`, `clock_hz`는 폼에 노출하지 않음(backend 기본값).
- manifest: `start` = `class: 'request'`, right operate (§9: SDK job/renewal 아님). `stop` = request, right watch, `safetyDirection: 'reduce'`, confirm none — eth.capture.stop와 같은 안전 방향 규약(보기 권한도 정지 가능). query = `logic.status`만.
- lifecycle: `logic.capture_state`를 `ctx.onControl` + snapshot/backlog 양쪽에서 받고 capture별 seq guard(`seq ≤ 적용된 seq`면 무시). snapshot capture payload에는 record seq가 없으므로 snapshot high-water(H)로 적용 — 이후 control의 seq ≤ H는 stale, > H는 반영. STOP 래치는 `ctx.latch`로 표시.
- edges: mask/initial = JSON number(u16), `0b…` 문자열은 이전 draft 호환으로만 허용. sample_end exclusive, 절대 index. `mask`/`final`/`active`/`seq` 추가 필드는 무시해도 무해(레인은 capture `channels` 기준).
- annotations: 단일·배치 모두, `class:null` 보존, decoder 문자열 원문, `dropped` 계상, `_raw`(sample null) 보존.
- snapshot: `crates/loom-logic/src/gateway.rs:500` `{captures, annotations, edges}` 와 `readSnapshotItem()` 일치 확인(코드 대조 + 테스트 fixture 동일 shape). 실제 daemon 연결 E2E는 ROOT 몫.
- `logic.status`: §9 `{engine, compiled, implemented}` 로 읽음.

## 3. Sol 리뷰 판정

| ID | 판정 | 조치 / 테스트 |
|---|---|---|
| R1 P1 무전이 chunk 무제한 | **수용·수정** | `MAX_CHUNKS_PER_CAPTURE=4096` 추가, edge 상한과 함께 퇴출(chunks/seenChunk/unknown 동시). gap 재계산을 전체 rescan → 삽입 이웃만 갱신으로 변경. 퇴출 구간은 unknown. 테스트 `R1: edge-free (idle pin) chunks…` |
| R2 P2 invalid sample/rate | **수용·수정** | sample/count = non-negative safe integer만, rate = 유한 > 0. annotation `end<start`·음수는 거부·`형식 오류`로 계상. tStart/tEnd는 rate가 있으면 둘 다 sample에서 계산(역전 불가). 테스트 `R2…` |
| R3 P1 중간 malformed edge → 가짜 HIGH | **수용·수정** | 첫 malformed edge에서 파싱 중단, 마지막 정상 edge 다음부터 chunk 끝까지 unknown(`cause: malformed`). backend prefix cap(`dropped`)과 별도 카운터. 테스트 `R3…` |
| R4 P2 dense pixel이 chunk 끝/unknown 침범 | **수용·수정** | dense run을 piece 끝(`xOf(p.b)`)과 viewport로 clip. 인접 chunk에 나뉜 같은 pixel의 전이는 합산하지 않는다(각 piece 내에서만 dense 판정 — 근사임을 명시). 테스트 `R4…` |
| R5 P2 긴/중첩 annotation 누락 | **수용·수정** | start 정렬 + prefix-max-end 배열(version별 lazy 재계산)로 하한 탐색, 그 뒤 overlap 필터. 표시 상한(2000) 초과분은 `omitted`로 반환하고 파형에 "표시 생략 — 확대" 표시. 테스트 `R5…`, `visible cap reports omitted…` |
| R6 P2 부분 중첩 chunk | **수용·수정** | 동일 span은 조용한 중복, 부분 중첩은 거부·`rejectedChunks` 계상, 이미 퇴출된 범위의 snapshot 재적용은 부활시키지 않음. 테스트 `R6…` |
| B2 demo pattern 200 kHz / rate 범위 | **수용·수정** | 패턴 "지정 안 함" 옵션 추가, 패턴 지정 시 rate 200 kHz 고정 + select 비활성 + 안내, RATES 1 kHz–10 MHz, 검증 문구. 테스트 `start params use only backend keys…` |

## 4. 검사 (이 작업 트리, 브라우저 미사용)

- `pnpm` 계열: `tsc -b` 0 오류, `eslint .` 0 문제, `vitest run` 전체 **53 files / 583 tests 통과**(logic 신규 22: model 15 + panel 4 + painter 1 + shell wiring 2), `check:schema` ok.
- `vite build`는 `ui/dist` 를 덮지 않도록 scratchpad outDir로 실행해 성공(기존 >500 kB chunk 경고만).
- jsdom에는 canvas가 없어 패널 테스트는 DOM/RPC/상태만 검증, painter는 stub 2D context로 smoke. 실제 렌더·상호작용·라이브 daemon은 **미검증** — ROOT 브라우저 QA 대상.

## 5. ROOT 확인 요청 / backend 계약 제안 (선택)

1. snapshot `captures[]` 항목에 record `seq`를 넣어 주면 H 대신 정확한 seq로 stale을 거를 수 있다(현재 H 적용도 안전 방향이라 필수 아님).
2. explorer 상태어: logic 도메인에 한해 `starting/stopping`=open, `stopped/failed`=closed(기록 보존)로 매핑했다(`store/channels.ts` domain 한정). failed 캡처 뷰가 닫히지 않게 하기 위함.
3. `stop` 은 safety direction(right watch)으로 두었다. ROOT 지시 "actions right operate"를 start에만 적용한 해석이다 — 다르면 manifest 한 줄 변경.
4. `CONTRACT.md` 는 ROOT 소유라 수정하지 않았다.

## 6. 변경 파일 (Git index 미사용)

신규: `ui/src/panels/logic/{logicModel.ts,waveform.ts,logicPanel.ts,logic.css,logic.test.ts}`, `ui/src/shell/logicWiring.test.ts`, 이 파일.
수정: `ui/src/panels/index.ts`(+1), `ui/src/store/channels.ts`(EXPLORER_DOMAINS·viewsForChannel·logic 상태어), `ui/src/shell/routes.ts`(preset `logic`), `ui/src/shell/dock/activeWorkspace.ts`(HISTORY_AFTER_CLOSE에 logic, 단독 analyzer slot 항상 허용).

## 7. 통합 검토 추가 반영 (ROOT 요청 2건)

1. **service sidebar**: `routes.ts` `SERVICE_DOMAINS`/`SERVICE_LABELS`(`로직 분석기`)/`SERVICE_PRESET`(`logic`)/`parseRoute` `/s/logic` 추가, `ServiceNav.tsx` `INTERFACE_SERVICES`에 logic 추가(1줄). 활성(running/starting/stopping) 캡처가 있을 때만 링크가 보이고, 미사용·정지 캡처만 있으면 숨김. `/s/logic` 은 활성 캡처의 `logic.analyzer` 슬롯만 연다. 테스트: `serviceSeparation.test.tsx` "lists the logic analyzer service only while a capture is active…" (+ 기존 SERVICE_DOMAINS 순회 route 파싱 테스트가 `/s/logic` 포함).
2. **시작 주체**: envelope `origin`(상태 보고 주체 = device)을 시작 주체로 쓰던 것을 제거. `payload.requested_by`(live·snapshot 모두)만 사용하고, 없으면 표시하지 않는다(`CaptureInfo.requestedBy`, `CaptureBook.applyState` origin 인자 제거). 테스트: `logic.test.ts` "starter comes from payload.requested_by…".

검사(변경분 후 전체): `tsc -b` 0, eslint(변경 파일) 0, `vitest run` **53 files / 585 tests 통과**. dist build는 ROOT가 수행(이번에는 미실행). backend·Git index 무변경.

## 8. Ego live 첫 수신 — 기본 zoom 버그 수정

원인: 캡처가 없을 때의 draw가 `clampView()`로 `spp=0` sentinel을 `MIN_SPP(1/64)`로 바꿨고, `initView`가 `if (spp) return`이라 나중에 들어온 에이전트 캡처에도 64 px/sample 창이 유지됨(증거: ROOT `uart-ui-live.json/png`, "21.601s–21.601s").

수정 (`logicPanel.ts`만):
- `clampView`는 `spp > 0`일 때만 clamp — 0(미설정)은 보존.
- `syncView(c)`가 `initView`를 대체: 보여 주는 capture id가 바뀌면(최신 따라가기 중 새 캡처 도착, 또는 선택기에서 명시 선택) view를 초기화하고 라이브 따라가기를 켠다. 기본 창(`DEFAULT_WINDOW_S` = sample clock 10 ms, rate가 없으면 10 000 샘플)은 samplerate나 데이터가 생긴 뒤에만 설정한다.
- 규칙: 사용자 zoom/pan은 같은 캡처 안에서만 유지되고, 다른 캡처로 바뀌면 기본 창으로 돌아간다. zoom/중앙 정렬은 아직 크기를 정할 수 없으면 아무것도 하지 않는다.

회귀 테스트 (`logic.test.ts`, 실제 DOM 창 표시값 파싱):
- "view: empty draws keep the unsized sentinel…": 캡처 없이 5 frame → 창 표시 없음, rate 없는 running → 없음, 21.601 s 시점 첫 edges → 창 ≈ 10 ms, 끝 = 21.601 s, `smp/px` 표시.
- "view: user zoom stays on the same capture…": ×4 확대는 같은 캡처에서 유지, 새 최신 캡처 `lc-2`가 오면 10 ms + follow로 초기화.

검사: `tsc -b` 0, `eslint .` 0, `vitest run` **53 files / 587 tests 통과**, `pnpm build`(→ `ui/dist`, ROOT 허용) 성공. backend·Git index 무변경. 브라우저 미사용.
