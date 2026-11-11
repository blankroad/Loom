docs/plan/02-core-principle.md 준수.

# UI P1 M1 — ui-opus: 접속 흐름 · 새 채널 자동 표시 · 가리키기

작성: ui-lead, 2026-11-11. 규칙은 `p0-common.md` 그대로(커밋 안 함). 경로는 항상 `/Users/ctmctm/workspace/Loom/...`(플레이북 §6). 서브에이전트가 `ui/src/net/**`, `ui/tools/**`, `ui/fixtures/**`, `ui/e2e/**`를 작업 중 — 수정 금지(필요한 net 변경은 보고서에 요청).

**운영 규칙(플레이북 §6)**: 대화형 질문 창을 띄우지 말 것 — 계약·계획에 맞게 스스로 정해 보고서에 적거나, 판단할 수 없으면 보고서에 질문을 쓰고 끝낸다. 경로는 `/Users/ctmctm/workspace/Loom/...`만.

## 근거
`docs/plan/ui/plan.md` §3.7(인증·자격), §4.1(`loom ui open`·딥링크), §5.7(새 채널 규칙 UD-31, 가리키기), §0.4(V5: 자동화 브라우저 = watch), 계약 `docs/contract/01-processes-ipc.md` §1.3(bootstrap fragment → POST, 60 s 1회용), `docs/contract/02-events.md` §2.5(`ui.focus_request`).

## 만들 것
1. **bootstrap 교환**: 페이지가 `#boot=<code>`(fragment)를 받으면 즉시 `history.replaceState`로 fragment를 지우고(주소창·기록에 비밀이 남지 않게) `POST /api/v1/bootstrap{code}`(경로는 CORE에 확인 — 계약 문구 "페이지가 POST로 쿠키와 교환")로 교환 → 성공하면 원래 딥링크(있으면)로 이동 + 연결 시작, 실패(만료·재사용)면 `#/need-auth`에 사유("코드가 만료됐거나 이미 쓰였습니다 — 터미널에서 `loom ui`를 다시 실행"). 비밀은 로그·토스트·DOM에 남기지 않는다. 시험에서 fetch 대역으로 확인.
2. **새 채널 규칙(UD-31, 숨은 채널 방지)**: 에이전트가 채널·대상을 새로 열면(`*.channel_state{opened}`·`uds.state`·`eth.capture_state` 등 도메인 상태 이벤트, 또는 snapshot items에 새 channel) 모든 탭의 **채널 바에 항상 추가** + 토스트 `에이전트가 uart dut2를 열었습니다 [패널 열기]` + 탭 설정 `auto_open_panels: on(기본)|off` — on이면 현재 레이아웃의 빈 자리 또는 탭 패널로 자동 열기, off여도 채널 바에는 남는다. 닫힘·세대 변경도 채널 바에 반영.
3. **가리키기**: `ui.focus_request{panel, props, at, reason, actor}` → 토스트 `에이전트 claude-1이 보여 주려 함 · CAN ch0 Trace · seq 5012로 이동 · 이유 …` [열기 ⏎] [이 탭은 자동으로 따라가기 ☐] [무시 Esc]. 탭 설정 `follow_agent: ask(기본)|auto|off`, auto라도 터미널 입력 중이면 보류. [열기]는 딥링크 라우팅 + `ctx.ui.reveal`(seq). 2 s 안 중복 요청은 합친다.
4. **자동화 브라우저 표시(V5)**: `navigator.webdriver`가 true면 상단에 "자동화 브라우저 — 보기 전용" 띠, 쓰기 버튼 비활성(서버도 watch만 주지만 UI에서도 분명히). STOP은 계약 v1.1대로 활성.
5. 시험: bootstrap 성공·실패·fragment 제거, 새 채널 토스트·채널 바·auto_open on/off, 가리키기 ask/auto/off·입력 중 보류·중복 합치기, webdriver 띠.

## 파일 범위
`ui/src/shell/**`, `ui/src/store/**`, `ui/src/sdk/**`, `ui/src/panels/**`(필요한 만큼), `ui/src/main.tsx`.

## 보고
`docs/impl/ui/reports/p1-m1b-opus.md` — 파일, 시험 수, CORE·도메인에 물을 것(bootstrap 경로, 채널 열림 이벤트 이름 등), net 요청. 끝나면 "완료: <경로>"만.
