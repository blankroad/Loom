docs/plan/02-core-principle.md 준수.

# UI M2 선행 보고 — ui-opus: 도킹 레이아웃 · 레이아웃 저장 · 패널 탭

작성: ui-opus, 2026-11-11. 기준 ui/main `b514d83`(dockview-core 8.8.3 추가), 작업 공간 `/Users/ctmctm/workspace/Loom/.worktrees/ui/ui` (커밋 안 함, 인덱스도 건드리지 않음).
근거: `docs/plan/ui/plan.md` §3.3·§5.1.1·§5.7·§7, `docs/plan/ui/r2-opus-review.md`.

## 0. 결과
- **`pnpm check` 통과**: check:schema · typecheck · lint · **33개 파일 358개 시험**(이번 추가 16개 = `src/shell/dock/dock.test.ts`; 수치에는 다른 에이전트의 새 `src/codec/**` 시험도 포함). `pnpm build` 통과.
- **`pnpm e2e:mock --workers=1` 전체 통과**(2.8분): **44 통과 · 1 건너뜀**, T07 CSP는 기존 `test.fail`(CORE P1) 그대로. T10: 리스너 273→276, DOM 노드 1,137→2,156(상한 +2,000 안), 서버 구독 3→3. 시험 파일 수정 없음.
- 첫 e2e 실행에서 T01·T10이 깨졌고 원인은 **dockview CSS 누락**(§3-1) — 고친 뒤 전부 통과.

## 1. 만든·바꾼 파일

