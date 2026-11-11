# EXPLORE P1 — explore-opus: `crates/loom-psu` (SimPsu, 플러그인 계약 검증 도메인)

먼저 `p1-common.md`를 읽는다(질문 창 금지 절 포함). 다음도 읽는다:
- `docs/plan/explore/plan.md` §5.1(PSU), §0.3–0.4
- 마스터 §2.5 F1
- `docs/impl/requests/ui-explore.md`의 `## explore 답변` — **UI가 이 모양으로 `psu.gauge` 패널을 이미 만들었다**
- `crates/loom-echo`(템플릿), `crates/loom-eth`(네가 만든 gateway·cli 구조를 재사용해도 된다)

배정 범위: 새 crate `crates/loom-psu/`, `skills/loom-psu/`만. eth 코드는 고치지 않는다(읽기만).

## 범위 (M1 = 시뮬레이션 전용, 실장비는 구매 시)
- **SimPsu 상태기계**(worker 안):
  - 설정 `set_v`·`set_i`, 출력 on/off, 보호(OVP/OCP/OTP)
  - 부하 모델: 저항 + 부팅 시 돌입 전류 + 잡음(결정적 시드)
  - CV/CC 모드
  - 지연·무응답 주입(`psu.sim.inject`)
- **이벤트**(ui-explore 답변 그대로):
  - `psu.state{ch, set_v, set_i, out, prot, mode}`: 변화 시
  - `psu.meas{ch, age_ms}` + 레코드 `metrics[{name: voltage|current|power, value, unit}]`: 폴링 기본 5 Hz, 장치당 미완료 질의 1
  - 무응답 → `source.gap{cause:"scpi_no_response"}`
  - 쓰기 행 `psu.write`(write_state, E8)
  - 64비트 값은 `U64` 문자열(계약 §6.1)
- **명령**: `psu.connect`(sim), `psu.status`, `psu.meas`, `psu.watch --for`(bounded), `psu.wait_for{curr_lt|curr_gt|volt…, hold_ms, timeout_ms}`, `psu.set{volt?, curr?}`, `psu.out{state}`, `psu.cycle{off_ms}`.
- **위험(F1: 표시만, 자동 실행)**:
  - `set` R2, `out on` R3, `out off`·`cycle` R3
  - 프로파일 한계(`vmax`·`imax`, 기본 16 V / 5 A)를 넘으면 **거부**(exit 4, clamp 아님)
  - 인터록: `uds.flashing` 등이 켜져 있으면 `presend_check`가 off/cycle을 거부한다. M1은 인터록 이름 등록과 훅만, 실제 신호는 없음.
- **비상 OFF**:
  - Registry `emergency_off`에 `psu.out{state:"off"}`와 **같은 핸들러**를 등록한다(STOP 패널과 패널 `!`가 같은 서버 동작).
  - 확인 없음, 인터록은 경고만(A3).
  - `safe_state`: `psu.out off`.
- **reducer**(채널별 최신 상태 + 최근 60 s 측정 요약), **evidence_verifier**: `psu.current_lt/gt{value, hold_ms}`(hold = 연속 샘플·간격 상한, gap이면 inconclusive), `psu.voltage_between`. **panel_manifest** `psu.gauge`, **help**.
- **CLI**: `loom psu connect|status|meas|watch|wait-for|set|out|cycle`(plan §5.1 d), 사람용 출력 예처럼.
- **스킬**: `skills/loom-psu/SKILL.md`(60줄 이하, 비상 OFF 존중·한계 거부·대기 명령·TC 단계 절).
- **시험**(testkit + worker-sdk MemorySink/SimClock):
  - 등록 전체
  - 한계 초과 거부
  - out on → 전류 상승 → `wait_for curr_lt` 실패/성공
  - 비상 OFF가 emergency_off 경로와 RPC 경로에서 같은 동작
  - STOP latch 중 agent 쓰기 거부·user 허용
  - 무응답 주입 → gap
  - evidence hold

## 관문·보고
- 관문: p1-common의 4개 명령.
- 보고서: `/Users/ctmctm/workspace/Loom/docs/impl/eth/reports/p1-psu-opus.md`(50줄 이하, 결정·질문·CORE 요청). 커밋하지 않는다.
