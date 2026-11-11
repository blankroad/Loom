FIX NEEDED: M1 사용자 쓰기의 lease 순서·미제공 기능 호출, cursor 소비 규칙, quarantine 회복 조건이 남아 있음

검토: [plan.md](plan.md) 1,114행, SHA-256 `1137f4ab8a8f1bc98dc998bd09ebb1fcb475cd45ef30a3e7f6f76c0b7633560f`. 검토 중 게시된 CAN worker 합의 반영본까지 포함했다. plan.md·기존 보고서는 수정하지 않았다. 아래는 **계획 정합성 판정**이며 구현 시험 결과가 아니다.

## A. P1 18건 반영 확인

S = [Sol R4 보고서](r4-check.md) F의 P1 번호, O = [Opus R4 보고서](r4-opus-check.md)의 P1 번호. **Y 14건 / 부분 4건 / N 0건**. Y는 요구의 핵심 조치가 반영됐다는 뜻이며, 교차 적용 중 생긴 모순은 B에서 별도 표시한다.

| ID | 반영 | plan.md 위치 | 반영 내용 / 부족하면 보완 문장 |
|---|---|---|---|
| S-P1-1 | Y | §7 끝 | 공개0xE0 근거·enum0xE1 불일치·ISO 원문 미대조·숫자 프로필 사용을 명시함. |
| S-P1-2 | 부분 | §3.3/3.4·D19·U-06 | quarantine/late/다른 SA 우회 금지는 반영. **“첫 timeout도 검증된 회복 관문 통과 전에는 새 epoch·일반 요청을 허용하지 않는다”** 추가 필요(B4). |
| S-P1-3 | 부분 | §1.1·§4.2·§11·U-08/10/19 | M1 dry-run·fail-stop·R2 패널, reset M2는 반영. 그러나 **“M1 스킬은 capabilities가 없는 probe/wait를 요구하지 않으며 관련 수용 시험도 같은 단계에 배치한다”**가 필요(B2). M1 R2 패널을 넣은 대안 자체에는 동의함. |
| S-P1-4 | Y | §7 D4 관문·D4·U-04/05/20 | M1 결함 시험+독립 smoke / M2 전체 회귀, 되돌림도 동일 관문 통과로 정렬됨. |
| S-P1-5 | Y | §4.3 DTC 예시 | 19 02에 format 없음, 앞선 count/검증 프로필에서 확보, 미확인 raw24bit 유지로 수정됨. |
| S-P1-6 | Y | §4.2·§10.2/10.3 | comm enable/disable·DTC on/off·IO return/01–03 및 vehicle 복구 예외가 통일됨. |
| O-P1-1 | Y | §0 불변식6·§3.6·§4.4·U-08 | origin은 연결 자격, Python API는 agent와 같은 완화, M1 뷰어 자격 반영. 단 사용자 R2 쓰기와 lease의 교차 문제는 B1. |
| O-P1-2 | Y | §1.1·§2.0 t2·§11 M1·U-10/17 | M1 R2 승인 최소판 / M2 R3 고급으로 통일됨. |
| O-P1-3 | Y | §2.0 전제/t0·§10.2/10.6·U-08 | env=sim 자동 등록, daemon 소유 가상 주소만 매칭, allow_max·rate 면제 설정 반영. |
| O-P1-4 | 부분 | §4.1·§2.0 t3·U-07 | 안정 client id·TTL24h·others_since_last는 반영. **“알림에 조회 시작 cursor를 고정해 반환하고 후속 trace는 그 cursor를 사용한다”**가 필요(B3). |
| O-P1-5 | Y | §6.4·§10.3·D20·U-02 | untrusted DB 하향 무시+감사, 사용자 정책 trusted_db 해시 예외를 명시함. |
| O-P1-6 | Y | §10.3/10.6·D22·U-02/18 | R1+ functional은 buses/gateways 범위 허가 없으면 거부. 물리 대상 허가만으로 실행하지 않음. |
| O-P1-7 | Y | §3.4·§10.3·D19/22·§9.3 | 제안한 정숙 구간 quarantine·late 기록, 0x2A/0x86 잠정 R4 반영. 다만 이것보다 강한 S-P1-2의 회복 확인 요구는 B4처럼 미완료. |
| O-P1-8 | Y | §7·D4·U-04/05/20 | 관문 시점, fallback의 gateway 동시 대상1개 제한, activation0x11/OEM 필드 시험 반영. |
| O-P1-9 | 부분 | §3.3·§10.7·D16·U-15 | active_overrides와 수동 recover는 반영. **“abort 후 quarantine과 recover의 우선순위·허용 시점을 명시하고 즉시 복구를 무조건 보장하지 않는다”** 필요(B5). |
| O-P1-10 | Y | §5.2·§10.4·U-10/17 | 포커스·Y/N·1초 지연·R3 suffix·다중 선택·명령줄 차단, 시나리오 키 n 반영. §2.2의 소문자 잔여 표기는 B8. |
| O-P1-11 | Y | §4.2 confirm·§10.2·D7·U-12 | 별도 confirm 동사·M1 ask 검증·vehicle viewer 기본값 반영. 기존 confirm 인자의 잔존과 channel enum은 B6에서 정리 필요. |
| O-P1-12 | Y | §3.1·§11 선행·U-00/05/08/11·스킬 exit7 | core 소유/임시 구현, daemon start, sim 배선, U-11의 U-08 의존성 반영. |

