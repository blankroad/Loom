# R2 교차 리뷰 — uds-opus → `r1-sol.md`

작성: uds-opus, 2026-11-09. 기준: `02-core-principle.md` > `00-brief.md` §3·§4 > `uds/brief.md`, `_xteam/can-uds-isotp.md` v0.1, `_xteam/viz-requirements.md` `## uds`.
표기: "Sol §n" = `r1-sol.md` 절, "Opus §n" = 내 `r1-opus.md` 절.
요약: Sol 초안은 **전송·타이밍 사실 조사와 실패 모드 사고**(epoch 격리, `may_have_executed`, `rx_start`, 감사 불능 시 송신 정지)가 강하다. 반면 **사용자 승인 UX, 서비스·서브기능별 위험표, 스킬 설계, 진단 DB 스키마, 시나리오 위험 요약, NRC 해석**이 얇다. M1을 두 전송 모두로 잡은 것은 CAN 팀 일정과 맞지 않는다. 그 점은 Sol 스스로 §15-10에 적었다.

---

## A. 핵심 원칙(02) 충족 여부

| # | 02 항목 | Sol 판정 | 근거 / 미흡점 |
|---|---|---|---|
| 1 | 맨 앞 핵심 루프 4요소 표 | ✅ 충족 | 첫 절 표 + M1 증거 열이 좋음. |
| 2 | 종단 간 시나리오 | ✅ 충족 (부분 미흡) | Sol §2.3: connect → 22F190 → TUI 행 → 사용자 `w` 덮어쓰기 → 에이전트 재확인. 미흡: (a) "응답→화면 몇 ms"가 화면 목업의 예시 값(24 ms)뿐이고 단계별 타임라인이 없다. (b) 에이전트가 사용자 개입을 **어떤 명령으로 알아채는지**가 `trace --tail 10`뿐이라 커서 기반 "마지막 호출 이후"가 없다(Opus §4.1 `--since last`). (c) 사용자가 grant를 **어디서 어떻게** 발급하는지 화면이 없다. |
| 3 | 실시간 목표 + 고부하 정직 표시 | ✅ 충족 | p95 ≤ 100 / p99 ≤ 200 ms, 20 Hz 배치, gap 이벤트, `LIVE-DEGRADED`, 집계 카운터, capture loss 표시(Sol §5). 내 안보다 p99와 지연 구간 분해(`ecu_rx→broker→viewer→render`)가 낫다. 미흡: TesterPresent·0x78의 **화면 형태**(행 갱신인가 행 추가인가)는 문장으로만 있고 목업이 없다. |
| 4 | M1 = 걷는 뼈대 | ⚠ 부분 | 4요소 모두 M1에 있다. 다만 **두 전송(CAN-TP + DoIP)을 M1에 동시에** 요구한다(Sol §11). CAN 팀 초안은 ISO-TP 최소판을 CAN M2(`can/r1-opus.md` K9a), 전체를 M4로 둔다(CAN Sol도 M4). 이대로면 UDS M1이 타팀 일정에 묶여 "걷는 뼈대"가 늦어진다. Sol도 §15-10에서 이를 인지하고 "M1b 분리"를 대안으로 적었다. → **DoIP 단독 M1 + CAN-TP M1b(=CAN ISO-TP 최소판 직후)** 를 권장(F-1). |
| 5 | 사용자 화면이 에이전트 세션과 분리되지 않음 | ✅ 충족 (원칙 수준) | 상주 관리자가 Client·큐를 소유하고 CLI 직접 Client는 금지(Sol §3.1). snapshot watermark + `seq>snapshot` 이어받기(Sol §5)가 정확하다. 확인 필요 1건: 사용자 화면 경로가 **CAN 데몬 ↔ UDS 관리자 IPC 이벤트 + 공통 broker** 두 단계라서 "publish-before-reply"(사용자가 에이전트보다 늦게 보지 않음) 보장이 명시되지 않았다(Opus §0 불변식 2). |
| 6 | 비기여 기능 후순위/비범위 | ✅ 충족 | ODX/웹/시나리오/플래시 후순위·비범위 명확. |

