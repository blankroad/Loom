# UART v1.1 반영 검증 — Sol

검토: 2026-11-10. 대상 [plan.md v3](plan.md), 마감 재확인 1,286줄, SHA-256 `11d55c4b6b8a…`. 기준은 [마스터 §2/§2.1](../00-master-plan.md)(`4e781cacd354…`), [master-review (c)](master-review.md), [핵심 원칙](../02-core-principle.md)이다. **A1–A16 우선**, 마스터가 채택하지 않은 이전 UART 제안은 위반으로 요구하지 않았다. plan.md 수정·구현·새 스파이크 없이 점검했다. 점검 중 다른 작성자의 갱신을 읽어 100행·위험 등급·쓰기 상태·일부 예시 수정은 반영 완료로 갱신했다. 이후 변경은 별도 재점검 대상이다.

**판정: 주 구조는 반영됐지만, 본문·예시의 잔여 위반/누락을 정리해야 준수 완료다.** §16.1이 우선한다고 적힌 과거 DR 표는 이력으로 취급했다. 아래 문제는 현재 동작 설명·CLI·스키마·수용 기준에 남은 것이다. F1–F8의 수정 문안은 마지막 목록에 모았다.

## 1. D1–D14 적용 점검

| 결정 | 판정 | 위치·근거 / 수정 |
|---|---|---|
| D1 단일 loomd+worker | **준수** | §0/3.1–3.2, §12 T08: gateway 하나+uart worker. §0의 과거 owner 명칭도 마감 갱신에서 정리됨. |
| D2 IPC·발견·role·구독 | **위반(잔재1곳)** | §3.9–3.10은 단일 포트/daemon.json/core.subscribe/적응형 배치/우선 큐/publish-before-reply 반영. E1 role은 수정됐지만 **§3.3 role(agent/user)**가 남음. 연결 role은 agent/viewer, origin만 user. **F4**. |
| D3 라이브=기록·두 층 순번·필수 필드 | **위반** | §3.9/6.4 일부 반영. §3.3·§6.2는 worker seq/client 구형 정의, `stream_epoch`와 `epoch` 관계가 없고 JSON 예시는 필수 필드 생략. **F3**. |
| D4 쓰기 상태 | **준수** | §3.5·DR-M6: 성공 종단 sent, takeover는 outcome_unknown+written_bytes. 점검 중 partial 현행 결과 표현이 제거되고 옛 DR은 이력으로 표시됨. |
| D5 CLI 봉투·코드·출력·client | **위반(오류 봉투)** | 100행/코드/client/cursor와 dry-run·expect 성공 예시는 마감 갱신에서 반영. **§4.1 실패 JSON에 others_since_last/warnings/meta가 없음**. 모든 결과에 붙여야 함. **F2**. |
| D6 공통 안전·정책·승인 | **준수** | §10의 환경 축·R2 confirm/R3 viewer/R4 viewer·차단/정책2층 반영. rm -rf/sf erase의 R3 잔재는 마감 갱신에서 R4로 수정됨. 사용자 제어 키의 core 게이트 의미는 F5 권장 보강. |
| D7 자동 기록·no-record·보존 | **누락(감사 보존)** | §3.12·6·10.7에 payload만 비기록/감사 유지·마스킹·30일/20GiB 있음. **감사는 무기한이며 데이터 quota/회전 삭제에서 제외**라는 조건이 없음. **F7**. |
| D8 daemon 수명주기 | **준수** | §3.10: 공통 자동 기동, LOOM_NO_AUTOSTART, loom daemon, 설정된 autostart만 포트 open. |
| D9 뷰어·키·웹 소유 | **준수** | §3.11·5.5·12 T24: STOP x/종료 q, hex g, 공통 배지·heartbeat, CAN SPA 패널/소스. §1/5.8의 “별도 웹 뷰”처럼 읽히는 옛 문장은 CAN SPA 기여로 정리 권장. |
| D10 플러그인 | **누락(등록 경로)** | §3.2 Registry 구성요소·core 파이프라인은 반영. **entry point 그룹 `loom.domains` 등록**은 적지 않음. **F7**. 콘솔 소유권은 상위 A4대로 UART로 옳게 변경. |
| D11 ISO-TP 경계 | **직접 해당 없음** | UART는 CAN/UDS ISO-TP를 구현하지 않음. 공통 gateway/Registry와 시각 연동만 사용. |
| D12 runner | **누락(조건부 연계)** | §10.3-7/DR-M16은 safe_state 기본 없음과 STOP 시 finally 미실행만 명시. profile이 safe_state를 선언할 수 있다면서 정상/error/cancel·복구 키·인터록/deny 적용은 없음. 상위 A15 적용 문안 **F8**. |
| D13 skill 배포 | **준수** | §4.8·DR-M10: skills 원본, sync 복사 기본, 선택 symlink, 공통 loom/도메인 역할 분리. 과거 DR-29는 명시적으로 대체된 이력이라 위반으로 세지 않음. |
| D14 플랫폼 | **준수** | §7/8/9: Python≥3.11·uv·macOS 개발/Windows HW·3 OS sim CI. CAN HW 구매 우선순위는 UART 직접 비범위. |

