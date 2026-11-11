# UDS R2 교차 리뷰 — Sol → Opus R1

검토 대상: [r1-opus.md](r1-opus.md), 1,175행, SHA-256 `b2fbed0b91ec6fbf2e6c46925602bf520848cd053446d1f861da1eba7476315d`. 기준은 [02 핵심 원칙](../02-core-principle.md) > 공통 브리프 §3·§4 > UDS 브리프, xteam v0.1 두 문서다. 본인 R1과 다른 파일은 수정하지 않는다. 아래 **P1은 통합 전에 고칠 계약/수용 기준**, P2는 후속 보강이다. 구현된 제품의 결함을 판정한 문서가 아니다.

**판정:** 핵심 방향은 충족한다. 상주 세션·같은 스트림·사용자 송신이 명확하고 숨은 ECU 연결 경로도 금지했다. 다만 렌더 지연의 검증 방법, bounded timing/기록, 승인·정지의 보장 범위를 고쳐야 최종 계획의 수용 기준으로 쓸 수 있다.

## A. 핵심 원칙 위반/미흡

| 02 체크리스트 | 판정 / 상대 초안 위치 | 보완 |
|---|---|---|
| 1. 맨 앞 4요소 표 | 충족 — §0 | 장치 단독 소유·읽기·쓰기·라이브 표시 담당이 모두 있다. |
| 2. 구체적 종단 간 시나리오 | 충족, M1 연결은 일부 미흡 — §2.0·§11 | VIN→응답→시간표/목업→사용자 직접 송신은 좋다. 다만 E2E-1은 읽기 중심이고 takeover는 §11에서 M2다. M1에 **agent의 실제 DID 값 변경→되읽기→user 변경→agent 재확인**을 추가하고 M1/M2 개입 범위를 맞춘다. |
| 3. 실시간 목표·고부하 정직한 표시 | 부분 충족 — §5.4·§5.5·§9.2 | p95, 집계/gap/STALLED는 충족. 그러나 이벤트 수신만으로 렌더 지연을 판정하고, 기록 무손실을 자원 조건 없이 단정한다. 실제 TUI 렌더 완료 계측·기록 실패 표시/송신 정책이 필요하다(C1·C4). |
| 4. M1 걷는 뼈대 | 범위는 충족, 수용 증거는 미흡 — §11 M1 | sim DoIP+CLI 읽기/쓰기+TUI+사용자 송신을 포함한다. **헤드리스 구독자 수신은 실제 화면 표시/입력 성공의 증거가 아니다.** U-11 의존성에 U-10 TUI를 추가하고 render hook·입력 자동화까지 수용한다. |
| 5. 같은 세션·즉시 live attach | 설계 충족, 재접속 계약 보강 — §0 불변식1·§3.1·§4.4·§11(5) | CLI/Python API 모두 데몬 경유라 숨은 연결 위반은 찾지 못했다. 늦은 attach의 snapshot watermark→연속 seq 합류 순서를 명시해야 최근 N건과 live 사이의 누락을 막는다. 거래 origin만 있는 §6.1에는 응답의 `device` 출처도 명시한다. |
| 6. 비기여 기능 후순위/비범위 | 충족 — §1.2·§1.3·§11 | ODX/TLS/웹/플래시 후순위가 분명하다. M1을 DoIP 하나로 먼저 완성하는 선택도 02 원칙 위반이 아니다. |

## B. 틀린 사실 / 사실처럼 단정한 보장

버전·날짜·주요 라이선스 표(§7)는 R1에서 실제 수집한 [PyPI metadata](../../../spikes/uds/evidence/pypi.json)와 일치한다. `50/5000 ms`도 **제품 기본 예시**라면 오류가 아니다. 모든 ECU의 고정 표준값이나 udsoncan 기본값으로 해석하지 않게 구분하면 된다.

