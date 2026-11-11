# R2 교차 리뷰 — can-opus → `r1-sol.md`

작성: can-opus, 2026-11-09. 기준: `02-core-principle.md`, `00-brief.md` §3·§4, `can/brief.md`, `_xteam/can-uds-isotp.md` v0.1, `_xteam/viz-requirements.md`.
내 초안(`r1-opus.md`)은 수정하지 않았다. 사실 재확인은 PyPI/npm API, python-can 4.6.1·can-isotp 2.0.7 wheel 소스, vector.com 페이지, `spikes/can/r1-results.json` 원시값으로 했다.

---

## 1. 요약 판정

- **강점 ①** 실측과 출처. 직접 돌린 ISO-TP/주기 송신/GIL 부하 spike(`spikes/can/r1-results.json`과 숫자 일치 확인), asammdf↔canmatrix 의존성 충돌, Vector `flush_tx_buffer` 위험, CANoe MCP Server 같은 결정적 사실을 찾았다.
- **강점 ②** 정직한 손실·시각 모델. capture/record/display 손실 구분, `timestamp_quality`, TX requested/accepted/confirmed 분리, "send() 반환 ≠ wire 송신" 같은 규칙이 정확하다.
- **강점 ③** ISO-TP 계약 비판이 날카롭다. functional SF 용량이 주소 방식에 따라 다르고, P2 판정이 "timestamp만으로 IPC 무관"하지 않다는 지적은 UDS 초안과 내 초안이 놓친 부분이다.
- **약점 ①** 스킬 설계가 사실상 없다(§10.3에 3줄). SKILL.md 구성, references, 에이전트 워크플로, Codex 방안이 브리프 §3-11 요구 대비 크게 부족하다.
- **약점 ②** 승인 UX가 정의되지 않았다. `--grant g1`이 어디서 누구에 의해 발급되는지, 에이전트가 자기 요청을 승인할 수 없다는 보장, 뷰어가 없을 때의 동작이 없다.
- **약점 ③** 에이전트·사용자 UX가 JSON 위주다. 사람용 기본 출력 예시가 없고(공통 §3-4), 모든 명령에 `--session`을 요구한다. CANoe식 Fixed(ID별 덮어쓰기) Trace, 에이전트에게 사용자 개입을 알리는 장치, `viewers: 0` 경고도 없다.

## 2. 핵심 원칙 체크리스트 점검 (Sol 초안)

| # | 항목 | 판정 | 근거 / 미흡점 |
|---|---|---|---|
| 1 | 맨 앞 핵심 루프 표 | **통과** | §0 표: 4요소 → 명령/구성요소/확인 결과. 사용자 화면(어떤 패널)이 열로 따로 있지 않아 다소 약하다 |
| 2 | 종단 간 시나리오 | **통과** | §2.1: send 0x100 → echo 0x101 → TUI p95<200 ms → TUI `t` 송신 → `trace --origin user`. 화면 ASCII는 없고 문장 서술만 있다 |
| 3 | 실시간 목표 + 고부하 진실성 | **통과(우수)** | §3.5 표(Trace p95<200/p99<500, TX 표시<100, Graphics age<250, late attach<1 s, 과부하 배지<1 s). 배치 40 ms/200프레임/64 KiB, "N 프레임 화면 생략 · 원본 기록 정상/불완전", downsampled 배지, record/display 큐 분리. §9.4에 시험 |
| 4 | M1 = walking skeleton | **통과** | §11 M1, §12 T0–T4. "로그만 남거나 사용자 송신이 없으면 M1 실패"라는 판정 문구가 좋다 |
| 5 | 사용자 화면이 에이전트 세션과 분리되지 않음 | **대체로 통과** | §2.5 원자적 snapshot+subscribe, §3.1 단일 장치 owner, §2.1 "별도 bus open/별도 가짜 스트림은 실패". **미흡:** 뷰어가 하나도 없을 때 에이전트·사용자에게 알리는 장치가 없다. 에이전트의 읽기/대기가 사용자에게 보이는지도 불명확(Activity 패널은 "actions"만) |
| 6 | 부수 기능 후순위/비범위 | **통과** | §1.3/1.4, §11 M5 |

## 3. 틀린 사실

