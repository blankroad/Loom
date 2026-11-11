PASS with fixes — 반드시 고칠 것(P1) 7건.

# CAN R4 최종 점검 — can-sol

점검일: 2026-11-09. 대상은 `docs/plan/can/plan.md` 1,031줄 전체다.
대상 SHA-256: `2a906bd54588ec37bc7effa753d32202b10f730c7003974cc95e8b4f5f258296`.
최우선 원칙·공통 브리프 §3/§4·CAN 브리프와 현재 타팀 계약을 대조했다.
`can-uds-isotp.md`는 계획 입력의 v0.2보다 새 버전인 **v0.3**이다. 기존 `can 의견`은 아직 v0.2 기준이다.
R1/R2 원시 결과·패키지 metadata·설치 소스를 다시 확인했다. 새 HW 시험이나 성능 재측정은 하지 않았다.
결정적 산술 확인만 `spikes/can/r4_audit.py`로 수행하고 결과를 `r4-audit.json`에 남겼다.
계획서·타팀 파일은 직접 수정하지 않았다. 아래 수정은 리드에게 요청한다.

## 반드시 고칠 것 (P1)

| ID | 위치 | 문제 | 수정 제안 — 문장 수준 |
|---|---|---|---|
| P1-01 | §3.8 STOP, §9.3-7, §10.6, §15.2 cancel | 목표는 STOP 이후 신규 submit 0인데 동작은 ISO-TP의 **신규 요청만** 차단한다. 이미 진행 중인 CF 송신·수신에 대한 FC와 BCM 작업이 계속될 수 있다. `cancel(transfer_id)`만으로 채널의 송수신 전체 abort도 정의되지 않는다. | “STOP은 worker의 최종 send 경로에 TX latch를 먼저 걸고, 활성 ISO-TP 송수신 및 대기 PDU·주기/BCM 작업을 중단한다. 이후 CF/FC를 포함한 신규 driver submit은 0이다. `isotp.abort(isotp_id)`를 제공하고 이미 driver에 접수한 잔여 송신은 별도 보고한다. 재개는 명시 동작으로만 허용한다.” |
| P1-02 | §3.2/3.3 worker seq 부여, §3.5 전역 seq, §6.1, CR3 | CAN worker가 seq를 부여하면서 gateway의 승인/Activity 및 UART/UDS까지 하나의 전역 seq라고 한다. 할당 주체·재시작 범위가 없어 중복 방지, snapshot high-water, gap 판정의 의미가 확정되지 않았다. | “원본 event ID는 `(producer_id, producer_epoch, source_seq)`로 worker가 할당하고 저널에 보존한다. gateway가 모든 도메인에 단일 `stream_seq`를 부여하며 구독 커서·snapshot은 `stream_epoch/stream_seq`를 사용한다. CAN 원본 seq와 gateway 전달 순번은 구분한다.” 또는 동등한 단일 할당 설계를 CR3에서 명시한다. |
| P1-03 | §3.3/3.6 배치, §11 M2/M4, §12 K10/K14, §15.2, xteam v0.3 §11 | 계획과 `can 의견`은 v0.2까지만 수용했다. v0.3의 원시 프레임 request_id 주석, 채널 abort·서버 채널·결함 훅·async 시그니처·PDU 우선 전달에 대한 수용/거절 및 일정이 없다. 일반 40ms frame 배치만으로는 PDU 전달 p99≤5ms와 양립하지 않는다. | “v0.3 delta를 수용표에 추가한다. CAN M2/K10은 R-TP-06/07/08/12/14 및 최소13·15, M4/K14는04/11·확대13을 담당한다. PDU·rx_start·오류·STOP은 raw 40ms 배치와 분리된 우선 제어 큐로 전달한다. UDS M1b/M3와 CAN M2/M4의 대응을 양 팀이 확인한다.” |
| P1-04 | §3.6 ‘CPU-bound 없음’, §4.5 JSON 추정, §6.2 용량, §13 R5, D3 | 200B/frame·3.6GB/h는 이전 spike의 축약 schema 가정이다. 현재 §6.1 예제는 compact JSONL **540B/행**이다. 또한 worker 안 JSON 직렬화·저널 writer·SimNode가 있으므로 CPU 경쟁을 설계상 제거했다고 말할 수 없다. | “현재 §6.1의 1B payload 예제는 540B/행으로 5kfps에서 약9.72GB/h·2.70MB/s다(비압축 산술, 최대값 아님). 이전 0.98MB/s·CPU9ms/s는 축약 schema microbench이며 현 schema의 예산은 M0에서 재측정한다. decode/export는 격리했지만 journal/SimNode의 GIL 비용은 worker 내 부하 시험으로 검증한다.” |
| P1-05 | §6.5 allow_ids 주석, §10.2 R4, §14 Q6, D24 | Q6은 allow_ids 설정 전 read-only인데 설정 주석은 빈 allow_ids면 deny_ids 외 전체 허용이다. deny_ids/allow_ids 밖도 ‘매번 승인’으로 송신 가능한지, 하드 차단인지 불분명하다. D24의 “allow_ids로 이미 위험 제한” 근거도 무너진다. | “실HW에서 미설정/빈 `allow_ids`는 TX 거절이다. `deny_ids` 또는 allowlist 밖은 일반 R4 승인으로 우회하지 않는 하드 차단이다. 정책 변경은 별도 명시 설정·감사로만 가능하다. agent/user 및 ISO-TP 예약 ID에 동일 적용한다.” |
| P1-06 | §4.3 `--base last`, §10.2, §13 R12, §14 Q4, D17 | Q4는 E2E 메시지를 plugin 전 경고+차단하지만 R12는 경고만, D17/R4는 수신 CRC/counter 복사를 승인하면 허용하는 것으로 읽힌다. 승인으로 올바른 CRC/alive counter가 만들어지지 않는다. | “E2E/CRC/counter 보호가 선언된 메시지는 검증된 encoder/plugin이 없으면 실HW 신호 송신을 거절한다. `--base last`와 R4 승인은 이 차단을 해제하지 않는다. 비보호 메시지만 fresh·mux·필수값·revision 검증 후 RX 복사를 허용한다.” |
| P1-07 | §15.2 P2 drain barrier, §3.3 Notifier/stack 분리, §12 K10 | ‘seq X까지 처리 완료’는 raw 큐를 처리했다는 것인지 transport의 rx_start 발행까지 끝났다는 것인지 모호하다. 두 경로가 다르면 raw watermark를 받고도 늦게 나온 rx_start 때문에 P2를 오판할 수 있다. RPC/응답 필드와 barrier 자체 대기 상한도 없다. | “barrier는 `{isotp_id, epoch, deadline_mono_ns}`를 받고 해당 host-ingest 시간 범위의 transport 처리와 rx_start 이벤트 발행 완료를 확인한 뒤 `{covered_ingest_seq, transport_event_seq, covered_mono_ns, complete}`를 반환한다. raw 큐 drain만으로 완료라 하지 않는다. barrier 대기도 유한하며 실패 시 판정 불확실을 보고한다. 이전 wire 응답의 req_id가 확정되지 않으면 추정/미확인으로 남긴다.” |