## 2. 우선 결정 A1–A16 적용 점검

| 결정 | 판정 | 위치·근거 / 수정 |
|---|---|---|
| A1 쓰기 3클래스 | **준수** | §3.2: 요청형 전체 게이트, 사용자 lease 때1회 정책·줄/1s 감사·청크 저널·worker 중재. 잡형은 향후로 분리. 다만 사용자 break/DTR 같은 **로컬 제어 동작**은 raw 입력 스트림과 구분해 F5처럼 명시. |
| A2 classify/binding/presend·event 순서 | **준수** | §3.2/3.5/10.2: 훅3개·신원/세대/profile digest·승인 후 presend·같은 request_id 행 갱신 후 reply. |
| A3 STOP | **준수** | §5.5/10.3/11(f): x/stop-all은 agent·script latch, 사용자 입력 유지. UART에 PSU형 출력유지/비상OFF가 없다는 비적용 설명 타당. |
| A4 loom/console UART 소유 | **준수** | §3.1–3.2/12 T05–T11·T07b: 처음부터 공유 패키지, EXPLORE 검토·import, CORE 소유로 이관하지 않음. |
| A5 배치·분할·대용량 | **누락(대용량 경로)** | §3.9:40ms/200건/64KiB 상한·첫 이벤트 즉시·max_batch_ms10·snapshot 분할 반영. 대용량 export/viz의 **동일 오리진 인증 HTTP GET 또는 raw_ref** 경로가 없음. 지연표도 내부IPC 한 구간을 빠뜨림. **F6/F7**. |
| A6 대기·timeout 코드 | **준수** | §4.1:단일 결과3600s, 조건 대기만3, 전송 오류7. approval 대기 timeout3과 승인 객체 만료5 구분 타당. UART 단순 바이트 송신에는 장치 ACK timeout이 없어 E_NO_RESPONSE 직접 대응 불필요. |
| A7 UART100행·확장 cursor | **준수** | §3.6·상한표·read/run synopsis가 마감 갱신에서100행으로 통일됨. off:N/send:last 허용 반영. |
| A8 R2 경로 | **준수** | §10.1·14 Q4/Q5·4.8: sim auto/bench token_or_viewer/vehicle viewer, 완화는 viewer 승인, ask 규칙 설치·Codex viewer 권장. master-review c10의 “R2 토큰 없이 항상 무송신”보다 sim auto가 우선이며 §11(f)는 bench로 한정해 옳게 수정됨. |
| A9 승인 객체 사용 기한·job lease | **준수(적용 범위)** | §5.6/10.1/DR-M7:10분은 승인 객체 사용 기한, UART scope once. UART M1–M3 잡 없음으로 시작 job renew의 별도 구현은 직접 해당 없음. |
| A10 vehicle 기본 차단 | **준수** | §10.1: vehicle R1+ 송신 기본 거부·unlock 전 금지. UDS R0 화이트리스트 예외를 UART에 잘못 확장하지 않음. |
| A11 차단 3층 | **준수** | §10.1:default_blocked unlock+viewer, hard deny 승인 우회불가, immutable 설정 해제불가. UART immutable 기본 선언 없음은 도메인 판단으로 명시됨. |
| A12 기록/감사 장애의 사용자 예외 | **준수** | §3.7-8/10.3-6/11(f):sim 포함 agent·script 새 R1+ 중단, 사용자 Ctrl-C 허용·failed 배너·stderr 감사. 핵심 사용자 개입을 막지 않음. |
| A13 손실 3층 | **준수** | §3.8/3.9/3.11/6.4:display.gap/record.gap/source.gap와 cause, 재열거 count=unknown, record.health. |
| A14 sent 의미 | **준수** | §3.5: sent=driver 수용·expect 별도 결과, takeover=outcome_unknown+written_bytes. §3.11 지표 제목도 driver 수용 기준임을 보강하면 더 명확. |
| A15 finally·복구 | **누락(조건부 경로)** | 기본 safe_state 없음은 허용되지만, profile이 선언하면 core finally·user recovery를 어떻게 따르는지 없음. **F8**. |
| A16 ISO-TP 명칭 | **직접 해당 없음** | UART API에 isotp 채널/RPC 없음. can.isotp.*를 재정의하지 않음. |

