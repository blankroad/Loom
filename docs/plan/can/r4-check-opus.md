# R4 최종 점검 — can-opus → `plan.md`

**판정: PASS with fixes — 반드시 고칠 것(P1) 11개.** 구조와 원칙 반영은 좋다. 다만 M1 E2E(§2.0)를 그대로 구현하면 깨지는 곳 3개, 안전 규칙끼리 서로 맞지 않는 곳 5개, 2프로세스 결정과 어긋나는 곳 3개가 있다. 모두 문장 수준 수정으로 고칠 수 있다.

점검 범위: plan.md 전체(1,031줄). 관점은 r4-opus.md 1~5. 사실 확인은 Sol 담당이라 다시 하지 않았다.

---

## 1. M1만 구현했다고 가정한 §2.0 흐름 추적

| 단계 | 필요한 것 | plan.md 근거 | 결과 |
|---|---|---|---|
| 사전 준비 | virtual 채널 열기 + 가짜 ECU 시작 | `open`(§4.3 M1), `node start`(§4.3 M1, K4) | 구현은 있지만 §2.0에 **준비 명령이 없다** (P2-1) |
| T0 사용자가 monitor 실행 | viewer role 발급, 원자적 snapshot | C1 role, C2 snapshot, T1 | 통과. 단 snapshot·Fixed 스냅샷을 **어느 프로세스의 링버퍼에서** 만드는지 미정 (P1-3) |
| T1 에이전트 send | agent role, 정책(sim), tx.requested, `viewers:` | C1·C4·K1·K3 | 통과. 단 **env=sim이 무엇으로 정해지는지** 규칙이 없다 (P1-7) |
| T1 직후 TUI 표시 p95 < 200 ms | worker→gateway 배치, gateway→TUI 배치, 10 Hz refresh | §3.3·§3.8 | **예산이 배분되지 않았다**: 40 ms + 40 ms + 최대 100 ms (P1-2) |
| T2 `trace --since @last` | 커서, 링버퍼 질의, Activity | §3.5, K3 | 통과 |
| T3 사용자 `:send` | viewer role TX, origin=user | T1, §3.4 | sim에서는 통과. M2 이후 같은 ID에 에이전트 주기 job이 있으면 **사용자 개입이 막힌다** (P1-11) |
| T4 `trace --since @last --ids 0x300,0x301` + NOTE | 필터를 바꾼 @last 커서, user 이벤트 집계 | §3.5 커서 키 `(client, topic, filter_key)` | **깨진다**: 필터가 다르면 다른 커서라 @last의 기준점이 없다 (P1-1) |
| §9.3-7 STOP 시험 | stop-all 후 신규 submit 0 | §10.6 "SimNode TX 해제" | 가짜 ECU(device)까지 멈추므로 "실HW에서 화면·명령 동일"과 모순 (P1-5) |

M1 작업 분해(§12.1)에서 빠진 것: gateway 측 링 미러/질의 엔진(P1-3), `loom can demo` CLI 항목(K4에만 있음), `loom audit tail`(§10.8에만 있음).

---

## 2. 반드시 고칠 것 (P1)

