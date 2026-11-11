docs/plan/02-core-principle.md 준수.

# UI P1 M1 — 수용 시험 스위트 + mock 시나리오 (ui-sol 대행 서브에이전트)

작성: ui-lead, 2026-11-11. 기준 ui/main `9e93ef2`(main 761870a 병합 — 이제 같은 worktree에 CORE crate가 있다. **wire 타입은 `schema/v1` 생성 타입**(`ui/src/proto/gen`, 수정 금지), `pnpm check`가 `check:schema`부터 돈다). 규칙은 `p0-common.md` 그대로(커밋 안 함, git stash 금지, 장시간 10분 이내, Playwright 브라우저 `~/.cache/ms-playwright`).

**운영 규칙(플레이북 §6)**: 대화형 질문 창을 띄우지 말 것 — 계약·계획에 맞게 스스로 정해 보고서에 적거나, 판단할 수 없으면 보고서에 질문을 쓰고 끝낸다. 경로는 `/Users/ctmctm/workspace/Loom/...`만.

## 근거
`docs/plan/ui/plan.md` §11 **UI-T01–T10**, §0.1 E-UI-1/2/3, §9.2–9.3. 패널 명세 수용 시험: `docs/plan/_xteam/{uart,uds,can,eth}-ui.md`, `ui-test.md`, `explore-ui.md`(psu). 이전 대행 보고: `docs/impl/ui/reports/p0-sol.md`, `p1-can.md`. ui-opus 보고: `p1-opus.md`(UART·UDS), `p1-eth.md` §5(eth mock 요구), `p1b-opus.md` §5(test·psu mock 요구).

## 할 것
1. **mock 시나리오 추가**(`ui/tools/mock/**`): eth(p1-eth §5 1–7), test·psu(p1b-opus §5 1–4). 모양은 각 패널 fixture·모델 머리 주석의 가정을 따른다.
2. **수용 시험 스위트**(`ui/e2e/**`, Playwright, **DOM으로 판정**: `data-request-id`·`data-origin`·`data-env` 등 — WS 수신만으로 합격 처리 금지):
   - UI-T01 늦은 합류(1,000 이벤트 후 열기 → 1 s 안 snapshot, 이후 seq 중복·누락 0; UART 패널 500 ms)
   - UI-T02 에이전트 `uart send`/`can send`/`uds read-did` → 같은 request_id 행·AGT
   - UI-T03 지연은 **논리 판정만**(각 경로가 렌더 보고·표본을 내는지, 값이 시계 불확실성 안에서 계산되는지). 수치 합격은 `LOOM_PERF=1` 고정 러너 전용
   - UI-T04 사용자 개입 3종(UART 키 → 칩 `나` → 에이전트 대기 → Ctrl-C → `others_since_last`·`⇢ 전달됨` / CAN 1회 송신 USR / UDS 명령줄 + [제어 가져오기] → 에이전트 exit 6 상당)
   - UI-T05 정직성(서버 gap 3층 각각의 문구·색, heartbeat 중단 3 s → STALLED + 쓰기 잠금, 재연결 이음, UART flood 60 s view skip 0·heartbeat 연속)
   - UI-T06 STOP(1 프레임 띠 → latch 확인, 에이전트 쓰기 거부 행, 사용자 키 통과, 복구 송신, WS 끊김 중 HTTP STOP, **watch 탭도 STOP 가능**)
   - UI-T07 보안(DA·DSR·DECRQSS·OSC 4/10/11/12 `?` 질의 fixture → 의도 없는 TX 0, readonly 0, 패널 lint 위반 fixture가 eslint 실패)
   - UI-T08 자격(watch 탭: 송신·입력 비활성 + 서버도 거부, STOP 가능; mock `?rights=watch`)
   - UI-T09 쓰기 단계(UDS 0x78 ×5 한 행, 역순 주입 후퇴 없음, `outcome_unknown` 주황)
   - UI-T10 수명주기(패널 100회 열기/닫기 후 listener·구독 원상; 10분 이내)
   - eth 수용 1–7(`eth-ui.md`), shell.test 수용 5개(`ui-test.md`), psu 비상 OFF
3. **두 백엔드**: Playwright 프로젝트 2개 — `mock`(기존) / `replay` = CORE `loomd --fixture-replay builtin --scenario all --echo-writes --port 4790 --allow-origin http://localhost:7420`(`crates/loom-gateway/README.md`, `cargo run -p loom-gateway --bin loomd`). replay에서 돌 수 있는 시험(연결·늦은 합류·gap·STOP·쓰기 echo·snapshot·saturate 재연결)만 replay 프로젝트에 태그로. Vite 프록시 대상은 `LOOM_DAEMON` 환경 변수(이미 vite.config에 있음). 첫 cargo 빌드가 오래 걸리면 미리 `cargo build -p loom-gateway`로 한 번 빌드(10분 이내).
   - replay에서 계약 차이가 나오면(필드·이름) **net/mock을 계약 쪽으로 고치고**, CORE 쪽 문제로 보이면 보고서에 재현 절차와 함께.
4. 스크립트 이름(리드가 package.json에 넣는다 — 필요한 명령을 보고서에): 예 `e2e:mock`, `e2e:replay`.

## 파일 범위
`ui/tools/**`, `ui/fixtures/**`, `ui/e2e/**`, `ui/src/net/**`(api.ts 제외). **수정 금지**: `ui/src/{store,sdk,shell,panels}/**`(ui-opus 작업 중 — 패널 DOM에 시험용 속성이 더 필요하면 보고서에 요청), `ui/src/proto/**`, 리드 소유 파일, Rust crate 전부.

## 보고
`docs/impl/ui/reports/p1-m1-accept.md` — 시험 목록·결과(mock/replay 각각), 실패·건너뜀과 이유, CORE에 낼 계약 차이, 리드가 넣을 스크립트·설정.
