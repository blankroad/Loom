# UI → EXPLORE 요청 — psu.gauge (2026-11-11)

docs/plan/02-core-principle.md 준수. UI는 `psu.gauge`를 fixture 위에서 먼저 만들었다(ui/main 612417d).

1. `psu.state` payload `{set_v, set_i, out, prot, mode}`와 측정값을 record `metrics[{name: voltage|current|power}]`로 주는지(별도 `psu.meas` kind인지).
2. 비상 OFF RPC: `psu.out{channel, state:'off'}`로 가정. 같은 동작을 `core.safe_state.list`의 `emergency_off` 항목으로도 노출할지(STOP 패널의 [비상 OFF]는 그쪽을 씀) — 두 경로가 같은 서버 동작이어야 한다.
3. 출력 ON = R3, 값 설정 = R2 배지는 명세대로 정적 표시. 서버 classify 등급을 이벤트·결과로 주면 그 값을 쓰겠다.
4. 보호 상태 값 집합(`ok|ovp|ocp|otp|…`)과 CV/CC 모드.


## explore 답변 (explore-lead, 2026-11-11)
PSU는 P1에서 **SimPsu로 플러그인 계약만 검증하는 도메인**이다(eth 다음 순서로 구현). 모양은 아래로 확정하고, 구현 시 이 모양을 지킨다.
1. **상태와 측정은 분리**한다.
   - `psu.state{ch, set_v, set_i, out(bool), prot, mode}`: 변화 시 발행.
   - `psu.meas`: 폴링 샘플. 값은 레코드 `metrics[{name:"voltage"|"current"|"power", value, unit}]`이고 payload는 `{ch, age_ms}`.
   - 측정 실패는 `source.gap{cause:"scpi_no_response"}`.
2. **비상 OFF**: 서버 동작은 하나다. `psu.out{channel, state:"off"}` 핸들러를 Registry `emergency_off`로도 등록하므로, `core.safe_state.list`의 항목과 패널 `!` 버튼이 같은 서버 동작을 탄다(확인 없음, 인터록은 경고만 — A3).
3. **등급 배지**: 서버 classify 결과를 쓰기 행의 레코드 `risk`(CORE 이벤트 필드)로 준다. UI는 그 값을 쓰고 정적 표시는 대비용으로만 둔다. F1이라 승인 없이 자동 실행되며 등급은 표시만 한다.
4. **값 집합**: `prot ∈ ok|ovp|ocp|otp|unknown`, `mode ∈ cv|cc|off|unknown`.

## explore 구현 알림 (2026-11-11, loom-psu ca07142)
- `psu.state.mode`는 **소문자** `cv|cc|off|unknown`이다(UI fixture의 `'CV'`와 다름). 추가 필드: `vmax, imax, cycling`.
- `psu.meas` metrics 순서는 voltage, current, power로 고정한다. 포인터는 `/metrics/{0,1,2}/value`.
- 인자 정본은 `ch, volt, curr`이고, UI가 보내는 `channel, v, i`도 받는다.
- 쓰기 결과에 `readback`이 들어간다. 다채널 비상 OFF(채널 없이 `psu.out{state:"off"}`)는 `{results:[{ch, ok}]}`.
- 패널 manifest의 topic filter는 선언형이다: `{"domains":["psu"],"channelParam":"channel","defaultChannel":"bench"}`.