| # | 위치 | 문제 | 수정 제안 (문장 수준) |
|---|---|---|---|
| P1-1 | §3.5 커서, §2.0 T4 | 커서 키가 `(client, topic, filter_key)`라 §2.0 T4처럼 필터를 바꾸면 `@last`의 기준점이 없다. NOTE 집계 범위도 불명확 | §3.5를 다음으로 교체: "커서 키 = `(client, channel)`. 필터는 읽을 때 적용하고, 필터에 걸리지 않은 프레임도 '본 것'으로 보고 커서가 전진한다. 전진시키지 않으려면 `--peek`. `NOTE:`는 필터와 무관하게 커서 이후의 user origin 이벤트 전부를 센다." |
| P1-2 | §3.3, §3.8 | worker→gateway 내부 배치(≤ 40 ms)와 WS 배치(40 ms)가 이어지고, TUI 10 Hz refresh까지 더하면 p95 < 200 ms 예산이 빠듯하다. 배분표가 없다. `wait-for`·`trace` 가시성 목표 행도 없다 | §3.3 내부 배치를 "**≤ 10 ms 또는 200프레임**(내부 IPC, 화면 배치와 별개)"로 바꾼다. §3.8에 지연 예산 행 추가: "ingest→gateway ≤ 10 ms, gateway 처리 ≤ 10 ms, WS 배치 ≤ 40 ms, TUI refresh ≤ 100 ms → 합 p95 ≤ 160 ms". 목표 행 2개 추가: "프레임 → `trace --since @last`에 보임 p95 < 50 ms", "프레임 → `wait-for` 반환 p95 < 100 ms (M2)" |
| P1-3 | §3.2·§3.3·§3.5, §12.1 | 링버퍼는 CAN worker에 있는데 커서·`trace.query`·snapshot(backlog·Fixed 스냅샷)은 gateway 몫이다. gateway가 worker에 매번 질의하는지, 미러를 갖는지 정하지 않았다 | §3.3에 추가: "gateway는 배치로 받은 이벤트로 **조회용 링 미러**(채널별 최근 N프레임 + Fixed 테이블)를 유지하고, trace.query·snapshot·NOTE 집계를 여기서 처리한다. worker 링은 저널·ISO-TP용 단기 버퍼다. 미러 밖 구간은 저널에서 읽는다." §12.1 C2 산출물에 "링 미러 + 질의 엔진 + Fixed 테이블"을 명시 |
| P1-4 | §3.1·§3.6 vs §3.2·§9.1 | §3.1은 "CPU 작업은 CAN worker에 두지 않는다", §3.6은 "CAN worker에 CPU-bound 작업이 없으므로 FC 지연을 피한다"고 한다. 그런데 SimNode(M2부터 DBC 인코딩·신호 생성기·**UDS 팀 가상 ECU 코드**)와 저널 JSON 직렬화가 같은 worker에 있다. 실HW 채널과 sim 채널이 한 worker에 있으면 sim 부하가 실HW ISO-TP 타이밍을 해친다 | §3.2에 추가: "**virtual 채널과 SimNode는 별도 sim worker 프로세스**에 둔다(python-can virtual은 같은 프로세스 안에서만 연결되므로 virtual 버스와 노드를 한 프로세스에 함께 둔다). 실HW worker에는 SimNode를 두지 않는다. M1은 sim worker 하나로 충분하다." 저널 직렬화는 §11 M0(c)에 "저널 on 상태에서 FC 지연" 측정 조건을 추가 |
| P1-5 | §10.6, §4.3 stop-all, §9.3-7 | STOP이 SimNode TX까지 멈춘다. 가짜 ECU는 `origin=device`(실 ECU 대역)인데 STOP하면 ECU가 사라진다 → "실HW에서는 명령·화면 동일"(§2.0)과 모순된다. 시험 "신규 submit 0"의 대상도 모호하다 | §10.6 첫 줄을 이렇게 바꾼다: "`stop-all` = **agent·user·script origin**의 모든 주기 job·replay·대기 TX 해제 + ISO-TP 신규 송신 차단. SimNode(device 역할)는 멈추지 않는다(`loom can node stop`로 별도)." §9.3-7: "stop-all 후 agent/user/script origin 신규 submit 0" |
| P1-6 | §10.1-1·§10.9, CR7 | 정책은 사람만 편집하는 `loom.toml`에 있는데 데몬은 변경을 **감사만** 한다. 에이전트가 파일을 고치면(스킬로만 금지) 승인 체계 전체가 무력해진다 | §10.1에 원칙 7 추가: "**정책 완화는 뷰어 승인 후에만 적용.** 데몬은 `loom.toml` 변경을 감지하면 강화(tx 축소, deny 추가)는 즉시 적용하고, 완화(tx deny→confirm/allow, env 변경, allow_ids 확대, rate·lease 상향)는 `policy.change` 승인 요청을 띄워 viewer가 승인할 때까지 이전 정책을 유지한다. 데몬 기동 시 지난 승인 이후 바뀐 완화 항목도 같다." |
| P1-7 | §10.1-2, §6.5 | env=sim(송신 자유)이 무엇으로 정해지는지 없다. 실HW 채널에 `env="sim"`을 쓰면(사람 실수 또는 P1-6 우회) R1으로 무승인 송신이 된다 | §10.1-2에 추가: "**env=sim은 interface가 `virtual`(또는 Linux `vcan`)일 때만 유효**하다. 실HW interface에 sim을 지정하면 open을 거부한다. virtual 채널은 설정이 없어도 env=sim이다." |
| P1-8 | §4.3 `open` 위험 열, §10.2 R4, §6.5 예시, §11 M2 | `configure` = R4(매번 승인)인데 §6.5 예시 ch0는 `mode="configure"` + `autostart=true`이고, M2 첫 실HW도 `mode=configure`다. 그대로면 데몬을 켤 때마다 R4 승인이 필요하고, 뷰어가 없으면 NO_APPROVER로 시작조차 못 한다 | §4.3 open 위험 열을 이렇게 바꾼다: "R0(loom.toml에 사람이 쓴 설정 그대로) / **R4(CLI가 loom.toml과 다른 bitrate·FD·mode를 요청할 때)**". §10.2 R4 문구를 "init access/비트레이트 변경을 **에이전트가 요청**(loom.toml 밖)"으로 고친다 |
| P1-9 | §6.5 주석 vs §10.1-2·§14 Q6 | §6.5는 "allow_ids가 비면 deny_ids 외 전부(bench)", §14 Q6은 "실HW는 allow_ids 설정 전 read-only"라고 한다. 서로 반대다 | 둘을 하나로 통일한다(Q6 쪽 권장): "실HW 채널에서 **allow_ids가 비어 있으면 송신 불가**. 전부 열려면 명시적으로 `allow_ids=["*"]`(R2 이상 판정은 그대로)." §6.5 주석도 같이 바꾼다 |
| P1-10 | §10.2 R3 `sig set`, §10.3 plan hash scope | 승인 scope=session은 "같은 plan hash 재사용"인데 IG 신호 patch는 바이트가 바뀌어 hash가 매번 바뀐다 → bench에서 `sig set`마다 승인이 뜬다. A2/U3 흐름(쓰기 루프의 핵심)을 실HW에서 쓸 수 없다 | §10.3에 추가: "주기 job의 plan = {channel, id, 주기 범위, 신호별 허용 범위(기본 DBC min/max, 승인 시 사용자가 좁힐 수 있음), lease}. hash는 이 범위로 계산하고, 범위 안의 `sig set`은 재승인 없이 revision만 올린다. 범위를 벗어나면 새 승인." |
| P1-11 | §10.2 '차단' 행, D13, §2.2 U3 | "같은 (채널, ext, ID)에 두 번째 producer 거절(agent·user 모두)". 에이전트 주기 job이 0x300을 보내는 중에 사용자가 TUI `:send 0x300#00`을 하면 거절된다 → 원칙의 "사용자 직접 개입"이 막힌다. TUI에는 takeover 경로도 정의돼 있지 않다 | '차단' 행 아래에 추가: "producer = 주기 job·replay·ISO-TP 채널. **사용자의 1회 송신은 producer가 아니며 우선한다**: 다른 주체의 job과 ID가 겹치면 송신하고, Trace에 `USR override` 배지, job 소유 에이전트 출력에 `NOTE: user overrode 0x300 (job p3)`. ISO-TP 예약 ID는 사용자도 차단(상태기 보호). TUI는 `:takeover p3`로 job 소유권을 넘겨받는다." |

