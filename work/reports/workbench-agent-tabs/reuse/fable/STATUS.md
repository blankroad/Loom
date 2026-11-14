# STATUS — reuse/fable (개정 2: 핵심 패널만)

- 2026-11-14 · 모델 claude-fable-5-1 (Claude Code, 환경 보고 모델 ID와 요청 일치, 대체 없음) · foreground · subagent/Git index/Cargo/새 브라우저 미사용 · base 143fd46
- 변경 파일: `index.html`, `app.js`, `styles.css`, `STATUS.md`, `PROPOSAL.md`(부록만 추가). 소유 `reuse/fable/**`만. 원본 UART/ETH/UDS 파일·제품 엔진 무변경. 이전 스크린샷(`browser-qa*.png/json`) 보존(개정 1 증거).
- 확인: `node --check app.js` 통과. HTTP root 127.0.0.1:64785에서 fable/UART/UDS index 200. 브라우저 DOM 검사는 원본 소스(index.html/app.js/styles.css)의 셀렉터·grid 정의를 읽어 설계; 실제 렌더 QA는 ROOT Ego.

## 바뀐 의도 (사용자 수정 반영)
- 통합 보기 = **핵심 관측면만**: 서비스 전체 iframe이 아니라, 같은 frame 안에서 adapter CSS(`core` 모드)로 카탈로그·네비·연결 폼·매크로·전역 툴바를 `display:none` 처리. 원본 DOM과 상태는 그대로(숨김만).
  - UART: 남김 = pane별 세션 탭(식별), 세션 헤더(alias·port·baud/프레임·flow·EOL·연결 상태·RX/TX), xterm, 일시정지/지우기/시각/기록/검색/HEX/EOL/연결, pane 분할·최대화. 숨김 = 레일, 모듈 툴바(`.mtbar`), 세션·매크로 서랍(`#side`), 세션 추가(`.tab-add`, 카탈로그), 매크로 메뉴, 세션 설정 폼. grid를 `"work"` 단일 영역으로 재정의(모든 폭, `!important`라 원본 ≤900/≤640 분기보다 우선).
  - UDS: 남김 = compact 헤더(ECU 선택·경로·주소 요약·연결 상태·연결 버튼), 트랜잭션 트레이스(`#txBody`)+기록 범위 필터+trace 도구+JSON/CSV, 응답 순서·해석·PDU 바이트(hex 복사), 확정 값. 숨김 = 도구 탭/서랍(서비스·ECU·DID·시퀀스·감시·프로젝트), DID 카탈로그, 요청 편집기(`#req`), 연결 설정 폼·주소 상세·SIM 옵션 패널, 기록 지우기. `.wsp` grid를 `"center vals"` 2열로 재정의.
  - Ethernet(선택, 기본 숨김): 남김 = 표시 필터·범위·패킷 목록·상세·바이트. 숨김 = 모듈 툴바, 채널 카드 바, 소켓 테스트.
  - CAN: 독립 원본 없음 → "범위 외" slot 유지, 미연결 미리보기도 두지 않음(요약 표 위장 없음).
- **식별은 세션/채널 단위**: 상단 세션 칩 `UART·mcu /dev/… 115200 ● 연결됨 RX…`, `UART·aux`, `UDS·BCM DoIP …`를 원본 frame DOM(세션 탭/`.phead`, `#selEcu`/`#segRoute`/`#linkSt`, `#chbar`)에서 1초마다 읽어 표시. 클릭 = 해당 세션 탭 클릭(원본 포커스), ×/👁 = 세션 pane 숨김/표시(`:has()` CSS 주입, 원본 세션·로그 상태 유지). 사이드바 카탈로그 없음 — 관측면(서비스) 선택은 헤더 "패널 ▾" 드롭다운.
- **에이전트 운영 strip**(한 줄): 대상(UDS·BCM) · 작업 · 단계 n/m · 상태(진행/대기/실패/완료/취소/일시정지 + 대기 조건/오류) · 근거 링크(세션 단위로 포커스) · 최근 이벤트 시각. 대상 패널 헤더에 "에이전트 대상 · 단계" 칩, 대상 UART 세션의 원본 `.phead`에는 CSS `::after` 배지로 같은 내용(DOM 변경 없음).
- 사용자 제어: 하드웨어 STOP(상단, 작업 취소와 분리) · 작업 일시정지/재개 · 작업 취소 · 패널별 ⏸(원본 자체 일시정지를 누름; UDS는 원본에 없어 비활성로 정직 표기) · 필터/복사/검사는 원본 컨트롤 그대로 · **서비스 점프 = "전체 서비스" 토글**(같은 frame에서 adapter를 `full`로 바꿔 툴바·세션·카탈로그·요청 편집기까지 펼침, reload 없음) + ↗ 새 탭.
- 레이아웃: 분할/크기 조절/최대화/이동은 사용자 소유(기존 유지). 기본 UART 위 / UDS 아래.

