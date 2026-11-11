ISSUES
# CAN 리드 — 마스터 계획(00-master-plan.md v1) 점검

대상: §2 D1–D14, §3. 대조: `can/plan.md`(R4 반영본). 대부분 CAN 안과 같은 방향이며 설계를 깨는 것은 아래 (a) 4건, 모두 문구 보강으로 해결 가능.

## (a) CAN 설계를 깨는 결정 — 이유와 수정 제안
1. **D10 "모든 쓰기가 core 파이프라인(policy→승인→audit→backend→events)을 지난다"** — 문자 그대로면 주기 송신의 매 주기 프레임, ISO-TP CF/FC, SimNode 응답까지 gateway를 왕복해야 한다. 그러면 D1에서 worker를 분리한 이유(FC p99 52 ms 실측)가 무너지고 10 ms 주기·STmin 준수가 불가능해진다.
   → 제안: Registry에 **job형 쓰기**를 둔다. core는 job의 start/update/stop/renew를 게이트(정책·승인·감사)하고, 프레임 단위 송신은 worker가 승인된 plan 범위 안에서 직접 한다. 이때 worker가 TX latch·예약 ID·레이트·하드 차단을 프레임마다 다시 집행한다. 프레임은 events/저널로만 남기고 프레임마다 감사하지 않는다(job 단위로 감사). ISO-TP PDU 송신도 같은 방식(채널 open = job, PDU = UDS 가드).
2. **D6 R3 승인 "scope once/session, 최대 10분"** — bench 주기 송신(IG)은 수십 분~수 시간 돌기 쉽다. 10분마다 재승인을 받거나 `sig set`마다 막히면 CAN의 쓰기 루프(plan §10.3 "승인 범위 안의 sig set은 재승인 없음", lease+renew)가 깨진다.
   → 제안: 10분 상한은 **승인 객체의 사용 기한**(그 안에 job을 시작해야 함)으로 정의한다. 시작된 job은 자기 plan(신호 범위·주기·ID)과 lease 안에서 계속 돈다. renew는 `loom.toml`의 `max_job_duration`(기본 1 h) 안에서만 재승인 없이 허용하고, 넘으면 새 승인을 받는다.
3. **D2 "메시지 64 KiB 상한"** — snapshot(plan: backlog 최대 2 MiB/10k 프레임), viz 질의 응답(series당 최대 2만 점 × 여러 series), M3 열 지향 바이너리 블록은 64 KiB를 넘는다.
   → 제안: 64 KiB는 **클라이언트→서버 요청**과 `core.events` 배치 한 개에 적용한다. snapshot은 순번이 붙은 여러 메시지로 나눠 보내고(`snapshot_part n/N` → `snapshot_end{stream_seq}`), viz 대용량 질의는 같은 오리진 HTTP GET(토큰·쿠키 검사)으로 받는다.
4. **D6 R2 `token_or_viewer`(bench 기본) + U4 기본값** — 호스트 `ask` 규칙이 없는 환경(Codex, Claude Code bypass 권한 모드)에서는 `loom can confirm <tok>`가 사실상 자기 승인이 된다. 실HW 버스 송신에는 약하다.
   → 제안(D6이 허용한 "도메인이 더 엄격하게"의 범위 안): CAN 실HW 채널 기본값 `tx="confirm"`은 `r2_channel=viewer`로 둔다. 사람이 `loom.toml`에 `tx="allow"`를 명시한 채널만 `token_or_viewer`를 쓴다. sim 송신은 R1 자동이라 영향 없음. 메인이 동의하면 U4 문구에 "CAN 실HW는 viewer 기본"을 추가해 달라.

## (b) 마스터 계획이 CAN 계획을 잘못 옮긴 곳
- 큰 오기는 없다. 사소한 것 2건:
  - §1 표의 `wait-for` → 정확한 명령은 `loom can sig wait-for`.
  - §3.2 P0의 "K1 CAN worker"는 C1/C2(gateway·이벤트 허브)에 의존한다 → CORE stub에 대고 개발한다는 전제를 명시해 달라.
- §3.2의 CORE/CAN 작업 경계가 비어 있다. CAN plan의 C2에는 **CAN 전용 gateway 측 부품**이 섞여 있다: 조회용 링 미러의 CAN 질의(trace 필터·Fixed 테이블·NOTE 집계), DBC 디코더/SignalCache, viz 데이터셋. CORE 이벤트 허브(seq·snapshot·우선 큐·gap)와 분리해 CAN 팀 몫으로 적어 달라. D10 Registry가 **gateway 프로세스에 올라가는 도메인 모듈**(worker만이 아니라)을 지원해야 한다는 점도 함께 명시 요청.