검증 결과 **명백히 틀린 사실은 찾지 못했다.** 항목별:

| Sol 주장 | 확인 결과 | 근거 |
|---|---|---|
| asammdf 8.8.32가 `canmatrix[arxml,dbc]==1.2` 고정 | **맞음** (내 초안은 이것을 놓쳤다) | PyPI `requires_dist` |
| cantools 45.1.2가 `python-can>=4.6.0` 요구 | 맞음 | PyPI `requires_dist` |
| uPlot 1.6.32, MIT | 맞음 (내 초안의 "1.6.x 미확인"을 대체) | registry.npmjs.org/uplot/latest |
| PyPI `canoe` 0.1.1은 2013년 별개 프로젝트 | 맞음 (2013-03-20) | PyPI |
| Vector `flush_tx_buffer()` 위험 | 맞음. docstring: "will flush the queue and send a high voltage message (ID = 0, DLC = 0)" | `can/interfaces/vector/canlib.py:934-943` |
| SocketCAN 타임스탬프는 `SO_TIMESTAMPNS`(커널 SW) | 맞음 | `socketcan.py:641,792` |
| vector/pcan/kvaser는 `_send_periodic_internal` 없음 → 스레드 fallback | 맞음 (내 확인과 같음) | `grep` 결과: socketcan, ixxat만 구현 |
| CANoe 20 SP2에 CANoe MCP Server + AI Package | **맞음** ("The CANoe MCP Server is available now in CANoe 20 SP 2") | vector.com/en/product/canoe/canoe-ai-features/ 본문 |
| spike 수치(FC p99 1.486/51.86 ms 등) | 원시 JSON과 일치 | `spikes/can/r1-results.json` |
| PCBUSB v0.13 (2024-07-28), binary EULA | **의심(미확인)** — 재확인 시도 중 GitHub API rate limit | — |
| XL Driver Library 26.30.16 (2026-07-23) | **의심(미확인)** — vector.com 페이지에서 버전 문자열 추출 실패 | — |
| python-can upstream 2026-11-08 commit | **의심(미확인)** — rate limit | — |
| MF4 writer append 미지원 | 미확인 | — |

**해석이 지나친 부분 (사실은 맞고 결론이 과함):**
- §7.10의 "GIL decode load"는 **순수 Python 무한 루프 스레드**다. CPython 기본 스위치 간격 5 ms(`sys.getswitchinterval()`)를 그대로 드러내는 최악 조건이고, FC p50 9.2 ms가 이 값과 맞아떨어진다. 이것으로 "decode를 같은 프로세스에 두면 안 된다"는 방향은 맞지만, 완화책 `sys.setswitchinterval()` 축소를 시험하지 않았다. Sol 측정에서 3신호 decode가 약 45만 fps이므로, 수천 fps 지연 decode는 이렇게 연속된 CPU 점유가 아니다. → plan.md에는 "같은 프로세스 내 CPU-bound 스레드는 FC를 10~50 ms 지연시킬 수 있음(실측)"을 사실로 쓰고, 해법(프로세스 분리 vs switchinterval)은 M0 비교 측정 항목으로 둔다.
- §9.1 "virtual은 프로세스 간 연결 불가 → vcan 또는 bridge": 맞다. 다만 python-can에 `udp_multicast` 인터페이스가 있는데(일부 옵션은 Linux 전용, `udp_multicast/bus.py:17,276,406`) 선택지로 언급하지 않았다. 둘 다 데몬 내 가짜 ECU를 기본으로 하므로 영향은 작다.

### 3.1 Sol 결과가 내 초안의 "미확인"을 어떻게 바꾸는가 (plan.md에 어느 쪽을 쓸지)

