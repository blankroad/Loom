# R4 최종 점검 — explore/plan.md (explore-opus)

작성: explore-opus, 2026-11-09
대상: `docs/plan/explore/plan.md`(766줄, R3 통합본). plan.md는 수정하지 않았다.
기준: `02-core-principle.md`(최우선), `explore/brief.md`, `00-brief.md` §3·§4, R1 두 편, R2 리뷰 두 편, r2b/r2c, 세 팀 `plan.md`(uart 1230줄, can 1071줄, uds 1114줄).
확인 방법: 본문 대조, 점수 공식 재계산(22행), TOML 블록 `tomllib` 파싱(`python3 -I`, 스크립트는 세션 scratchpad), 세 팀 plan grep 대조. §5.5의 장비 사실은 Sol 검증 범위라 사실 검증은 하지 않고 구조·원칙만 봤다.

**총평**: 핵심 루프·종단 간 시나리오·late viewer·쓰기 상태 단계·안전 기본값(pyOCD attach/auto_unlock, 기본 거부, deny list)은 R2 지적을 거의 다 반영했다. 남은 큰 문제는 세 가지다.
1. **R2 등급 의미가 세 팀과 다르다.** "세 팀과 같은 R0–R4"라고 쓰고 있지만, 탐색 R2는 자동이고 UART/UDS R2는 확인이 필요하다.
2. **§4 장비 평가표 규칙 3과 스코프 상위 포함이 모순이다.**
3. **00-brief §4의 절 몇 개(HW/OS 매트릭스, 프로세스 구성도, 마일스톤 표, 병렬 작업 분해)가 빠졌다.**

TOML 두 블록은 모두 파싱된다. 점수는 3행만 틀렸다.

---

## 1. 반드시 고칠 것 (P0 / P1)

### P0-1. 위험 등급 R2의 의미가 세 팀과 다르다 — "같은 척도" 주장이 사실과 다름
- **위치**: §5 공통 전제 1번째 줄, §6.3 게이트 줄("R0–R2 자동(UDS식 plan 토큰 등 팀 규칙 준용)"), §6.5 첫 항목, D7, 각 §5.x i 표의 R2 행.
- **문제**: 세 팀 plan의 R2는 다음과 같다.
  - UART: "R2 교란(리셋, break, DTR/RTS, reboot 문자열)" — **세션 첫 1회 사용자 확인**(uart §안전 표, 171·789행).
  - UDS: "R2 쓰기·삭제·리셋" — **확인 토큰 또는 뷰어 Y**(uds 777행).
  - CAN: "R2 단발" — `tx=allow`면 허용, `confirm`이면 승인(can 730행).
  
  탐색 plan은 R2를 **자동**으로 쓴다(PSU `set` R2 자동, debug RAM 쓰기 R2, AWG on R2, DLT 광역 레벨 R2 + 배너). 시나리오 러너가 여러 도메인을 섞어 "사전 위험 요약"을 낼 때 같은 R2가 도메인마다 다른 행동을 하게 된다. 셸은 UART InputArbiter를 재사용한다고 하면서 위험 패턴 등급도 UART와 다르다(`reboot`: UART R2, 탐색 R3).
- **수정 문장(§5 공통 전제 첫 항목 교체)**:
  > 위험 등급은 세 팀과 같은 R0–R4 척도를 쓴다. **R2의 기본 동작은 "확인 필요(세션 범위 승인 허용)"이고, `env=sim`이거나 벤치 프로파일이 해당 동작을 `allow`로 지정한 경우에만 자동**이다(UART `R2="allow"`, CAN `tx=allow|confirm`과 같은 방식). R3는 뷰어 1회 승인, R4는 기본 거부다. 영구 금지는 deny list로 둔다.
- **추가 수정**:
  - §0.1과 §5.1 b는 `set`을 자동으로 보여 준다. 그 근거로 "(프로파일 `psu.set = "allow"` 또는 env=sim)"을 덧붙인다.
  - §5.3 i의 셸 위험 패턴은 "UART §안전 표(R2 교란/R3 위험/R4 비가역)의 패턴 목록을 그대로 공유, 도메인별 재정의 금지"로 바꾼다.

