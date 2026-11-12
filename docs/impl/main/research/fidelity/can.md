# CAN / CAN-TP fidelity — PHASE1 matrix + PHASE2 구현 완료 / READY_FOR_REVIEW

> 구현 결과·deviation·selector·검사: `docs/impl/main/tasks/benchmark-fidelity/can.md` 의 "CAN owner handoff" 절. MAIN 결정에 따라 §3 S3(주기 작업)·§1-8 채널 줄(`can.status`)은 이번 범위에서 **제외**되었다.

작성: 2026-11-12, 모델 Claude Opus 5.5 (`claude-opus-5-5`, 세션 시스템 정보로 확인), 포그라운드 단일 세션, subagent·git 미사용.
기준 base: main 26dc5e8. 소유 파일: 이 문서, `work/reports/fidelity/assets/can/` 만.
주 벤치마크: **PEAK PCAN-View** (Windows). PCAN-View 및 CAN 하드웨어는 **실행하지 않았다**.
증거 표기: **[V]** 공식 스크린샷/그림에서 직접 보임 · **[D]** 공식 매뉴얼/제품 페이지 문장 · **[I]** 추론(검증 안 됨) · **[L]** Loom 소스로 확인.

## 0. 확보한 1차 시각 자료 (`work/reports/fidelity/assets/can/`, 출처 `SOURCES.md`)

| 파일 | 내용 | 비고 |
| --- | --- | --- |
| `ref-manual-p22-receive-transmit-empty.png` | PCAN-USB 매뉴얼 3.2.0 §6.1 그림: 빈 Receive/Transmit 탭 전체 헤더 + 상태줄 | 원본 해상도 2362×1476, 가장 정확한 컬럼 근거 |
| `ref-PCAN-View.jpg` | 제품 페이지 합성 이미지: 데이터가 찬 Receive/Transmit + Connect 대화상자 | 마케팅 합성(로고 아트 포함) — 행 내용만 참고 |
| `ref-PCAN-View_Tracer.png`, `ref-manual-p24-trace-tab.png` | Trace 탭 Stopped / Recording… 두 상태 | 상단 통계 줄 근거 |
| `ref-PCAN-View_New-Message.png`, `ref-manual-p23-new-transmit-fd.png` | New Transmit Message 대화상자(FD) | 편집 흐름 근거 |
| `ref-PCAN-View_Bus-load.png`, `ref-PCAN-View_Connect_PCAN-Gateways.png` | Bus Load 탭, Connect 대화상자 | 미지원/범위 밖 근거 |

화면 구조 [V]: 메뉴(File·CAN·Edit·Transmit·View·Trace·Help) → 아이콘 툴바(연결/해제, 새 메시지, 편집, Trace 기록●/일시정지/정지) →
탭 `Receive / Transmit | Trace | <인터페이스명> | Bus Load | Error Generator` → 하단 상태줄.
Receive/Transmit 탭은 **위 Receive / 아래 Transmit 두 표를 세로 분할**, 각 표 왼쪽에 세로 글씨 라벨과 접기 버튼.

## 1. Fidelity matrix