| ID / 우선순위 | 상대 위치·문제 | 근거와 정정 |
|---|---|---|
| B1 / P1 | §0 불변식2 “publish-before-reply라 사용자가 늦게 보지 않는다” | 발행 선행은 **화면 렌더 선행을 보장하지 않는다**. §2.0 타임라인도 CLI 반환14.6ms와 TUI 허용지연100ms라 반례가 되고, §5.4는 이미 이 한계를 인정한다. 불변식을 “이벤트 발행은 RPC 완료 전, 렌더는 별도 SLO”로 통일한다. viewer ACK를 기다려 장치 처리를 묶는 해결은 피한다. |
| B2 / P1 | §3.3 L278 “CAN periodic이면 지터 없이” | 데몬 내 배치는 IPC 지연을 줄일 뿐 OS/driver/버스 지터를 없애지 않는다. [Python time.sleep](https://docs.python.org/3.14/library/time.html#time.sleep)은 scheduling에 따른 지연 가능성을 설명한다. “주기 목표·허용 지터를 실측”으로 정정한다. 현재 xteam periodic 계약에는 active UDS 요청 중 억제 조건도 없다. |
| B3 / P1 | §5.2 RTT를 P2와 비교해 정상/초과 색 지정, §6.1 first-frame 시각만으로 “판정용” | **전체 RTT와 응답 시작 P2는 다른 구간**이다. 제시간 FF 뒤 긴 CF 재조립만으로 RTT>P2가 될 수 있다. P2 시작 지연·완전 PDU RTT·pending 총경과를 분리한다. 완성 PDU에 first timestamp를 사후 첨부하는 것만으로 runtime timeout은 해결되지 않는다. [ISO-TP의 FF/CF와 상위 PDU 경계](https://docs.kernel.org/networking/iso15765-2.html), [udsoncan의 timeout 계약](https://udsoncan.readthedocs.io/en/latest/udsoncan/client.html#configuration). rx_start 보강은 G1 참조. |
| B4 / P1 | §5.2·§10.9 “NRC0x36/0x37은 남은 지연시간 표시” | 일반 `7F 27 37`에는 **남은 시간 필드가 없다**. 정확한 ECU countdown을 안다고 표현하면 안 된다. OEM 값/별도 정보가 없으면 “ECU 지연 미상; 로컬 정책 재시도 금지 시각”으로 구분한다. [부정 응답 파서](https://github.com/pylessard/python-udsoncan/blob/v1.26.1/udsoncan/Response.py), [NRC 정의](https://github.com/pylessard/python-udsoncan/blob/v1.26.1/udsoncan/ResponseCode.py). |
| B5 / P1 | §3.5-6·§15.2 R-TP-10 외부 송신 주체 감지 요구 | CAN ID는 외부 물리 송신자의 ECU/프로세스 신원을 보증하지 않는다. 데몬이 아는 로컬 송신은 확정할 수 있지만, 외부는 “예약 tx ID의 예상 밖 프레임/충돌 징후” 수준으로 표현하고 capability를 명시해야 한다. [SocketCAN의 CAN ID 및 로컬 loopback 설명](https://docs.kernel.org/networking/can.html). |
| B6 / P2 | §4.3 DTC 원시 code 열 `0x9234/0xC100/0x4561` | §6.4의 `0x923400`과 달리 원시3바이트 중 마지막 바이트가 빠졌다. 표시용 base code라면 이름을 구분하고 실제 raw24bit 및 DTC format을 유지한다. snapshot/ext 명령의 `B1234`→3바이트 변환 규칙도 정한다. [ReadDTCInformation의 DTC 인자](https://udsoncan.readthedocs.io/en/latest/udsoncan/client.html#readdtcinformation). |

**미확인 사항 보강(“틀린 사실”로 취급하지 않음):**

- §1.3의 CDD parser “존재 미확인”은 갱신 가능하다. [Python cdd_utils](https://github.com/fshoocn/cdd_utils/tree/da10c76ff3d10ee5aa0208e7cf6245f3f447ab71)는 공개되어 있으나 라이선스 허가 미확인, [Go gocan/cdd](https://github.com/tomrford/gocan/tree/0b385f99468f36715d35ee2ef6e00650184aa3f6/cdd)는 MIT다. 완전 호환은 검증하지 않았다. CDD native 비범위 선택은 유지해도 된다. CANdelaStudio의 [ODX export](https://www.vector.com/en/product/candelastudio/)는 공식 확인 사항으로 바꿀 수 있다.
- §7의 대체 `Client`/doipclient 경로는 기존 [R1 스파이크](../../../spikes/uds/evidence/roundtrip.json)로 판단한다. custom BaseConnection+별도 프로세스 CAN PDU 왕복100회와 NRC0x78 처리는 성공했다. 반면 doipclient1.3.0 기본 경로는 **P2=50ms 설정 무응답에2001.871ms**를 소모했고, [기본 TLS](../../../spikes/uds/evidence/tls-default-probe.json)는 hostname 누락 오류로 handshake 전에 실패했다. 어댑터 보강 없이 정상 RTT만으로 적합 판정하지 않는다. 이는 상대가 제안한 자체 asyncio 전송의 결함을 증명한 결과가 아니다.

## C. 빠진 것

**공통 브리프 §4의 15개 절은 전부 존재한다.** 아래는 절 누락이 아니라 통합 시 남는 명세/검증 공백이다.

| ID / 우선순위 | 관련 절·요구 | 필요한 보완 |
|---|---|---|
| C1 / P1 | §2·§5·§9·§11·§12: 핵심 loop 수용 | 실제 렌더+사용자 입력을 포함한 read/write/readback E2E, 늦은 attach와 agent CLI 종료 뒤 동일 세션 유지. 저부하 정의·표본수·timestamp clock/오차도 명시한다. U-11은 U-09/U-05뿐 아니라 U-10에 의존한다. |
| C2 / P1 | §3·§4·§9: bounded 동작 | queue 크기/바이트, 연결·activation·ACK·P2/P2*·재조립·전체 job deadline, 최대 pending 횟수, 취소 후 완료/미확정 상태를 정의한다. `--timeout` 지원만으로 반복0x78·연결 대기까지 자동 제한되지는 않는다. §10.10의0x21 자동 재시도 언급은 기본 재시도 금지와 충돌하므로 읽기만 허용된 bounded 정책으로 정한다. 0x78 처리는 대기 연장이지 재송신이 아님도 분리한다. |
| C3 / P1 | §3.5·§4 raw·§9: 응답 연관 | 물리 ECU 중복 별칭 방지, connection epoch, timeout 뒤 late response 격리·복구, raw `--no-wait`의 실제 수신 책임이 없다. **호출자에게 먼저 반환해도 데몬은 응답을 끝까지 drain하며 다음 요청을 직렬화**해야 한다. R0라고 응답을 버리면 다음 읽기와 섞일 수 있다. |
| C4 / P1 | §5.5·§6·§9: 기록·과부하 | disk full/rotation/quota·writer queue 고갈·기록 장애의 상태와 새 송신 중단 정책이 없다. “기록은 완전” 배너는 실제 capture completeness를 확인한 때만 쓴다. 최종 결과/정지/승인 이벤트가 표시 큐의 오래된 항목 삭제로 함께 사라지지 않도록 우선 보존·snapshot 회복을 정한다. |
| C5 / P1 | §10·§3.4: 승인/정지 계약 | R2 토큰은 agent에게 먼저 공개되어 **동일 요청 재제출 확인 수단**이지 daemon이 인간 동의를 관측했다는 증거는 아니다. 위협 모델의 한계 인정은 좋으나 확인된 보장과 skill/host 의존을 구분한다. 실전송 직전 대상의 물리 주소·epoch·DB/정책 hash·lease·TTL 재검사도 필요하다. abort 후 `finally: session default`(§6.5)가 새 송신을 하는 충돌, 완료 전 쓰기 `may_have_executed` 상태를 해결한다. |
| C6 / P1 | §3.3·§10.9: 상주 프로세스 응답성 | seed-key 플러그인 동기 실행의 시간/출력 상한·취소·실패 격리, 소켓 idle alive pump의 단일 수신자 계약을 명시한다. 특히 하나의 asyncio 루프에 blocking plugin/동기 Client를 직접 올리지 않는다. task group만으로 native DLL crash까지 격리할 수는 없다. |
| C7 / P2 | §6·§7: DB·라이브러리 | DB 없는 multi-DID의 응답 경계, 가변 길이 codec, struct bit/endian 규칙, importer coverage/variant와 미지원 거부가 필요하다. ODX→단순 YAML 변환은 정보 손실을 검사해야 한다. §7의 마지막 commit/유지보수 상태·시뮬레이터 대안은 본인 R1 §7 증거를 통합하면 된다. |
| C8 / P2 | §8·§9: DoIP/OS 시험 | HW 계획과 실제 합격을 ✅로 혼동하지 않게 범례 추가. parser length 상한·TCP 분할/합침·오주소/ACK·duplicate SA/socket full·UDP 다중 응답·TLS 인증 실패·timer 경계 사례와 OS/Python 별 수용 lane을 명시한다. `virtual` Bus의 프로세스 내 공유 제약도 적는다. |
| C9 / P2 | §1·§15: CAN·flash 범위 근거 | 주소별 functional SF 길이, FD max PDU cap·padding/STmin·source/extension 파라미터와 타이밍 예산은 xteam 참조만으로 미확정이다. flash 비범위 근거에는 max block length에서0x36 헤더2바이트 차감·counter wrap·DFI/ALFI·OEM 형식/검증·복구 책임까지 짧게 보강한다. |

## D. 더 나은 선택 / 양보할 부분

| 결정 | 제안·양보 및 이유 |
|---|---|
| M1 범위 | **상대안에 양보: DoIP sim 하나로4요소부터 완성**, CAN loop는 후속 M1b/M3로 분명히 명시한다. 내 R1의 두 transport 동시 M1은 CAN API 일정이 확정되지 않아 첫 live loop를 지연시킬 수 있다. |
| DoIP 소켓 | **상대안에 양보: gateway/DoIPInterface+tester SA별 소켓 공유를 기본**으로 하고 target별 queue·source LA demux를 둔다. [AUTOSAR R24-11 SWS_DoIP_00105](https://www.autosar.org/fileadmin/standards/R24-11/CP/AUTOSAR_CP_SWS_DiagnosticOverIP.pdf)는 같은 interface의 다른 활성 연결에 동일 SA가 등록됐으면 alive check 후0x03 거부를 규정한다. 내 R1의 ECU별 연결은 **서로 다른 허용 SA와 socket 자원을 확보한 경우만** 대안으로 유효하다. 최초 `max_parallel=1`에 동의한다. |
| TxnEngine / Client | “가드·이벤트 통제 때문에 Client를 못 쓴다”는 근거는 약하다. guard는 호출 전, TX/RX/pending은 custom Connection/callback에서 발행 가능하고 스파이크 근거도 있다. **M1은 재사용 경로 우선, 자체 엔진은 first-frame/P2·demux 등 구체 요구로 결정**하자는 내 입장을 유지한다. 다만 자체 runner의 검증 범위를 작게 정의한다면 상대안도 타당하다. 기능량을 숨기지 말고 NRC·echo 검증·전체 deadline 회귀를 수용한다. [Connection 계약](https://udsoncan.readthedocs.io/en/latest/udsoncan/connection.html), [Client 계약](https://udsoncan.readthedocs.io/en/latest/udsoncan/client.html). |
| 자체 DoIP | 기본 doipclient의 deadline·idle pump·TLS 공백을 고려하면 상대의 asyncio 단일 수신 pump는 설득력 있다. 그러나 “8바이트 header라 단순”은 비용 근거가 아니다. 주소 검증·ACK/UDS demux·부분 프레임·버퍼 cap·activation/alive state·TLS 회귀를 포함한 **좁은 구현/어댑터 중 수용 시험을 통과한 쪽**을 택한다. 같은 자체 client/server 왕복만으로 표준 적합 판정하지 않는다. |
| 프로세스 | 내 **CAN 장치 worker와 상주 UDS 서비스의 분리+공통 broker 접속점 하나**를 선호한다. 외부 native plugin 장애·부하 격리에 유리하고 뷰어가 반드시 두 데몬에 직접 붙어야 하는 것은 아니다. 다만 단일 loomd도 소유권/worker 경계를 지키면 핵심 원칙을 충족한다. 메인 공통 결정에 따른다. |
| TP·기록·타이밍 | 내 R1의 UDS scheduler·idle/inflight gating·lease, bounded 기록 실패 정책, rx_start와 전체 deadline 분리를 채택한다. 상대의 CAN periodic 무조건 사용·무손실 단정보다 계약이 검증 가능하다. |
| UX·상태·안전 문서 | **상대안 우세**: assumed/confirmed/stale, shared/exclusive·takeover, 서비스별 상세 분류표, 사용자 위협 모델 한계, 짧은 SKILL.md+references를 수용한다. 위험 등급 숫자는 메인과 통일하고 의미/허가 조건을 보존한다. |

## E. 합의 가능한 부분

- 상주 TargetSession이 연결·세션·보안·요청 queue·상태의 원천이며 CLI/Python API/뷰어는 같은 daemon API를 사용한다.
- ECU당 outstanding1개, security seed→key는 원자 단계, 기능 주소는 전송 범위 조정이 필요하다.
- TUI를 M1 화면으로 사용하고 웹·DID plot은 `loom.viz`로 확장한다. tx→pending→완료는 같은 거래 행 갱신으로 표시한다.
- publish-before-reply를 **발행 순서 계약**으로 채택하고 렌더 지연은 별도 수용한다. seq/gap/STALLED/LAG·집계 표시를 유지한다.
- daemon의 바이트 기준 단일 guard가 typed/raw/scenario/사용자 수동 송신에 적용되고 dry-run·대상 allowlist·레이트·감사를 제공한다.
- 실차 범용 flash·CDD native는 초기 비범위, OEM seed-key는 사용자 플러그인·실패 후 자동 반복 금지다. TLS 요구 대상은 인증 검증 전 연결을 지원 완료로 표시하지 않는다.
- JSONL 원문·manifest hash와 long-form DID sample을 쓰고, 실패 샘플을0으로 그리지 않는다. 요청자 origin과 ECU 응답 출처를 함께 보존한다.
- 소유권/감사/승인은 core 공통 기능으로 제안하되 최종 API·origin·dir·종료 코드 enum은 메인이 확정한다.

## F. 리드 결정 필요 쟁점

| 쟁점 | 내 입장(R2) | 상대 입장(R1 위치) | 판단 근거 |
|---|---|---|---|
| 첫 M1 transport | DoIP live loop 우선으로 양보, CAN 일정 별도 | sim DoIP M1, CAN M3(§11) | 02는 최소 backend의4요소를 요구. CAN 팀 ISO-TP 일정과 동시 M1 미합의 |
| Client 재사용 vs 자체 UDS 엔진 | M1 재사용 우선, runtime timing/demux가 요구하면 runner 자체화 | 코덱만 사용·자체 TxnEngine(§7) | BaseConnection 스파이크 성공은 가능성 증거. 자체 엔진도 NRC/echo/timer 전체 회귀 필요 |
| DoIP 구현 | pin+좁은 보강 또는 좁은 asyncio 구현을 동일 gate로 비교 | 자체 asyncio 전송 M1, doipclient 비교용(§7) | 기존50ms→2s·TLS 실패는 보강 필요 증거이지 전체 재작성만이 답이라는 증거는 아님 |
| daemon 구성 | CAN worker 분리, broker/정책/뷰어 접속은 통합 | 하나의 loomd+모듈·가능하면 CAN/UDS 동거(§3.1·C1) | 물리장치 단독 소유는 둘 다 충족; crash/부하와 common core 비용 비교 |
| TP 실행 위치 | UDS scheduler 소유, CAN periodic은 inhibit/lease 계약 있을 때만 | CAN periodic 사용 찬성(§3.3·R-TP-05) | active request·pending·취소와 blind periodic 충돌, client disconnect 의미 |
| R2 승인 근거 | daemon grant와 skill/host 확인의 보장 경계를 명시. 가능하면 인간 채널에서 승인 기록 생성 | 채팅 동의 후 agent 토큰 재제출, host ask 보조(§10.4) | 토큰 소지는 인간 동의 증거가 아님. 같은 OS 악의적 우회 차단은 양쪽 모두 범위 밖 |
| raw CAN 충돌 우회 | 활성 ISO-TP 소유 동안 기본 차단, 강제 전환은 quiesce·lease 해제 후 | 차단하되 user `--force` 예외(§15.2) | 사용자 강제 송신도 진행 중 CF/FC·UDS 응답을 깨뜨릴 수 있음 |
| origin/dir 스키마 | raw 이벤트는 기존 enum, TP는 purpose/actor 필드, 거래는 별도 파생 레코드 선호 | daemon/keepalive origin·txrx 추가(C2/C3) | 공통 브리프는 agent/user/device/script 및 rx/tx. 확장은 합의 가능하지만 조용히 혼용 금지 |
| TLS·HIL 시점 | 요구 장비가 TLS면 실장비 첫 수용의 선행 gate, 그 전 plaintext sim부터 | TLS·HIL 주로 M5, 요구 시 조기 smoke(§11·§13) | OEM 요구/장비에 따라 필수성 달라짐. 인증 실패 시 plaintext fallback 금지 |

## G. 리드 xteam 초안에 대한 의견

1. **CAN–UDS §3:** tx_done의 driver enqueue/wire 완료 구분, clock_id/epoch·불확실성·req_id, **rx_start→rx_complete**를 추가한다. PDU 완성 때 제공하는 첫 프레임 시각은 사후 분석에 유용하지만 이미 발생한 P2 timeout을 예방하지 못한다. 수신 길이·N_Cr/완료 상한과 전체 job deadline을 분리한다.
2. **CAN–UDS §2·§4:** 주소 소유 키는 mode/source/target/address_extension까지 포함한다. functional SF의7/62B는 normal 기준이며 address byte1개가 붙으면6/61B다([주소 형식](https://docs.kernel.org/networking/iso15765-2.html#addressing), [can-isotp 주소 구현](https://github.com/pylessard/python-can-isotp/blob/v2.0.7/isotp/address.py)). FD라고2³²−1 할당을 허용하지 말고 초기 max PDU4,095B 등 cap을 둔다. 알려진 응답자별 FC 경로부터 수용한다.
3. **CAN–UDS §5·§7:** periodic이 필요하면 `idle_only`, in-flight 억제·owner-death·lease expiry·정지 완료 이벤트를 계약화한다. 소유자는 단발 CLI가 아닌 UDS 관리자다. CAN100% 부하에서는 장치 응답 도착시간을 보장하지 말고 **수신 이벤트→화면**의 부하/지연과 loss/degraded 표시를 시험한다. 지연 수치를 미측정 목표로 표시한다.
4. **viz uds V1–V5:** 완성 거래 파일과 live tx/pending/완료 이벤트 스키마를 구별하고 같은 corr_id의 행 갱신을 지원한다. 사용자 송신/정지/승인·snapshot→live watermark·capture loss/표시 drop 분리·렌더 계측을 추가한다. DTC P/C/B/U 표기는 format이 맞을 때만, 원시24bit는 항상 보존한다. 명령 grant token 자체를 화면 데이터/로그로 내보내지 말고 승인 참조 ID만 쓴다.