| 내 초안의 미확인/주장 | Sol 결과 | plan.md에 쓸 것 |
|---|---|---|
| ISO-TP FC 응답 "수 ms 이내 기대" (§15.2) | idle p99 1.49 ms, **CPU 경쟁 시 p99 51.9 ms / max 60.5 ms** | **Sol.** "부하 없을 때 수 ms, 같은 프로세스 CPU 경쟁 시 수십 ms"를 명시하고 hot path 격리를 필수 설계로 |
| M2 수용 기준 "10 ms 주기 지터 p99 < 1 ms" | macOS virtual, python-can 스레드 기준 10 ms 간격 p99 13.05 ms(절대 오차 p99 5.06 ms) | **Sol 쪽 사실.** 내 1 ms 목표는 근거 없음 → 철회. Windows/실HW 측정 후 수치 확정(잠정 "p99 오차 < 2 ms, 실측 후 조정") |
| 지연 decode가 꼭 필요한가 | 3신호 decode ≈ 451,855 fps | 필요성은 약해졌다. 다만 Notifier 콜백 안에서 decode 금지 규칙은 유지(둘 다 동의) |
| uPlot 버전 | 1.6.32 MIT | Sol |
| python-can 유지보수 | upstream commit 2026-11-08 (내 재확인은 rate limit으로 실패) | Sol (미확인 표기 유지) |
| macOS PCBUSB 버전/장치/라이선스 | v0.13, Apple silicon, PCAN-USB/FD 한정, binary EULA | Sol (등급 "실험적") |
| Vector 공식 Python 패키지 | PyPI에 없음. 대신 **CANoe MCP Server(20 SP2)** | **Sol.** 내 "py_canoe COM 4동사(M5)"를 "공식 MCP 우선, COM은 fallback"으로 교체 |
| CANoe FD 채널 공존 | 여전히 미검증. `fd=True`가 init 요청을 만든다는 점은 둘 다 확인 | 둘 다 같음. M2/M4 실HW gate |
| MF4 의존성 | asammdf가 canmatrix==1.2 고정 | **Sol** (내 초안은 canmatrix 1.3.3 + asammdf를 같은 extra 계열로 둠 → 충돌) |
| Windows `time.sleep` 고해상도 | Sol도 Windows 미측정 | 미확인 유지, M0 Windows 측정 필수 |
| STOP 구현 | `flush_tx_buffer`가 ID 0 프레임 송신 | **Sol 규칙 채택:** STOP ALL TX는 host 큐/스케줄러 해제만 하고 `flush_tx_buffer()`를 호출하지 않는다 |

## 4. 빠진 것

| # | 빠진 것 | 어디 / 왜 중요한가 |
|---|---|---|
| G1 | **스킬 설계** | §10.3 세 줄뿐. SKILL.md 본문, references 목록·내용, 에이전트 워크플로(status→db→관찰→dry-run→송신→정지), 금지 규칙, Codex 경로가 없다. 공통 §3-11 필수 → 내 초안 §15.5 채택 권고 |
| G2 | **승인 흐름의 주체와 UI** | `grant`, `plan hash`, scope는 좋은 개념이지만 발급 주체(사람/뷰어), 에이전트 자기 승인 차단, 뷰어 부재 시 동작(`NO_APPROVER`), 승인 대기 중 CLI 동작(보류/즉시 반환)이 없다. 안전의 핵심 고리 |
| G3 | **사람용 기본 출력** | §2·§4 예시가 모두 `--json`. 공통 §3-4는 "기본 사람용, `--json`은 기계용". 사람용 표 형식과 잘림/커서 요약 줄이 정의돼야 한다 |
| G4 | **Fixed 모드 Trace** | §5.2 "고정/자동 스크롤"은 스크롤 잠금이다. CANoe의 Fixed(ID별 1행 덮어쓰기)는 CANoe 사용자에게 기본 화면이고, 고부하에서 **생략 없이 정확한 유일한 화면**이다(ID 수에 비례). 원칙의 "고부하에서 거짓말하지 않기"와도 직결된다 |
| G5 | **에이전트에게 사용자 개입 알림** | 에이전트가 `--origin user`를 직접 물어야만 안다. 일반 trace 출력에 "사용자가 N개 송신/작업 정지" 요약 줄이 있어야 사용자 개입을 놓치지 않는다(원칙 4요소 중 '개입') |
| G6 | **뷰어 부재 경고** | 사용자가 아무 뷰어도 열지 않으면 "에이전트만 보는" 상태가 된다. `status`/송신 결과에 `viewers: 0` + 여는 방법 안내, 스킬 규칙 필요 |
| G7 | **에이전트 읽기의 가시성** | 사용자는 에이전트가 무엇을 기다리는지(`wait-for`)와 무엇을 조회했는지 볼 수 있어야 한다. Activity의 범위를 읽기/대기까지 넓힐 것 |
| G8 | **환경 등급(sim/bench/vehicle)** | profile·allowlist는 있지만 "실차" 축이 없다. 실차 채널은 기본 deny + 매번 승인처럼 다르게 다뤄야 한다 |
| G9 | **세션 기본값 규칙** | 모든 명령이 `--session`을 요구하는데, 열린 세션이 하나뿐일 때 생략 규칙이 없다. 에이전트 토큰 낭비와 오류 유발 |
| G10 | **lease 갱신 UX** | 주기 job lease 기본 5 s/최대 30 s. 에이전트는 한 턴(생각+도구 호출)에 30 s를 넘기 쉬워 주기 송신이 계속 끊긴다. `renew` 명령/자동 갱신 규칙 또는 env별 기본값이 필요 |
| G11 | **신호 송신 기본값** | "초기 전체 값 필수"는 안전하지만 IG UX가 무겁다. DBC `GenSigStartValue`(초기값)를 기본으로 쓰고, 초기값 없는 신호만 필수 입력으로 하는 절충이 필요 |
| G12 | **`loom viz` 에이전트용 요약** | 에이전트가 수치를 덤프하지 않고 범위/최소/최대만 받는 `viz summary`류가 없다 |
| G13 | **설정 파일 / 데몬 자동 기동** | 채널·정책·profile을 어디에 저장하는지(`loom.toml`), CLI가 데몬을 자동으로 띄우는지 불명확 |
| G14 | **화면 ASCII** | §2.1·§5.2가 문장/한 줄 블록 수준이라 리드가 레이아웃을 판단하기 어렵다(내 §5.2/§5.3/§2.0 화면으로 보완 가능) |
| G15 | **wait-for 식 범위** | `--name --op --value` 단일 조건만. `changed(X)`, AND 조합이 없다. 제한 AST라는 방향은 좋다 |