숨은 연결 위험 지점: Sol §9.2 스파이크의 부모 `Client`는 harness라고 명시했으므로 문제없다. 다만 Sol §3.1 "udsoncan 동기 호출은 worker에서" 구조에서는 **Client 내부 송수신이 이벤트 발행 지점을 거치지 않는다**. 커스텀 `BaseConnection`의 `specific_send`/`specific_wait_frame`에서 반드시 tx/rx 이벤트를 발행하도록 계약에 적어야 한다. 그렇지 않으면 0x78 pending이나 udsoncan 내부 재시도가 화면에 안 보이는 구간이 생긴다.

---

## B. 틀린 사실 / 확인 필요

| # | Sol 위치 | 내용 | 판단 / 근거 |
|---|---|---|---|
| B1 | §3.5 "M2 기본은 target당 연결/worker를 직렬로 할당" | 같은 게이트웨이 뒤 여러 ECU에 **ECU마다 TCP 연결**을 여는 안 | **ISO 13400-2 라우팅 활성화와 충돌 가능.** 같은 tester SA로 두 번째 소켓에서 활성화하면 응답 코드 0x03(SA가 다른 소켓에 이미 등록·활성)으로 거부되는 것이 표준 동작이다. 근거: doipclient 1.3.0 `messages.py` `RoutingActivationResponse.ResponseCode.DeniedSARegistered = 0x03`(로컬 `.venv` 소스 확인). Sol도 "tester SA 중복 정책 먼저 확인"이라는 단서를 달았다. 하지만 기본안은 **엔티티(게이트웨이)당 소켓 1개 + 논리 주소 다중화**가 되어야 하고, ECU별 소켓은 SA를 ECU마다 달리 쓸 수 있을 때의 예외로 두어야 한다(Opus §3.5-2). |
| B2 | §7.3 표 "0xE1 CentralSecurity" | 라우팅 활성화 타입 | doipclient enum은 0xE1(소스 주석 "Table 47"). 내 기억으로 ISO 13400-2:2012에서는 **0xE0** = central security, 0xE1–0xFF = VM-specific이다. 판본 차이일 수 있어 **확인 필요**(표준 원문 대조). doipclient 값을 표준값으로 인용하지 말 것. |
| B3 | §3.3 "udsoncan 기본값은 P2=1 s, P2*=5 s, request_timeout=5 s" | | ✅ 맞음(로컬 `udsoncan/configs.py` 확인). |
| B4 | §3.4 STmin F1–F9 = 100–900 µs, N_Bs 1000 ms | | ✅ 맞음(ISO 15765-2). |
| B5 | §15-5 functional SF 최대: 주소 바이트 1이면 classic 6 / FD 61 | | ✅ 맞음. 리드 xteam §4의 "FD 62 / classic 7"은 normal 주소일 때만 맞으므로 xteam 수정이 필요하다(G). |
| B6 | §6 "VIN … 기본 마스킹" | VIN을 기본 마스킹 | 사실 오류는 아니지만 **02 원칙과 충돌 위험.** 라이브 화면과 에이전트 결과에서 VIN을 가리면 E2E 시나리오의 "같은 데이터를 본다"가 깨진다. 마스킹은 **내보내기/공유용 기록**에만 적용하고, 라이브 뷰·로컬 기록은 원문이어야 한다. seed/key 마스킹(key는 항상, seed는 정책)에는 동의. |
| B7 | §10 표 "R3 보안/플래시: 0x27" / "R2: 0x2E/2F/11/14/31" | 등급 배치 | 사실 오류는 아니고 판단 문제(D-3). 다만 **0x2F IO 제어(액추에이터 실제 구동)를 0x27(잠금 해제만, 그 자체로 물리 영향 없음)보다 낮게** 둔 것은 위험 기준(물리 영향)에 반한다. |
| B8 | §3.3 "_xteam v0.1의 first/last 시각 보고만으로는 런타임 타임아웃을 해결 못 함" | | ✅ 정확한 지적. 내 초안은 사후 판정 기준만 다뤘다. `rx_start`(FF 수신) 이벤트 필요에 동의(E). |
| B9 | §9.2 측정값 | Pipe IPC p99 2.0 ms / max 10.4 ms, ISO-TP 왕복 p99 3.1 ms, DoIP 무응답 2001.9 ms(P2 50 ms 설정) | 증거 파일(`spikes/uds/evidence/roundtrip.json`)과 일치함을 확인. **macOS·Py3.14·loopback 한정** 단서도 올바르게 달았다. |

