> **v2.0 공지 (2026-11-10)**: 이 브리프의 §3(Python 모노레포·`loom` 패키지 등)은 `00-master-plan.md` §2.3 **C1–C8**이 대체한다 — 코어·장치 경로는 Rust, Python은 대안이 없을 때만(CANoe COM 등), UI는 1급 제품, 통합 테스트(TC·자연어) 신규. 계획서 15절 구성과 규칙(§4·§5)은 그대로.

# Loom — 공통 브리프 (모든 팀 필독)

> **최우선 원칙: 에이전트가 장비에 직접 접근해 읽고 쓰고, 사용자에게 실시간으로 보여준다.** 상세와 체크리스트는 `docs/plan/02-core-principle.md` — 이 브리프보다 우선한다.

작성: 메인 오케스트레이터 (workspace w1 "Loom"), 2026-11-09
상태: **계획 단계**. 구현 코드는 만들지 않는다 (아래 "스파이크" 예외만 허용).

## 1. 사용자 요구사항 (원문)

> 1. COM포트 기반 UART를 에이전트가 접근해서 출력을 읽고 입력을 넣을수 있음. 그리고 유저가 그것을 볼 수 있음. 터미널 수준으로
> 2. python 기반을 벡터 케노이 혹은 peak CAN 툴 접근 가능한 것들이 있음. 이것도 역시 에이전트가 접근해서 읽고 값을 쓸수도 있도록. 그리고 각종 편의 툴들도 만들어서 유저가 플랏등도 볼 수 있도록.. 그때그때 이런거 안만들게 왠만한건 데이터만 받으면 렌더링이 되도록. 케노이 수준으로
> 3. DoIP 및 CAN-TP 기반 UDS. 이것도 역시 에이전트가 이더넷이나 캔 접근해서 유저가 원하는 행동을 할 수 있도록..

해석:
- 결과물은 **에이전트 스킬** (Claude Code `SKILL.md` 기준, Codex에서도 쓸 수 있으면 좋음) + 그 스킬이 호출하는 **Python 도구**.
- 공통 원칙: **에이전트가 조작하고, 사용자는 같은 장치를 동시에 실시간으로 본다.** 에이전트만 보는 숨은 세션이 되면 안 된다.
- "그때그때 안 만들게": 매번 일회성 스크립트/플롯을 짜지 않도록, 데이터만 넘기면 렌더링되는 범용 뷰어/도구를 미리 갖춘다.
- 목표 수준: UART = 터미널 프로그램(TeraTerm/minicom) 수준, CAN = CANoe/CANalyzer 수준(트레이스, DBC 신호 해석, 플롯, 송신), UDS = 진단 테스터 수준.

## 2. 팀 구성

| Workspace | 컴포넌트 | 리드(서브 오케스트레이터) | 워커 |
|---|---|---|---|
| UART | ① UART/COM | `uart-lead` (Opus 5.5) | `uart-opus` (Opus 5.5), `uart-sol` (Codex Sol 6.1) |
| CAN | ② CAN + 시각화 | `can-lead` | `can-opus`, `can-sol` |
| UDS | ③ UDS (DoIP, CAN-TP) | `uds-lead` | `uds-opus`, `uds-sol` |
| EXPLORE | ④ 추가 컴포넌트 탐색 | `explore-lead` | `explore-opus`, `explore-sol` |

메인 오케스트레이터(w1)가 세 계획을 통합해 `docs/plan/00-master-plan.md`를 만든다.

## 3. 공통 아키텍처 계약 (초안 — 변경 요청 가능)

세 팀이 따로 설계하면 충돌하는 부분만 먼저 정해 둔다. 바꾸고 싶으면 계획서의 "공통 계약 변경 요청" 절에 근거와 함께 적는다. 메인이 통합할 때 결정한다.