## 상태 보존
- iframe은 `#frameHost`에 한 번 생성, 재부모화 없음(좌표만 rAF 동기화). 탭 전환·분할·최대화·세션 숨김·핵심/전체 전환은 모두 CSS/좌표 변경이라 문서·xterm·트랜잭션 상태 유지. 검사(`검사`/`?`/패널 🔍): 핵심면 셀렉터 표시 ✓, 비핵심 셀렉터 "DOM 보존·숨김", 로드 1회·window 마커, 탭 전환 전후 지문.

## 프로토타입 vs 엔진 범위 (정직 표기)
- 모든 패널 데이터는 각 원본의 로컬 SIM. 엔진/daemon 미연결. 세션 칩의 신선도는 DOM 카운터 변화 시각(폴링)이며 텔레메트리가 아님.
- 에이전트 작업/단계/타임라인은 고정 목업. "▶ 시연 1단계(로컬 SIM)"는 이 프로토타입이 원본 SIM 패널의 자체 입력(UDS `#btnLink`/`#didInput`/`#req` submit, UART 세션 탭·connect 버튼)을 대신 누르는 고정 절차로, 실제 에이전트 실행·엔진 연결을 주장하지 않는다(타임라인에 `[시연]` 표기).
- 제품 적용 시: adapter CSS는 원본 모듈의 "core mount 옵션"으로, 세션 칩은 세션 레지스트리 구독으로, CAN은 트레이스 컴포넌트 독립 mount가 선행 과제.

## ROOT Ego QA 체크리스트
`http://127.0.0.1:64785/workbench-agent-tabs/reuse/fable/index.html` (p2)
1. 로드 후 UART 패널: 레일·툴바·세션 서랍 없음, mcu|aux 두 pane의 세션 헤더+터미널만. UDS 패널: ECU/경로/연결 한 줄 + 트랜잭션/응답 순서/해석/바이트/확정 값만(DID 목록·요청 편집기·도구 탭 없음).
2. 세션 칩 행에 `UART·mcu`, `UART·aux`, `UDS·BCM` 표시. 칩 클릭 → 해당 pane 활성. `UART·aux` × → aux pane 숨김, 👁 복원(로그 유지).
3. strip "▶ 시연 1단계" 2회 → UDS 연결·F190 트랜잭션이 원본 트레이스에 추가, 대상이 UART·mcu로 바뀌며 mcu `.phead`에 배지. 
4. 에이전트 탭 ↔ 통합 탭, 최대화, "전체 서비스"↔"핵심면만" → `검사`: 로드 1회·마커 보존·핵심면 ✓·비핵심 "DOM 보존·숨김".
5. "원본 shell 보기" → 원본 헤더/툴바 복귀(원본 무변경 확인). 스크린샷 → `browser-qa-core.png` 권장(ROOT 소유).

## 검증 범위 선언 (사용자 최신 지시)
- 이 산출물은 **UI 핵심 패널 프로토타입 구현 + 구문 검사까지**다. 기능/에이전트 검증 PASS를 주장하지 않는다. "▶ 시연 1단계"는 가짜 작업 fixture이며 실제 에이전트 PASS가 아니다(타임라인 `[시연]`/`[목업]` 접두). 공식 QA(HTML 리포트+브라우저 스크린샷)는 별도 Opus 5.5 슬롯, 그 전까지 ROOT 소스/Ego 확인만.