## 3. master-review (c) 1–16 반영 여부

| c번호 | 판정 | 위치·남은 것 |
|---|---|---|
| 1 종료 코드 | **반영** | §4.1/4.8 재매핑, §0 사용자 개입 코드·DR-4도 마감 갱신에서6으로 정정됨. |
| 2 이벤트/레코드/hex RPC | **일부** | §3.9/6.4 신형, RPC data hex 반영. §3.3/6.2의 seq/client 정의·필수 필드·epoch 대응은 F3. |
| 3 쓰기 상태/idempotency | **반영** | §3.5·4.1·3.9 상태조회/idempotency_key, partial 현행 결과 표현 정정됨. |
| 4 봉투/others/100행 | **일부** | 100행·dry-run/expect 성공 예시·M1 others_since_last는 마감 갱신에서 반영. §4.1 **실패 봉투**만 F2처럼 보강. |
| 5 cursor 문법 | **반영** | §3.3/4.1·SKILL 공통 문법, expect synopsis의 @last도 마감 갱신에서 정정됨. |
| 6 LOOM_CLIENT 기본 | **반영** | §3.3/4.1·DR-M5 안정 id. 이전 DR의 명시적 이력만으로 미반영 판정하지 않음. |
| 7 발견/role/사용자 CLI | **일부** | descriptor 하나·viewer RPC·키 h/A/y/c·사용자 CLI 삭제. E1 role은 수정, §3.3 agent/user만 F4. |
| 8 프로세스/daemon 명령 | **반영** | §0/3.1–3.2/3.10/4.7·DR-M1. |
| 9 공통 안전 매핑 | **반영** | §10/14·DR-M7 및 rm -rf dry-run/sf erase 화면의 R4 정정까지 확인. |
| 10 M1 안전 | **반영** | §0/11/12 T10: R3/R4 E_NO_APPROVER·exit5, R2는 A8에 따라 sim auto/bench confirm. 공통 패널은 M2. |
| 11 no-record/장애 | **반영** | §3.12/10.3/10.7:payload만 생략·감사 유지, 장애 때 사용자 입력 유지. 감사 무기한은 별도 F7 보강. |
| 12 STOP/hex/배지 | **반영** | §3.11/5.5/10.3:x STOP/g hex·배지·display.gap. |
| 13 skills sync·공통 분리 | **반영** | §4.8/12 T20·DR-M10. |
| 14 core gate/Registry/console | **일부** | §3.2/10/12의 core 소유·risk table·hooks·console 소유 반영. D10 entry point 한 줄 누락(F7). |
| 15 CORE 작업 이관 | **반영** | §12:T01/T02 삭제, T05 journal adapter, P0 T03–T07b, P1 CORE stub, T12 공통 하네스. |
| 16 지연표 재계산 | **미반영** | §3.9 worker→gateway≤10ms와 gateway 구독≤10ms를 모두 정했지만 §2.0②의35ms 표는 배치10ms 한 번만 계산. F6. |

## 4. 핵심 원칙 6항목 유지 여부

| 항목 | 판정 | 근거 |
|---|---|---|
| 앞의4요소 표 | **유지** | §0의 네 요소와 uart worker owner 표시. |
| 구체 E2E·출처·사용자 개입 | **유지** | §2.0:agent open/send→reply→late attach→Ctrl-C→다음 결과 others_since_last. E1 연결 role·사용자 충돌 코드는 갱신에서 정정됨. |
| 실시간 목표·폭주 시 정직성 | **부분** | §3.9/3.11의 적응형 배치·stdout 계측·gap·최소 서비스 수준 유지. 새 worker IPC를 포함한 예산 F6 반영 후 재판정. |
| M1 걷는 뼈대 | **유지** | §11/12:P1에서 PTY 네 요소·M1 gate. R3/R4 무승인 송신을 막아도 R1 일반 send와 사용자 입력은 가능. |
| 같은 stream·숨은 연결 금지 | **유지(스키마 보완)** | 공통 hub+worker·snapshot/live·채널 결합·no-direct 유지. F3의 epoch/seq 의미를 고정해야 재시작 후 같은 위치를 정확히 식별 가능. |
| 무관 기능 후순위 | **유지** | §1/M4. 웹은 CAN SPA 확장이고 UART가 별도 장치를 열지 않음. |

**마스터 때문에 사용자 개입이 막힌 설계는 찾지 못했다.** A1의 스트림 입력 예외, A3 STOP의 사용자 예외, A12 기록 장애 예외가 본문과 M1(f)에 들어 있다. 이를 검사하는 fixture는 core gate까지 연결해 **STOP/record failure 상태에서 새 viewer lease를 얻고 0x03이 장치에 도달**하는 것까지 확인해야 한다(기존 lease에서만 성공하는 시험으로 대체하지 않음).

