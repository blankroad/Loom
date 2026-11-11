FIX NEEDED: 공통 계약의 본문·예시·시험표 잔여 불일치 정리 필요

검토: plan.md SHA-256 `4e1c9f7c5b30ed805ae209468db5dc246f5bf451ed23eb096836b67a8fe4974e`. 마스터 §2.1 A가 D보다 우선. 계획 정합성 검토이며 구현 시험은 수행하지 않았다. plan.md는 수정하지 않았다.
Y=관련 요구가 일치함, N=일부 반영됐지만 명시적 불일치가 남음. 아래 D 번호는 마스터 결정이며 UDS §16의 동명 번호와 구별한다.

## A1–A16 / D1–D14

| ID | 판정 | plan.md 위치 | 확인 / 한 줄 수정 문장 |
|---|---|---|---|
| A1 | Y | §3.3/3.4·U-15 | 요청형·TP/폴링 잡형, 단위별 latch·레이트 집행을 명시함. |
| A2 | Y | §0·§3.4·U-08 | core 파이프라인·훅 3개·UDS 직렬화·발행 후 응답이 일치함. |
| A3 | Y | §2.0·§5.2·§10.7 | STOP은 agent/script·잡 latch, 사용자 입력 유지·상태 유지 배지. |
| A4 | 해당 없음 | — | UART 공유 콘솔 엔진 소유 결정이며 UDS는 콘솔 구현을 소유하지 않음. |
| A5 | Y | §3.6·§5.3/5.5 | core 배치 40 ms 상한·적응형 flush·분할 snapshot을 사용함. |
| A6 | Y | §4.1/4.2·D15 | 조건 대기 timeout=3, ECU 요청 무응답=7 E_NO_RESPONSE로 구분됨. |
| A7 | Y | §2.0 t3·§4.1/4.2 | @last/seq:N·from_cursor·40행/공통 하드 상한이 일치함. |
| A8 | N | §10.2/10.4/10.6·§9.3 | “R2는 r2_path=auto면 즉시 실행하고, token_or_viewer/viewer일 때만 승인 객체를 발급한다”로 흐름·시험을 조건화한다. |
| A9 | Y | §10.2·D7 | 10분은 승인 객체 사용 기한, UDS R3 기본 scope once로 더 엄격함. |
| A10 | N | §10.2/10.6·Q2 | “vehicle의 화이트리스트 밖 대상은 R0도 거부하며 unknown_targets=R0는 bench에만 적용한다”로 통일한다. |
| A11 | N | §2.2 U3·§3.4·§9.3 ↔ §10.2/10.3 | “R4는 default_blocked 해제 후 매번 뷰어 승인, 0x2A/0x86·잘못된 raw는 immutable_deny”로 잔여 표기·시험을 통일한다. |
| A12 | N | §1.1·§9.3 ↔ §5.5·§11 | 기록 불능 시험·범위표도 “agent/script R1+만 중단, 사용자 입력 허용·stderr 감사”로 바꾼다. |
| A13 | Y | §3.6·§6.1 | display/record/source.gap 및 cause가 구분됨. |
| A14 | N | §3.4 ↔ §6.1 | codec=requested·정책 통과/queued=accepted로 상태 매핑을 맞추고 이벤트 fixture에 write_state를 넣는다. |
| A15 | Y | §6.5·§10.7·D16 | 정상/오류/agent cancel의 finally, 사용자 STOP의 수동 복구·인터록·finally_blocked를 반영함. |
| A16 | N | §15.2·U-13 | 현행 상태 설명의 isotp.open/cancel/recv를 can.isotp.*로 바꾸고 구버전 계약은 역사 기록으로 명시한다. |
| D1 | Y | §3.1/3.2 | gateway의 UDS 관리자·CAN worker·PDU RPC 경계가 일치함. |
| D2 | N | §0·§3.6·§4.4 ↔ D18 | D18의 “Python API=script”를 “hello agent role, origin=agent”로 고친다. |
| D3 | N | §6.1 | 확정 fixture의 필수 공통 필드·두 층 순번·문자열 uint64·ts.wall_ns/quality를 실제 표/JSON에도 반영한다. |
| D4 | N | §3.4·§6.1 | A14 우선 상태 전이를 표와 이벤트 예시에 동일하게 적용한다. |
| D5 | N | §4.4·§3.4 규칙3·§10.9 | Python 예시 exit-3→exit-5, QUARANTINE→E_QUARANTINE, 스킬 필드 경로→data/error.detail 하위로 고친다. |
| D6 | N | §10.2–10.6·§9.3 | A8/A10/A11/A12의 잔여 불일치와 R4 항상 거부 시험을 해소한다. |
| D7 | N | §6.2 | 정본 디렉터리의 can.blf를 CAN 저널 참조로 바꾸고 BLF는 export 산출물로 표시한다. 경로·30일/20 GiB는 반영됨. |
| D8 | Y | §3.1·§4.1/4.2 | 자동 기동·LOOM_NO_AUTOSTART·데몬 실패 exit 9가 일치함. |
| D9 | Y | §5·U-23 | Textual·CAN viz 플러그인·공통 키/상태 배지·no_viewer_attached를 반영함. |
| D10 | Y | §0·§3.4·U-01/08 | Registry 등록·core 쓰기 경로·UDS 훅의 소유 경계가 일치함. |
| D11 | N | §15.2 ↔ §11·U-13/24 | “남은 미결 1건”을 삭제하고 UDS 정책 위임 확정·K10/K14·can.isotp.*를 현행 계약으로 통일한다. |
| D12 | N | §6.5·§4.2 | YAML의 session/security/read-did 등 구문을 core 러너의 uds.* 단계 어댑터 문법으로 맞춘다(A15 finally는 반영됨). |
| D13 | Y | §10.9·U-12/25 | skills 원본·sync 복사·공통 loom 스킬·공통 ask 패턴을 반영함. |
| D14 | N | §8·§9.3·Q1/Q10 | Q10의 CANoe 동시 사용 “없음 가정”을 마스터의 “많음, M4 공존 검증 필수”로 바꾼다. Python 3.11·3 OS는 반영됨. |