P1-03은 새로 들어온 타팀 요구에 대한 통합 누락이지 R3 작성 당시의 사실 오류로 보지는 않는다.
각 항목은 한 계약 문제로 세었다. 같은 문제의 여러 절 수정은 별도 P1로 중복 집계하지 않았다.
P1-02의 필드명은 구체 대안이다. 리드가 다른 순번 설계를 택해도 단일 권위·복구·cursor 의미를 만족하면 된다.
P1-05는 사용자에게 새 승인 단계를 요구하는 것이 아니라 문서 내부의 안전 기본값 충돌을 해소하는 요청이다.

### P1 근거와 최소 수용 시험

| ID | 확인 근거 | 수정 후 필요한 시험 |
|---|---|---|
| 01 | 설치 `isotp/protocol.py:1691–1693`의 기본 txfn은 `bus.send`를 직접 호출한다. `stop_sending/stop_receiving`도 별도 API이며 완료 대기가 최대1초인 경로가 있다(1649–1669). heap 해제만으로 stack 전체 정지를 증명할 수 없다. | FF/CF 진행 중, 수신 후 FC 직전, BCM 주기 중, 승인 대기 중 각각 STOP. latch 확인 후 submit 0, pending 승인 취소, 잔여 가능성 보고, 자동 재개 없음. |
| 02 | plan §3.3·§12 K1은 worker seq, §3.5는 도메인 전역 seq, §6.2는 worker 원장이다. 둘을 연결하는 할당 규칙이 없다. | CAN/UDS/UART/Activity 동시 발행과 worker/gateway 재시작에서 event ID 유일성, 필터 구독의 합법적 순번 건너뜀, snapshot/live 무중복·무누락. |
| 03 | `_xteam/can-uds-isotp.md` §11의 R-TP-04/06/07/08/11/12/13/14/15. `can 의견`과 plan §15.2는 v0.2 선언이다. | 100% bus load/raw backlog 조건에서 rx_start/PDU/STOP 우선 전달, request_id 추적, 서버 virtual ECU, 최소 fault fixture, UDS M1b 계약 시험. |
| 04 | `r4-audit.json`: 540B×5,000×3,600=9,720,000,000B/h. `r1-results.json`의 batch는200개39,091B로 다른 schema다. | 현 schema·FD 길이·두 viewer·SimNode·journal 회전 조건의 CPU/RSS/대기열/FC 측정. 직렬화 시간을 이전9ms/s로 대체하지 않음. |
| 05 | §6.5 “비면 deny_ids 외 전부”와 §14 Q6 “설정 전 read-only”가 직접 충돌한다. | allowlist 미설정/빈 값/범위 밖/deny_ids/agent/user 모두 deny, 허용된 프로파일만 실행. |
| 06 | §14 Q4는 차단, §13 R12는 경고, D17은 last+R4 허용이다. | protected message에 start값·RX값·승인 token을 주어도 plugin 부재 시 byte submit 0. 비보호 메시지의 명시 RX 복사는 별도 시험. |
| 07 | raw Notifier→RX queue와 ISO-TP thread 경로는 동기 완료 관계가 아니다. 계약에는 transport watermark의 정의가 없다. | FF는 deadline 이전 ingest, rx_start 발행은 지연되는 fixture. raw drain만 먼저 완료돼도 조기 timeout하지 않음. barrier 단절/만료를 유한 오류로 처리. |

