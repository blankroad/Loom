# P1 보고 — explore-opus: `crates/loom-psu` (SimPsu, 플러그인 계약 검증) (2026-11-11)

**완료.** 코드는 eth worktree(`.worktrees/eth/crates/loom-psu/`, `skills/loom-psu/`), 커밋 안 함. 관문 4개 green: `cargo fmt --all --check` · `clippy --workspace --all-targets -D warnings` · `clippy -p loom-eth --features live` · `cargo test --workspace`(실패 0, loom-psu 14개). `Cargo.lock`에 loom-psu 항목만 추가됨(외부 crate 추가 없음, 루트 Cargo.toml은 `crates/*`라 손대지 않음).

## 만든 것
- `sim.rs` SimPsu: set_v/set_i, 출력, OVP(vmax×1.1)·OCP(선택)·OTP(150 W 2 s), 부하 40 Ω + 돌입 1.5 A(τ 50 ms) + 시드 잡음, CV/CC.
- `worker.rs`: `psu.connect/status/meas/poll/sim.inject` 호출, `set/out/cycle` 쓰기. 5 Hz·미완료 질의 1(느린 응답은 주기 늘림·`age_ms`), 무응답 = `source.gap{cause:"scpi_no_response"}` + 샘플 없음. 쓰기 행 `psu.write`: `sent → effective_verified|effect_unverified`(되읽기), 실패 `failed`, 되읽기 무응답 `outcome_unknown`. STOP latch: agent/script 거부, user 허용.
- 이벤트(UI 답변 그대로): `psu.state{ch,set_v,set_i,out,prot,mode}`(+`vmax,imax,cycling`), `psu.meas{ch,age_ms}` + `metrics[voltage,current,power]`.
- gateway: 명령 9개, 위험표, hooks, PerChannel(4) 워커(16/32 MiB, 계획 C2), reducer(최신 상태 + 60 s 요약), 증거 3개, summarizer, `psu.gauge` manifest(UI와 같은 동작 3개), help, `emergency_off`·`safe_state` = `psu.out`, 인터록 `psu.cycling`.
- CLI `loom psu connect|status|meas|watch|wait-for|set|out|cycle|sim inject`(+`--dry-run`), 계획 §5.1 출력 예 형식. 스킬 33줄.
- 시험 14개: 등록 전체, 분류표·한계 block, 한계 거부(게이트웨이·워커 둘 다, clamp 없음), out on → 전류 상승 → `wait_for` 성공/시간 초과/OFF 뒤 성공, 대기 중 사용자 OFF → exit 6, 비상 OFF 경로 = RPC 경로(같은 op), STOP latch, 인터록, 무응답 gap·지연, cycle, 증거 hold, reducer/watch, CLI.

## 결정
1. 한계 초과: classify가 `block: HardDeny` + 사유(확인으로 우회 불가), 핸들러도 `E_DENIED`(exit 4)로 워커 전에 거부, 워커도 재검사. `connect`의 vmax/imax는 **낮추기만** 받는다(올리는 건 사용자 loom.toml).
2. `psu.out{state:"off"}`에 채널이 없으면 아는 모든 전원을 끈다 = 비상 OFF 동작(채널마다 `rid/ch`로 쓰기 행 분리). 사용자 off는 인터록이 있어도 통과(classify 사유에 WARNING), agent/script off·cycle은 `presend_check`가 `Locked`(exit 6).
3. `wait_for`는 **상태 조건**: 호출 전 `hold_ms` 구간 샘플도 센다. hold = 연속 샘플, 간격 ≤ 600 ms(3주기), 손실이면 다시 시작. 대기 중 사용자 출력 OFF → `E_INTERRUPTED_BY_USER`(계획 §5.1 h-2). 폴링 100 ms(runtime 없는 Delay).
4. 증거: `current_lt/gt` = hold 런이 있으면 Match, 없고 손실·긴 간격·coverage 불완전이면 Inconclusive, 아니면 Mismatch. `voltage_between`(hold 없음) = 창 안 모든 샘플이 범위 안(Complete).
5. 파라미터 이름: 정본 `ch, volt, curr`, UI가 보내는 `channel, v, i`도 받는다.
6. `psu.connect`는 쓰기 클래스 R0(감사), `--visa`/`backend:"visa"`는 `E_NOT_IMPLEMENTED`(가짜 성공 없음). `psu.sim.inject`는 읽기 클래스(sim 전용).
7. `psu.state`에는 request_id를 넣지 않는다(E8: request_id는 쓰기 행 표시) — origin·client_id만.
8. 인터록 이름: Registry `interlock`은 자기 도메인 이름만 받아 `uds.flashing`을 등록할 수 없다 → 지키는 이름 목록(`uds.flashing`, `debug.flashing`)과 주입식 조회(`Psu::with_interlocks`, 기본 "모두 꺼짐")만 두고, 자기 인터록 `psu.cycling`(cycle 중)을 등록했다.

## CORE 요청
1. **워커 주기 tick**: Worker trait에 타이머가 없다 → 5 Hz 폴링을 `psu.poll` 호출로 받는다. 호스트(`run_worker`)가 kind별 주기로 부르거나 `on_tick` 추가 필요.
2. **훅에서 다른 도메인 인터록 조회**(`WriteRequest`에 `interlocks` 뷰 등) — 없으면 `uds.flashing`을 볼 수 없다.
3. `EmergencyOffSpec`에 `args`가 없다 → 코어가 `psu.out`을 `{state:"off"}`(+대상 채널)로 부른다고 가정했다. 계약에 인자 명시 필요.
4. 데이터 레인 `Emitter::rx`에 metrics가 없다 → `psu.meas`를 `priority`(ctl 레인)로 낸다. metrics 받는 rx 변형 필요.
5. 한계 초과 전용 코드(`E_LIMIT`, exit 4) 또는 `BlockClass::Limit` — 지금은 `HardDeny`/`E_DENIED`로 표현.
6. write 클래스의 `dry_run`은 정책·감사 생략(eth 요청 1과 같음), 채널별 워커 지정(eth 요청 2와 같음), bounded wait API(P1-4).
7. `policy_section`으로 `[psu.<name>] vmax/imax`를 읽어 오는 경로(loom.toml 반영) — 지금은 기본값과 connect 축소만.

## UI 영향
- `psu.state.mode`는 소문자 `cv|cc|off|unknown`(UI fixture는 `'CV'`). 추가 필드 `vmax, imax, cycling`.
- meas metrics 순서 고정(voltage, current, power) — 포인터 `/metrics/{0,1,2}/value`.
- 쓰기 결과에 `readback`; 다채널 비상 OFF 결과는 `{results:[{ch, ok}]}`.
- manifest topic filter는 선언형 `{"domains":["psu"],"channelParam":"channel","defaultChannel":"bench"}`(UI 코드의 함수 filter 대신).

## 질문
- 없음. 실장비(raw TCP SCPI)는 구매 시.
