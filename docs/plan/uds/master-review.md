ISSUES

작성: uds-lead, 2026-11-10. 대상: `00-master-plan.md` §2 D1–D14, §3 ↔ `uds/plan.md`(R4 FINAL PASS본). 대부분 수용하며, 아래 (a) 5건만 결정 보완이 필요하다.
## (a) UDS 설계와 충돌하는 결정 — 이유와 제안
1. **D12 `finally`가 사용자 STOP 뒤에도 실행 ↔ D6 `stop-all` TX latch, UDS D16.**
   - 문제: 사용자 STOP 직후 UDS `finally`(`10 01`, `2F xx 00`)가 자동 송신되면 "멈춰라"는 사용자 의도와 충돌한다. 버스 이상 때문에 멈춘 경우에는 프레임을 더 보내는 것이 해롭다. latch가 걸린 상태에서 finally가 송신될 수 있는지도 D6·D12 어디에도 정의돼 있지 않다.
   - 제안: on_error나 에이전트 cancel로 끝난 경우에는 finally를 실행한다(D12대로). **사용자 STOP / stop-all**이면 finally를 자동 실행하지 않고, `safe_state` 동작을 뷰어의 "복구 송신" 1키(user origin, latch 예외는 safe_state 항목만)로 제시한다. UDS `uds.recover`를 core 공통 패턴으로 승격하자는 제안이다.
2. **D6 "vehicle은 기본 tx deny" ↔ UDS 실차 진단.**
   - 문제: UDS는 읽기(0x22·0x19)도 요청 프레임을 보내므로, 그대로면 실차에서 DID 하나도 못 읽는다. 실차 DID 읽기는 진단의 주 용도다.
   - 제안: vehicle의 기본 tx deny는 **raw CAN·주기·IG에 적용**한다. UDS는 화이트리스트 대상에 대한 **R0 진단 요청만 기본 허용**하고 R1+는 `unlock` 전까지 거부한다.
3. **D10 core 소유 쓰기 파이프라인에 UDS 전용 단계 자리가 필요.**
   - 문제: UDS 안전은 승인 이후에도 송신 직전 검사가 필요하다. R2 대기 120 s 동안 상태가 바뀔 수 있기 때문이다. 검사 항목은 epoch·물리 주소·lease·quarantine 미해결 집합 U와의 SID 겹침·현재값 변화(토큰 무효화)다. 또 대상당 인플라이트 1건 큐와 seed→key 원자 실행이 backend 앞에서 직렬화돼야 한다.
   - 제안: core 파이프라인에 도메인 훅 3개를 둔다.
     - `classify(bytes, target) → (risk, reason)`
     - `approval_binding(req) → extra digest fields`
     - `presend_check(req)`: 승인 후·backend 직전, 실패 시 승인 무효 + `E_QUARANTINE`/`E_LOCKED`
   - 이벤트 발행 순서: `queued → tx → rx`를 거친 뒤 RPC 응답(publish-before-reply). 같은 `request_id`로 행이 갱신돼야 한다.
4. **D5 종료 코드 3 vs 7 경계가 UDS에서 모호.**
   - 문제: ECU 무응답(P2/P2* 초과)이 3 `E_TIMEOUT`인지 7 "전송 timeout"인지 정해져 있지 않다.
   - 제안: **장치 요청 무응답 = 7**(`E_NO_RESPONSE`, 쓰기는 `outcome_unknown`). 3은 `wait-for`, `approval wait`처럼 조건을 기다리는 대기형 명령에만 쓴다. 공통 스킬 분기가 달라지므로 계약 v1에 명시해야 한다.
5. **D4 상태 이름과 ISO-TP `tx_state` 값의 충돌.**
   - 문제: `_xteam` 합의의 `tx_state: accepted|confirmed`(드라이버 접수/버스 확인)가 D4의 `accepted`/`confirmed`(정책 접수/장치 확인)와 이름이 같지만 의미가 다르다. 뷰어·로그에서 혼동된다.
   - 제안: ISO-TP 필드를 `tx_state: driver_accepted|bus_confirmed`로 바꾼다(CAN 팀과 1줄 합의).