## master-review (c) 1–11

| # | 판정 | 위치 | 확인 / 남은 보완 |
|---|---|---|---|
| 1 | N | §4.1/4.4·D15 | 공통 코드표는 교체됐지만 Python 확인 예시 exit-3이 남음. |
| 2 | Y | §2.0·§4.1 | 현행 LOOM_CLIENT·@last·seq:N 전환 완료. §15.1 C4는 “요청(원안)” 역사 기록임. |
| 3 | N | §4.1/4.3·§10.9 | JSON 봉투는 반영됐으나 스킬은 approval.channel·write_state·timing·detail을 구 봉투 위치로 참조함. |
| 4 | N | §6.1 | 공통 필드 설명은 추가됐으나 “확정안” envelope/JSON은 단일 seq·옛 ts·필드 누락 상태임. |
| 5 | N | §10·§2.2·§3.4·§9.3 | key R3·정책 2층·차단 계층은 반영됐으나 A8/A10/A11/A12 불일치가 남음. |
| 6 | N | §4.4·D18 | 본문은 Python API agent, 현행 결정 D18은 script로 충돌함. |
| 7 | Y | §3.1·§6.2 | 자동 기동·LOOM_NO_AUTOSTART·기록 경로·보존/용량 반영. BLF 정본 문제는 D7에 별도 기록함. |
| 8 | Y | §5.2·§4.1 | STOP·!·/·Space·no_viewer_attached를 반영함. |
| 9 | N | §6.5·U-22 | core 소유·finally·동적 값 이관은 반영, 예제 YAML의 uds.* 문법이 미반영. |
| 10 | Y | §10.9·U-12/25 | 원본/sync·공통 규칙 이관·Bash(loom * confirm:*)를 반영함. §10.2의 좁은 ask 예시도 같은 패턴으로 정리 권장. |
| 11 | Y | §12 | CORE 소유·UDS 훅·공통 지연 하네스·K10/K14 반영. U-ID 중복/미존재 의존은 발견하지 못함. |

## 남은 내부 모순과 최소 수정

- 종료 코드: §4.4 `exit-3 상당`만 확인 필요의 옛 번호다. §4.2 approval wait의 exit 3은 A6에 맞다. 현행 NRC=exit 2 표기·c-000…·uds.gap·r2_channel은 찾지 못했다.
- 스킬: §10.9의 approval.channel/token→data.approval.*, write_state/timing→data.*, detail.nrc_name→error.detail.nrc_name으로 맞춘다. §3.4 규칙3의 error.code도 E_QUARANTINE으로 통일한다.
- 정책: §10.4는 모든 R2에서 토큰 발급, §10.6 max_auto_grade=R1은 자동 허용 상한이라고 적는다. r2_path=auto의 우선순위/예외를 정의하고 §9.3의 “R2 토큰 없이 거부”는 비-auto 시험으로 한정한다.
- 등급/기록: §2.2 U3 “R4 금지”, §3.4 “0x2A/0x86은 R4”, §9.3 “R4 항상 거부·감사 불능 송신 중단”을 현행 차단 계층·A12의 주체/등급 조건으로 바꾼다.
- 이벤트: §6.1의 설명과 확정 fixture를 일치시키고 aggregate의 origin=— 등도 공통 enum/dir:null을 따르게 한다. §3.4 accepted→queued와 §6.1 queued=requested의 역행도 제거한다.
- 계약/이력: §15.1 원안의 LOOM_CLIENT_ID·exit 7과 §16 “기각안”의 옛 코드는 역사 기록으로 구별된다. §15.2 “남은 미결”과 D18은 현행 서술이므로 반드시 갱신한다.
- 일정: §11 M1b∥M2·DoIP 실HW 독립·U-26 예외는 일치한다. U-15의 “폴링 잡형 구현 M2”는 M3 폴링 서비스와 혼동되지 않게 공통 잡 기반 작업이라고 명시하면 된다(P2).

