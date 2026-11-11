# UI 패널 명세 템플릿 (도메인 팀 → UI 팀)

docs/plan/02-core-principle.md 준수.

작성: ui-lead, 2026-11-10 (v0.1). 도메인 팀은 `_xteam/<comp>-ui.md`에 패널마다 아래 양식을 채워 주세요. 이미 다른 형식으로 썼다면 그대로 두고 빠진 항목만 덧붙이면 됩니다. 소유: **내용 명세 = 도메인 팀**, 프레임워크·렌더링 구현 = UI 팀.

```markdown
## 패널: <domain>.<name>   (예: can.trace, uart.terminal, uds.console)

- 목적 (한 줄): 사용자가 이 패널로 무엇을 보고 어떻게 개입하는가
- 마일스톤: M1 최소판 / 이후 확장 (무엇이 M1인지 분명히)
- CANoe/TeraTerm 등 대응 기능:

### 데이터 (읽기)
- 구독: filter{domain, channel, kinds[]} — 이벤트 kind 목록과 각 payload 필드 (스키마 링크)
- 늦은 합류 snapshot에 필요한 상태: (예: ID별 최신 프레임, 화면 백로그 N줄)
- 질의 RPC (범위·검색·디코드): 이름, params, 결과 크기 상한
- 예상 유입률: 정상 / 최대 (예: 2k ev/s, 버스 100% 시 10k ev/s)
- 표시 규칙: 열·정렬·필터식·행 갱신 규칙(같은 request_id 행 점진 갱신 등)·색/배지
- 해석은 도메인 payload 그대로 표시 (UI가 NRC 이름·DBC 디코드 등을 재구현하지 않음) — 필요한 해석 필드 목록

### 개입 (쓰기)
| 사용자 입력 | 호출 RPC | 위험 등급 | 확인/승인 방식 | 쓰기 상태 표시 |
|---|---|---|---|---|

### 실시간성·정직성
- 장치 → 화면 지연 목표 (p95 / p99)
- 고부하 시 동작: 축약·다운샘플·드롭 표시 방법, 절대 드롭하면 안 되는 이벤트
- 상태 배지: 연결·세대(generation)·source.gap·record.health 등 도메인 특유 상태

### 와이어프레임 (ASCII, 선택)

### 수용 시험 (UI 자동 시험으로 확인할 것)
- 예: sim에서 에이전트 `loom can send …` → 200 ms 안에 Trace에 AGT 배지 행
```

참고: UI 팀 공통 셸이 이미 제공하는 것(패널에서 다시 만들지 않음) — 전역 STOP·비상 OFF, 승인 대기열, env 배지, LIVE/LAG/gap 배지, 마커, 기록 탐색·재생, Activity(에이전트 행동 타임라인), 테스트 실행 화면.

## v0.2 추가 (ui-lead, 2026-11-10)
- 패널 프레임워크 정본은 `docs/plan/ui/plan.md` §4.2(manifest·`PanelContext`·`ActionSpec{rpc, class, right, confirm, safetyDirection}`). 명세의 "개입" 표에 **최소 자격(watch/operate/approve)**과 **안전 방향 여부**(STOP·중단·비상 OFF)를 한 열씩 추가해 주세요.
- 쓰기 행은 kind와 무관하게 `request_id` + `payload.write_state`가 있어야 셸 공통 행 갱신 표(RowStore)가 그린다.
- 셸 슬롯: `channel_bar`(채널 상태), `approval_body`(승인 카드 본문, 기본은 `approval.display` JSON Schema 일반 렌더러), `evidence_ref`(TEST REF `<domain>:<channel>@seq:a..b`) — 패널 대신 슬롯으로 기여 가능.
- 지연 보고·시계는 셸 공통(`core.metrics.report`, `core.time`) — 도메인별 이름을 만들지 않는다.