| # | 참조 영역/조작 (증거) | 실제 상호작용 | 현재 Loom (L) | 제안 매핑 | 유지할 Loom 의미 | 의도적 차이/미구현 |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | 탭 구조 Receive/Transmit · Trace · Interface · Bus Load · Error Generator [V p22,p24] | 탭 전환, 기본 진입은 Receive/Transmit [D §6.1-4] | `can.trace` 한 장(시간순)만 존재 `canTrace.ts` | 패널 내부 보기 전환 `수신/송신 │ 트레이스 │ 채널` — 기본은 **수신/송신** (사용자 승인 시; 아니면 트레이스 기본 유지) | 채널 패널 slot 1개/채널(`routes.ts`) 그대로, 보기 전환은 수집과 무관 | Bus Load·Error Generator 탭 **만들지 않음**(§1-9,10) |
| 2 | **Receive 표**: `CAN-ID │ Type │ Length │ Data │ Cycle Time │ Count` [V p22 헤더, jpg 데이터 행] | ID당 1행, 새 프레임이 같은 행을 덮어씀·Count 증가·Cycle Time=직전 수신 간격(ms) [D 제품페이지 "sorted list" + V jpg; 행 갱신 방식 자체는 I] | 없음. `traceModel.ts`는 링만 보유, ID별 최신 상태 없음 | 신규 panel-local `rxLatest` 모델(ingest 경로에서 링과 **동시에** 갱신, 링 퇴출과 독립). 키 = (id, ext) — 채널은 패널당 고정. 컬럼: `CAN-ID │ 유형 │ Len │ Data │ 주기(ms) │ 횟수` + Loom 추가 `최근(s 전) │ 출처` | Count는 "이 패널이 받은 수"이며 버스 총계 아님; gap 발생 후 행은 `≥N`/gap 표시; dir=rx(device)만 집계, 자기 echo 제외 | ID 수 상한(예: 4,096) 초과 시 "요약 생략 N" 정직 표시. 주기는 hub 수신 시각 기반이면 그렇게 라벨 |
| 3 | Type 셀 표기: `FD BRS` 배지, ext ID는 `17F00100h`, Trace Type=`Data`/`FD, BRS` [V jpg, Tracer, p24] | 읽기 전용 | `traceModel.ts`가 FD·DLC·RTR 표시, ID `0x…` | 유형 셀 = `STD/EXT · FD · BRS · RTR` 짧은 배지; ID는 ext 8자리/std 3자리 hex(접미 `h`는 쓰지 않고 Loom 기존 표기 유지 가능) | BRS는 이벤트 payload에 있을 때만 표시 | `h` 접미·배지 색은 Loom 디자인 토큰 |
| 4 | **Transmit 표**: `CAN-ID │ Type │ Length │ Data │ Cycle Time(☑ + ms) │ Count │ Trigger │ Comment` [V p22 헤더, jpg 체크박스 행] | 행 선택 → Space/Transmit>Send로 수동 송신, 주기 행도 수동 추가 송신 가능 [D §6.2-4]; 체크박스로 주기 on/off [V, 동작 I] | 단발 `sendLine` + 이력(HISTORY_MAX 50) | 송신 표 = (a) daemon `can.periodic.list` 작업 행(`period_ms`, `sent`→횟수, `state` running/paused, `owner`, `lease_left_ms`, `missed`) + (b) 패널 로컬 **수동 메시지 행**(cycle 0). Trigger 열 → `수동/주기(ms)`; Comment → 로컬 메모 | 모든 송신 = 기존 ActionGate(`operate`) + dry-run 위험 배지(`can.send{dry_run}` / `classify_periodic`); 소유자 다른 작업은 조작 비활성+`owner` 표시 | 주기 작업은 **daemon이 원천**, 로컬 생성 흉내 금지. Trigger의 RTR 응답형 등은 미구현 |
| 5 | **New Transmit Message**: `ID(hex) │ Length ▾ │ Data 0..7 바이트 칸(+… FD) │ Cycle Time ms │ ☐Paused │ Message Type: ☐Extended ☐Remote ☐CAN FD ☐BRS │ Comment │ OK/Cancel` [V New-Message, p23] | Cycle 0=수동, >0=주기 [D §6.2-2]; OK → Transmit 표에 추가 [D §6.2-3] | 텍스트 한 줄 파서 `sendLine.ts` | 패널 로컬 편집 폼(바이트 칸 그리드, Length=DLC 유효값 드롭다운 `dlcForLen`) → 결과는 **기존 `parseSendLine` 구조 파라미터와 동일**. 텍스트 줄은 빠른 입력으로 유지 | 검증 규칙 단일화(폼↔텍스트 동일 결과 테스트) | Paused 체크 = `can.periodic` pause 경로가 실재할 때만(§4 의존성) |
| 6 | **Trace 표**: `Time(s, 4자리) │ CAN-ID │ Rx/Tx │ Type │ Length │ Data` [V p24, Tracer] | 툴바 ●기록/‖/■로 버퍼 기록 시작·정지 [V 툴바, D §6.3.1 linear/ring] | 컬럼 `bar·t·dir·origin·id·len·data·state·note` `traceView.ts:68` | 컬럼 순서를 `t │ id │ dir │ 유형 │ len │ data` 로 맞추고 Loom 고유 `출처·상태·note`는 오른쪽 유지 | 수집은 항상(링), **일시정지=렌더만 정지**, 생략/유실 행 유지 | PCAN의 "기록 시작/정지"를 Loom 공통 기록(Recording)과 **연결하지 않음** — 명칭 혼동 금지 |
| 7 | **Trace 통계 줄**: `Recording…/Stopped │ 21,8026 s │ 0,97 % │ Ring Buffer │ Rx 968 │ Tx 0 │ Status 0 │ Errors 0 │ Other 0` [V p24, Tracer] | 읽기 전용 | 하단 status = 화면 행수·fps(`canTrace.ts`) | 트레이스 상단 줄: `실시간/일시정지 │ 링 시간폭 s │ 링 사용 % (rows/capacity) │ 링 │ Rx n │ Tx n │ gap n │ 화면 생략 n` | fps는 화면 지표로 별도 유지, bus 상태로 부르지 않음 | `Status/Errors` 칸은 **버스 오류 소스가 없어 표시 안 함** |
| 8 | **상태줄**: `Connected to hardware …, Channel 1 │ Bit rate 1 MBit/s / 2 MBit/s │ Status: OK │ Overruns: 0 │ QXmtFull: 0` [V 모든 탭] | 읽기 전용 | 없음(채널 상태는 셸/연결 탐색기) | 패널 하단 채널 줄 = `can.status` 채널 항목: `채널·bus·backend·open·gen │ 비트레이트: 알 수 없음 │ 버스 상태: 미지원 │ 관찰 유실 (observer.drops/gaps) │ 주기 작업 n │ latched` | generation 변경 시 요약 리셋 표시 | Overruns→"관찰 유실"로 **다르게 명명**(컨트롤러 overrun 아님). Status OK 녹색 표시 금지 |
| 9 | Bus Load 탭: 막대 % + 이력 그래프 + 최대/최소/평균 [V Bus-load] | 측정 on/off, Reset | 없음, worker에 bus load 값 없음 | **미구현(빈 자리도 만들지 않음)**. fps·프레임률로 유도 금지 | — | backend 계약 필요(S4) |
| 10 | Error Generator 탭: 단일/다중 프레임 파괴 [V/D §6.3.4] | 의도적 프로토콜 위반 | 없음 | **범위 밖·제안 안 함** (안전 정책상 고장 주입) | — | 영구 제외 |
| 11 | Connect 대화상자: 하드웨어·채널·Clock·Nominal/Data bit rate·Filter From/To(Std/Ext)·Listen-only [V jpg, Connect] ; 다채널은 인스턴스 추가 [D §6.1-5] | 연결 전 설정 | 채널 개방은 연결 탐색기/셸 | 패널 내 연결 UI 없음. ID 필터에 범위 문법 `300-3FF` 추가(패널 로컬 `parseIdFilter`) | 1 채널 = 1 패널은 PCAN 다인스턴스와 일치 | Listen-only·비트레이트 설정 미지원 |
| 12 | CAN-TP | PCAN-View 기본판에 ISO-TP 보기 **없음**(1차 자료에서 확인 안 됨) | worker `isotp.rs`, 프레임 `IsoTpTag` 주석 존재; UI 미사용 | 원시 trace 불변. 이후 단계에서 별도 `재조립` 보기(endpoint·주소·실패 표시) | UDS `TP` 배지 = **TesterPresent**; UDS manager는 doip/sim, CAN ISO-TP worker와 별개 | PCAN 근거 없음 → 벤치마크 충실도 대상 아님, Loom 고유 설계 |