- 참고(충돌 아님): D6이 0x27 보안 접근을 R3(뷰어 승인)으로 올렸다. UDS는 수용한다(R2 토큰 → 뷰어 승인). 0x2A/0x86은 UDS 표에서 "R4"였지만 미구현 기능이므로 승인으로 풀리면 안 된다. **하드 차단(`E_UNSUPPORTED`, exit 4)** 으로 분류하겠다.
## (b) 마스터가 UDS 계획을 잘못 적은 곳
- §3.2 P2 "M2 (보안접근, lease, R3) → M1b": UDS M1b는 M2에 의존하지 않는다. CAN K10이 나오면 M2와 병렬로 진행한다(plan §12 그룹 D).
- §3.2 P4 "M3b → M4 (실 DoIP/CAN HW)": 실 DoIP HW는 CAN 일정과 무관하다. 장비가 있으면 M2 HIL 스모크(plan §7 D4 M2 관문)부터 한다. M4 중 **CAN-TP 실HW만** M3b·CAN M4에 의존한다. 이대로면 임계 경로가 실 DoIP 검증을 불필요하게 늦춘다.
- D11/§3의 CAN K10/K14는 UDS plan·xteam에선 K9a/K9b. 같은 작업이면 번호만 내가 맞춘다. 그 외 §1 표, D1, D5, D6(`confirm`·`r2_channel`), D11, U12는 일치.
## (c) plan.md 수정 목록 (결정 확정 후 반영)
1. 종료 코드표(§4.1, 스킬 §10.9, D15)를 D5로 교체한다.
   - 2 사용법·DB / 3 대기형 timeout / 4 정책·하드 차단 / 5 확인·승인(R2 토큰 = `E_CONFIRM_REQUIRED`) / 6 LOCKED·QUARANTINE / 7 전송·ECU 무응답 / 9 데몬 / 10 NRC(`E_NRC`)
   - 예시의 `exit 3`(확인 필요)은 5로, `exit 2`(NRC)는 10으로 고친다.
2. `LOOM_CLIENT_ID` → `LOOM_CLIENT`. 커서는 `c-000429` → `seq:N`, `--since last` → `@last`, `from_cursor`는 `seq:N` 형식.
3. JSON 결과를 D5 봉투(`ok, cmd, data, meta, warnings, others_since_last, error{code,message,hint,retryable,request_id,detail}`)로 옮긴다. UDS 고유 필드(`timing`, `values`, `approval`, `may_have_executed`)는 `data` 아래에 둔다. 사람용 알림 줄은 `NOTE:`.
4. 이벤트(§6.1)
   - 필드 추가: `session_id, client_id, generation, risk`, 두 층 seq
   - `uds.gap` → `display.gap`/`record.gap`, `core.heartbeat` 표기 정리
   - 관리 이벤트는 `dir: null`. `timeout` 이벤트의 origin은 device가 아니라 요청 주체로 바꾸고 `dir: null`
   - `write_state`(D4) 매핑 추가: queued = requested/accepted, tx = sent, 긍정 응답 = confirmed, 되읽기 일치 = effective_verified, NRC = failed, timeout = outcome_unknown
5. 안전(§10)
   - 0x27 sendKey R2 → R3
   - 기존 "R4 금지" 항목을 D6 R4 + `default_blocked`로 재분류: 플래시 0x34–0x38, 0x3D, FF00, 미지 SID, 응답 SID raw
   - 0x2A/0x86은 하드 차단
   - vehicle은 (a)-2 결과 반영
   - R3 승인 scope 기본 once(UDS가 더 엄격, 0x27만 session 허용 검토)
   - 정책 파일 `$LOOM_HOME/policy/uds.yaml` → `loom.toml [uds]` + `~/.loom/config/policy.toml`, `trusted_db` 포함
6. Python API 연결 role은 agent(origin agent), script origin은 core 러너 전용(§4.4, §10.2).
7. 데몬 자동 기동(D8, `LOOM_NO_AUTOSTART`), 기록 경로 `~/.loom/records`(§6.2), 보존 30일/20 GiB.
8. 뷰어 키(§5.2)
   - STOP = `Ctrl-X`/`Ctrl-] x`(전 도메인 stop-all)
   - UDS 대상 abort는 `!` 유지
   - 필터 `f` → `/`, `Space` 일시정지 추가
   - viewer 0개 시 결과 `warnings`에 `no_viewer_attached`
9. 시나리오(§6.5, U-22): 러너는 core `loom run`, UDS는 `uds.*` 어댑터만. finally 규칙은 (a)-1 결정에 따른다. 동적 값 규칙은 core 러너 요구로 이관한다.
10. 스킬(§10.9, U-12/U-25)
    - 원본 `skills/loom-uds/` + `loom skills sync`
    - 종료 코드·`others_since_last`·데몬 규칙은 공통 `loom` 스킬로 넘기고 UDS SKILL.md는 UDS 절차·금지만 남긴다
    - `ask` 패턴 `Bash(loom * confirm:*)`
11. 작업 분해(§12): U-00은 CORE 팀 산출물로 대체(UDS는 변경 요청만). U-08/U-17/U-18의 승인·정책·감사 일반부는 core 사용, UDS는 분류표·훅((a)-3)만. U-11의 지연 측정은 core 공통 하네스 사용. K 번호 정정.