---

## C. 빠진 것 (브리프 §4 15절 + UDS 브리프 "꼭 다룰 것")

| # | 빠진 것 | 영향 | 제안 |
|---|---|---|---|
| C1 | **서비스·서브기능별 위험 등급표**(0x10 01/02/03, 0x11 01/02/03/04, 0x28 서브기능, 0x2F 00 vs 03, 0x31 FF00/FF01, 0x85 01/02, 0x3D, 0x23, 0x87, 0x29, 0x84, 공급자 SID) | raw 분류기 구현 근거가 없음 | Opus §10.3 표 채택 |
| C2 | **승인 UX의 구체 경로**: grant를 누가 어디서 발급하나(채팅? 뷰어 키? CLI?), 에이전트가 스스로 발급하지 못하게 하는 방법 | "모델이 토큰을 자체 발급하지 못한다"(Sol §10)는 선언뿐이고 메커니즘이 없음 | Opus §10.4: R2 = 요청 바이트에 묶인 1회용 토큰(채팅 확인 + Claude Code `ask` 권한), R3 = 뷰어 전용 승인(CLI에 approve 명령 없음, 뷰어 자격). 같은 OS 사용자 위협 모델의 한계 명시 |
| C3 | **R4(금지) 단계와 해제 경로**, env(bench/vehicle) 보정 | 실차/벤치 구분 없음 | Opus §10.2 |
| C4 | **스킬 설계**: SKILL.md 개요, references 목록, 절차, 금지 규칙, Codex 경로 | 브리프 §3-11 요구. Sol §10 한 단락뿐 | Opus §10.10 |
| C5 | **진단 DB 스키마**: enum, bitfield, struct, limits, protected/risk, routine/io/dtc 정의, 표준 F1xx 내장 이름 | Sol §6은 스칼라 3줄 예시 | Opus §6.4 |
| C6 | **seed-key 플러그인 인터페이스**(시그니처, 등록, DLL 32bit 브리지, 실패 규칙) | Sol은 "provider"라는 이름만 | Opus §10.9 |
| C7 | **시나리오 YAML 형식 + 사전 위험 요약** | 브리프 "run scenario.yaml" | Opus §6.5, §10.7 |
| C8 | **NRC 해석표 + 다음 행동 힌트**(에이전트·사용자용) | 브리프 "NRC 해석" | Opus §4.3, §5.2, references/nrc.md |
| C9 | **0x28 CommunicationControl, 0x85 ControlDTCSetting** 언급 없음 | 리드 지시 목록 | C1에 포함 |
| C10 | **TUI 전체 목업**(상태 바: 세션/보안/TP/P2/lease, 승인 패널, 명령줄, 단축키) | 사용자 개입 UX 불명확 | Opus §5.2 |
| C11 | **CLI 출력 예시 다양성**: 사람용 출력 예가 없고 JSON 1개뿐. dtc list 상태 비트, status, dry-run, 0x78 진행, NRC 출력 | 브리프 §4-4 "출력 예시" | Opus §4.3 |
| C12 | **세션 상태 신뢰도**(assumed/confirmed/stale)와 외부 리셋 탐지(0xF186 probe) | 에이전트가 틀린 전제로 쓰기 | Opus §3.3 |
| C13 | **lease 세부**: shared/exclusive, seed→key 원자 실행, 기능 주소 요청의 전송 범위 락 순서 | 다중 클라이언트 경합 | Opus §3.5 |
| C14 | **유휴 만료**: 아무도 보지 않는 비기본 세션이 TP로 무기한 유지되는 것 방지 | Sol §3.4에 "lease 60 s, 절대 10 min"이 있어 대부분 커버됨. 만료 전 뷰어 경고만 추가 | 경미 |