## 고치면 좋은 것 (P2)

| ID | 위치 | 문제 | 수정 제안 |
|---|---|---|---|
| P2-01 | §6.2, D2 | BLF 18,080누락 숫자는 맞지만 “마지막 container 미기록”으로 원인을 확정할 증거는 없다. 단일 실행을 일반 유실률처럼 읽을 여지도 있다. | “stop/명시 flush 없는 child abrupt exit 1회에서100,000중81,920개만 reader가 복구했고 오류는 없었다. 원인별 분해·전원 장애 내구성은 미측정”으로 바꾼다. |
| P2-02 | §6.2 flush/fsync | 1초 file flush는 OS 전원 장애까지1초 내구성을 보장하지 않는다. seal은60초 조건이고 manifest/압축 교체 순서도 요약뿐이다. | process-crash RPO와 host/power-loss RPO를 구분하고 fsync 주기·manifest commit 순서·압축본 seal 전 원본 유지 규칙을 적는다. |
| P2-03 | §3.8 vs §11 M1/M3 | 목표 표의 p99·TX 표시·UART200ms 조건이 milestone gate에서 일부 빠진다. M1 논리는3OS, 성능은 기준 머신이라는 구분도 흐리다. | M1/M3 수용 표에 “§3.8 해당 행 전체 및 V-UART-0, V-UDS-0의 p95/p99를 기준 머신에서 측정”을 추가한다.3OS는 논리 시험으로 명시. |
| P2-04 | §4.1 follow | `--timeout 또는 --count`는 출력 개수만 유한하고 무응답 때 시간은 무한일 수 있다. 기본10초가 count와 함께도 적용되는지 명시하지 않았다. | “count와 무관하게 기본10초 timeout이 항상 적용되고 둘 중 먼저 도달하면 종료”로 정한다. |
| P2-05 | §3.5 @last | 응답 전송 후 자동 cursor 전진은 연결 손실·동시 CLI에서 유실/중복 의미가 미정이다. | query cursor/revision을 응답에 넣고 ACK/명시 next_cursor 재사용 정책을 정한다. client 이름만으로 두 호출이 서로 전진시키지 않게 한다. |
| P2-06 | §7 gateway 표 | Starlette를 선택 후보로 쓰지만 버전·라이선스·플랫폼 확인 행은 uvicorn/websockets만 있다. | 최종 선택은 메인 결정으로 유지하되 Starlette의 검증 버전·metadata를 추가하고 lock 담당을 지정한다. |
| P2-07 | §1.3 CANoe | 공식 MCP/COM 판단은 충분하지만 브리프가 요청한 XIL API의 범위 판단이 통합본에서 사라졌다. | “XIL은 HIL/testbench 연동 후보이나 설치 API·entitlement·Python binding 미확인, M1–M4 비범위” 한 문장을 복구한다. |
| P2-08 | §8 Linux vcan, §9.2 | 매트릭스는 M2 정식 CI인데 시험표는 권한 없으면 선택 skip이다. | vcan 인증 runner를 정식 gate로 확보하거나 해당 backend는 인증 전 조건부로 둔다.3OS virtual 성공을 vcan 통과로 대체하지 않는 기존 원칙은 유지. |
| P2-09 | §7.1/§8.1, §11 M4 | shared에서 init 권한 부여를 설정 write 전에 거절하려면 stock constructor를 감싼 사후 검사만으로 충분하지 않다. FD 제약은 이미 잘 표시했으나 구체 경계가 필요하다. | adapter의 pre-write hook/별도 open 경로가 선행 작업임을 K13에 적고 얇은 adapter 없는 shared-FD는 미지원으로 보고한다. |
| P2-10 | §5.3·§11·§12·D23 | M2 Vector 대체 가능 여부와 M4 정식의 구분은 설명 가능하지만 배치 표현이 다르다. | “M2 단독 configure smoke는 PCAN/Vector 중1종, M4 shared CC/FD 인증은 Vector 필수”로 통일한다. |
| P2-11 | §2.1 A2, §4.3 sig set | CAS라고 하지만 `--expected-rev`는 선택이고 A2에서 생략한다. 무조건 덮어쓰기처럼 구현될 수 있다. | 생략 시 서버가 읽은 revision에 CAS를 적용하고 충돌이면 거절하거나, agent에는 expected-rev를 필수로 하며 예제를 고친다. |
| P2-12 | §15.2·D11 | “패딩 바이트 차이는 거의 문제가 안 됨”과 “실 ECU 다수”는 확보한 자료/스파이크로 뒷받침되지 않는다. | “0xCC는 초기 호환성 정책값이며 ECU별 DLC/padding byte 검증을 대신하지 않는다. profile 우선, actual bytes/tx_dl을 공개”로 바꾼다. |

