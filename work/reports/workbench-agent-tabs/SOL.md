# SOL — 선택한 관찰 공간 + 요청한 작업 장부

2026-11-14 · 독립 제안 전용. 제품 코드 변경 없음. SOL.html은 외부 의존·backend 연결 없는 **DEMO**이며 실제 장치 상태를 표현하지 않는다. 다른 모델 산출물과 공용 static shell은 읽지 않았다.

## 추천 구성

상단에 **통합 보기 / 에이전트 작업** 두 개의 주 탭을 둔다. 통합 보기는 사용자가 고른 서비스 인스턴스를 동시에 관찰하는 공간, 에이전트 작업은 사용자가 요청한 실행을 추적하는 장부다. 공통 헤더의 환경·연결 신선도·하드웨어 STOP은 탭 밖에 둔다. 작업 탭 배지는 `실행 1 · 개입 1`처럼 원인을 드러내고, 탭 이동은 실행 수명에 영향을 주지 않는다.

```text
LOOM / Bench A     연결·기록 상태             하드웨어 STOP
[통합 보기] [에이전트 작업 · 개입 1]
통합: [서비스·인스턴스 선택] [저장 구성] [분할 방향]
  사용 중                 선택한 관찰 패널 (분할선 이동)
  UART                    UART / dut / session-a / gen 2
    ☑ dut   ☐ aux         CAN / can0 / session-a / gen 4
  CAN                     UDS / bcm / ECU 0x1001 / via NIC-A
    ☑ can0                Ethernet / NIC-A / capture-17
  UDS ☑ bcm               공통 시간 커서·각 패널의 신선도
  Ethernet ☑ NIC-A
작업: [진행·개입·완료 목록] | 요청 원문 / 대상 / 실제 상태
                             명시 계획 → 현재 단계 → 실행 근거
                             [사용자 응답] [작업 취소]
                             결과 / 미완료 / 산출물 / 로그
```

### 통합 보기의 규칙

- 선택 단위는 서비스 이름이 아니라 `서비스 → 세션 → 인터페이스/ECU → 뷰`다. 같은 UART 두 포트, 같은 ECU 별칭의 서로 다른 세션, NIC별 캡처를 구분한다. 표제에 별칭과 실체 식별자를 함께 둔다. UDS와 CAN/Ethernet이 나란히 있다는 이유로 전송 경로를 추정하지 않는다.
- 처음은 빈 선택과 “사용 중인 소스에서 선택” 안내. 현재 open 인스턴스만 기본 선택기에 노출한다. 사용하지 않는 PSU 등을 고정 메뉴로 채우지 않는다. 새 채널은 “새 소스 1”로 알리고 자동 편입하지 않는다. 연결 관리는 별도 진입점이다.
- UART+CAN, CAN+UDS, 3개 이상의 임의 조합을 지원한다. 2개는 좌우, 3~4개는 격자, 그 이상은 스크롤 가능한 분할 그룹으로 시작한다. Dock의 분할·resize·최대화와 키보드 이동을 사용하며 기존 32패널 상한을 명시한다. 공통 시간 커서는 동기 보장 범위만 묶고 시계 불확실·누락 구간을 표시한다.
- 서비스 전용뷰는 선택 패널의 “전용뷰”에서 연다. 기존 `#/s/...` 및 패널 링크를 보존한다. 통합 선택과 전용뷰의 배치·필터는 각각 유지하고, 기초 세션과 기록은 공유한다. 뒤로 돌아오면 선택을 재구성하지 않는다.
- 저장은 선택·뷰별 설정·분할 비율·구성 이름을 저장한다. 장치를 열거나 작업을 재실행하지 않는다. 재접속은 session/generation을 비교하고 다른 실체나 새 세대로 자동 치환하지 않는다. 해당 칸을 `연결 끊김 / 마지막 수신 / 기록 범위`로 남겨 사용자가 다시 연결 대상을 지정한다.
- STOP 뒤에는 칸을 지우지 않고 `정지 시점의 기록`으로 전환한다. 원본 기록을 다시 조회할 수 없는 서비스는 `현재 캐시만 / 재실행 후 복구 불가`를 표시한다. 선택기에는 “선택된 오프라인 소스”만 예외로 남긴다. 저장 구성 복원과 live 재연결은 별개 행동이다.

### 에이전트 작업의 규칙

목록은 사용자 요청으로 생성된 task만 포함한다. agent client, 토큰, RPC 한 건을 task로 만들지 않는다. 행은 제목·요청자·대상·현재 단계·실제 상태·경과·최종 근거 시각을 담는다. 개입 필요가 먼저 오고 완료는 접는다. 요청 입력은 작은 요청 폼으로 가능하며 채팅 타임라인으로 대체하지 않는다.

상세는 요청 원문, 범위/성공 기준, 공개된 계획(개정 버전 포함), 단계별 실행, 결과 순이다. 현재 단계에 도구명·시작/종료·요약·request ID·입출력/로그 위치·읽은 seq 범위·오류·대기 조건과 deadline을 붙인다. 계획은 의도, 실행 영수증은 사실로 구분한다. 진행률은 `완료 2/4 단계`만 사용하고 계획 변경 시 분모와 변경 이유를 갱신한다. 근거가 없으면 완료로 표시하지 않는다. 원시 LLM 토큰·내부 추론은 노출하지 않는다.