## 누락 엔진 바인딩 (Opus 검증용, `app.js` 기준)
| 위치 | 현재(목업) | 실제 바인딩에 필요한 것 |
|---|---|---|
| `TASKS`, `TIMELINE` 상수 | 고정 배열 4건·3건 | 에이전트 런타임의 작업/단계/이벤트 스트림 구독(작업 id·단계 index·state·target{svc,alias}·wait·err·evidence 세션 참조) |
| `demoStep()` | 원본 SIM 패널의 자체 버튼/폼을 대신 클릭 | 삭제 대상. 에이전트가 loom CLI/daemon으로 실제 조작하고 UI는 결과 이벤트만 반영 |
| `SERVICES[*].sessions(d)` | iframe DOM(`.tab[data-tab]`, `.phead`, `#selEcu`, `#linkSt`, `#chbar`)을 1초 폴링 | 세션 레지스트리(UART 세션·UDS ECU/route·ETH 채널·CAN 채널) 구독; alias/port/baud/connection/RX·TX는 엔진 상태값 |
| `SERVICES[*].freshness(d)`, `rec.sessRx` | DOM 텍스트 변화 시각 | 소스별 last-event ts, lag/drop/error 카운터 |
| `btnHwStop` | `state.hw.stopAt` 기록 + toast | daemon STOP 명령(전 장치 출력 정지)과 ack/실패 표시 |
| 작업 일시정지/취소 (`renderAgentStrip`) | `state.paused`, `t.state='cancel'` 로컬 변경 | 에이전트 런타임 pause/cancel API, 작업 취소 ≠ 하드웨어 STOP 유지 |
| 패널 ⏸ (`svc.freeze`) | 원본 패널 일시정지 버튼 클릭(UDS 없음→비활성) | 뷰 freeze는 UI 로컬로 충분; UDS 트레이스 freeze는 원본에 추가 필요 |
| adapter CSS (`core`/`full`) | iframe `<style data-fable-adapter>` 주입 | 제품 UI에서는 모듈 "core mount" 옵션으로 대체(iframe 불필요) |
| CAN slot | 원본 없음 표기 | `ui/src/panels/can` 트레이스 컴포넌트의 독립 mount + CAN·can0 세션 칩 |

리뷰 대상 파일: `index.html`(구조), `app.js`(SERVICES adapter/세션/strip/검사), `styles.css`(Bench Light). 접근 URL `http://127.0.0.1:64785/workbench-agent-tabs/reuse/fable/index.html`.

## 2026-11-14 ROOT 초기 스냅샷 결함(`core-panels-initial.png`) 조치 — PASS 아님, 사전 QA 수정
- 원인: 스냅샷의 레이아웃(에이전트 strip이 1fr 행을 차지해 ~750px 공백, 세션 칩·패널이 하단으로 밀림, strip 무간격)은 **이전 styles.css(16:50, 4행 grid)**가 브라우저 휴리스틱 캐시로 적용된 결과. HTTP 서버(SimpleHTTP)는 캐시 헤더가 없다. 디스크/서버의 styles.css는 17:10 신규(6행 grid)였음.
- 조치: `index.html`이 `styles.css?v=20261114c`, `app.js?v=20261114c`로 참조(캐시 무효화). `styles.css` 보강: `.app` 행을 `40px 32px 30px minmax(0,1fr) auto 22px`로 고정, strip/세션 행 고정 높이, strip 항목 구분선·폭 상한(단계 30vw·최근 22vw·상태 24vw), `.body/.main/.layout-scroll` 높이 100%로 패널 영역이 즉시 남은 뷰포트를 채움(뷰포트 크기 무관).
- ROOT 재확인 시 강력 새로고침(캐시 무시) 권장. 기대: 상단 40px + strip 32px + 세션 30px 아래로 UART/UDS 패널이 바로 시작하고 하단 상태바 22px까지 채움, 공백 블록 없음.

## 2026-11-14 17:31 Opus 5.5 QA (w26) finding → narrow fix → self-retest (ROOT-assigned; NOT original Fable output)
- Finding (Ego, 1908×892, v20261114c): combined tab `.main` auto-placed into `.body`'s content-sized `auto` column while `#sideAgent[hidden]` → main 787/560/427 px, right 1120–1480 px blank. Footer fell into the `auto` row (15 px) with 22 px blank below.
- Fix (styles.css append only + index.html stylesheet `?v=20261114d-opusfix`; app.js/behavior unchanged): `.body>.side{grid-column:1}` `.body>.main{grid-column:2}` `.app>.check{grid-row:5}` `.app>.status{grid-row:6}`.
- Self-retest (hard reload): combined 1908 → body cols `0 1908`, main 1908; after cancel/maximize unchanged; agent tab `290 1618`; 1100 px combined 1100 / agent 290+810; footer y870 h22. iframe nonce + UDS draft preserved through cancel/maximize/restore/session hide-show/tab switches/viewport change. Evidence: validation-report/raw/fable/r-fix-recheck.json, screens/fable/r1–r3. Pre-fix copies: validation-report/raw/fable/pre-fix/.
- Scope unchanged: local SIM prototype; engine/agent binding NOT IMPLEMENTED.