## 5. 더 나은 선택 (Sol vs 나)

| 쟁점 | Sol 안 | 내 안 | 추천 | 이유 |
|---|---|---|---|---|
| 데몬 구조 | supervisor + CAN worker + recorder 프로세스 + decode/query worker + gateway | 단일 loomd, 측정 후 분리 | **절충: M1부터 2프로세스** — `loomd` gateway(RPC/WS/정책/decode/viz) + `can-worker`(Bus·Notifier·ISO-TP·TX 중재·journal writer). recorder/decode의 추가 분리는 측정 후 | Sol 실측(같은 프로세스 CPU 경쟁 시 FC p99 52 ms)으로 hot path 격리 근거가 생겼다. 다섯 프로세스는 M1 걷는 뼈대에 과하다. journal을 CAN worker에 두면 gateway가 죽어도 기록이 이어진다 |
| 외부 IPC | 127.0.0.1 HTTP+WS, JSON-RPC 2.0, token, Origin/Host 검사, 쿠키 bootstrap, seq 문자열, idempotency key | 같음 + 바이너리 열 블록 | **Sol 기반, 바이너리는 측정 후** | 거의 같다. Sol의 idempotency·문자열 seq·쿠키 bootstrap이 더 꼼꼼하다. Sol 추정(5k fps ≈ 1 MB/s JSON)으로는 JSON이 M1에 충분 |
| 주소 지정 | `--session s1` 필수 | `--channel ch0` | **세션=데몬의 기록/소유 단위, 채널=이름.** 열린 세션이 1개면 생략 가능, CLI는 `--channel`만으로 동작 | Sol의 세션 개념(epoch/seq, 기록 경계)은 필요하다. 에이전트 입력 부담은 줄여야 한다 |
| 뷰어 기술 / 순서 | TUI(M1) → 웹(M3) | TUI(M1) → 웹 Trace/IG(M2) | **Sol 순서** — M2는 실HW/DBC/주기/승인(TUI), 웹은 M3에서 Graphics와 함께 | 내 M2가 너무 무겁다. 승인 UI는 TUI로 충분하다 |
| 주기 송신 엔진 | python-can task + `modify_data`, BCM 활용, `periodic_engine` capability | 자체 타이머 휠 단일 스레드 | **M0 비교 측정 후 결정, capability 모델은 Sol** | 둘 다 실측 근거가 없다(Sol 수치는 python-can 스레드 방식). 다수 job(IG 수십 개)에서 스레드 수와 지터 비교 필요 |
| 기록 canonical | JSONL chunk + manifest + seal, BLF/ASC는 export | BLF + events.jsonl 사이드카 | **Sol** + seal 시 chunk 압축(zstd 등) | origin/정책 이력이 한 레코드에 있고 crash 복구가 쉽다. 내 BLF 주안은 손실 시 마지막 블록을 잃고 사이드카 조인이 복잡하다. 디스크 사용(5k fps ≈ 3.6 GB/h 비압축 추정)은 압축으로 대응 |
| origin 표현 | agent/user/device/script + actor_id, initiator_origin, purpose | agent/user/device/replay/script + via, sim_node | **Sol enum 4종 + `initiator_origin`/`purpose` + 내 `sim_node`** | replay도 `script`+`replay_id`로 충분. 가짜 ECU는 둘 다 `device`로 표시한다 |
| origin 판정 주체 | 서버가 client role로 결정, 자칭 금지 | CLI는 agent, `--as-user` 허용 | **Sol** (내 `--as-user` 철회) | 승인·감사 신뢰성. 사용자 개입은 TUI/웹으로 한다 |
| 위험 등급 | R0–R3, 실HW 기본 read-only, profile·ID allowlist, plan hash grant | R0–R4, env(sim/bench/vehicle), ID 충돌 승격, 뷰어 전용 승인 | **병합:** 공통 R0–R4 틀(UDS와 맞춤) + 내 env 축 + Sol의 실HW 기본 read-only·profile allowlist·plan hash grant + 내 ID 충돌 승격 + Sol의 ISO-TP 예약 ID 차단 + 승인은 user role 뷰어에서만 | 서로 빈 곳을 메운다 |
| lease | 기본 5 s, 최대 30 s | 기본 10 min, renew | **env별:** sim 무제한, bench 10 min + renew, vehicle 30 s | 에이전트 턴 시간 현실 반영. 위험한 곳만 짧게 |
| 출력 상한 | 100행 기본/1000 최대/256 KiB, timeout 5 s | 200/2000/64 KB | **100행 기본, 1000 최대, 64 KiB** | 256 KiB는 수만 토큰으로 컨텍스트 보호 목적에 어긋난다 |
| 종료 코드 | 0,2,3,4(권한),5(장치/연결),6(자원/부분 손실) | 0~7 (5=승인, 6=데몬 없음) | **통합안:** 0 ok, 1 내부, 2 사용법, 3 timeout/no-match, 4 정책 거부, 5 승인 대기/거부, 6 데몬/연결, 7 장치 오류, 8 부분 손실 | 승인 대기와 거부를 에이전트가 구분해야 다음 행동(사용자에게 요청 vs 보고)이 갈린다. 부분 손실은 Sol의 좋은 추가 |
| `wait-for` 문법 | `--name --op --value`, 제한 AST | 식 문자열 | **식 문자열 → 제한 AST** (eval 금지), `--name/--op/--value` 별칭 | 에이전트가 쓰기 쉽고 안전성은 같다 |
| 신호 송신 기준값 | 전체 초기값 필수, RX 묵시 복사 금지 | `--base last|initial|zero` | **기본 DBC 초기값, 초기값 없는 신호만 필수, `last`(RX 복사)는 명시 옵션** | Sol의 안전 우려 수용 + IG 사용성 |
| TX 에코 매칭 | backend correlation 있을 때만, 아니면 별도 관측 | "seq 매칭" (모호) | **Sol** | 외부 ECU 프레임을 잘못 숨길 위험 |
| ISO-TP API 형태 | send→accepted+transfer_id, cancel, recv after_pdu_seq, lease, idempotency | UDS 비동기 이벤트 수용 | **Sol + UDS 이벤트 이름** | 취소·재시도 안전성이 낫다 |
| CANoe 제어 | 공식 CANoe MCP 우선, COM fallback, M5 | py_canoe COM 4동사 M5 | **Sol** | MCP Server 존재를 확인했다. 단 Loom 스킬에 "CANoe MCP=CANoe 설정/측정, Loom=raw 버스 공유·가시성" 역할 분담을 적을 것 |
| Python 지원 | 3.11–3.13 정식, 3.14 별도 | 미기재 | **Sol** | 개발 머신이 3.14.1이므로 CI에서 명시 필요 |
| 스킬 구조 | 3줄 | `loom`(공통) + `loom-can` + `loom-viz` + references 7/3개 | **내 안** | G1 |
| 웹 레이아웃 / TUI | 한 줄 블록, 키 `t`/`s` | 패널 ASCII, Fixed/Chrono, 승인 바, `:send`, Ctrl-X | **내 레이아웃 + Sol의 배지 규칙**(LIVE/REPLAY, lag, 생략·기록 상태 분리, `지원 안 됨`≠0) | 서로 보완. 단축키는 UART 팀과 공통으로 |

