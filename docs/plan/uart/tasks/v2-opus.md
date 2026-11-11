docs/plan/02-core-principle.md 준수 (최우선)

# v2.0 — uart-opus: 구조 · UI 패널 명세 · 통합 테스트 · 스킬 (40분 내외)
먼저 `docs/plan/uart/tasks/v2-common.md`를 읽는다.

출력: `docs/plan/uart/v2-opus.md` (plan.md에 넣을 교체 문안, 절 번호 붙여서). 끝에 "V2 OPUS DONE" 한 줄.

## 쓸 것
1. **§0 핵심 루프 표 v2**: 4요소(직접 접근·읽기/쓰기·사용자 실시간 표시·사용자 개입)를 Rust `loomd` uart worker + **Loom UI 터미널 패널** 기준으로. TUI(`loom uart attach`)는 보조 행.
2. **§2.0 E1 종단 시나리오 v2**: 에이전트 `loom uart send` → 장치 응답 → **브라우저 UI 터미널 패널**에 표시 → 사용자가 패널에서 Ctrl-C/타이핑으로 개입 → 에이전트 다음 결과의 `others_since_last`. 구간별 지연 예산 표를 다시(worker → gateway 허브 → WS → 브라우저 → xterm.js 렌더 완료). 측정 방법(브라우저 시계와 호스트 monotonic을 어떻게 맞출지)도 제안.
3. **§5 사용자 UI → "UART 패널 명세"**로 재작성 (TUI는 끝에 보조 절로 축소):
   - 터미널 패널(xterm.js 등): 원시 키 입력 경로(브라우저 → WS → gateway → worker InputArbiter), **Ctrl-C 우선 경로**, 입력권(lease) 의미를 여러 브라우저 탭·여러 사용자 클라이언트에서 어떻게, 읽기 전용 모드, 출처 강조(agent 송신 표시), 상태줄·배지(LIVE/LAG/STALLED/LIVE-DEGRADED, `env=vehicle`, REC, gap 3층), 백로그 → 라이브 이음.
   - **보안 주의**: xterm.js 같은 터미널 에뮬레이터는 장치가 보낸 질의(DA·DSR·창 크기 질의 등)에 **자동 응답을 `onData`로 내보낸다** → 사용자 키로 오인되어 장치에 주입될 수 있다. 자동 응답 차단·구분 방법, OSC 52 클립보드·하이퍼링크 등 위험 escape 처리(서버측 sanitizer 유지 여부)를 결정안으로.
   - 브라우저가 가로채는 키(Ctrl-W, Ctrl-T, Ctrl-N 등) 처리, 접두키(`Ctrl-]`)를 웹에서 유지할지 버튼/단축키로 바꿀지.
   - 트레이스 보기(타임스탬프·출처 행·hex), 검색·마크, 시그널 버튼(break·DTR/RTS·reset — core 정책 게이트 통과, R2 확인은 공통 승인 대기열).
   - STOP·승인은 공통 셸 소유(UART는 무엇을 요청하는지만).
   - 각 항목: 무엇을 / 어떤 지연으로 / 사용자가 어떤 입력으로 개입.
   - 이 절을 그대로 UI 팀에 줄 수 있게 **`_xteam/uart-ui.md` 초안**도 같은 파일 끝에 별도 절로(패널 목록, 데이터 계약: 구독 이벤트 kind·질의 RPC·쓰기 RPC, 성능 요구, UI 팀에 묻는 질문).
4. **C7·C8 통합 테스트 지원** (새 절 제안): UART가 제공할 대기·검증 명령(예: `expect`, `wait-for`, 부정 검증 "N초 동안 패턴이 나오지 않음", 카운트), **증거로 인용 가능한 이벤트**(rx 청크 seq 범위, expect 매치 이벤트의 seq 범위·매치 텍스트, send 이벤트 request_id)와 Loom이 그 증거를 검증하는 방법. 섞어 쓰는 자연어 TC 예 1개(UART + UDS + CAN, 예: "UDS 0x11 리셋 → 5 s 안에 UART 부팅 로그 'App start' → CAN 0x123 heartbeat 재개")와 에이전트가 실행하는 명령열·증거 인용 예.
5. **§4.8 스킬 v2**: `loom-uart` 본문 짧게, 상세는 `loom uart help --json`. `loom-test`와의 관계(UART 증거 인용 규칙).
6. **M1 v2 정의**: Rust + 웹 UI 기준 걷는 뼈대(무엇이 M1에 들어가고 TUI는 언제), 수용 기준 (a)–(i) 개정안.
7. **§3.12 숨은 연결 금지**를 UI 기준으로 다시(UI가 안 열려 있을 때 `no_viewer_attached` 경고 의미 등).

v1.2의 입력 중재 규칙(§3.7), 승인·STOP 사용자 예외(A1·A3·A12)는 유지한다. 바꾸는 곳만 이유를 쓴다.
