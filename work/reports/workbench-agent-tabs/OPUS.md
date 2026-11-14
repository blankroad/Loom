# 통합 보기 / 에이전트 작업 — OPUS 제안 (2026-11-14)

기준: main `d382628` + 미커밋 logic 작업. 읽은 native 파일: `ui/src/shell/{ShellApp.tsx,ServiceNav.tsx,routes.ts,dock/activeWorkspace.ts,dock/layoutStore.ts}`, `ui/src/store/{channels.ts,activityStore.ts,adapter.ts(ActivityEntry),stopController.ts,testSession.ts}`. 목업: `OPUS.html` (외부 의존 없음, **모든 데이터 DEMO**).

## 0. 현재 구조 요약 (사실)
- 라우트: `#/l/<preset|saved>`, `#/s/<service>`, `#/p/<domain>/<panel>`. `ServiceNav`는 **open 채널이 있는 서비스만** 노출 → 요구 "사용 중인 서비스만"은 이미 원칙으로 존재.
- 조합은 고정 preset(`can-uds` 등)뿐. 임의 조합은 dock에서 패널을 수동 배치 후 `LocalLayoutStore`(**현재 구현: 브라우저 localStorage, 프로필별**, ≤32개)로 저장. 파일 주석상 서버 RPC `core.ui.layout.{list,get,put}`은 "CORE가 제공하면 교체" 예정으로 요청만 접수된 상태이며 **미구현**. `activeLayout()`은 닫힌 채널 패널을 **제거**(eth/logic만 닫힌 뒤 유지).
- `core.activity`/`ActivityStore`: RPC 단위 활동 ring(5,000, seq·method·actor·waiting). **계획·단계·결과·취소 개념 없음** → task 모델로 쓰면 안 됨.
- `TestSessionStore`: 단계·ask/answer·verdict reducer — task 모델과 형태가 가장 가까운 기존 자산.
- STOP: `core.latch` 기반 하드웨어 잠금(`StopController`). task 취소 경로 없음.
- Bench Light static mock과 차이: native는 daemon 이벤트·open 채널로 화면이 **유도**됨. 목업의 고정 채널 목록·타이머 진행은 연출일 뿐.

## 1. 추천안
### 1-A. 통합 보기 = "관찰 세트(View Set)" 선택기 + 기존 dock
좌측 **선택기**(사용 중 채널만, 도메인별, 별칭·장치 식별 표기) 체크 → 우측 dock에 패널이 split으로 추가. 배치 프리셋(자동격자/가로/세로) 후 dockview로 자유 resize. 세트를 이름으로 저장. 공통 **시간 커서**(`at`)로 한 패널 클릭 시 다른 패널을 같은 시점으로 reveal.

### 1-B. 에이전트 작업 = daemon 소유 Task 레지스트리 + 목록/상세
사용자가 요청해 시작된 작업만 `task_id` 단위로 표시. 상세 = 요청 원문 · 명시된 계획(단계) · 단계별 툴 실행(=`core.activity` 중 해당 `task_id`)과 도구 결과/명시 조건 · 사용 자원 · freshness(lease/status) · 사용자 질의(필요 시만) · 산출물 · 결과. 승인은 현재 Loom 정책대로 **기본 R0–R4 자동**, 사용자가 approval을 설정한 경우에만 `waiting_user`. LLM 토큰/내부 추론은 **받지도 그리지도 않음**.

### 짧은 대안 (비채택)
| 대안 | 장점 | 기각 이유 |
|---|---|---|
| A1. 조합별 preset 추가 (`uart-can`, `can-uds-eth` …) | 구현 최소 | 조합 폭발, 복수 UART/ECU 구분 불가 |
| A2. Grafana식 변수 1개로 패널 반복 | 동일 도메인 다채널에 강함 | 이종 도메인 혼합 약함 → 1-A의 "같은 도메인 일괄 추가"로 흡수 |
| B1. ActivityStrip을 actor로 필터 | backend 0 | 단계/진행근거/결과/취소 없음 → "작업 외 활동" 보조 섹션으로만 재사용 |
| B2. 채팅창 | 익숙 | 요구 위반(원시 대화 전시), 근거 연결 약함 |