## (c) plan.md를 결정에 맞추려면 고칠 곳 (P0 착수 전에 반영 예정)
1. §4.1 종료 코드: 6 = 사용자·락 충돌(`E_LOCKED` = ISO-TP busy, `E_INTERRUPTED_BY_USER` = 사용자 takeover로 job 중단), 7 = 장치, 9 = 데몬. `ERR_TX_RESERVED`·하드 차단은 4(`E_DENIED`). 기존 "6 = 데몬"은 9로 이동.
2. §4.1 JSON 봉투: D5 필드(`cmd`, `warnings[]`, `others_since_last{count, origins, from_cursor}`, `error.hint`)로 교체. `NOTE:` 줄은 `others_since_last`의 사람용 표시로 정의.
3. §4.1·§3.5: `--since mark:X` 추가. `LOOM_CLIENT` 기본값을 `agent:default`에서 "부모 셸 세션+사용자 기반 안정 id"로 바꾼다.
4. §4.5 RPC 이름:
   - `can.tx.periodic.*` → `can.periodic.*`, `job.*` → `core.job.*`
   - ISO-TP 메서드 `isotp.*` → **`can.isotp.*`**. 이벤트 이름도 `<domain>.<event>` 규칙으로 맞춘다(`can.isotp_rx_start` 등, 정확한 표기는 CORE 계약 v1을 따름). `_xteam/can-uds-isotp.md`도 같이 갱신.
   - `core.hello{proto, token, client_id, want_role}`, `after_seq`(stream_seq 의미).
5. §3.4·§6.1·§15.2 TX 상태를 D4로:
   - requested → accepted(정책·중재 통과) → **sent**(`bus.send` 반환 = 드라이버 접수) → confirmed(backend TX 확인) | failed | outcome_unknown(전송 timeout·끊김)
   - plan의 "accepted = 드라이버 접수"는 `sent`로 바꾼다. ISO-TP `tx_state: accepted|confirmed` → `sent|confirmed`.
6. §6.1 레코드:
   - `event` → `kind`
   - 필드 추가: `client_id, job_id, generation, risk`
   - `ts = {mono_ns, wall_ns, clock_id, source, quality}`로 통합. 드라이버 시각은 `payload.device_ts{value, clock_id}`로 유지.
   - 바이트: `{encoding:"hex", data}`
   - 신호 디코드 값은 선택 필드 `metrics[]`로도 내보내기(viz 자동 플롯)
7. §10.2·§10.3: R2 bench 단발 = confirm(`loom can confirm <tok>` 또는 뷰어 Y, 채널 기본은 (a)4 안). R3 승인 scope 최대 10분 + (a)2의 job 수명 규칙. `default_blocked` 개념 반영(현재 하드 차단 표와 연결).
8. §6.2·§6.5·§10.8: 경로를 `~/.loom/{run,records,config}`로 옮기고 정책 2층(`~/.loom/config/policy.toml`)을 반영한다. 보존은 30일/20 GiB, 감사는 해시 체인. **기록·감사 불능 시 새 R1+ 송신 중단**(sim 포함 — plan의 "sim은 경고만"을 삭제).
9. §3.7 I5: `viewers: 0` 힌트 → `no_viewer_attached` 경고로 이름 통일. §3.8·§5: `STALLED`, `LIVE-DEGRADED` 배지와 heartbeat 1 s 추가.
10. §5.2: STOP 키에 `Ctrl-] x` 추가(`Ctrl-X` 유지).
11. §11·§12: D8 `LOOM_NO_AUTOSTART`. D12 Registry에 `can.periodic.stop`·`can.stop_all`을 `safe_state: true`로 선언. C0–C4는 CORE 소유로 표시하고, CAN 몫은 S0, K1(stub 기반), K4(P0) → K2(core 기록기 기반), K3, T1(공통 TUI 키트), Q1, D0와 "CAN gateway 측 모듈"(신규 ID).
12. §15.5: 스킬 원본 위치를 `skills/loom-can/`로 옮긴다(`loom skills sync`로 배포). §15.1 CR1–CR8은 "D1–D13으로 확정"으로 정리.