## 체크리스트 1 — 최우선 핵심 원칙 6항목

형식적 제목 유무와 실제 실행 흐름을 따로 확인했다.

| 항목 | 판정 | 실제 충족 근거·보완 |
|---|---|---|
| 맨 앞 핵심 루프 4요소 표 | 통과 | §0이 open/read/write/viewer와 실제 장치 owner CAN worker·TUI를 각각 연결한다. |
| 종단 간 시나리오 | 통과 | §2.0에 `send 0x300#01`→0x301 응답→AGT/DEV Trace와200ms 목표→`:send 0x300#00`→agent USR/NOTE 조회 및 CLI/TUI 목업이 있다. |
| 실시간 목표·고부하 정직 표시 | 통과, P1/P2 보완 | §3.8 정상5kfps/두 viewer/ISO-TP와20kfps·100% bus load를 분리하고 display/record gap·unknown·lag·다운샘플을 명시한다. 용량/PDU 우선 경로는 P1-03/04 수정 필요. |
| M1 walking skeleton | 통과 | §11 M1·§12.1에 virtual open, agent send/trace, fake ECU, TUI live와 사용자 송신, 저널·STOP이 포함된다. DBC/웹/실HW를 미뤘다. |
| 같은 세션·늦은 참가·숨은 연결 금지 | 통과, 순번 계약 보완 | §3.7 I1–I6와§9.3에 단일 장치 owner·원자 snapshot+live·사용자 개입 NOTE가 있다. 실제 cross-process seq 권위는 P1-02에서 확정해야 한다. |
| 부수 기능 후순위/비범위 | 통과 | §1.2·§11 M5에 Replay/CANoe 제어/ARXML/Qt 등 부수 기능과 비범위가 명확하다. |