## 02 체크리스트

| # | 판정 | 근거 |
|---|---|---|
| 1 | PASS | §0 맨 앞 4요소 표·불변식 유지. |
| 2 | PASS | §2.0의 명령·응답·렌더 지연·사용자 takeover/송신 경로 유지. |
| 3 | PASS | §5.4/5.5의 p95/p99·gap/drop/LAG·기록 실패 표시 유지. |
| 4 | PASS | §11 M1은 sim 읽기/쓰기·실제 TUI·사용자 입력을 함께 수용함. |
| 5 | PASS | 데몬 소유·공통 스트림·snapshot/live·숨은 연결 금지 유지. |
| 6 | PASS | §1/§11의 웹·ODX·시나리오·플래시 후순위 유지. |

## 재확인

FIX NEEDED: viewer 승인 대기 식별자와 이벤트 uint64 예시 불일치가 남음

검토: plan.md SHA-256 `f5f523033e8bbcb4b37f20e29c2ce2f60ea29c61e6d7f825bbec31fb2632c5e6`. 앞선 지적과 이번 변경만 검토했으며 plan.md는 수정하지 않았다.
- 수정 확인: A8의 auto 우선순위·비-auto 승인 시험, A11 차단 계층, A12 기록 불능 범위, A14 queued=accepted, A16/D11 현행 CAN 계약, D2/D18 Python origin, D7 정본 저널, D12 uds.* YAML, D14 CANoe 기본값은 반영됐다.
- D5/(c)1·3의 Python exit-5·data/error.detail 경로·E_QUARANTINE, D3/(c)4의 필수 envelope·두 층 순번·집계 origin도 수정됐다. U-15의 폴링 기반/M3 서비스 구분과 공통 ask 패턴도 반영됐다.
- **잔여 D3/(c)4:** §6.1은 uint64를 JSON 문자열로 선언했지만 fixture의 `payload.timing.tx_done_ns/rx_start_ns/rx_complete_ns`는 여전히 숫자다. 세 나노초 필드도 문자열로 통일해야 한다.
- **이번 변경의 모순:** §10.4는 viewer 경로에서 토큰이 없다고 명시했는데, §4.2·§10.9는 `approval wait <tok>`를 요구한다. §4.1의 approval 객체에도 token 외 대기용 식별자가 없어 viewer 경로의 지정 절차를 실행할 수 없다.
- 최소 보완: viewer 응답에 승인 대기용 ID를 반환하고 `approval wait`가 그 ID를 받도록 명령표·스킬·JSON을 통일한다. token 경로의 confirm 토큰과 구별하고 viewer의 종료 코드는 E_APPROVAL_REQUIRED로 맞춘다.
- P2(A10): 실제 정책의 vehicle 미등록 R0 차단은 수정됐으나 Q2의 “화이트리스트 밖(R0)”는 그대로다. bench 한정 표기를 붙인다.
- 기존 02 체크리스트의 계획 구조 6개 PASS는 유지한다. 위 잔여 계약 불일치 때문에 V11 전체 PASS는 아직 불가하며 구현 시험 판정은 아니다.

## 최종 확인

V11 PASS

- 검토: plan.md SHA-256 `f084892ef62e2c342c8f3da0727df3d8ffeab066a6f079640343898bc4b105f5`. 직전 재확인 항목과 이번 변경만 검토했으며 plan.md는 수정하지 않았다.
- §4.1/4.2·§10.4/10.9: 공통 대기 식별자 approval_id, viewer의 E_APPROVAL_REQUIRED·토큰 미발급, approval wait의 ID 인자가 일치한다. confirm 토큰과도 구별된다.
- §6.1: timing의 tx_done_ns/rx_start_ns/rx_complete_ns가 모두 JSON 문자열로 수정됐다.
- Q2는 bench 미등록 R0 허용·vehicle 미등록 R0 거부로 통일됐다. 검토 범위의 잔여 지적과 새 모순 없음. 계획 정합성 판정이며 구현 시험 결과는 아니다.