### 비교 기준
① 임의 조합 표현력 ② 복수 동일 도메인 구분 ③ 저장·재접속 복원 정확성(가짜 장치 없음) ④ STOP 후 기록 가시성 ⑤ task 실제 상태의 **서버 근거성**(에이전트 자기보고와 관측 분리) ⑥ HW STOP ↔ task 취소 분리 ⑦ 기존 코드 재사용량.

## 2. 참고 공식 자료 (적용점)
1. GitHub Docs — *Using workflow run logs* <https://docs.github.com/en/actions/how-tos/monitor-workflows/use-workflow-run-logs>: Job→Step 목록, 실패 step 자동 펼침, 줄 permalink, 로그 검색. → **단계표에서 실패/대기 단계 자동 펼침**, 툴 실행에 `seq` permalink(`#/p/...?at=`), 상세 내 로그 필터.
2. GitHub Docs — *Managing agent sessions* (Copilot) <https://docs.github.com/en/copilot/how-tos/use-copilot-agents/coding-agent/track-copilot-sessions>: 세션 목록(내가 시작한 것) → 로그·개요, **Stop session**은 실행만 끝내고 이미 push한 결과는 유지. → task 취소는 "실행 중단 + 산출물 보존", 하드웨어 상태는 건드리지 않음을 명시. 차이: Copilot은 내부 추론을 로그에 노출하지만 Loom은 **명시 계획+툴+산출물만**.
3. Grafana — *Variables* <https://grafana.com/docs/grafana/latest/dashboards/variables/>: 상단 선택값이 모든 패널을 갱신, 편집 없이 보기 전환, 변수로 패널 반복. → 통합 보기 상단에 "세트 선택" + 도메인 일괄 추가(예: CAN 전체), URL 동기화.

## 3. 정보 구조
```
TopBar [통합 보기 | 에이전트 작업 ●2 ]                    [STOP]
├─ 통합 보기 (#/v/<set>)
│  ├─ 선택기: 사용 중 채널만 (도메인 > 채널[별칭·장치·상태])
│  │    UART  ttyUSB0 "ECU-A console" · ttyUSB1 "Gateway"
│  │    CAN   can0 500k · can1 FD     UDS  ecu_front(DoIP) · sim_bcm
│  │    ETH   en5 (closed, 기록 있음)
│  ├─ 세트 바: [저장된 세트 ▾] [저장] 배치(자동|가로|세로) 시간커서 동기 ☑
│  └─ dock: 선택 순서대로 split, 경계 드래그 resize, 각 패널 → "전용뷰 열기"(#/s/..)
└─ 에이전트 작업 (#/tasks/<id>)
   ├─ 목록: 필터(진행|사용자 대기|완료) · 제목 · 단계 n/m · 상태 · 마지막 근거 시각
   └─ 상세: 요청 / 계획단계표 / 현재단계 툴실행(seq 링크) / 자원(R/W) /
            freshness(lease) / 질의(필요 시) / 산출물 / 결과 · [작업 취소] (STOP과 별도 위치·색)
```
```
┌ 선택기 ─────────┬ ttyUSB0 ECU-A ──────┬ can0 500k ──────────┐
│☑ UART ttyUSB0   │ [boot] U-Boot 2024… │ 0x7E0 02 10 03 …    │
│☐ UART ttyUSB1   │ login:              │ 0x7E8 06 50 03 …    │
│☑ CAN  can0      ├─────────────────────┴─────────────────────┤
│☑ UDS  ecu_front │ ecu_front  10 03 → 50 03  22 F190 → 62 …  │
└─────────────────┴───────────────────────────────────────────┘
```