| 파일 | 내용 |
|---|---|
| `src/shell/dock/layoutModel.ts` (신규) | **앱 레이아웃 모델** `{loom_layout:1, name, env_hint, root, pinned, vendor:{dockview:{version, state}}}`. 노드 = `split`(h 좌우 / v 상하, `sizes` 상대 가중치) · `tabs`(도킹 그룹 하나, `active`) · 패널 잎 `{panel, props}`. 프리셋 → 초기 배치(`presetToLayout`: 모든 패널이 **분할**로 보이게 — 숨은 탭 아님), 딥링크 → 1패널(`slotLayout`). `parseLayout`: 1 MiB 상한(바이트), JSON·형식 검사, **이주**(M1 `{slots[]}` → v1, 더 새 `loom_layout` 거부, 프리셋 참조 거부), 깊이 ≤ 8·패널 ≤ 32, 패널 id 형식, **모르는 패널은 자리표시로 보존**(다시 저장해도 지워지지 않음), props는 manifest `propsSchema`로 검증(틀린 값·모르는 prop 버림 + 경고, `at` 커서 허용). `serializeLayout` 1 MiB 상한, `uniqueName`(충돌 시 `-2`, `-3`…). dockview JSON ↔ `root` 변환(`toDockview`/`fromDockview`: dockview는 깊이마다 방향이 번갈아 가므로 같은 방향이 아닌 분할은 한 단계 감쌈, 크기는 상대 가중치로, 균등 분할은 `sizes` 생략). `vendorMatches`: **같은 dockview 버전 + 같은 패널 구성일 때만** vendor 상태 사용, 아니면 `root`로 재구성 |
| `src/shell/dock/layoutStore.ts` (신규) | 저장 레이아웃: localStorage(`loom.layouts.index`, `loom.layout.<name>`), 모든 접근 try/catch(막히면 이유 보고, 화면은 동작), ≤ 32개, 프리셋 이름 예약, **덮어쓰기 없음**(새 이름). `LayoutBackend` 인터페이스 — CORE `core.ui.layout.*`가 생기면 같은 인터페이스로 교체 |
| `src/shell/dock/dockHost.ts` (신규) | **도킹 호스트**(`createDockview`, 테마 abyss, 플로팅 끔). 탭 내용 렌더러 = 패널 수명주기: 열림 → `createPanelContext` + factory + mount, **가려짐 → `setVisible(false)`**(그리기 중단, 구독 유지·데이터는 미러로), 다시 보임 → 최신으로 1회 그림, **resize는 rAF당 1회로 합침**, 닫힘 → instance·context dispose(구독 refcount 감소). `shell.test`는 React 화면을 탭 안에 렌더. 모르는 패널 → `이 버전에 없는 패널: <id>` 자리표시(`data-testid=panel-unknown`), 필수 props 누락 → `props 검증 실패` 자리표시. 그룹 머리에 [최대화]/[복원]. `load`(vendor 또는 재구성, 실패 시 재구성으로 재시도), `snapshot`, `find`/`open`(이미 있으면 그 탭 활성화, 없으면 **같은 도메인 패널 옆 탭**으로, 없으면 활성 그룹) |
| `src/shell/dock/DockLayout.tsx` (신규) | 도킹 React 래퍼(인스턴스 1개, 탭 수명 동안 유지) + 하위 바 **레이아웃 바**(프리셋/저장됨 선택, 이름 입력 + [저장], 저장 레이아웃 [삭제]). 경로: `#/l/<name>` = 저장 레이아웃 → 같은 이름 프리셋 → 기본 프리셋, `#/p/<d>/<p>?…` = 그 패널 하나로 **도킹을 교체**(T10: 딥링크 100회에 패널이 쌓이지 않음). 레이아웃 경고(모르는 패널, 버린 props, vendor 미사용)는 토스트 1개 |
| `src/shell/dock/dockview.css` (신규, vendored) | dockview-core 8.8.3 스타일시트 원문 + MIT 고지 + 출처 주석(§3-1) |
| `src/shell/ShellApp.tsx` | `LayoutView`/`PresetPicker` → `DockLayout`/`LayoutBar`. 상단 바·STOP·채널 바·배너·STOP 패널·Activity 띠는 **도킹 밖 고정**(어떤 배치에서도 STOP이 같은 자리) |
| `src/shell/Layout.tsx` (삭제) | M1 고정 그리드 화면 — 도킹이 대체. 경로 함수는 이미 `routes.ts` |
| `src/shell/routes.ts` | layout 경로에 요청한 `name` 보존(저장 레이아웃 이름이 프리셋이 아니어도) |
| `src/shell/services.ts` | `dock`(현재 도킹, 없으면 null) · `attachDock()` · `layouts`(저장소). **새 채널 → 도킹 탭**: `showSlot`이 도킹이 있으면 `dock.open`(auto_open on이면 자동, off면 토스트 [패널 열기]). **가리키기**: 도킹이 있으면 경로를 바꾸지 않고 탭 활성화/추가 + reveal. 도킹이 없을 때(시험·need-auth)는 M1 방식 유지(`extraSlots`/딥링크). 쓰지 않게 된 `uiState.maximized` 제거 |
| `src/store/renderScheduler.ts` | 패널별 `setVisible`/`visible`: 숨은 패널은 그리지 않고 dirty만 기억, 보이면 1회 그림 |
| `src/sdk/context.ts` | `PanelContextHandle.setVisible`; 숨은 패널은 LIVE 배지 계산에서 빠짐(`visible:false`) |
| `src/sdk/propsSchema.ts` (신규), `manifest.ts`, `index.ts` | **props JSON Schema 부분집합**(`string/number/integer/boolean`, `enum`, `pattern`, `maxLength`, `minimum/maximum`, `required`) + 검증기(URL 문자열 숫자·불리언 허용, 키 ≤ 32, 문자열 ≤ 256). `CHANNEL_PROP`(loom-proto 채널 문자 집합) |
| 패널 manifest | `propsSchema` 추가: uart.terminal·uart.trace·psu.gauge·eth.packets `{channel}`, uds.console `{targets: 쉼표 목록 ≤ 16}`, can.trace `{channel, ch, rows 1..2,000,000}` |
| `src/shell/shell.css` | `.dock-host`(flex 1, 높이 확정), 탭 내용 채우기, 레이아웃 바 |

## 2. 시험 (`src/shell/dock/dock.test.ts`, jsdom — dockview가 jsdom에서 실제로 동작, `ResizeObserver`만 대역)

| 요구 | 시험 |
|---|---|
| 직렬화·역직렬화 | 프리셋 5개 모두 dockview JSON 왕복 시 `root` 동일, 탭·중첩 분할·세로 루트 왕복, `DockHost.load` 후 `snapshot().root` = 프리셋 |
| 버전 이주 | M1 `{slots[]}` → v1(경고), `loom_layout:2` 거부, 프리셋 참조·잘못된 JSON·빈 분할 거부; vendor 상태는 같은 버전·같은 패널일 때만 |
| 모르는 패널 자리표시 | 파싱 시 보존 + 경고, 도킹에서 `이 버전에 없는 패널: viz.plot` 렌더, 스냅샷에 남음 |
| 1 MiB 상한 | 읽기(>1 MiB 거부)·저장(vendor 상태로 1 MiB 초과 → 저장 거부) |
| props 검증 | `rows:"5000"`→5000, 모르는 prop·틀린 채널 버림, `at` 유지 |
| 저장소 | 저장→목록→읽기, 프리셋·기존 이름 → `-2`, 삭제, 저장소 막힘 → 오류 보고(예외 없음), 32개 상한 |
| 탭 숨김 시 렌더 중단 | 같은 그룹 두 탭: 뒤 탭은 데이터가 와도 0회 그림, 활성화하면 1회 |
| resize | 연속 5회 layout → 다음 프레임에 resize ≤ 1회 |
| 수명주기 | 딥링크로 교체 시 이전 패널 3개 dispose 후 새 패널 mount, 탭 닫기 → 그 패널만 dispose |
| 새 채널 탭 | `uart.channel_state{opened}` dut2 → uart dut 옆 탭으로 추가 + 토스트, 다시 `showSlot` → `shown`(중복 없음) |
| 가리키기 | 열린 CAN 탭을 활성화하고 `can:ch0@5012` reveal(경로 변화 없음) |