여섯 요구의 계획상 흐름은 충족하므로 FAIL로 판정하지 않았다.
다만 P1을 미해결 상태로 두고 공유 스트림/STOP/UDS 타이밍 계약이 완성됐다고 표기해서는 안 된다.
M1에서 헤드리스 구독자 수신만으로 끝내지 않도록 §9.3-6의 실제 TUI refresh 계측을 유지한다.

## 체크리스트 2 — 공통 브리프 §4의 15개 절

| # | 요구 절 | 존재 | 충분성·근거 |
|---|---|---|---|
| 1 | 범위 / 비범위 | 있음 | 충분. §1.1–1.3 우선순위·M 배치·CANoe 전체 대체 제외. XIL 판단만 P2-07. |
| 2 | 사용 시나리오 | 있음 | 충분. §2.0 E2E,agent A1–A6,user U1–U6로 각각3개 이상. 명령·화면·개입이 구체적. |
| 3 | 아키텍처 | 있음 | 보완 필요. §3.1–3.8 process/RX/TX/IPC/가시성은 명확하나 전역 seq·control 우선·worker 잔여 CPU 비용은 P1-02/03/04. |
| 4 | CLI / API 명세 초안 | 있음 | 대체로 충분. §4.1–4.5에 인자·응답·종료 코드·이벤트. STOP/abort와 barrier는 P1-01/07, 시간 상한은 P2-04. |
| 5 | 사용자 뷰어 UX | 있음 | 충분. §5 TUI/웹/Qt 비교 후 결정,Trace/Graphics/Data/State/IG/승인/STOP. M3 p99 gate는 P2-03. |
| 6 | 데이터/기록 포맷 | 있음 | 보완 필요. envelope/manifest/정본/export·long/wide가 구체적. 용량 540B와 내구성 창은 P1-04/P2-02. |
| 7 | 라이브러리 선택과 근거 | 있음 | 대체로 충분. §7·7.1 버전·license·동작은 snapshot과 일치. Starlette 선택 후보 정보만 P2-06. |
| 8 | HW/OS 지원 매트릭스 | 있음 | 충분, 인증 상태 표현 보강. §8/8.1에서Windows x64·macOS 실험적·SW/HW 시각·shared FD 제한을 구분. |
| 9 | 시뮬레이션 & 테스트 | 있음 | 충분. §9 fake ECU·3OS·부하·실HW·장애·핵심 루프 시험. vcan runner gate는 P2-08. |
| 10 | 안전 / 가드레일 | 있음 | 보완 필요. §10.1–10.9 role/plan/lease/rate/감사는 구체적. STOP·allowlist·protected TX는 P1-01/05/06. |
| 11 | 마일스톤 | 있음 | 충분. M1 네 요소를 우선하고 M2 실HW/최소TP→M3웹→M4공존/전체TP로 나눈다. v0.3 delta는 P1-03. |
| 12 | 구현 단계 작업 분해 | 있음 | 충분. §12.1/12.2 추천 O/S·의존성·산출물이 있고 Q1에 저널/정책 의존 반영. K10/K14의 신규 계약만 보강. |
| 13 | 리스크 | 있음 | 충분. §13 타이밍·권한·기록·timestamp·확장 범위가 반영됨. R5/R12는 P1-04/06에 맞춰 수정. |
| 14 | 사용자 질문·기본값 | 있음 | 충분. §14 Q1–Q13에 HW/OS/승인/bitrate·DB·기록·패딩 등이 있다. Q6와 본문 충돌은 P1-05. |
| 15 | 공통 계약 변경 / 타팀 의존성 | 있음 | 보완 필요. CR1–CR8·UDS/viz·스킬 구조는 충분하나 xteam v0.3와 P2 barrier를 동기화해야 함. |