---

## D. 더 나은 선택

### D-1. 내가 양보할 지점 (Sol 안이 더 낫다)

| 지점 | Sol 안 | 이유 / plan.md 반영 방법 |
|---|---|---|
| TesterPresent 주체 | UDS 관리자가 스케줄러 소유. CAN periodic은 idle gating·in-flight 억제·owner death·감사가 있을 때만 선택(Sol §3.4) | 내 안(CAN-TP는 `isotp.periodic_start`)은 UDS 큐·pending과 충돌할 수 있다. **양보.** 다만 "소유자가 죽으면 정지 → S3 만료로 기본 세션 복귀"라는 안전 쪽 실패 속성은 UDS 관리자 구현에도 똑같이 요구 |
| 늦은 응답 처리 | 연결 epoch, 이전 epoch 응답 격리(quarantine), 같은 요청 재전송으로 덮지 않음 | 내 초안에 없음. **채택** |
| 실행 불확정 | `may_have_executed`, stateful 재시도 0, timeout 후 쓰기 성공/실패 단정 금지 | **채택.** JSON 결과·감사·뷰어 행(⌛? 불확정 배지)에 반영 |
| P2 판정 분리 | `rx_start` 이벤트, 첫 응답 P2 / pending P2* / PDU 완료 상한(N_Cr·P6) 분리, 외부 monotonic 총 deadline | **채택.** xteam에 `rx_start` 추가 |
| 감사 불능 시 | 디스크·감사 큐 고갈이면 새 장치 송신 중단, 수신·FC는 계속 | **채택**(Opus §6.3에 추가) |
| 이벤트 origin | origin enum은 유지(agent/user/device/script), 응답은 `device` + `request_origin`, TP는 `actor/purpose` 필드 | 내 `daemon` origin 추가 요청(C3)보다 계약 변경이 작다. **양보.** 뷰어의 `·`(keepalive) 표시는 `purpose=tester_present`로 렌더 |
| 지연 계측 | 구간 분해, clock offset 추정, p99 추가 | **채택** |
| 레이트 기본값 | ECU당 5 req/s, burst 2, 폴링 5 Hz 기본 | 실ECU 보수성 면에서 낫다. 내 20 req/s는 sim 기준이었다. **양보**(정책으로 상향 가능) |
| CDD | 공개 파서 존재 확인(cdd_utils: 라이선스 미확인, gocan/cdd: MIT Go) | 내 "공개 파서 미확인"은 틀렸다. **정정.** 결론(CDD native 비범위, ODX/CSV export 경로)은 같다 |
| 라이브러리 조사 | 커밋 시각·증거 파일·py-uds/aioisotp/doip-server 등 대안 | 내 §7보다 낫다. **Sol 표 채택** |

### D-2. 내 안(또는 제3안)이 나은 지점