---

## 3. 고치면 좋은 것 (P2)

| # | 위치 | 제안 |
|---|---|---|
| P2-1 | §2.0 | 표 위에 준비 명령 2줄 추가: `loom can open ch0 --iface virtual` / `loom can node start examples/sim/m1/bcm.yaml` (또는 `loom can demo`). M1 시험 스크립트와 1:1이 된다 |
| P2-2 | §4.3 위험 열 | `periodic start`·`sig set`·`isotp open`을 "R1(sim) / R3"로 (§10.2 R1 "sim 주기"와 맞춤) |
| P2-3 | §10.2 ID 충돌 승격 | "env=sim에서는 R4로 올리지 않고 경고만" 명시. 안 그러면 가짜 ECU 메시지를 흉내 내는 sim 실험마다 승인이 뜬다 |
| P2-4 | §10.5 lease | "CLI `--lease`는 채널 상한(§6.5 `lease`)을 넘을 수 없다. `renew`는 Activity에 표시되고 감사된다" 추가 |
| P2-5 | §10.2 R4 `--base last` | "카운터/CRC 신호" 판별 규칙(DBC 속성·이름 패턴·사용자 목록) 명시. 판별 못 하면 `last` 전체를 R4로 |
| P2-6 | §10.3 viewer role | "TTY 대화형 확인"의 실체를 정의한다(예: 시작 시 화면에 표시된 4자리 코드를 사람이 입력). 비 TTY에서 실행된 monitor는 **승인 권한 없는 보기 전용**. 스킬에 "monitor를 직접 실행하지 말 것" 추가 |
| P2-7 | §3.5 `LOOM_CLIENT` | 기본값 `agent:default`를 여러 에이전트가 같이 쓰면 커서와 감사 주체가 섞인다. gateway가 같은 이름의 동시 사용을 감지하면 `hint:`를 주고, 스킬은 고유 이름 사용을 권장 |
| P2-8 | §3.2 감독 | gateway가 죽었다 재시작하면 **실행 중인 worker에 다시 붙는다**(새 worker 생성 금지, 장치 이중 open 방지). worker의 discovery/lock 파일 규칙 추가 |
| P2-9 | §13 | 잔여 위험 추가: "Vector/PCAN은 여러 앱의 같은 채널 open을 허용 → 에이전트가 스킬을 어기고 직접 연 숨은 연결을 데몬이 막거나 탐지하지 못할 수 있음(탐지 가능성 미확인)". 완화: 스킬 규칙 + 감사 |
| P2-10 | §4.3 | `loom can demo`, `loom audit tail`, `loom help --agent can`을 명령 표와 §12(K4/C4/D0)에 넣는다 |
| P2-11 | §5.2 | TUI 목업이 M2 기능(pcan, 승인 바, 신호명)을 보여준다. "M2 화면" 라벨을 달거나 M1 목업(§2.0)을 참조 |
| P2-12 | §11 M0 | (c)에 "저널 writer on + 20k fps"에서 FC 지연을 측정하는 조건 추가(P1-4와 연결) |
| P2-13 | §3.8 | Fixed 모드의 `×count`·주기가 capture gap 이후 "정확"하지 않을 수 있음은 3번에 있다. 화면 표기 예(`×41210?`)까지 정해 두면 TUI 구현자가 헷갈리지 않는다 |