## 4. 사용 흐름
**① UART+CAN+UDS 동시 관찰 → 저장 → 재접속 → STOP**
1. 사용자 UART 2개·CAN·DoIP 연결 → 통합 보기 선택기에 4개 표시(ttyUSB0/1은 별칭으로 구분).
2. ttyUSB0·can0·ecu_front 체크 → 자동격자, 하단 UDS 높이 resize → "부팅-진단" 저장.
3. 다음날 재접속: can0만 open → 세트 로드 시 can0 패널만 활성, 나머지는 선택기에 **"미연결 · 세트 구성원"** 회색(패널 자리 대신 목록에만, 가짜 장치 생성 없음). 연결되는 순간 원래 위치로 복귀.
4. STOP: 패널은 사라지지 않고 **"STOP 잠김 · 기록 seq a–b"** 동결 표시, 스크롤·검색 가능. 쓰기 입력만 비활성.

**② 에이전트 작업 진행·개입·취소**
1. 사용자 요청 "ECU 리셋 후 부팅로그·DTC 확인" → task `T-0412` 생성(queued→running), 계획 4단계 표시.
2. 2단계 "ECU Reset(0x11)": 기본 정책(R0–R4 자동)이므로 **승인 없이** 실행 → 도구 결과 `uds.request 11 01 → 51 01` (seq 8806)로 단계 완료 표시, 통합 보기 패널에도 같은 seq 하이라이트. 사용자가 approval 정책을 켠 경우에만 이 지점이 `waiting_user`+승인 버튼이 됨.
3. 3단계 "login: 대기 ≤ 60 s"는 RPC가 오래 없어도 `running` 유지. 근거는 lease 갱신(`task.status{lease_ms, status:"uart.wait_for login: · 12 s/60 s"}`)으로 별도 freshness 칩에 표시. 다른 작업(T-0414)이 요청 모호성 때문에 **명시 질문**을 하면 그 작업만 `waiting_user`, 탭 배지 `●1 대기`.
4. 사용자가 HW STOP → task는 **`blocked(STOP)`**(취소 아님): 계획 유지, "잠금 해제 시 재개/취소 선택".
5. 사용자 [작업 취소] → `cancelling`(task-scoped 취소 요청, 에이전트 ACK 대기) → ACK 후 `cancelled`. 공유 runner의 다른 작업은 영향 없음, 수집된 부팅로그 산출물 유지, 장치 상태 변경 없음.

## 5. Task 단계·상태
| 상태 | 의미 | 진입 근거(서버) | 사용자 조작 |
|---|---|---|---|
| queued | 요청 접수, 자원 대기 | `task.created` | 취소 |
| running | 단계 수행 중 (긴 wait 포함) | `task.step active` + 유효 lease | 취소 |
| waiting_user | 명시 질문, 또는 사용자 설정 approval 정책이 요구한 승인 | 열린 `task.ask{reason: question\|policy}` | 답변·거부·취소 |
| blocked | HW STOP 잠김/채널 소실 | `core.latch on` 또는 자원 closed | 해제 후 재개 / 취소 |
| cancelling | task-scoped 취소 요청, 에이전트 ACK 대기 | `task.cancel` 수신, `task.cancel_ack` 없음 | — (ACK 지연 시 "확인 안 됨 · 재요청"; 전용 worker면 "프로세스 중단" 추가 노출) |
| cancelled / failed / succeeded | 종결 | `task.ended{outcome}` | 결과·산출물 열기, 재요청 |

단계(step) 상태: `pending · active · waiting_user · done · failed · skipped`. 단계 완료/실패는 **도구 결과(응답·NRC·wait 매치/timeout) 또는 계획에 명시된 조건**으로만 바꾸고, 선언 단계와 관측 근거(seq)를 같은 행에 병기.