16번 결정 기록과 마지막 핵심 원칙 자기점검도 존재한다.
필수 절을 통째로 누락한 것은 없다. 보완 대상은 대부분 같은 계약의 절간 불일치다.

## 체크리스트 3 — CAN 브리프 ‘꼭 다룰 것’

| 브리프 항목 | 판정 | 위치·누락 여부 |
|---|---|---|
| python-can vector/pcan/virtual/socketcan/kvaser | 충족 | §7.1·§8 backend별 지원·timestamp·주기 송신; Kvaser 후순위도 명시. |
| FD·다채널·nominal/data bitrate·HW timestamp | 충족 | §4.3 open, §7.1, §8 및 CC/FD fixture. 지원 사실과 현장 인증을 구분한다. |
| CANoe 같은 Vector 채널 공존·앱 할당/init | 충족 | §8.1 logical→physical, init 요청/실부여, CC 후보/FD 미확정, 종료 순서 시험. pre-write adapter 경계는 P2-09. |
| CANoe COM/공식 Python/XIL 범위 | 부분 | §1.3·§7·D20 공식 MCP→비공식 COM, PyPI canoe 오인 방지. XIL이 통합본에서 빠져 P2-07. |
| DBC encode/decode·mux·값 테이블 | 충족 | §4.3·§7·§9.2. 초기값·revision·비활성 mux 검증 포함. protected TX는 P1-06. |
| ARXML/LDF 범위 판단 | 충족 | §1.2/1.3 ARXML M5 부분, LDF/LIN 비범위. |
| agent 단발/주기/신호 patch | 충족 | §2 A2·§4.3·§10.5 heap/BCM·lease·CAS. |
| trace filter/tail/since·신호 get/wait | 대체로 충족 | §4.3의 max/since/last로 유한 조회 가능. `--tail N` 자체가 없어도 요구 행동은 있음. count timeout은 P2-04. |
| bus load·에러/상태·기록 start/stop | 충족 | §4.3 stats·record clip/mark·§6.2 자동 journal. 추가 clip과 자동 기록 구분. |
| Trace/Graphics/Data/IG/Statistics/Replay | 충족 | §5.3·§11에 모두 있고 Replay는 M5. herdr TUI는 M1. |
| 웹 vs 데스크톱 vs TUI 비교·결정 | 충족 | §5.1에서 기능/배포/공유 근거로 웹+TUI 선택, 근거 없는 성능 서열 삭제. |
| 범용 viz 파일6종·live·한 줄 명령 | 충족 | §4.4·§6.3/6.4·§15.3의 CSV/JSONL/Parquet/BLF/ASC/MF4/live, 선택 MF4 일정 명시. |
| million-scale·LTTB·다운샘플 | 충족 | §3.8·§6.4·§11 M3의 min/max·MinMaxLTTB 옵션·원본 cursor, 1M/10M. |
| UART/UDS viz 요구 수용 | 충족 | §15.3과 현재 viz xteam의 CAN 응답이 일치. UART live 200ms·UDS 100/200ms를 M3 gate로 재연결(P2-03). |
| BLF/ASC/MF4 입출력 | 충족 | §6.3 표, BLF M2/MF4 M4 extra·append 제약·sidecar. |
| CAN-TP 호스팅·STmin/FC/N_Bs/N_Cr | 부분 | CAN worker 안 호스팅과 §15.2 파라미터는 충족. v0.3/abort/우선 큐/barrier는 P1-01/03/07. |
| virtual+DBC 가짜 ECU·HW 없는 CI | 충족 | §9·M1 raw→M2 DBC, virtual 동일 process 제약·별도 ECU vcan 대안 명시. |
| mac 개발/Windows 실HW·PCBUSB | 충족 | §8 조건부/실험적 등급·드라이버 라이선스·미인증 OS가 명확함. |

## 체크리스트 4 — 사실·실측 정확성