---

## 4. 스킬 설계 점검 (§10.9, §15.5)

- **분량:** SKILL.md 골자가 약 25줄이라 90줄 안에 충분히 들어간다. 아래 규칙을 넣어도 45줄 안팎이다.
- **반영된 공통 원칙:** viewers 0 → monitor 요청, dry-run → 한 줄 보고 → 실행, NOTE 대응, exit 4/5, loom.toml 완화 금지, 자기 승인 금지, 직접 장치 open 금지, stop-all.
- **빠진 규칙 (SKILL.md 본문에 추가 권장):**
  1. `env=vehicle` 채널은 **사용자가 이번 대화에서 명시적으로 요청한 경우에만** 송신 시도(§10.9에도 없음). 데몬이 막아 주더라도 R4 승인 요청 자체가 사람을 귀찮게 한다.
  2. `WARNING: data loss` / exit 8 → 결과를 결론에 쓰기 전에 손실 구간을 사용자에게 알린다.
  3. exit 5 중 `NO_APPROVER` → "monitor를 열어 주세요"(일반 승인 대기와 다른 문구).
  4. exit 6 → `loom daemon status`/`logs --tail 20`으로 확인하고 보고. 데몬을 강제로 죽이거나 장치를 직접 열지 않는다.
  5. 긴 주기 송신은 `periodic renew`. lease 만료로 멈추면 다시 시작하기 전에 사용자에게 알린다.
  6. `loom can monitor`를 직접 실행하거나 승인 UI를 흉내 내지 않는다(P2-6).
  7. 고유한 `LOOM_CLIENT` 이름을 쓴다(P2-7).
  8. `hint:`는 그대로 사용자에게 전달한다.