| task/단계 상태 | 진입 근거와 화면 | 사용자 행동 |
|---|---|---|
| queued / planned | 요청 수락 ID / 아직 실행 안 됨 | 취소 |
| running | 실행 시작 영수증, 현재 도구 | 취소 |
| waiting_event | 실제 wait 등록, 조건·deadline | 취소, 근거 열기 |
| waiting_user | 질문·필요 입력·영향 범위 | 응답 또는 취소 |
| blocked | STOP, 자원 충돌, 권한/연결 문제의 명시 이벤트 | 원인 확인, 조건 해소 후 명시 재개 |
| cancelling | 취소 접수, 중단 대기 도구·잔여 영향 | 중단 확인 대기 |
| cancelled | 실행자 중단/정리 확인, 이미 실행된 영향 | 부분 결과 열기 |
| succeeded / failed | 종료 영수증과 산출물 또는 오류 | 결과·근거, 새 시도로 재실행 |

연결 신선도는 위 상태와 별도다. 통신이 끊겨도 마지막 `running`을 `failed`로 바꾸지 않고 `상태 확인 불가 · 마지막 확인 12:03`을 겹쳐 표시한다. 단계는 skipped도 허용하고 task 완료는 모든 필수 단계의 종료·증거와 일치해야 한다.

**작업 취소**는 해당 task의 이후 실행을 막고 실행 중 작업에 중단을 요청한다. 이미 전송된 명령은 되돌릴 수 없으며 취소 확인 전에는 cancelled로 표시하지 않는다. **하드웨어 STOP**은 기존 안전 경로의 범위·ACK를 그대로 표시하며 task취소 ACK를 대신하지 않는다. STOP으로 막힌 task는 자동 재개하지 않는다. 분석 등 영향을 받지 않는 작업은 계속할 수 있다.

## native에서 확인한 재사용과 간극

| 실제 읽은 위치 | 현재 동작 | 제안의 변화 |
|---|---|---|
| `ui/src/shell/{ShellApp,ServiceNav,routes}.tsx/ts` | open 채널 기반 서비스 메뉴, 통합·서비스·딥링크 | 두 주 탭과 사용자 선택 상태 추가. 현재 route union에는 task 없음 |
| `dock/DockLayout.tsx` | m1-default와 서비스 초기 root를 tabs로 구성; route 변경 시 load | 통합은 명시적 선택 기반 split. 돌아올 때 이전 geometry/selection 복원 |
| `dock/{layoutModel,layoutStore}.ts` | 분할/탭·props·vendor 직렬화, 최대 32패널; localStorage 저장 | 기존 Dock 재사용. 선택 인스턴스의 안정적 binding과 오프라인 슬롯 메타 추가. daemon 동기화는 현재 없음 |
| `dock/activeWorkspace.ts` | 닫힌 채널은 주로 제거, ETH·logic만 기록 유지 예외 | 사용자 선택 슬롯과 live availability 분리. 모든 도메인의 기록 복구를 이미 지원한다고 가정하지 않음 |
| `store/{channels,shellControl}.ts` | domain/channel/generation과 연결·신선도·STOP 정보 | 세션·실체 binding 계약 보완, task와 독립 표시 |
| `store/activityStore.ts`, `adapter.ts` | 기본 5,000건 ring; actor/method/range/waiting, agent cursor | 보조 감사 타임라인 재사용. ring은 영속 task 장부가 아님 |
| `crates/loom-proto/src/core_events.rs:76`, `loom-gateway/src/daemon.rs:682` | core.activity는 초당 client/method RPC 집계도 발행 | task/step ID, 상태 전이, 결과·취소 계약이 없음. 활동 빈도로 task 성공/진행을 추정하면 안 됨 |

확인한 store/proto 범위의 `task_id / core.task / TaskStore / TaskState` 검색에서 정식 task 계약은 발견하지 못했다. 저장소 전체에 task 개념이 없다고 단정하는 것은 아니다. 기존 testSession은 테스트 실행용이며 임의 사용자 요청 task와 자동 동일시하지 않는다.

## 필요한 최소 계약 — 모두 신규 제안

1. `ObservationBinding`: workspace/session ID, domain, channel alias, stable instance ID, generation, panel+props, history capability와 기록 참조. UI의 선택 intent와 실제 live binding을 분리한다. 기존 alias만으로 복원이 안전하지 않으면 사용자가 재지정한다.
2. `Task`: task ID, 사용자 요청/요청자, 범위, runner ID, 상태, 공개 plan version, timestamps, revision, steps, 결과·미완료·영향. `Step`: ID, 의존 단계, 상태, attempt, tool/request ID, wait조건/deadline, error, evidence refs. 참조에는 session+source+generation+seq/cursor, artifact ID를 포함한다.
3. `task.create/list/get/events/respond/cancel`과 영속 snapshot+증분 cursor. 재접속 시 snapshot과 누락 이벤트를 맞추고 revision으로 중복·역순을 처리한다. 취소는 idempotency key와 실행자 ACK/미중단 이유를 반환한다. 재시도는 새 attempt로 남긴다.
4. 실행 등록 주체가 task/step 상관 ID를 도구 호출에 전달한다. core.activity는 가능한 경우 그 ID에 연결하되 기존 집계로 대체하지 않는다. runner와 작업 구독은 Shell services/daemon 측 수명에 두고 탭 unmount와 분리한다. 권한·승인·STOP gate를 우회하지 않는다.