| 대상 | 대조 결과 | 판정 |
|---|---|---|
| §3.1 FF→FC p99 1.49→51.9ms, max60.5ms | R1 1.486→51.862708, max60.502584와 반올림 일치. host send hook 값이며 wire 아님을 유지. | 일치 |
| §3.1 256B RTT p99 9.6→253.8ms | R1 9.565833→253.774166. STmin0/BS0, 조건별50회. | 일치 |
| §3.1 10ms 오차 p99 4.6→46.8ms | R2 task0x100의 4.555958→46.833291. task0x101은 6.776625→59.752792. §10.5의 범위가 더 완전함. | 일치, task 한정 |
| UDS Pipe 22F190 p99 3.09ms | `spikes/uds/evidence/roundtrip.json`의 `ipc_isotp_uds_roundtrip.p99_ms=3.087959`.100회, 최대5.972416. 순수 Pipe ping p99는2.003208로 별개다. | 일치, 5ms 전달 보증 아님 |
| §6.2 BLF 처리량/누락 | R2 100,000/0.988155s≈101,199fps, abrupt81,920 읽힘/18,080 누락. | 숫자 일치, 원인 해석 P2-01 |
| §6.2 200B·3.6GB/h | §6.1 compact 예제540B(개행 포함),9.72GB/h. | 불일치, P1-04 |
| §7 주요14개 package 버전/license | `r2-package-evidence.json`과 일치. python-can4.6.1/can-isotp2.0.7/cantools45.1.2/asammdf8.8.32 등. | 일치 |
| §7 asammdf/canmatrix | asammdf requires `canmatrix[arxml,dbc]==1.2`, 독립 변환기1.3.3과 환경 분리. | 일치 |
| §7 tsdownsample wheel | 보관 metadata에 CPython3.11–3.14 대상3OS 주요 wheel 존재. | 일치, 실행 인증과 구분 |
| §7 uPlot/공식 CANoe MCP | R1/R2 조사 uPlot1.6.32 MIT, CANoe20SP2 공식 MCP와 일치. | 일치 |
| §7.1 periodic | 설치4.6.1에서 override는 SocketCAN/IXXAT, vector/pcan/kvaser는 thread. BCM modifier_callback fallback도 확인. | 일치 |
| §7.1 timestamp | Vector XL tick→float+offset, SocketCAN SO_TIMESTAMPNS kernel SW, virtual host SW. | 일치 |
| §8 PCBUSB/OS | snapshot v0.13/2024-07-28, Intel10.13+/Apple12+, 지정 PCAN-USB/FD, binary EULA. 현재 OS 인증 아님. | 일치 |
| §8.1 Vector init | `canlib.py:234–255` 요청/반환 mask, 설정 write 교집합과 일치. FD shared는 미검증 유지. | 일치 |
| §10.5 timer/heap/jitter | 실측 범위 정확, heap 자체 미측정·p99<3ms는 잠정 목표로 표시. ±1ms 보장 철회됨. | 일치 |
| §10.6 Vector flush 금지 | `canlib.py:934–943`의 추가 ID0/DLC0 프레임 경고와 일치. 문서의 high voltage 표현을 물리 전압 해석으로 확대하지 않음. | 일치 |

이번 R4는 보관한 버전의 계획 사실을 점검했다. “최신 버전”을 새 시점으로 재조회한 인증이 아니다.
R1/R2는 macOS27 arm64/CPython3.14.1의 단일 실행이다. Windows·wire timing·제품2process 배치는 여전히 미검증이다.
기존 microbench를 모두 재실행하지 않았다. 새 실행으로 기존 결과를 덮어쓰면 계획 인용과 비교 기준이 바뀌기 때문이다.

## 체크리스트 5 — 내부 일관성·타팀 계약