## 3. 정한 것 (질문 창 없이)
1. **dockview CSS를 vendored로 가져옴**: dockview-core 8.8.3의 ESM 입구(`dist/package/main.esm.mjs`)는 CSS를 넣지 않고 `dist/styles`는 배포 파일에 없다(UMD 번들만 스타일을 주입). CSS가 없으면 탭 내용이 내용 높이만큼 늘어나 **VirtualTable이 스크롤 없이 모든 행을 만든다**(첫 e2e: T01 UART 트레이스가 꼬리 대신 맨 앞 줄, T10 DOM 노드 32k→92k). UMD 번들의 스타일 문자열을 원문 그대로 `src/shell/dock/dockview.css`로 추출(MIT 고지·출처·재추출 시점 주석). 판단 근거: UMD를 부작용 import하면 라이브러리 1 MB가 두 번 들어감. dockview 버전을 올리면 이 파일도 다시 추출해야 한다(`DOCKVIEW_VERSION` 옆에 적음). CSS gzip 약 16 KB.
2. **딥링크 `#/p/…`는 도킹을 교체**(기존 동작 유지): 탭으로 쌓으면 T10(구독 기준선 복귀)이 깨지고, 딥링크는 "이 패널을 보여 줘"라는 뜻. 반면 **가리키기·새 채널은 현재 도킹에 탭으로 추가**(사용자 배치를 바꾸지 않음). 이미 열린 패널이면 그 탭 활성화.
3. **프리셋은 탭이 아니라 분할**로 배치 — 모든 패널이 처음부터 보이고(수용 시험의 DOM 기대와 같음), 탭은 사용자가 끌어서 만든다.
4. **저장은 명시적**(레이아웃 바 [저장])이고 덮어쓰지 않는다(plan §4.1 "put은 기존 이름 덮어쓰기 금지"). 자동 저장은 하지 않음 — 레이아웃을 잘못 끌어 놓아도 저장된 것은 그대로.
5. props는 URL에서 문자열로 오므로 숫자·불리언은 문자열 표기도 허용, 저장 시 실제 타입으로 정규화. 틀린 값은 버리고(패널 기본값) 경고, 필수 prop 누락만 자리표시.
6. 플로팅 그룹·팝아웃 창은 끔(M2 범위 밖, 팝아웃은 별도 창의 WS·STOP 위치 문제).
7. 숨은 탭은 dockview 기본 `onlyWhenVisible`(DOM에서 떼어 냄) + 우리 스케줄러 그리기 중단. 구독은 유지하므로 다시 보일 때 snapshot을 다시 받지 않는다.

## 4. 물을 것·요청
- **CORE**: `core.ui.layout.{list,get,put}`(plan §4.1 `loom ui layout`) — 생기면 `LayoutBackend`로 연결. put의 이름 충돌 응답(`E_EXISTS`?)과 1 MiB 상한을 서버도 지키는지.
- **리드**: dockview 버전 고정(`8.8.3`)과 vendored CSS를 함께 올려야 한다 — `package.json` 갱신 때 알림 부탁. 번들 JS가 1.32 MB(빌드 경고)로 커졌다 — 도킹을 lazy chunk로 나눌지(설정 소유 리드).
- e2e 추가 제안(시험 파일 수정 금지라 제안만): 저장 → 새로고침 → `#/l/<저장 이름>` 복원, 탭 끌기 후 STOP 버튼 위치 불변, 가리키기 시 경로 불변.

## 5. 남은 것 (M2)
- 레이아웃 바의 [삭제]는 현재 경로가 저장 레이아웃일 때만 보임. 가져오기/내보내기(파일) UI.
- `pinned`(Activity 등 고정 패널)는 모델에만 있고 아직 도킹 밖 고정 영역과 연결하지 않음.
- 도킹 키보드 이동(그룹 간 포커스)과 접근성 문구 한국어화(dockview 기본 영어 메시지).