## 6. 합의 가능한 부분 (그대로 plan.md 채택 권고)

1. 단일 장치 owner(CAN 프로세스만 장치를 연다), ISO-TP는 그 안의 `NotifierBasedCanStack`, Notifier 단일 소비자, 구형 `CanStack` 금지(Sol §3.2, §7.7).
2. Notifier 콜백 안에서는 decode·JSON·파일 I/O 금지, 큐에만 넣고 반환(Sol §3.2).
3. 관측 필터 ≠ 장치 필터, 사용자 필터가 ISO-TP 프레임을 제거하면 안 됨(Sol §3.2).
4. 실시간 목표 표와 과부하 규칙(Sol §3.5) + 내 §3.12 보강(Fixed 모드 정확성, pause≠수집 정지, lag 2 s 초과 시 라이브 점프, DRIVER OVERRUN 구분).
5. 원자적 snapshot+subscribe, late attach 1 s 이내, 뷰어 접속은 장치 핸들을 추가로 열지 않음(Sol §2.5, §3.4 / 내 §3.11).
6. `display.gap`/`record.gap`/`recording_incomplete`/`count=unknown` 이벤트 체계(Sol §4.4, §6.2).
7. 시각 모델: `mono_ns`(host ingest) + device timestamp + `timestamp_source/quality`, 64-bit 값은 JSON 문자열(Sol §6.1).
8. 기록 불가 시 실HW TX fail-closed(Sol §6.2). `record stop`은 추가 export만 끄고 자동 기록은 유지(둘 다 같음).
9. Vector 공존 절차(앱 `Loom` 등록, shared/configure 모드, init 권한 검사, `listen_only`를 shared 기본으로 쓰지 않음), `flush_tx_buffer` 호출 금지(Sol §7.3).
10. backend capability 목록과 `unknown`≠`false`, unsupported 기능을 조용히 SW로 흉내 내지 않음(Sol §8).
11. MF4는 `[mf4]` extra, asammdf↔canmatrix pin 충돌은 환경 분리(Sol §7.6).
12. 플롯: 서버측 MinMaxLTTB, series당 점 상한, 알람/wait/통계는 원본 샘플로 평가, enum은 step(Sol §7.9).
13. 비신뢰 입력 처리(XML entity 차단, 크기/regex 상한, HTML 미실행), 비밀 redaction(Sol §10.3).
14. Replay: 기본 가상 target, HW replay는 plan+승인, 원본 ECU와 ID 충돌 시 차단(Sol §6.4).
15. M1 정의와 실패 판정 문구(Sol §11) + 내 M1 수용 기준 ①~⑥.
16. 데몬 내 가짜 ECU + virtual bus 두 개로 E2E 시험, 느린 viewer 하나만 gap이 나는지 시험(Sol §9.1).
17. viz 입력: wide/long CSV·JSONL, UART 추출은 UART 소유·viz는 표시, UDS 의미는 재구현하지 않음, V-UDS-1 우선(Sol §5.4, §15.3. 내 §15.3과 같음).
18. 사용자 질문 표: 두 초안 병합. Sol의 "Windows에서 agent/daemon 실행 가능?"(원격 bridge 결정)과 "CANoe 버전(20 SP2 이상이면 MCP)"을 추가.