## 2. 패널 레이아웃 계획 (can.trace 패널 내부, 셸 변경 없음)

```
┌ [수신/송신] [트레이스] [채널]          ID 필터[        ] 출처▾  ‖일시정지 ┐  ← 보기 전환 + 기존 필터
├─ 수신 ─────────────────────────────────────────────────────────────────┤
│ CAN-ID   유형      Len  Data                      주기ms  횟수  최근   │  rxLatest (ID당 1행)
├─ 송신 ───────────────────────────────── [+ 새 메시지] [Space 송신] ────┤
│ ☐ CAN-ID 유형 Len Data          주기   횟수  트리거  소유자  메모      │  주기작업(daemon) + 수동행(로컬)
│ [ 텍스트 송신 줄 ……………………………… ] [위험: R1 sim] [보내기]              │  기존 sendLine 유지
├──────────────────────────────────────────────────────────────────────────┤
│ pt · sim · open · gen 3 │ 비트레이트 알 수 없음 │ 버스 상태 미지원 │ 관찰 유실 0 │ 주기 2 │
└──────────────────────────────────────────────────────────────────────────┘
트레이스 보기: 상단 통계줄(실시간│span│링%│Rx│Tx│gap│생략) + Time│ID│Rx/Tx│유형│Len│Data│출처│상태│note
```
수신/송신 분할 비율은 사용자 조절(패널 로컬 상태), 각 표 접기 가능 [V 접기 버튼].