**Freshness(상태와 분리된 부가 표시)** — task 상태를 바꾸지 않음:
| 표시 | 조건 | 의미 |
|---|---|---|
| 활성 | lease 유효 + 최근 RPC/상태 갱신 | 정상 |
| 대기 중(정상) | lease 유효, RPC 없음, `status`가 선언된 wait(예: ≤60 s) | 긴 wait/no-RPC도 진행 증거 있음 |
| lease 만료 | lease 기한 경과, 갱신 없음 | "상태 미확인" 경고. 상태는 그대로, 사용자는 취소/대기 선택 |
| 종결 | 종결 상태 | 표시 없음 |

## 6. 최소 계약
### Backend (신규, daemon 소유 — 사내 자료 추측 없음, 제안)
- RPC: `task.list{state?}`, `task.get{task_id}`, `task.cancel{task_id, reason}`, `task.answer{task_id, ask_id, value}`; 에이전트용 `task.start{title, request_text, plan[]}`, `task.step{task_id, step_id, state, note?}`, `task.artifact{…}`, `task.end{outcome, summary}`.
- 이벤트(priority, 늦은 합류 snapshot): `task.created/state/step/ask/answer/artifact/ended`.
- **`core.activity` payload에 `task_id?` 추가**: CLI가 `LOOM_TASK_ID` 환경으로 붙이면 daemon이 그 세션의 모든 RPC를 자동 귀속 → 툴 실행 근거는 에이전트 보고가 아니라 서버 기록.
- `blocked` 계산은 daemon: latch on 또는 task 자원 채널 closed.
- Lease: 에이전트가 `task.status{task_id, lease_ms, status}`를 주기 갱신(긴 wait 중에도). daemon은 RPC 활동 또는 lease 갱신으로 freshness 계산, task 상태 자체는 바꾸지 않음.
- 승인: 별도 승인 단계를 새로 만들지 않음. 기존 R0–R4 자동 + 사용자 approval 설정을 따르며, 정책이 승인을 요구하거나 에이전트가 명시 질문할 때만 `task.ask{reason}`.
- 취소(기본) = **task-scoped cooperative**: `task.cancel` → 에이전트가 해당 task만 정리 후 `task.cancel_ack{task_id}`; daemon은 이후 그 task_id의 신규 장치 RPC를 거부(다른 task·공유 runner는 무영향). **프로세스 중단은 runner가 `dedicated`(그 task 전용 worker)로 등록된 경우에만** 허용(ACK 시한 초과 후 사용자 선택). **latch 변경 없음**. STOP은 latch만 걸고 task를 취소하지 않음.
- 통합 보기 기록: 닫힌 uart/can 채널을 seq 범위로 조회하는 경로가 있는지 확인 필요(현재 history 유지는 eth/logic만). 없으면 STOP/close 시 **UI 메모리 ring 동결**로 1차 대응.
- 레이아웃/View Set 저장 — [사실] 현재는 `LocalLayoutStore`(localStorage)만 존재, 같은 브라우저 프로필에서만 재접속 복원. [제안·미구현] 다른 브라우저/기기 간 복원이 필요하면 후속 신규 RPC `core.ui.layout.{list,get,put}`(요청만 접수됨)을 CORE와 별도 합의 후 `LayoutBackend` 구현체로 교체. 1차 구현은 서버 RPC에 의존하지 않음.

### UI 타입
```ts
interface ViewSet { name: string; members: { domain: string; channel: string; panel: string; view?: string }[];
  arrange: 'auto'|'rows'|'cols'|'custom'; layout?: LoomLayout; syncCursor: boolean }
interface Task { taskId: string; title: string; requestText: string; requestedBy: string; agent: string;
  state: TaskState; steps: TaskStep[]; resources: { domain: string; channel: string; mode: 'r'|'w' }[];
  asks: Ask[]; artifacts: { kind: 'log'|'report'|'capture'|'file'; ref: string; label: string }[];
  lastEvidence: { seq: bigint; wallMs: number } | null;
  lease: { expiresMs: number; status: string } | null; runner: 'shared'|'dedicated'; outcome?: { result: string; summary: string } }
interface TaskStep { stepId: string; title: string; state: StepState; activitySeqs: bigint[]; note?: string }
```

