# STATUS — Astra core panels / ready for root → Opus QA

- 완료 파일: `index.html`, `app.js`, `styles.css`, 신규 `core-panels.css`, `PROPOSAL.md`, `STATUS.md`.
- 소유 범위 `reuse/astra/**`만 작성. canonical UART/ETH/UDS 변경 없음. 이전 `browser-qa.png`, `browser-qa.json` 보존.
- Fable 구현 읽기, browser, new apps, Cargo, Git index 작업, subagents, 모델 substitution 없음. 요청 모델 gpt-6-astra/native foreground; 런타임 모델 식별은 별도로 검증하지 않음.
- 최신 의도: whole services → core observation surfaces. 서비스 조작은 같은 인스턴스의 명시적 서비스 열기로 접근.

URL: http://127.0.0.1:64785/workbench-agent-tabs/reuse/astra/index.html

## 이번 수정 검사

- `node --check work/reports/workbench-agent-tabs/reuse/astra/app.js`: PASS.
- 기존 port 64785의 신규 `core-panels.css` HTTP 응답: 200. 서버 추가 실행 없음.
- 코드상 iframe src 설정은 초기 생성에만 존재. 전환은 표시/CSS 속성 변경이며 reparent 없음.
- 브라우저/DOM 실행 검증은 이번 턴에 하지 않음. 이전 STATUS의 DOM 검사와 기존 스크린샷은 이번 수정의 검증 결과가 아님.

## root가 Opus5.5 opus-validation w26에 전달할 QA

1. 초기 UART+UDS가 viewport를 채우고 숨긴 sidebar/toolbar 자리의 빈 grid가 없는지. 가로/세로 배치, 3개 패널, 최대화, 경계 드래그/방향키 확인.
2. UART MCU/AUX 원본 헤더·baud·RX/TX·어두운 xterm, TERM/HEX/시각/검색/일시정지 확인. 좁은 pane과 작은 높이에서 원본 반응형/fit 동작 확인.
3. UDS 서비스 열기 → BCM 선택/연결/읽기 → 돌아가기. 요청/응답 행 선택, 응답 순서, decoded/hex와 복사 유지. 통합에는 catalog/연결/editor/tool drawer가 없어야 함.
4. Ethernet 서비스 열기에서 캡처 설정 후 돌아가기. 표시 필터, 채널 범위, 패킷 선택/details/hex, 상위 표시 일시정지 확인.
5. 선택·스크롤백·필터/요청 초안을 만든 뒤 탭/배치/숨김/최대화/서비스 열기를 반복. `window.astraReuse.inspect()`의 loads=1 및 동일 Document, 원본 상태 유지 확인.
6. DEMO strip의 현재 target/step/수신 시각과 확장 journal 확인. 취소 후 늦은 SIM 이벤트가 작업 완료를 되살리지 않는지. 비활성 STOP은 task cancel과 구분.

## 범위와 제한

소스 바인딩은 원본 rich renderer와 UDS host 구독. 실제 daemon/device/agent/STOP 엔진 바인딩은 없음. UART/ETH 상위 RX 지표를 만들어내지 않으며 원본 표시를 사용한다. 새 fixture 송신 없음; UDS 근거는 서비스 열기에서 직접 실행 후 생성된다. CAN native standalone 없음을 UI에 명시. 최종 Ego 시각/행동 검증은 미실행이며 Opus 소유.

## 2026-11-14 17:5x Opus 5.5 QA (w26) findings → narrow fix → self-retest (ROOT-assigned; NOT original Astra output)
- Findings (Ego): (1) `#waitFor` had a malformed `</option value="link">` → only `txn` option; (2) separator drag collapsed a non-adjacent panel to ~9 px. Root cause: `resize()` wrote the first panel's `flex` before measuring the others (read/write interleave → relayout → ETH measured ~5 px). (3) assets had no version query.
- Fix: index.html option repaired + `styles.css?v=20261114d-opusfix` / `app.js?v=20261114d-opusfix`; app.js `resize()` reads all sizes first, then writes (adjacent clamp ≥100 px kept); styles.css appended min size for visible non-maximized panels (stack 96 px / columns 220 px) so viewport shrink or the task journal cannot squeeze a panel unreadable.
- Self-retest (hard reload, 1908 + 1100 px, stack/columns/task tab): non-adjacent panel unchanged on drag (ETH 248 px while UART/UDS dragged), adjacent clamp 101 px, min 96/220 respected, no workspace overflow, loads 1/1/1 and iframe nonce intact throughout, waitFor `txn/link`, observe(link)→cancel works. Evidence: validation-report/raw/astra/r-fix-recheck*.json, screens/astra/r1–r5; pre-fix copies validation-report/raw/astra/pre-fix/.
- Scope unchanged: local SIM prototype; STOP/agent engine binding NOT IMPLEMENTED. core-panels.css link (built in app.js load handler) still unversioned — not touched (outside granted function).