- **`loom` 공통 스킬:** 이름만 있고 골자가 없다. 데몬 기동/상태, 출력·종료 코드 규칙, 승인 원칙, 뷰어 안내, stop-all을 공통 스킬로 옮기면 도메인 스킬은 짧게 유지된다. 세 팀 합의가 필요하므로 §15.1 CR6에 "골자 초안은 CAN 팀 제공"을 추가 권장.
- **Codex:** `.agents/skills/` symlink는 Windows에서 개발자 모드/권한이 필요할 수 있다(미확인). `loom skills sync`(복사)를 기본으로 하고 symlink는 선택으로.
- **description:** "CANoe, DBC, ISO-TP/CAN-TP" 키워드를 넣으면 트리거가 잘 된다(UDS 작업은 loom-uds로 안내).

---

## 5. 결정 기록(§16)에 대한 의견

**강하게 반대하는 결정은 없다.** 두 가지를 보완해야 한다.

| 결정 | 의견 | 대안 |
|---|---|---|
| D13 (같은 tx_id 기본 차단, 모두) | ISO-TP 예약 ID에는 동의한다. 하지만 **일반 주기 job ID에 사용자 1회 송신까지 막으면** 원칙 4요소의 "개입"과 충돌한다 | P1-11: 사용자 1회 송신은 override(배지+NOTE), ISO-TP 예약 ID만 사용자도 차단 |
| D1 (2프로세스) | 동의. 단 "CAN worker = 모든 채널 + SimNode"로 읽히면 D1의 근거(hot path 격리)가 무너진다 | P1-4: 실HW worker와 sim worker를 분리. 사용자에게는 여전히 loomd 하나 |
| D9 (승인 = viewer role 전용) | 동의. 다만 정책 파일 수정이라는 우회로가 남아 있다 | P1-6: 정책 완화는 viewer 승인 후 적용 |

참고(반대 아님): D11 패딩 0xCC, D4 웹 M3, D21 `--channel`, D24 rate는 동의한다.

---

## 6. 반영 순서 제안 (리드용)

| 순서 | P1 | 막는 것 | 고칠 곳 |
|---|---|---|---|
| 1 | P1-1, P1-3, P1-2 | **M1 E2E 수용 시험**(§9.3-1, §11 M1 ①②) | §3.3, §3.5, §3.8, §12.1 C2 |
| 2 | P1-5, P1-4 | M1 STOP 시험 의미, 2프로세스 근거 | §3.2, §3.6, §9.1, §10.6, §9.3-7, §11 M0 |
| 3 | P1-7 | M1 최소 안전(sim만 송신) | §10.1-2 |
| 4 | P1-6, P1-8, P1-9 | M2 실HW 첫 연결 | §10.1, §4.3, §6.5, §10.2, §14 Q6 |
| 5 | P1-10, P1-11 | M2 IG·사용자 개입(원칙 4요소) | §10.2, §10.3, §2.2 U3, D13 |

1~3은 M1 착수 전에, 4~5는 M2 설계 전에 반영하면 된다. 모두 문서 수정만 필요하고, 새 조사나 spike는 필요 없다.

<!-- END R4 -->