| 지점 | Sol 안 | 내 안 / 이유 |
|---|---|---|
| 프로세스 구성 | UDS 관리자 ↔ CAN 데몬 **별 프로세스 + IPC**(Sol §3 다이어그램, Pipe 측정) | **단일 `loomd` + 도메인 모듈**(Opus §3.1). 근거: CAN 팀 Opus 초안도 `loomd` 단일 데몬을 전제한다(`can/r1-opus.md` 테스트 행 "loomd 서브프로세스"). 승인·lease·감사·이벤트 버스·뷰어 접속점이 하나가 된다. ISO-TP는 인프로세스라 IPC 지연 0. Sol의 Pipe 측정은 분리 구성도 가능하다는 근거이므로, `IsoTpPort`를 인프로세스/IPC 동일 시그니처로 두면 둘 다 수용된다(R-TP-06). |
| udsoncan 사용 방식 | 상주 worker 안에서 동기 `Client` + 커스텀 `BaseConnection` | **코덱만 사용 + 자체 트랜잭션 엔진**(Opus §7). Sol 자신이 "외부 monotonic deadline 필요", "`rx_start`로 P2 분리", "`nrc78_callback` 재진입 금지", "서버 타이밍 자동 덮어쓰기 충돌 방지"를 요구한다. 그러면 Client의 타이머 로직을 대부분 우회하게 되어 **타이머가 두 겹**이 된다. 큐·lease·이벤트 발행·abort를 한 곳에서 통제하려면 자체 엔진이 단순하다. 단 Sol 안이 구현은 빠르다 → F-2 |
| DoIP 전송 | doipclient 조건부 채택 + timeout/TLS/pump 어댑터 또는 upstream 패치 | **자체 얇은 asyncio DoIP 전송**(헤더 8바이트, 라우팅 활성화, alive check, ACK/NACK, 논리 주소 다중화). Sol이 찾은 결함 3건(무응답 시 2 s 고정 대기, TLS hostname 누락, 상주 수신 pump 없음)과 B1(소켓 1개 다중화 필요)이 모두 doipclient 구조의 핵심부다. 패치보다 새로 쓰는 편이 작다(추정, 수백 줄). doipclient는 탐색 로직 참고와 상호운용 테스트 상대로 쓴다 |
| 승인 UX | preview → grant(주체 불명) → `--grant` | **첫 호출 = 미리보기 + 토큰(exit 3)**, R2는 채팅 확인 후 `--confirm`, R3는 뷰어 승인만. preview 단계를 별도 명령으로 강제하면 호출이 늘어나고, 승인 채널이 정해지지 않으면 에이전트가 스스로 grant를 만들 경로가 남는다 |
| 위험 등급 | 4단계, 0x27 > 0x2F, 서브기능 구분 없음 | **5단계(R0–R4) + 서브기능 + DB 상향 + env 보정**(Opus §10.2–10.3). 0x2F 03(구동)은 R3, 0x2F 00(제어 반환)은 R1. 0x27 key는 R2 + 실패 레이트 잠금 |
| R1(세션 변경) | "최초 모드 변경 의도 확인/범위 허가" | **자동 허용 + 뷰어 알림 + 감사**. extended 진입은 거의 모든 진단의 전제이고 S3로 자동 복귀하는 가역 동작이다. 매번 확인을 받으면 사람이 확인 피로에 빠져 R2/R3 확인까지 형식적으로 넘기게 된다. programming(02)만 R3 |
| M1 범위 | DoIP + CAN-TP 동시 | **DoIP 단독 M1**, CAN-TP는 CAN ISO-TP 최소판(CAN M2) 직후 M1b/M3. A-4 참조 |
| publish-before-reply | 명시 없음 | 불변식으로 명시(Opus §0). 사용자가 에이전트보다 늦게 보지 않는다는 02 원칙의 기술적 보장 |
| 에이전트의 사용자 개입 인지 | `trace --tail` | 클라이언트별 커서 `--since last` + 스킬 절차 "매 호출 후 확인"(Opus §4.1, §10.10) |
| 종료 코드 | 0/2 인자/3 전송/4 NRC/5 정책/6 busy | 확인 필요(exit 3)를 별도 코드로 둬야 에이전트가 "승인 요청" 분기를 기계적으로 처리할 수 있다. F-6 |

---

## E. 합의 가능한 부분 (그대로 plan.md에 넣어도 됨)