## 7. `_xteam/can-uds-isotp.md` 미결 질문에 대한 내 최종 의견

| # | 질문 | 최종 의견 |
|---|---|---|
| 1 | ISO-TP 엔진 | can-isotp 2.0.7 `NotifierBasedCanStack`. 자체 구현 안 함. CAN worker 프로세스 안에서, CPU 소비자와 격리 |
| 2 | 패딩 기본값 | **실HW는 ECU profile에서 명시 필수**(기본값으로 숨기지 않음). profile이 없을 때 기본은 `0xCC`(sim·초기 데모). Sol의 `null` 기본은 DLC 8을 요구하는 ECU에서 조용히 무응답이 될 위험이 있다 — 의견 차이로 리드 결정 필요 |
| 3 | 같은 tx_id raw 송신 | **기본 차단**(agent·user 모두), 명시 takeover(감사 기록)만 허용. 내 R1의 "user는 경고만"은 철회(Sol 안 채택) |
| 4 | 주기 PDU(TesterPresent) | 데몬이 제공. 용도는 TesterPresent류 SF로 제한. 정지 조건 = 상주 UDS session owner의 heartbeat 끊김 **또는** lease 만료 **또는** Stop TX/bus-off/close. 일회성 CLI 종료로는 정지하지 않는다. 표시는 `origin=script`, `initiator_origin`, `purpose=tester_present`, 뷰어 기본 접기 |
| 5 | 전송 방식 | 공통 결정(JSON-RPC over WS)을 따르고, 같은 프로세스면 Python API로 같은 의미를 제공. 바이트는 hex로 통일 |
| 6 | BS 기본(추가) | 8 (can-isotp 기본값). 0은 profile에서 명시할 때만 |
| 7 | max_pdu / functional 길이(추가) | 기본 4095, 서비스 상한 64 KiB, 2^32−1 금지. functional SF 용량은 주소 방식·tx_dl별 라이브러리 계산값(normal 7/62, extended·mixed는 1바이트 감소). 고정 상수 금지 |
| 8 | 시각·P2(추가) | `tx_done`은 "driver 접수 시각 + quality"이고 wire 완료로 간주하지 않는다. 첫/마지막 프레임 시각은 frame→PDU 추적 hook으로 붙이고, 추적이 불확실하면 `estimated`. P2 판정은 UDS 측 단조 deadline + drain barrier(마감 시점까지 들어온 이벤트를 처리했음을 확인)로 한다(Sol 지적 채택) |
| 9 | unknown responder 탐색(추가) | 관측(SF 수신 기록)만. 새 주소로 FC를 자동 송신하거나 stack을 자동 생성하지 않음. 응답 ID 범위는 allowlist |
| 10 | 일정 | ISO-TP 최소판(normal 11/29, CC, virtual)은 M1 직후 **별도 병렬 트랙**으로 시작한다(Sol의 M4 고정은 UDS CAN 경로를 늦춘다). 완성판(FD, group, periodic PDU)은 M4 |