1. **저장소/언어**: 하나의 모노레포 `Loom/`, Python ≥ 3.11, 패키지 `loom` (`loom.core`, `loom.uart`, `loom.can`, `loom.uds`, `loom.viz`), `uv`로 관리.
2. **장치 소유권**: 물리 장치(COM 포트, CAN 채널, DoIP 소켓)는 **하나의 상주 프로세스(데몬)** 만 연다. 에이전트 CLI, 사용자 뷰어는 모두 그 데몬의 클라이언트다. (COM 포트는 Windows에서 배타적, Vector/PCAN 채널도 앱 단위 점유 이슈가 있음.)
3. **IPC**: 데몬 ↔ 클라이언트는 로컬 전용(127.0.0.1 또는 unix socket/named pipe). 요청/응답 + 이벤트 스트림 구독 두 가지가 필요. 구체 프로토콜(JSON-RPC over WebSocket 등)은 팀 제안을 받아 메인이 확정. 웹 뷰어가 직접 붙을 수 있으면 유리.
4. **에이전트 인터페이스**: `loom <domain> <verb> ...` CLI, 비대화형, 기본 출력은 사람이 읽기 좋게, `--json` 시 기계 판독. 무한 스트림 금지 — 항상 `--timeout`, `--tail N`, 필터, `expect/wait-for` 같은 **경계 있는** 동작. 출력 크기 상한(에이전트 컨텍스트 보호).
5. **사용자 가시성**: 에이전트가 보낸 입력/프레임/요청은 사용자 뷰어에 **출처 표시(agent/user/device)** 와 함께 보인다. 터미널 뷰어는 herdr 패인에서 실행 가능해야 한다.
6. **기록**: 모든 세션은 자동 기록. 공통 이벤트 레코드 필드: `ts`(monotonic + wall clock), `domain`, `channel`, `dir`(rx/tx), `origin`(agent/user/device/script), `payload`. 도메인별 표준 포맷 export(CAN: BLF/ASC/MF4, UART: raw + 타임스탬프 로그)도 지원.
7. **범용 시각화 `loom.viz`**: CAN 팀이 소유. "데이터만 주면 렌더링" — 파일(CSV/JSONL/Parquet/BLF/ASC/MF4) 또는 라이브 스트림을 받아 시계열 플롯, 트레이스 표, 상태 표시 등을 그린다. UART/UDS 팀은 자기 도메인이 viz에 바라는 요구사항을 계획서에 적는다 (예: UART 로그에서 숫자 추출해 플롯, UDS DID 값 추이).
8. **CAN-TP 경계 (CAN ↔ UDS 핵심 결정)**: CAN-TP(ISO 15765-2)는 Flow Control을 ms 단위로 응답해야 하므로 IPC 너머에서 프레임 단위로 돌리면 타이밍이 깨질 수 있다. 기본안: **CAN 데몬이 ISO-TP 스택을 프로세스 안에서 호스팅**하고 `isotp channel open(tx_id, rx_id, addressing, params)` + PDU 송수신 API를 노출. UDS 클라이언트는 그 위에서 동작. 두 팀은 `docs/plan/_xteam/`에 이 인터페이스를 합의해서 적는다.
9. **플랫폼**: 사용자 개발 머신은 macOS. 실제 HW 사용은 Windows 가능성이 높음(Vector XL Driver는 Windows 전용, PCAN은 Windows/Linux, macOS는 PCBUSB/MacCAN). 지원 매트릭스를 명시하고, **HW 없이 전부 돌릴 수 있는 시뮬레이션 백엔드**(가상 시리얼/pty 루프백, python-can virtual bus, 가상 ECU/DoIP 엔티티)를 필수로 계획한다.
10. **안전**: 장치에 쓰는 동작(UART 입력, CAN 송신, 특히 UDS write/reset/flash/routine/security access)은 스킬 레벨 가드레일이 필요. 위험 등급 정의, 사용자 확인 규칙, dry-run, 송신 레이트 제한 등을 계획에 포함.
11. **스킬 패키징**: 컴포넌트마다 스킬 1개 이상 (`.claude/skills/loom-uart/`, `loom-can/`, `loom-uds/` 등). SKILL.md는 짧게, 상세는 references 파일로. Codex 호환 방안도 검토.

## 4. 각 팀 산출물

`docs/plan/<comp>/plan.md` — 아래 절을 모두 포함:

1. 범위 / 비범위
2. 사용 시나리오 (에이전트 관점, 사용자 관점 각각 3개 이상, 구체적으로)
3. 아키텍처 (프로세스 구성도, 데이터 흐름)
4. CLI / API 명세 초안 (명령, 인자, 출력 예시)
5. 사용자 뷰어 UX (터미널/웹, 화면 구성)
6. 데이터/기록 포맷
7. 라이브러리 선택과 근거 (버전, 라이선스, 플랫폼 지원, 유지보수 상태 — 가능한 한 실제로 확인한 근거)
8. HW/OS 지원 매트릭스
9. 시뮬레이션 & 테스트 전략 (HW 없는 CI 포함)
10. 안전 / 가드레일
11. 마일스톤 (M0, M1, … 각각 수용 기준)
12. 구현 단계 작업 분해 — 병렬 가능한 단위로, 각 작업에 추천 담당(Opus 워커 / Sol 워커)과 의존성 표시
13. 리스크
14. 사용자에게 물어볼 질문 (결정 안 되면 기본값을 제안하고 진행)
15. 공통 계약 변경 요청 / 타팀 의존성

## 5. 규칙

- 파일 쓰기는 `docs/plan/<자기 comp>/`와 `docs/plan/_xteam/`만. 다른 팀 폴더는 읽기만.
- 스파이크(실현 가능성 검증용 짧은 코드)는 `spikes/<comp>/` 안에서만, 계획 근거용으로만. 실제 구현 시작 금지.
- 전역 설치 금지 (pip install --user, brew 등). 필요하면 `spikes/<comp>/.venv`.
- 사용자 설정 파일(~/.claude, ~/.codex 등) 수정 금지.
- 완료 시 리드가 `docs/plan/<comp>/STATUS.md`에 `DONE`과 한 단락 요약을 쓴다. 막히면 `BLOCKED: <이유>`.