| 비교 | 판정 | 조치 |
|---|---|---|
| §0/§2.0/§11 M1의4요소 | 일치 | raw+TUI+사용자 송신+자동 기록을 유지. |
| §3.8 목표 vs §11 gate | 부분 | TUI p95·웹 p95·Graphics age는 일치. p99/UART/TX 표시·성능 runner 범위를 P2-03으로 연결. |
| §10.2 R1 vs §4.3 periodic/isotp R3 | 표기 차이 | 위험 열을 “실HW 기준, sim은 R1”로 주석 달면 충돌 해소. |
| §10.1 끊김 정지 vs §10.5 CLI 종료 | 대체로 일치 | 상주 owner 단절과 ephemeral CLI 종료 구분이 있다. sim 무lease의 장기 job은 명시 정리 정책 유지. |
| §10.6 신규 ISO-TP 차단 vs §9.3 신규 submit0 | 불일치 | P1-01. 진행 중 CF/FC·승인 대기·BCM까지 정의. |
| §6.5 빈 allowlist vs §14 Q6 | 불일치 | P1-05. |
| §13 R12 경고 vs §14 Q4 차단 | 불일치 | P1-06. |
| §15.2 vs xteam `can 의견` | v0.2 기준 대체로 일치 | padding0xCC/BS8/raw 차단/UDS 소유 keepalive/worker 분리/시각/cancel이 같다. |
| plan/xteam CAN 의견 vs xteam v0.3 §11 | 미합의 | P1-03. UDS의 구작업명 K9a/K9b는 현 CAN K10/K14로 매핑 명시. |
| §15.3 vs viz xteam CAN 응답 | 일치 | UART/UDS 수용 순서·원본/quality·same-stream·웹 개입 모두 존재. |
| M2 Linux vcan 정식 vs §9 선택 skip | 불명확 | P2-08. |
| D16 근거 vs §4.1 exit5 | 설명 불일치 | 근거는 승인 대기/거부 구분이라 하지만 exit5로 통합됨. machine-readable `error.code/state`로 구분한다고 설명 수정. |

## §16 결정 기록 — 반대·근거 보강 의견

| 결정 | 의견 | 짧은 대안 |
|---|---|---|
| D1: M1부터2process | 수용. CPU 부하 격리의 방향은 근거가 있다. 다만 “두 리뷰가 같은 절충 권고”는 과장이다. 내 R2는 M1 단순 배치 허용/M2 이후 decode 격리였다. | 리드의 통합 판단으로 기록하고 journal CPU·gateway control 부하를 포함한 M0 검증을 전제로 한다. |
| D2: journal 정본 | 수용. BLF의 식별 메타데이터 부재와 abrupt 시험이 근거다. **journal이면 내구성이 저절로 해결된다는 주장에는 반대**한다. | P2-02의 durability 창/manifest commit 시험. BLF 유실18%를 일반 비율로 쓰지 않는다. |
| D6: 잠정 p99<3ms | 목표로 수용, 달성 예측은 보류. 근거는 기존 fallback4.6–6.8ms라 개선 가능성을 검증한 값이 아니다. | Windows/heap 실측 전에는 지원 SLA로 게시하지 않고 M2 정확도 등급으로 보고한다. |
| D11: Classic0xCC 기본 | profile 기반 선택이면 조건부 수용. **“패딩 값 차이는 거의 문제없음”이라는 보편적 근거에는 반대**한다. ECU별 요구 자료가 없다. | 0xCC를 제품 정책값으로 명시하고 null 옵션·실효 DLC/bytes·ECU profile 검증을 유지한다. |
| D17: DBC 초기값/last+R4 | 비보호 메시지 초기화는 수용. **E2E/counter 복사를 승인으로 대체하는 해석에는 강하게 반대**한다. | P1-06처럼 validated encoder 없는 protected TX는 차단한다. |
| D24:200fps/+10% | 선택한 벤치의 상한 후보로는 수용. **allowlist/lease가 안전성을 이미 증명한다는 근거에는 반대**한다.50fps도 보편 안전값은 아니다. | 빈 allowlist deny+명시 profile/동작 범위 검증.200fps는 안전 보증이 아닌 측정/운영 한도로 표시. |

그 밖의 D3/D4/D5/D7–D10/D12–D15/D18–D23에는 강한 반대가 없다.
수정 우선순위는 STOP·전역 순번·v0.3 경계·기록 예산·TX 차단 규칙·P2 barrier다.
P1 반영 후 동일 문서 버전의 계획 계약 검토를 마칠 수 있다. 제품 구현·HW 인증 완료를 뜻하지 않는다.

<!-- END R4 -->