## B. 반영 후 남은/새 교차 모순

| ID | 위치 | 충돌과 최소 보완 |
|---|---|---|
| **B1 — M1 개입 차단** | §2.0 t2→t3→t4 ↔ §3.5 lease | t2의 agent R2 쓰기가 lease를 얻고 유지한다. t3의 user R2 쓰기는 아직 T를 누르기 전이므로 “R1 이상은 소유자만” 규칙에서 LOCKED다. **t3을 `T`로 경계에서 제어권 획득→수동 쓰기 순서로 바꾸고, t4는 유지된 user lease에서 agent 쓰기 거부를 검증**한다. 사용자 확인 완화가 lease 우회를 뜻한다고 암묵적으로 해석하지 않는다. |
| **B2 — M1이 M2 기능 요구** | §10.4/스킬 절차3·exit3 ↔ §4.2·§11·U-15/19 | 연결 직후 assumed인데 스킬은 write 전 `status --probe`를 요구한다. probe는M2다. M1 뷰어 승인 경로도 `approval wait`를 쓰지만 명령표는M2다. SPRMIB도 M1 U-06 시험 목록과 M2 U-19 배치가 엇갈린다. **M1 sim의 probe 미지원 처리(상태 가정 표시·승인)를 명시하고, approval wait는M1로 당긴다. SPRMIB는 M1 최소 구현으로 넣거나 M2 전까지 raw에서 거부하고 M1 시험에서 구분**한다. |
| **B3 — 알림 뒤 trace가 비게 됨** | §4.1 ↔ §2.0 t3·스킬 절차5 | 모든 호출에서 “마지막 본 seq”를 갱신한다면 read-did가 others_since_last=1을 반환한 직후 `trace --since last`는 그 사용자 거래를 지나친 cursor에서 시작한다. **알림에 `from_cursor`를 반환하고 `trace --since <from_cursor>`로 조회**하도록 cursor 이동·소비 시점과 예시를 함께 고친다. |
| **B4 — 시간 경과를 회복 완료로 취급** | §3.4·D19 ↔ S-P1-2 | 첫 timeout에서는 max(P2*,1s) 정숙 후 drain·새 epoch, 반복 timeout에서만 전송 회복을 한다. 정숙 구간보다 늦게 오는 이전 응답은 새 epoch로 오인할 여지가 남는다. **정숙은 최소 대기일 뿐 회복 증거가 아니며, 최초 timeout부터 전송/OEM별 검증된 회복 관문을 통과해야 재개**한다고 명시한다. CAN drain barrier는 IPC에 남은 과거 이벤트 확인이지 미래 ECU 응답 부재 증명이 아니다. |
| **B5 — recover와 quarantine** | §3.4 ↔ §3.6 recover·§10.7·D16 | abort는 모든 새 요청을 막는 quarantine에 들어가는데 D16은 recover로 “즉시 복구”한다고 한다. RPC에는 “뷰어 자격 전용 … + CLI”, CLI 표에는 일반 recover라 권한도 모호하다. **recover는 명시적인 user 제어권·현재 PDU 정리·검증된 복구 허용 조건을 거친 별도 경로로 정의하고 대기/거부를 표시한다. CLI 허용 자격도 명시**한다. 안전한 순서가 정해지기 전 “즉시”는 삭제한다. |
| **B6 — confirm/channel 이중 계약** | §3.4/3.6·§4.4 ↔ confirm 동사·§2.0 t2·§4.1 | canonical `uds.confirm`을 추가했지만 `uds.request.confirm?`와 `write_did(..., confirm=token)`이 남았다. 또한 예시는 `channel token_or_viewer`, JSON/스킬 분기는 token 또는 viewer다. **재제출은 uds.confirm으로 통일(기존 API를 유지하면 위임·같은 정책임을 명시)**하고, `r2_channel=token_or_viewer`는 정책값, `approval.channel=token|viewer`는 선택 경로로 구별한다. |
| **B7 — M3b 표기/작업 누락** | §1.1·§4.2 discover/connect/raw·§8·§9.3 ↔ §11·U-24·§15.4 | 전체 CAN-TP를M3b로 옮겼지만 앞 절은 FD/extended/mixed/functional/discover를M3로 표기한다. §11 M3의 normal29/normal-fixed도 구현 작업은 전체 주소 모드 U-24(M3b)에만 있다. **K9b 의존 기능은 모두M3b로 통일하거나 normal29/fixed의 별도 M3 작업·CAN 선행 합의를 명시**한다. |
| B8 — 작은 표기·단계 잔여 | §2.2 U2·Q6·§5.2·U-10/17 | U2는 아직 y/n, 정식 규칙은Y/N. Q6 기본값은 일괄 token_or_viewer지만 §10/D7은vehicle=viewer다. 1초 승인 입력 무시는 모든 패널 규칙인데 작업표에는 R3 U-17에만 명시돼 있다. **표기/기본값을 통일하고 M1 R2에도 포커스·입력 지연 규칙 적용을 U-10에 명시**한다. |

