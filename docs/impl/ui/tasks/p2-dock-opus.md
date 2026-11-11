docs/plan/02-core-principle.md 준수.

# UI M2 선행 — ui-opus: 도킹 레이아웃 · 레이아웃 저장 · 패널 탭

작성: ui-lead, 2026-11-11. 규칙은 `p0-common.md` 그대로(커밋 안 함, **질문 창 금지**, 경로 `/Users/ctmctm/workspace/Loom/...`). 기준 ui/main(최신 커밋, P1 M1 MERGE READY 19c6276 이후 dockview-core 추가).

## 근거
`docs/plan/ui/plan.md` §5.7(레이아웃 JSON `loom_layout:1`, 앱 버전 모델로 vendor 직렬화 감싸기), §7(도킹 = dockview, 무료 MIT 범위만), §5.1.1(상단 바·STOP 위치 고정), §5.7 새 채널 규칙(UD-31 — M1의 "추가 패널 ≤ 4"를 도킹 탭으로 바꾸는 지점), §3.3(패널 수명주기·숨은 패널 렌더 중단), R1b·R2의 Dockview 판단(`docs/plan/ui/r2-opus-review.md`).
의존성: **`dockview-core` 8.8.3**(프레임워크 독립 — 패널은 이미 명령형 `mount/dispose`). React 래퍼는 쓰지 않는다.

## 만들 것
1. **도킹 레이아웃**: 고정 프리셋(`m1-default`, `uart-only`, `can-uds`, `eth`, `test`)을 도킹 초기 배치로. 분할·탭·최대화·끌어서 옮기기. STOP 버튼·상단 바·채널 바·Activity 띠는 도킹 밖 고정(어떤 배치에서도 STOP이 같은 자리).
2. **레이아웃 저장**: 앱 모델 `{loom_layout:1, name, env_hint, root, pinned, vendor:{dockview:<toJSON>}}` — vendor 직렬화는 하위 필드로만(라이브러리 버전이 바뀌면 `root`로 재구성). 저장소: 지금은 localStorage(try/catch, 실패해도 동작) + CORE `core.ui.layout.*` RPC가 생기면 그쪽(요청 Q 이미 있음). 이름 충돌 시 새 이름. 1 MiB 상한, 모르는 패널 id는 "이 버전에 없는 패널" 자리표시, props는 manifest JSON Schema로 검증.
3. **새 채널 → 도킹 탭**: 새 채널 규칙의 "추가 패널"을 도킹 탭으로(auto_open_panels on일 때).
4. **수명주기**: 탭이 가려지면 패널 렌더 중단(구독은 유지, 데이터는 스토어로), 다시 보이면 최신으로. 패널 닫기 → dispose·구독 refcount 감소(UI-T10 기준 유지). xterm·uPlot resize는 rAF당 1회.
5. 딥링크 `#/l/<name>`·`#/p/<domain>/<panel>?…`와 가리키기(`ui.focus_request`)가 도킹에서도 동작(이미 열린 패널이면 그 탭 활성화 + reveal).
6. 시험(vitest + jsdom 가능한 범위): 레이아웃 모델 직렬화·역직렬화·버전 이주, 모르는 패널 자리표시, 1 MiB 상한, 탭 숨김 시 렌더 중단 신호, 새 채널 탭 열기. 그리고 **`pnpm e2e:mock --workers=1` 전체가 계속 통과**해야 한다(수용 스위트가 레이아웃 DOM에 기대는 곳이 깨지면 고치되 시험 파일은 수정 금지 — 필요한 변경은 보고서에).

## 파일 범위
`ui/src/**`(net·proto 제외). **수정 금지**: `ui/src/net/**`, `ui/src/proto/**`, `ui/tools/**`, `ui/e2e/**`, `ui/fixtures/**`, 설정, Rust crate(ui-sol이 crates/loom-viz 작업 중).

## 보고
`docs/impl/ui/reports/p2-dock-opus.md`. 끝나면 "완료: <경로>"만.