## 3. 구현 단계 (데이터 모델 변경 순)

- **S1 (UI만, 신규 RPC 없음)**: `rxLatest` 모델 + 수신 표, 보기 전환, 트레이스 컬럼 재배열·통계 줄. 기존 ingest/링/생략 정직성 테스트 불변.
- **S2 (기존 `can.send` 경로)**: 새 메시지 폼 + 패널 로컬 수동 행 + Space 송신(표 포커스 시에만).
- **S3 (`can.periodic.list/start/update/stop` 실재, gateway `classify_periodic` 확인)**: 송신 표에 daemon 작업 행, 시작/정지, 소유자·lease 표시. STOP 시 `core.safe_state.run` → `can.periodic_stop`이 행에 반영되는지 확인.
- **S4 (backend 계약 필요, 이번 GO 범위 밖)**: 비트레이트·버스 상태/오류 계수·bus load, ISO-TP 재조립 보기.

## 4. 공유 의존성 (main 결정 필요, 직접 수정 안 함)

1. UI SDK/ActionGate에서 `can.status`, `can.periodic.*` 호출 허용·권한 분류 여부 — `ui/src/sdk`·`ui/src/net`에서 사용처 미발견(L). 미허용이면 S3·채널 줄의 status 부분은 대기.
2. `can.periodic` pause/resume RPC 형태(이벤트 enum에 `paused` 존재, 호출 방법 미확인) → Paused 체크는 확인 전 비활성.
3. Space 단축키가 셸 전역 단축키와 충돌하지 않는지.
4. 기본 보기를 트레이스→수신/송신으로 바꿀지(사용자 결정; 기본값 제안: CAN 서비스는 수신/송신, UDS 동반 CAN 패널은 트레이스 유지).

## 5. 수용 검사 (S1–S3)

1. sim `0x310` 연속 수신 시 수신 표 1행, 횟수 증가, 주기 ms가 trace Δt와 일치(±hub 해상도).
2. 같은 ID 표준/확장 분리, 다른 채널 패널끼리 섞이지 않음.
3. 링 퇴출·일시정지 중에도 수신 표 횟수 계속 증가 정의 명시, gap 발생 시 해당 행 gap 표시.
4. 폼 입력과 동일 텍스트 줄이 같은 `can.send` 파라미터를 만든다(단위 테스트).
5. 송신은 모두 dry-run 위험 배지·권한 거부 경로 유지; 권한 없음이면 버튼 비활성+사유.
6. 주기 행은 `can.periodic.list` 결과만 표시, 다른 owner 작업은 조작 불가, STOP 후 목록이 비거나 stopped.
7. 채널 줄에 `OK`·비트레이트 숫자·bus load가 backend 값 없이 나타나지 않는다(부정 테스트).
8. Bus Load/Error Generator 컨트롤 부재 확인. UDS `TP` 배지 의미 불변.

## 6. 남은 불확실성

PCAN 행 갱신·정렬 세부(ID 정렬 vs 도착순)와 Trigger 열 값 종류는 앱 Help(비공개 CHM) 미확인 [I].
버스 오류 이벤트 실물 동작은 하드웨어 미시험.

### MAIN integration correction
CAN service opens `view=rxtx`; UDS companion keeps Trace default. MAIN removed per-frame hardcoded R1 badge overwrite (daemon dry-run text persists, regression tested). The transmit editor now reserves space while open and scrolls within its parent; no controls are forced through overlaid notifications during QA. Cyclic transmit/status/ISO-TP UI remain deferred as in the owner handoff.