1. ECU 통신 코드는 상주 프로세스에만 존재. CLI 안의 udsoncan `Client` 금지. CLI 종료 ≠ 세션 종료.
2. ECU당 인플라이트 요청 1개, 대상별 직렬 큐, 사용자 takeover는 다음 트랜잭션 경계에서.
3. 세션 키에 addressing mode·source/target/extension 포함. 별칭으로 같은 ECU 잠금을 우회 불가.
4. 활성 ISO-TP 채널과 충돌하는 raw CAN 송신은 **기본 차단**(경고만으로는 부족). 둘 다 같은 주장이므로 CAN 팀에 공동 요청.
5. raw는 SID 파싱으로 같은 정책 관문 통과, unknown SID는 최고 위험·차단.
6. 0x78은 pending 이벤트이지 실패·재전송 사유가 아님. 반복 pending은 총 deadline·최대 횟수로 제한, 화면에 전부 노출.
7. stateful 재시도 0, `may_have_executed`, epoch 격리(Sol 안).
8. TesterPresent 스케줄러는 UDS 관리자 소유, 주기 2 s(S3 5 s 예), `3E 80`, in-flight 중 끼워 넣지 않음, 관리자 사망 시 정지 → S3 복귀, 자동 reset 금지.
9. `rx_start` 이벤트 추가, P2(첫 응답) / P2*(pending) / PDU 완료 상한 분리, 데몬 타임스탬프 기준 판정.
10. 실시간 목표: ECU 응답 수신 → TUI 렌더 p95 ≤ 100 ms / p99 ≤ 200 ms, 웹 p95 ≤ 200 ms. 뷰어 20 Hz 배치, gap 이벤트, lag/STALLED/DEGRADED 표시, 집계 카운터 = 입력 총량.
11. 뷰어 attach = snapshot(watermark seq) + 이후 연속 스트림. 새 연결을 만들지 않음.
12. 기록은 무손실 원시 이벤트 JSONL + 세션 manifest(버전, 주소, DB/정책 해시, clock 정보). 감사 불능이면 새 송신 정지.
13. 폴링: ECU당 1요청, 늦으면 tick 건너뛰고 `scheduled/skipped/actual_period` 보고. 실패 샘플은 `null + quality`(0으로 그리지 않음).
14. 플래시: 실차 범용 플래시 비범위, R4 기본 차단. 시뮬레이터에서만 다운로드 상태기 검증. OEM 프로필 마일스톤은 별도.
15. CDD native 비범위, ODX/PDX는 odxtools(strict 모드) 선택 의존성으로 후순위, 사용자 YAML/JSON 우선.
16. scapy(GPL-2.0)는 런타임 의존성 미채택. py-uds(Py ≥3.12)·pyvit·aioisotp 미채택.
17. IPC: 127.0.0.1 WebSocket JSON-RPC 2.0 + 토큰 + Origin 검사 + 요청 크기 상한.
18. key는 항상 마스킹, grant/토큰 원문은 로그에 남기지 않고 ID·해시·승인 주체만 기록.
19. 가상 ECU: 전송 무관 상태 모델 1개 + DoIP 어댑터 + CAN-TP 어댑터, 결정적 결함 주입(Sol §9.4 14개 목록 + Opus §9.1 YAML `faults` 형식).
20. python-can virtual은 프로세스 로컬이다. 별 프로세스 ECU 시험은 Linux vcan 또는 별도 가상 백엔드로.
21. Python 하한 3.11 유지(py-uds 때문에 올리지 않음).

---

## F. 리드 결정 필요 쟁점