## 두 사용 흐름

1. “부팅 지연을 CAN과 비교”: 사용자가 UART dut+aux와 CAN can0을 선택·저장한다. 명시 요청을 제출하면 task가 “로그 구간 수집 → 타임라인 비교 → 보고”로 등록된다. 작업 상세에서 wait조건과 실제 읽은 구간을 보고, 통합 탭에서 같은 근거 시점을 연다. aux 연결이 끊겨도 슬롯과 누락 구간이 남는다. STOP 후 기록 모드로 비교를 계속한다.
2. “BCM VIN 읽고 캡처와 대조”: UDS bcm+Ethernet NIC-A+CAN can0을 선택한다. task가 대상 확인을 요청하면 명시적으로 답한다. 취소 시 `취소 요청됨 → 실행자 확인 중 → 취소됨`을 구분하고, 이미 읽은 VIN·패킷은 부분 결과로 남긴다. task 취소로 다른 UART 작업이나 하드웨어 세션을 일괄 중단하지 않는다.

## 짧은 대안과 비교

대안은 기존 Dock 안에 “작업” 패널을 추가하는 것이다. 구현량은 적고 근거를 바로 옆에서 볼 수 있지만 패널을 닫으면 작업도 끝난다는 오해가 크며, 복잡한 관찰 배치에서 개입 요청이 묻힌다. **수명 독립성·개입 발견성·서비스 조합 자유도·기존 Dock 재사용**을 기준으로 주 탭 분리를 추천한다. 근거 열기는 통합 탭으로 이동하고 “작업으로 돌아가기”를 남겨 탐색 비용을 줄인다.

## 공식 참고 → 적용

- [VS Code Custom Layout](https://code.visualstudio.com/docs/configure/custom-layout): split/grid·분할선 resize·세션 간 배치 유지·키보드 이동. Loom은 기존 Dock 위에 선택 intent와 구성 복원을 추가한다. 새 floating window는 도입하지 않는다.
- [GitHub Actions workflow run logs](https://docs.github.com/en/actions/how-tos/monitor-workflows/use-workflow-run-logs): run→job→step, 실패 단계와 특정 로그 줄 연결. Loom task→step→도구 영수증/seq 근거 계층에 적용한다.
- [GitHub Actions workflow cancellation](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-cancellation): 서버 요청과 runner 중단 처리가 구분된다. Loom도 cancelling과 최종 ACK를 분리한다. GitHub의 signal·timeout 수치는 장치 작업에 복사하지 않는다.

2026-11-14 공개 웹 문서 확인. 위 적용은 설계 판단이며 해당 제품이 Loom의 장치·안전 모델을 제공한다는 의미가 아니다.

## 구현·검증 순서

1. binding·task event 계약과 상태 전이부터 고정한다. 취소/완료 경합, ACK 없는 취소, 재접속·역순·중복·유실, STOP 후 재개를 계약 fixture로 검증한다.
2. 기존 Dock에 선택·저장·오프라인 슬롯을 붙인다. 복수 UART/ECU/NIC, 0·1·3·32개 선택, 별칭 재사용·generation 변경, 저장소 차단, 서비스뷰 왕복을 확인한다.
3. task runner 영수증과 영속 조회를 연결한 뒤 목록/상세·응답·취소·근거 이동을 구현한다. 탭 이동·화면 재마운트에도 실행이 지속되고, 재실행을 자동 유발하지 않는지 확인한다.
4. STOP 실제 경로 및 도메인별 기록 보존은 후속 통합 검증 대상이다. 이번에는 장치·Cargo 실행 없이 제안/DEMO만 제공한다.

목업 범위: 두 탭, 인스턴스 다중 선택/조합, 방향·비율, 구성 저장/복원, 목록→상세, 사용자 응답, 취소 요청→확인, 근거 이동, STOP 후 기록 표시. 진행 버튼과 ACK 버튼은 가상 영수증을 주입하며 실제 자동 실행·기록 복원·드래그 Dock은 구현하지 않는다.

작성 후 검증: Node VM과 최소 DOM stub로 HTML 스크립트 구문, 3개 서비스 조합, 구성 저장/복원, 탭 전환 상태 유지, 응답, 취소→중단 ACK, 완료, 근거 이동, STOP 후 선택 유지, 외부 의존 없음 확인. 브라우저 렌더링·시각 QA는 수행하지 않았다. foreground workspace 유지를 위해 브라우저 창/네이티브 앱을 조작하지 않았다.