## 8. 리드 결정이 필요한 쟁점 Top 6 (우선순위 순)

1. **프로세스 수 (M1):** 2프로세스(gateway + can-worker) 권고. Sol 5프로세스 / 내 1프로세스 사이의 절충. 근거는 Sol FC 실측.
2. **승인 흐름 정의:** plan hash grant(Sol) + user role 뷰어 전용 승인(나) + `NO_APPROVER`·대기/거부 종료 코드. 이것이 없으면 M2 실HW 송신을 시작할 수 없다.
3. **기록 canonical:** JSONL chunk+seal(Sol) 채택 여부. 채택 시 압축과 5k fps 1시간 무손실 gate를 M1 수용 기준에 넣는다.
4. **ISO-TP 일정:** M1 직후 병렬 트랙(나) vs M4(Sol). UDS 팀 M1이 DoIP 경로인지 확인 후 결정.
5. **패딩 기본:** `null`(Sol) vs 실HW는 profile 필수 + 기본 `0xCC`(나). UDS 팀 의견(0xCC/0x55)과 함께 확정.
6. **lease·출력 상한·종료 코드 수치:** env별 lease, 100행/64 KiB, 통합 종료 코드 0–8. 세 팀 공통 계약(§3-4, §3-10)에 올릴 항목.

<!-- END R2 -->