종료 코드0–7과 comm/DTC-setting/IO의 기본 등급은 서로 일치한다. 복구 R1의 vehicle 승격 제외도 반영됐다. U-00·U-11 안전 의존성·reset의 M2 이동은 확인했다. 작업 ID 중복이나 존재하지 않는 U-ID 참조는 찾지 못했다.

## C. 02 핵심 원칙 최종 판정

| 항목 | 판정 | 위치 / 이유 |
|---|---|---|
| 1 맨 앞 4요소 표 | PASS | §0 표·불변식에 모두 있음. |
| 2 실행 가능한 종단 간 시나리오 | **FAIL** | §2.0의 user R2 쓰기가 현재 lease 순서로는 실행되지 않음(B1). 명령·응답·화면·지연의 서술 자체는 충분함. |
| 3 실시간 목표·정직한 과부하 표시 | PASS | §5.4/5.5·§9.3의 렌더 SLO·표본·gap/drop/lag/기록 장애. |
| 4 M1 걷는 뼈대 | **FAIL** | 실제 TUI+입력 수용은 유지됐으나 M1 스킬이 M2 probe/wait를 요구하고 cursor 소비도 미정이라 지정 경로가 닫히지 않음(B2/B3). |
| 5 같은 세션/스트림·숨은 연결 금지 | PASS | 상주 gateway 소유·CAN worker PDU RPC·snapshot/live·연결 자격 origin. |
| 6 비기여 기능 후순위/비범위 | PASS | §1·§11: DB 고급/웹/flash 후순위, DoIP M1 독립. M3b 표기만 B7대로 정렬 필요. |

**B1–B7을 반영하고 B8 표기를 정리한 뒤 재확정 필요.** 특히 B1/B2/B3이 해소되기 전에는 02 체크리스트 전체 PASS나 FINAL PASS로 표시할 수 없다.