## 7. 재사용 vs 추가
| 재사용 | 추가 |
|---|---|
| `DockLayout`/dockview split·resize, `layoutModel` 직렬화, `LayoutBackend` 인터페이스 + **`LocalLayoutStore`(현재 구현, localStorage)** | `ViewSet` 멤버 모델 + 선택기 컴포넌트, `#/v/<set>` 라우트 |
| `activeLayout`/`availableSlot`(open 투영), `deviceKey`(장치 구분) | 미연결 멤버 "고스트" 표시, STOP 동결 모드, (후속·미구현) 서버 레이아웃 RPC backend |
| `slotHash(at)` reveal 커서 | 패널 간 커서 동기 버스 |
| `ServiceNav` open-only 규칙, `defaultPanelFor` | TopBar 두 탭 + 작업 배지 |
| `TestSessionStore` reducer 패턴(ask/answer, seq dedup, C2 상한) | `TaskStore`(shell 수명, 라우트 무관 구독 → 백그라운드 지속) |
| `ActivityStore`(task_id 필터 뷰), `StopController` | 작업 취소 컨트롤러(cooperative ACK, 전용 worker일 때만 중단 옵션, STOP과 별 위치/색), freshness 칩(lease) |

## 8. 구현·검증 순서
1. **계약 합의**(task RPC/이벤트, `core.activity.task_id`) — CORE 확인 전 UI는 fixture.
2. `TaskStore` 단위테스트: 상태표 전 전이, seq dedup, late-join snapshot, 탭 전환 시 구독 유지(라우트 unmount 후에도 상태 진행).
3. 에이전트 작업 목록/상세 + 취소: 테스트 "STOP 클릭이 task.cancel을 보내지 않음", "task.cancel이 latch를 바꾸지 않음", "공유 runner에서 취소가 다른 task를 건드리지 않음", "lease 만료가 상태를 바꾸지 않고 freshness만 바꿈", "기본 정책에서 쓰기 단계가 waiting_user로 가지 않음".
4. 통합 보기 선택기 → dock 추가, 저장/로드, 재접속 고스트(가짜 장치 0개), 복수 ttyUSB 구분 테스트.
5. STOP 동결 + 기록 조회(backend 경로 확인 결과에 따라).
6. 실장치 없이 sim(DoIP sim, CAN sim)으로 흐름 ①②를 브라우저 증거로 기록 후 벤치 실측.

## 9. 미결/결정 필요(ROOT)
- 기존 "통합 작업 공간"(`#/l/m1-default`)을 통합 보기로 **대체**(추천) vs 병존.
- 닫힌 UART/CAN 기록의 서버 조회 가능 여부.
- task를 누가 시작하는가: CLI 명시 `loom task start`(추천, 사용자 요청만 등록) vs daemon 자동 추론(비추천 — "사용자가 요청한 작업만" 위반 위험).

## 10. ROOT 검토 반영 (2026-11-14)
- 쓰기=무조건 승인 예시 제거: 기본 R0–R4 자동, `waiting_user`는 명시 질문/사용자 설정 approval 정책일 때만, 단계는 도구 결과/명시 조건으로 표시.
- 취소: 프로세스 신호 → task-scoped cooperative 취소+ACK 기본, 전용 task worker일 때만 프로세스 중단.
- `stale` 상태 삭제 → lease/status 기반 freshness 부가 표시로 분리, 긴 wait/no-RPC도 lease로 running 근거 확인.
- 사실 정정: 레이아웃 저장은 현재 `LocalLayoutStore`(localStorage)뿐. `core.ui.layout.*`는 요청만 접수된 미구현 후속 제안으로 분리 표기(§0·§6·§7).