| # | 쟁점 | 내 입장 (Opus) | 상대 입장 (Sol) | 판단 근거 |
|---|---|---|---|---|
| F-1 | M1 전송 범위 | DoIP(sim) 단독 M1. CAN-TP는 CAN 팀 ISO-TP 최소판(CAN M2) 직후 별도 마일스톤 | DoIP + CAN-TP 모두 M1 (대안으로 M1b 분리 언급) | CAN 두 초안 모두 ISO-TP를 M2(최소)/M4(전체)에 둠. M1이 타팀 일정에 묶이면 걷는 뼈대가 늦어짐. DoIP만으로도 4요소 전부 증명 가능 |
| F-2 | udsoncan 사용 방식 | 코덱(서비스 encode/decode, DidCodec)만, 자체 트랜잭션 엔진 | 상주 worker 안 동기 `Client` + 커스텀 `BaseConnection` | Sol 안: 스파이크로 동작 입증, 구현 빠름. 내 안: 타이머 이중화 회피, 이벤트·abort·lease 통제 단일. **절충안**: M1은 Sol 안(빠른 걷는 뼈대)으로 하되 Connection 계층에서 모든 tx/rx/pending 이벤트 발행을 계약으로 강제한다. M2 `rx_start` 통합 때 Client 타이머가 걸림돌이면 자체 엔진으로 전환. 판단 지표: M2 late-FF/긴 응답 테스트 통과 여부 |
| F-3 | 프로세스 구성 | 단일 `loomd` + 도메인 모듈, ISO-TP 인프로세스 | UDS 관리자와 CAN 데몬 별 프로세스 + IPC | 공통 계약(메인) 결정 사항. CAN Opus 초안도 `loomd` 전제. 어느 쪽이든 `IsoTpPort` 동일 시그니처(R-TP-06)면 UDS 코드는 불변 |
| F-4 | DoIP 전송 구현 | 자체 asyncio 전송, 엔티티당 소켓 1개 + 논리 주소 다중화 | doipclient 조건부 + 어댑터/패치, ECU별 연결 직렬 | B1(SA 0x03 거부), Sol이 찾은 결함 3건 모두 구조 문제. 비교: 자체 수백 줄(추정) vs 패치 유지보수. doipclient는 탐색·상호운용 테스트에 유지 |
| F-5 | 위험 등급 체계 | R0–R4 5단계, 서브기능·DB 상향·env 보정, 0x2F 구동 R3 / 0x27 R2 | R0–R3 4단계, 0x27·플래시 R3, 0x2E/2F/11/14/31 R2 | 물리 영향 기준이면 IO 제어 ≥ 보안 해제. "금지" 단계가 따로 있어야 플래시/erase를 raw로도 막는 규칙이 단순해짐 |
| F-6 | R1(세션 변경) 확인 | 자동 + 뷰어 알림 + 감사 (programming만 R3) | 최초 모드 변경 의도 확인/범위 허가 | 확인 피로 vs 보수성. extended 진입은 가역(S3 복귀). 사용자 질문 Opus §14-6으로 기본값 확인 가능 |
| F-7 | 승인 채널 | R2: 요청 바이트에 묶인 토큰 + 채팅 확인(+ Claude Code `ask`) 또는 뷰어 y. R3: 뷰어 전용(CLI approve 없음) | preview → human grant(발급 경로 미정) | 에이전트가 스스로 승인하는 경로를 막는 구체 메커니즘이 필요. 공통 승인 서비스(C5)로 CAN·UART와 통일하면 리드 간 합의 필요 |
| F-8 | 종료 코드 | 0 ok / 1 사용법 / 2 NRC / 3 확인 필요 / 4 정책 / 5 전송 / 6 LOCKED / 7 데몬 없음 | 0 / 2 인자 / 3 전송·deadline / 4 NRC / 5 정책 / 6 busy·cancel | 전 도메인 공통이 되어야 함(메인). "확인 필요"를 별도 코드로 두는 것만은 유지 요청 |
| F-9 | 이벤트 origin 확장 | (양보) `daemon` 추가 철회 → `purpose` 필드 | enum 유지, actor/purpose 필드 | 합의 가능. xteam §5의 `agent:tp-keepalive` 표기는 삭제 필요 |
| F-10 | VIN 마스킹 | 라이브·로컬 기록은 원문, 내보내기에서만 마스킹 | 기본 마스킹 | 02 원칙(사용자가 같은 데이터를 봄). 개인정보 요구가 있으면 사용자 질문 |
| F-11 | 레이트 기본값 | (양보) 5 req/s/ECU, 폴링 5 Hz 기본 | 동일 | 합의 |
| F-12 | TLS 시점 | M5(요구 확인 시) | M2(검증 어댑터) | 사용자 질문(OEM 요구) 답에 따름. 요구가 없으면 후순위 |
| F-13 | 정책 파일·env | 사용자 소유 `policy/uds.yaml`, env 미지정 = vehicle, 뷰어에서만 리로드 | 명시 없음(allowlist·rate만) | Sol에 대응 항목이 없으므로 Opus 안 채택을 제안 |