### P1-2. 벤치 프로파일 한계를 프로젝트 `loom.toml`에만 두면 에이전트가 파일을 고칠 수 있다
- **위치**: §6.5 벤치 프로파일, D15, CR-X7.
- **문제**: 에이전트는 저장소 파일을 쓸 수 있다. "변경 감지 시 사용자 확인 후 반영"은 완화책이지만, UART는 이미 **사용자 정책 파일 `~/.loom/config/uart-policy.toml` + 프로젝트 `loom.toml`은 더 엄격하게만**이라는 2층 모델을 정했다(uart 1016행). 탐색만 1층이면 같은 데몬 안에서 규칙이 갈린다.
- **수정 문장(§6.5 벤치 프로파일 항목 끝에 추가)**:
  > 한계를 **완화**하는 값(vmax/imax 상향, `agent_power_on=true`, `allow` 지정, `write_allow` 확대)은 사용자 정책 파일(`~/.loom/config/` 아래, UART와 같은 위치)에서만 유효하다. 프로젝트 `loom.toml`의 `[bench]`·도메인 섹션은 **더 엄격하게만** 바꿀 수 있다(UART 2층 모델과 동일).

### P1-3. 규칙 3("△ 2개 이상 상위 불가")과 스코프(○○△△)의 상위 포함이 모순이다
- **위치**: §2.3 규칙 3·4, §4 5' 행, §5.5, D3.
- **문제**: 기본 추천 세트 B의 스코프(DHO804/SDS804X HD, AWG 없음)는 표에서 ○○△△다. 규칙 3을 적용하면 상위 추천이 불가하다. 그런데 §2.1의 실시간 ○ 기준은 "도메인 목표 지연(≤ 1 s) 안"이고 △는 "갱신 ≥ 1 s 간격"이다. 스코프 목표(1–5 Hz, 트리거→화면 ≤ 1 s)는 **자기 기준으로 ○**다. 판정과 기준이 서로 맞지 않는다.
- **수정 문장(§4 5' 행)**: 루프를 `○○△○ (AWG 모델 ○○○○)`, 총점을 **82 (AWG 86)**로 고친다(재계산: 24+26.25+6+8+3+5+10 = 82.25; AWG는 L=8로 24+30+6+8+3+5+10 = 86). 비고에 "실시간 ○는 1–5 Hz 단발 캡처 반복 기준, M0 실측 전 잠정"을 단다.
  - 대안: 판정을 그대로 두려면 §2.3에 규칙 8 "관측기 도메인은 교차 자극 조합(§5.5 a (c))으로 판정한다"를 명시한다. 이 경우 5위는 '조합' 단위 추천임을 §1 표에 적는다.

### P1-4. LA "원시 무손실 기록 + 라이브 디코드"를 sigrok-cli 한 번으로 할 수 없다(r2c F4 미반영)
- **위치**: §5.5 c(연속 스트리밍 행 "원시는 `.sr` 무손실"), §5.5 d `la stream --decode`, §5.5 h ⑤, §0 표 5행.
- **문제**: sigrok-cli 0.7.2는 `-P`(디코더)를 켜면 일반 출력 모듈 경로를 건너뛴다(r2c F4, `session.c` 소스 근거). 지금 설계대로면 디코드 중에 원시 기록이 비거나, 같은 USB 장비를 프로세스 둘이 열게 된다.
- **수정 문장(§5.5 c 표 아래에 추가)**:
  > LA 수집은 **장비당 sigrok-cli 프로세스 1개**만 소유한다(`-O binary`를 stdout으로). 데몬이 이 스트림을 (1) 원시 기록기(메타데이터 포함, 종료 후 `.sr` 패키징), (2) 디코더 하위 프로세스(`-I binary` stdin, `-P … -A`), (3) UI 요약으로 분기한다. **M0 gate**: 캡처 종료 전 주석 수신, 원시 샘플 수 = 생성 수, 디코더 정지 중에도 기록 지속, TTL 취소 후 정리·gap 표시. 디지털 파형 요약은 LTTB가 아니라 **전이 보존 요약**(toggle density, 원본 sample span)을 쓴다.

### P1-5. AD3를 두 도메인에 등록하면 장비 이중 open이 생길 수 있다
- **위치**: §5.5 구조 3번째 줄 "두 도메인에 백엔드(`dwf`)로 등록", §6.2 extras `loom[la-dwf]`.
- **수정 문장**:
  > 하나의 물리 장비를 여러 도메인이 쓰면 `ResourceIdentity`로 **공유 owner 워커 1개**를 두고, `loom scope`·`loom la`·출력(AWG/패턴)은 그 owner의 capability를 빌린다. 두 번째 plugin의 `open`은 새 연결이 아니라 attach다. 쓰기 lease는 장비 단위로 직렬화한다.

### P1-6. 시나리오 스키마가 UDS와 "정렬"됐다고 했지만 실제로는 다르다
- **위치**: §7.1 "스키마(UDS `loom_scenario: 1`과 동사 체계 정렬)", YAML 예시, 개입 UX의 `r` 재시도.
- **문제**:
  - UDS(uds §6.5) 단계 형식은 `- session: extended`(동사 키)이고, 오류 처리는 `on_error: stop`이다. `finally`는 **R0–R1만**, **abort 시 실행 안 함**이다.
  - 탐색은 `call:/wait_for:` 형식이고, 오류 처리는 `on_fail: pause|abort|continue`다. `finally`는 abort 시에도 실행하며 재승인을 거친다.
  - 또 §7.1은 "재시도는 후속"이라고 해 놓고 run 패널에 `r 재시도`를 둔다.
- **수정 문장**:
  > core 러너 스키마는 `loom_scenario: 1`을 확장한다. 키 이름은 UDS를 따르고(`on_error`), 단계는 `- <domain>.<verb>: {args}` 형식으로 UDS의 동사 키 단계를 그대로 포함한다. `finally`의 등급 상한과 abort 시 실행 여부는 **메인 결정 요청(CR-X6에 추가)**으로 둔다. 그 전까지 탐색 기본값은 UDS와 같다(R0–R1, abort 시 skip). `r`은 "사용자가 누르는 수동 재시도(같은 epoch·승인 재검사)"로 한정하고, 자동 재시도는 후속이라고 적는다.

### P1-7. CR-X5("UART Transport를 core 공개 API로")가 UART의 안전 설계와 정면 충돌한다
- **위치**: §5.3 j("uart plan §437"), §10 CR-X5, D-1, §11.
- **문제**: UART는 "포트를 여는 코드 경로는 데몬 하나뿐, `loom.uart`는 Transport를 공개 API로 내보내지 않는다"를 **우회 방지 원칙**으로 삼는다(uart 22·439행). 탐색에 필요한 것은 포트 open 경로가 아니라 콘솔 엔진(버퍼·커서·expect·InputArbiter·위젯)이다.
- **수정 문장(CR-X5 교체)**:
  > UART 콘솔 엔진(버퍼·커서·expect·InputArbiter)과 TUI 콘솔 위젯을 core 재사용 컴포넌트로 분리하고, 전송은 **데몬 안에서 플러그인 Backend로만 등록**한다(외부에서 직접 open하는 공개 Transport API는 요청하지 않음 — UART 우회 금지 원칙 유지).

### P1-8. 관측 계측 M1이 하나로 묶여 있는데 착수 단계는 둘로 나뉜다
- **위치**: §5.5 h(①–⑤ 한 M1) vs §8 E3(스코프+IO)·E6(LA).
- **수정 문장(§5.5 h 교체)**:
  > **M1-scope (E3)**: ①–④. 의존: PSU M1, SimScope.
  > **M1-la (E6)**: ⑤ + P1-4의 M0 gate. 의존: SimLa, UART sim(또는 SimLa 단독 자극 모드), sigrok-cli 설치 CI.
  > 각각 C1–C5 공통 수용 기준을 따른다.

### P1-9. 00-brief §4에서 빠진 절 — 구체 제안
| 절 | 현재 | 넣을 내용(문장 제안) |
|---|---|---|
| 8 HW/OS 지원 매트릭스 | O 점수만 있음 | 새 §5.6 표: 행 = 백엔드(pyvisa LAN, pyvisa USBTMC, NI-VISA, dlt TCP, asyncssh, pyOCD CMSIS-DAP/ST-Link, J-Link SDK, TRACE32 RCL, hidapi 릴레이, Modbus RTU, sigrok-cli(Homebrew/nightly/Windows 설치본), dwf runtime). 열 = macOS arm64 / Windows x64 / Linux / 필요한 외부 설치 / 검증 단계(계획·import·sim 왕복·HW). 예: "J-Link SDK — macOS ○(SEGGER 패키지), Windows ○, Linux ○, 외부 설치 필요, 검증 단계 import(pylink 1.7.0)" |
| 3 아키텍처(프로세스 구성도) | 쓰기 파이프라인 텍스트만 | §6 앞에 ASCII 한 장: `loomd` 게이트웨이(psu/io/dlt/shell 모듈) + 워커(debug: 프로브 SDK, la: sigrok-cli 하위 프로세스 소유, dwf 공유 owner) + 클라이언트(CLI agent, TUI/웹 viewer) + 기록기(세션 디렉터리). 데이터 흐름 화살표는 RPC→게이트→워커→장비, 장비→워커→기록→팬아웃 |
| 6 데이터/기록 포맷 | 도메인별로 흩어짐 | 표 하나: PSU `events.jsonl`(meas/cmd), DLT `wire.bin`+청크 인덱스 → export `.dlt`, 셸 콘솔 raw + 줄 인덱스(UART 포맷 재사용), debug watch JSONL + RTT raw, LA `binary`+meta → `.sr`, 스코프 `npz`/CSV artifact(sha256), run 로그·audit. 모두 `session_id/epoch/seq` 공통 |
| 9 시뮬레이션·테스트(CI) | 도메인별 sim은 있음 | CI 매트릭스: Linux/macOS/Windows × {순수 Python sim 전부(필수)}, Linux에만 {sigrok-cli demo, 네이티브 dlt-daemon 통합}, 수동/야간 {QEMU/Renode, 실HW 1종}. 지연 지표(render ack p95)는 nightly 기록 |
| 11 마일스톤 표 | M1만 도메인별, M0 일부 | 표: 도메인 × {M0 gate, M1 수용, M2 실장비 1종}. 예: DLT M0 = 네이티브 set-level 왕복, M1 = §5.2 h, M2 = 사용자 ECU(verbose) |
| 12 작업 분해(병렬 단위) | E-단계 + 담당만 | E1a를 병렬 단위로 쪼갠다. ① SimPsu+SimScpiServer(Sol) ② VisaBackend+드라이버 프로파일(Sol) ③ psu CLI·출력 상한(Opus) ④ gauge/state_lamps PanelSpec(Opus, viz 의존 D-2) ⑤ 벤치 프로파일 스키마·2층 정책(Opus, P1-2) ⑥ E2E 테스트(C1–C5). 의존 화살표 ①②→⑥, ④→⑥. E2/E3/E5–E7도 같은 형식으로 3–6개씩 |
| 2 사용 시나리오(에이전트 3 + 사용자 3) | 도메인별 E2E 5개(혼합) | 짧은 목록 추가. 에이전트: (a) 콜드 부팅 이상 전류 원인 찾기 (b) 슬립 전류 측정 시나리오 (c) 부팅 정지 단계를 watch+RTT로 좁히기. 사용자: (a) 뷰어에서 비상 OFF (b) DLT 컨텍스트 레벨을 직접 내림 (c) 스코프 트리거 재설정 후 Single |

---

## 2. 권장 보완 (P2)

1. **점수 오류 3건(§4)** — 공식 `30V/5+30L/8+10E/5+10M/5+5H/5+5O/5+10R/5`로 재계산했다.
   - SOME/IP 행(V3 L8 E2 M3 H3 O3 R3) = **70**인데 66으로 적혀 있다(§3 비범위 줄의 "66점"도 함께 고칠 것).
   - Bus Pirate/CH341(V2–3, M1–2) = **62–70**인데 "65–69"로 적혀 있다.
   - CCP(V2 L8 E1 M1 H3 O3 R3) = **58**인데 "~55"로 적혀 있다.
   - 나머지 19행은 반올림 범위에서 일치한다.
2. **참조 표기**: §5.3 j "uart plan §437"은 줄 번호다. "uart plan §3(직접 포트 열기 경로 없음)"처럼 절로 바꾼다. "5.5.3 저가 조합 추천"은 5.5에 a–j만 있어 번호가 튄다. "5.5 k. 저가 조합"으로 바꾼다.
3. **`loomd` 단일 게이트웨이를 '결정'으로 서술**(§6 첫 문단): UART는 도메인별 프로세스(`loomd-uart`)를 권장하며 메인 결정을 따른다고 했다(uart 260행). 문장을 "메인 결정 대기 — 탐색 도메인은 게이트웨이 모듈/워커 어느 배치든 동작"으로 바꾼다.
4. **RPC 메서드 이름**: CAN은 `<domain>.<noun>.<verb>`(can 383행), UDS도 `uds.approval.respond`처럼 3단이다. 탐색은 `psu.set`, `psu.out`, `dlt.marker`, `uds.dtc_list`(2단)다. §6.2 예시와 §7.1 YAML을 `psu.output.set`, `psu.setpoint.set`, `dlt.marker.add`처럼 3단으로 맞추거나, CR-X9에 "메서드 이름 규칙 통일"을 추가한다.
5. **이벤트 조회 상한**: `debug events`, `scope|la events`에는 `--since`만 있다. §6.7에 "모든 `events`는 `--tail`(기본 50)과 바이트 상한이 함께 적용"을 명시한다(R2-sol §3.3).
6. **착수 순서 vs 순위**: §1 표 순위(Shell 3·Debug 4)와 §8 착수(E3 스코프가 E5 SSH보다 먼저)가 다르다. §8 아래에 "순위는 가치, 착수는 비용·의존성(PSU 백엔드 재사용) 기준"이라고 한 줄 넣는다.
7. **viz 요구(D-2) 보강**: 디지털 파형의 전이 보존 요약(LTTB 금지), 캡처 시각과 다른 도메인 RPC 시각 사이 오차 라벨을 넣는다(r2c §3.3: PSU RPC와 스코프 트리거 시각의 정밀 동기화는 보증하지 않음).
8. **IO 레이트 정의**: "채널당 동작 ≤ 2회/s"와 "펄스 최소폭"이 동작 횟수 제한인지 전기적 edge 제한인지 정의한다(R2-sol §6).
9. **화면 배치**: herdr 패인 배치(에이전트 | 멀티 패널 뷰어)와 확인 배너 위치가 plan에 없고 r1 참조뿐이다. 메인 통합 때 plan.md만 남으므로 §6.6에 ASCII 한 장(r1-opus §5.15)을 옮긴다.
10. **r2c 사실 정정 반영 대기**(사실 검증은 Sol 소관이라 항목만 적음): §5.5.3 참고 줄의 "Hantek 6022BL … sigrok은 LA 모드만"(r2c F1), 세트 C "macOS는 확인 필요"·Rosetta 서술(r2c F2: dwf runtime arm64 지원), `demo` 채널 수(r2c F3)를 고친다.
11. **스킬 경로**: 탐색은 UART 방식(`skills/` 원본 + `.claude/skills`·`.agents/skills` 링크)을 따른다. UDS는 `.claude/skills/loom-uds/` 직접이고, CAN은 `.agents/skills`를 언급한다. 세 팀이 다르므로 CR-X9 옆에 "스킬 원본 경로 통일"을 추가한다.

---

## 3. 체크리스트 판정표

### 3.1 02-core-principle 6개 × 추천 5개 (내용 기준)

| 항목 | PSU(+IO) | DLT | Shell | Debug | 관측 계측 |
|---|---|---|---|---|---|
| 1 핵심 루프 표 | 충족 §0 | 충족 | 충족 | 충족 | 충족(단 §4 판정 모순 P1-3) |
| 2 E2E(사용자 개입 → 에이전트가 origin=user 읽음) | 충족 §0.1 | 충족 §5.2 b | 충족 §5.3 b | 충족 §5.4 b | 충족 §5.5 b |
| 3 지연 목표 근거 + 고부하 정직성 | 충족: 사건 지연 = poll+RTT+render, gap, age | 충족: wire.bin 무손실, gap 출처, 필터≠손실 | 충족(UART 준용, 64 KiB 상한) | 충족: coherent·HALTED·RTT unknown | **부분**: 원시+디코드 동시 기록 불가(P1-4) |
| 4 M1 = sim 걷는 뼈대(현실성) | 충족(IO는 E3로 분리) | 충족(+네이티브 M0 gate) | **부분**: 콘솔 엔진 재사용 전제가 UART 원칙과 충돌(P1-7) | 충족(SimTarget, 범위는 넓지만 sim이라 현실적) | **부분**: M1 하나에 두 단계 의존(P1-8) |
| 5 화면이 에이전트 세션과 분리 안 됨(late viewer) | 충족 h-3, C2 | 충족 C2 | 충족(exec 탭 자동) | 충족(GUI도 데몬 경유) | 충족. 단 AD3 이중 owner 방지 필요(P1-5) |
| 6 비기여 기능 후순위/비범위 | 충족 | 충족 | 충족 | 충족 | 충족 |
| (안전, 원칙 외 공통) | **부분**: R2 의미(P0-1), 프로파일 2층(P1-2) | 부분(P0-1) | 부분(P0-1 패턴표) | 충족(가장 엄격) | 부분(P0-1) |

### 3.2 explore/brief.md 산출물 5개
| 산출물 | 판정 | 비고 |
|---|---|---|
| 1 핵심 루프 기준표 | 충족 §2.1 | 검증 단계 열 추가는 좋음 |
| 2 후보 평가표(가치·루프·난이도·라이브러리 실측·HW·macOS/Windows·재사용) | **부분** | macOS/Windows가 O 숫자로만 있음 → P1-9 HW/OS 매트릭스로 보완. 점수 3건 오류(P2-1) |
| 3 상위 3–5개 a–j | 충족 | 5개 모두 a–j 있음(a는 §0 참조) |
| 4 플러그인 구조 | 충족 §6 | P1-5(공유 owner), P2-4(메서드 이름) |
| 5 착수 순서·질문 | 충족 §8, §9 | P2-6 설명 한 줄 |

### 3.3 00-brief §4 15개 절
| # | 절 | 판정 | 위치 / 조치 |
|---|---|---|---|
| 1 | 범위/비범위 | 충족 | §3 |
| 2 | 사용 시나리오(에이전트·사용자 각 3+) | 부분 | P1-9 |
| 3 | 아키텍처(구성도·데이터 흐름) | 부분 | P1-9 |
| 4 | CLI/API | 충족 | §5.x d, §6.2 |
| 5 | 뷰어 UX | 부분 | §5.x e는 텍스트, 배치 없음 → P2-9 |
| 6 | 데이터/기록 포맷 | 부분 | P1-9 |
| 7 | 라이브러리 근거 | 충족 | §4, §5.x f |
| 8 | HW/OS 매트릭스 | **누락** | P1-9 |
| 9 | 시뮬레이션·테스트 | 부분 | CI 매트릭스 P1-9 |
| 10 | 안전 | 부분 | P0-1, P1-2 |
| 11 | 마일스톤 | 부분 | 표 P1-9 |
| 12 | 작업 분해(병렬·담당·의존) | 부분 | E-단계만 → P1-9 |
| 13 | 리스크 | 충족 | §11 |
| 14 | 질문 | 충족 | §9(Q1–Q12) |
| 15 | 공통 계약 변경/의존성 | 충족 | §10(P1-7, P2-4·11 추가) |

### 3.4 R2·R2c P0/P1 반영 여부
| 지적 | 반영 |
|---|---|
| R2-sol F1 pyOCD 기본값(P0) | 반영 §4.1, §5.4 |
| F2 PyLink 1.7.0, F3 wire.bin/.dlt, F5 PSU 사건 지연, F6 IO 4상태, F7 라이선스 단정, F9 exec 채널(P1) | 모두 반영 |
| §3.2 TX 상태 단계(P0), §3.5 승인 바인딩·인터록 lease·기본 거부(P0) | 반영 §0, §6.3, §6.5 |
| §3.3 경계·lease(P0/P1) | 대부분 반영. events 상한 누락(P2-5) |
| §3.4 gap 출처·late attach(P1) | 반영 §5.2 c, C2, C5 |
| §3.6 origin enum·pause 의미·finally 재승인(P1) | 반영. 단 재시도·finally가 UDS와 불일치(P1-6) |
| R2-opus T4 뷰어만 승인, T6 halt R3, T11 SimTarget, T12 DLT sim, T13 SimWorld, T17 인터록, T20 TRACE32 | 반영 |
| R2-opus T16 벤치 프로파일 | 반영했으나 2층 정책 누락(P1-2) |
| R2-opus §3.1 화면 배치·herdr | 미반영(P2-9) |
| r2c F4 원시+디코드 분기(구조) | **미반영(P1-4)** |
| r2c AD3 단일 공유 owner(구조) | **미반영(P1-5)** |
| r2c F1/F2/F3(사실) | 반영 대기(P2-10, Sol 소관) |

---

## 4. 세 팀과의 충돌 목록

| # | 항목 | 탐색 plan | 상대 팀 서술(위치) | 조치 |
|---|---|---|---|---|
| X1 | R2 의미 | 자동 | UART 세션 1회 확인(789행), UDS 토큰/뷰어 Y(777행), CAN allow/confirm(730행) | P0-1 |
| X2 | 셸 위험 패턴 등급 | reboot 등 R3 | UART reboot 문자열 R2, dd/mkfs/fw_setenv/flash_erase R3, efuse R4(1004–1006행) | UART 표 공유(P0-1) |
| X3 | 정책 파일 층 | 프로젝트 `loom.toml` 1층 | UART 사용자 정책 + 프로젝트는 강화만(1016행), CAN `loom.toml`(253행) | P1-2 |
| X4 | Transport 공개 | CR-X5 공개 요청 | UART "Transport 공개 API 없음"(22·439행) | P1-7 |
| X5 | 시나리오 스키마 | `call:`/`on_fail`/finally abort 시 실행 | UDS 동사 키/`on_error`/finally R0–R1·abort 시 skip(uds §6.5) | P1-6 |
| X6 | RPC 메서드 이름 | 2단(`psu.set`) | CAN/UDS 3단(`<domain>.<noun>.<verb>`) | P2-4 |
| X7 | 데몬 배치 | 단일 `loomd` + 워커를 "결정"으로 서술 | UART `loomd-uart` 권장·메인 결정 대기(260행), CAN·UDS `loomd` | P2-3 |
| X8 | 종료 코드 | 의미만 요청(CR-X9) | CAN 0–7 표 확정(331행: 3 timeout, 4 정책, 5 승인/NO_APPROVER, 6 데몬 없음, 7 장치) | 충돌 아님. CAN 표를 기본으로 삼고 `interrupted-by-user`, `outcome-unknown` 코드만 추가 요청하면 단순 |
| X9 | 클라이언트 변수 | CR-X9로 통일 요청 | UART/CAN `LOOM_CLIENT`, UDS `LOOM_CLIENT_ID` | 이미 CR-X9에 있음 — 유지 |
| X10 | gap 이름 | 통일 요청 | CAN `display.gap`, UDS `uds.gap` | 이미 CR-X2 — 유지 |
| X11 | 스킬 원본 경로 | UART 방식 | UDS `.claude/skills/loom-uds/` 직접, CAN `.agents/skills` 언급 | P2-11 |
| X12 | origin/replay 필드 | `actor_id/job_id/original_origin` | CAN `purpose/actor_id/request_origin/original_origin`(487–492행), UDS `job_id/actor_id` | 대체로 일치. 탐색도 CAN의 `purpose`(replay, sim 등)를 함께 쓰겠다고 한 줄 추가 권장 |
| X13 | 확인 응답 주체 | 뷰어만 | UDS R2는 CLI `loom uds confirm <tok>`(채팅 토큰) 허용, R3는 뷰어만(777·832행) | 탐색 R2 기본을 "확인"으로 바꿀 때(P0-1) UDS처럼 토큰 경로를 허용할지 결정 필요 — 메인 결정 요청에 포함 |

<!-- END R4 -->