## 리드 수정 목록

점검 중 **반영 완료(F1·기존 F5 등)**: read/run100행, rm -rf/sf erase R4, 사용자 충돌 exit6, E1 viewer role, expect @last, dry-run/expect 성공 봉투, outcome_unknown 표현, 옛 상태 DR의 이력 표시는 재수정 불필요다.

**필수**

1. **F2 오류 결과 봉투:** §4.1 실패 JSON에도 공통 `meta`, `warnings`, **`others_since_last`**를 넣는다. 성공 예시만 고치면 조건 timeout/정책 거절 뒤 사용자 개입을 agent가 놓친다. 빈 count=0도 명시하고 필드별 선택 여부는 core schema를 따른다.
2. **F3 순번/저널 정본:** “worker는 `(producer_id,producer_epoch,source_seq)` 할당, gateway가 `stream_epoch/stream_seq` 할당. 외부 `epoch=stream_epoch`, `seq=stream_seq`, cursor는 epoch에 바인딩”을 명시한다. §3.3/3.5/6.2/6.4의 seq/client 정의를 통일하고 필수 nullable 필드·ts.source/quality·client_id를 넣은 **완전한 공통 레코드 한 개**를 제시한다. 축약 raw 인덱스는 정본 이벤트가 아닌 sidecar라고 명시하고 source tuple↔stream tuple 매핑을 보존한다.
3. **F4 남은 연결 role:** §3.3의 `role(agent/user)`→**`role(agent/viewer)`**, 사용자 입력의 이벤트 출처만 `origin=user`로 고친다. 나머지 코드/partial/@last/E1 role 정정은 이미 확인했다.
4. **F6 지연표:** §2.0②에 **worker→gateway 내부 IPC/배치**를 추가한다. 현재 설정대로 두 구간이 각각≤10ms면 합계는 **약45ms+내부IPC 처리/스케줄링 예산**이다(5+10+10+3+17). 한 배치로 합칠 것이라면 두 구간 합≤10ms를 계약으로 명시한다. p95<100ms 자체는 유지하되 CORE 공통 하네스로 전체 경로를 측정한다.
5. **F7 빠진 공통 계약 세 줄:** §3.2에 “entry point 그룹 **loom.domains**로 UART 등록”, §3.9/4.7에 “대용량 export/viz는 **같은 오리진 인증 HTTP GET 또는 raw_ref**(단일 WS 응답에 넣지 않음)”, §6.1/10.7에 “**감사는 무기한·데이터 quota/rotation 삭제 제외**”를 추가한다. payload 인코딩 enum에 raw_ref가 있다는 것과 실제 대용량 반환 경로를 정한 것은 구분한다.
6. **F8 safe_state 연계:** §10.3에 “기본 목록이 비어 있어도 profile이 선언하면 core runner 사용: 정상/on_error/agent cancel은 승인된 finally, **사용자 STOP에는 자동 finally 없음·user 복구 키로 제시**, 모든 경로에 인터록/deny 재검사·차단 시 skip+finally_blocked, finally 승인은 시작 시 일괄 포함”을 추가한다. 별도 UART runner/승인기는 만들지 않는다.

**권장**

7. **F5 사용자 제어 키 구분:** §5.5의 break/DTR/reset은 원시 키 스트림이 아닌 **요청형 제어 동작으로 core 게이트**를 따름을 명시한다. 사용자 명시 동작을 viewer 확인으로 소비하는 UI를 만들 수 있지만, `확인 없음`이 정책/하드 차단 우회를 뜻해서는 안 된다. raw Ctrl-C는 A1/A3 예외를 유지한다. 위험 등급 예시의 R4 정정은 이미 반영됐다.
8. §3.11의 “송신 시작”은 `sent`=driver 수용 기준임을 제목에도 명시한다. publish-before-reply 역시 **허브/뷰어 구독에 발행됨**을 보장하며 실제 화면 paint 완료의 동기 장벽은 아니다. 화면 완료는 별도 지연 gate로 판단한다.
9. §1/5.8의 웹 독립 서버처럼 읽히는 문구를 CAN SPA 기여로 정리한다. §16 옛 DR은 이력이라는 표시와 §16.1 우선 규칙을 유지한다.
10. core 계약 fixture로 **에러 봉투·viewer role·source/stream epoch·STOP/기록 장애 때 새 viewer lease와 사용자 0x03 전달**을 교차 검증하도록 T12/T15에 넣는다. 수정 후 §15 “반영 완료”와 core 자체 점검 표도 함께 갱신한다.