---

## G. 리드 xteam 초안 의견

`_xteam/can-uds-isotp.md` v0.1:
1. §3: `rx_start{isotp_id, req_id, ts_first_frame, expected_length}` 이벤트를 **추가**해야 한다(B8, Sol §3.3). 현재 문장 "IPC 지연은 판정에 영향 없음"은 사후 판정에만 맞다. 런타임 대기는 FF 수신 시점으로 P2를 끝내고 완료 대기로 넘어가야 한다.
2. §3: `tx_done` 타임스탬프의 출처(driver enqueue인지 wire인지, HW인지 host인지)를 `timestamp_source` 필드로 표기.
3. §4: functional SF 최대 길이를 주소 방식별로 표기. normal은 classic 7 / FD 62, extended·mixed(주소 바이트 1)는 classic 6 / FD 61.
4. §2: `max_pdu` 기본 4095, FD에서도 2³²−1 무제한 할당은 금지(Sol 동의).
5. §5 주기 PDU: UDS 쪽 결론이 "UDS 관리자 스케줄러 소유"로 바뀔 가능성이 크다(D-1). CAN 팀에 요청을 "선택(idle gating·in-flight 억제·소유자 사망 정지·감사 필수)"으로 낮추고, origin 표기 `agent:tp-keepalive`는 삭제(`purpose=tester_present`).
6. §2 소유권 키: (can_channel, tx_id, rx_id) → **addressing mode·TA/SA·address extension 포함**.
7. §8-3(같은 tx_id raw 송신): 두 워커 모두 **기본 차단** 의견. 사용자 origin에만 `--force` 허용 제안.
8. 추가 요구(Opus §15.2): `corr_id` 전달 → ISO-TP 원시 프레임 주석, `flush/abort`, 인/아웃프로세스 동일 시그니처, 서버 측 채널(sim), 결함 주입 훅, 응답자 범위 탐색은 선택에서 필수로.
9. 일정: CAN 팀 초안은 ISO-TP 최소판(normal 11bit, virtual)을 CAN M2에 둔다. UDS CAN-TP 마일스톤을 여기에 맞춰 확정해야 한다(F-1).

`_xteam/viz-requirements.md` `## uds`:
1. V-UDS-1 "행 = 트랜잭션"에 **진행 중 행 갱신 모델**을 추가해야 한다(queued → tx → pending ×N → 완료/타임아웃/불확정). 완료 레코드만 받으면 pending 동안 화면이 멈춘 것처럼 보인다(Sol §15-7, Opus §15.3 같은 의견).
2. 결과 열에 `may_have_executed`(불확정) 상태 추가.
3. 지연 목표(웹 p95 ≤ 200 ms)와 gap/drop/aggregate/STALLED 표시를 렌더러 계약에 명시.
4. 사용자 입력 경로(수동 송신, takeover, 정지, 승인)를 웹에도 요구. 승인 UI는 공통 승인 서비스와 연결.
5. DTC P/C/B/U 표기는 ECU의 DTC 형식이 SAE J2012 계열(0x59 응답의 DTCFormatIdentifier로 판별, 앞 2바이트 + FTB)일 때만 쓰고, 그 외 형식은 3바이트 hex 유지(Sol 의견 동의).
