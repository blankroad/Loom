docs/plan/02-core-principle.md 준수.

# R1b — ui-opus 지시 (ui-sol R1이 끝날 때까지의 보강 작업)

출력: `docs/plan/ui/r1b-opus-xteam.md` (한국어, 200~400줄). `r1-opus.md`는 고치지 말 것 — 리드가 R3에서 통합한다.

당신이 R1을 쓸 때 없던 문서가 생겼다. 읽고 당신 초안과의 **정합 점검표**를 만든다.

## 읽을 것
- 도메인 패널 명세: `docs/plan/_xteam/can-ui.md`, `_xteam/uart-ui.md`, `_xteam/uds-ui.md`
- CORE 초안: `docs/plan/core/r1-opus.md` (UI 관련 절 중심: IPC·이벤트 허브·전송 형식·인증·승인·STOP·스키마), `_xteam/can-core.md`, `_xteam/uart-core.md`, `_xteam/uds-core.md`(UI에 영향 있는 부분만)
- TEST 초안: `docs/plan/test/r1-opus.md` (테스트 실행 화면·이벤트·증거 검증)
- 리드가 쓴 `_xteam/core-ui.md`(UI 요구 v0.1)와 `_xteam/ui-panel-template.md`

## 쓸 것
1. **도메인 패널 명세 ↔ r1-opus 차이표**: 패널별(UART 터미널, CAN Trace/Fixed/IG/Data/Graphics, UDS 콘솔 등) — M 배정, 이벤트 kind/필드 이름, 개입 동작·자격·확인 방식, 지연 목표, 출처 색·기호, 메모리 상한이 어긋나는 곳. 각 행에 "누가 맞나 / 제안" 열.
2. **CORE 초안 ↔ UI 요구 차이표**: `core-ui.md` U1–U8과 r1-opus §15.1 C-1~C-15 각각에 대해 CORE 초안이 이미 수용/다르게 설계/언급 없음. CORE가 다르게 설계했으면 UI가 맞출지 CORE에 요청할지 제안.
3. **TEST 초안 ↔ 테스트 실행 화면**: 이벤트 이름·판정 상태값·증거 검증 형식·사람 개입·사전 승인 묶음 — 맞출 점.
4. **자기 승인 차단 근거 보강 (데스크 조사, 출처 URL 필수)**: (a) `http://localhost`가 WebAuthn 보안 컨텍스트·RP ID로 허용되는지, (b) Playwright/CDP 가상 인증기가 실제 플랫폼 인증기(Touch ID/Windows Hello) assertion을 만들 수 있는지, (c) 브라우저 확장 기반 자동화와 computer-use(화면 클릭) 각각이 OS 생체 프롬프트를 통과할 수 있는지, (d) Linux에서 플랫폼 인증기 현실. 확인 못 한 것은 [미확인].
5. **R3 통합 때 리드가 결정해야 할 쟁점 목록** (5~15개, 각 쟁점에 당신 추천과 이유 한 줄).

규칙은 `tasks/r1-common.md` §4 그대로. 끝나면 "완료: <경로>"만 답한다.
