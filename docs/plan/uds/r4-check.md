PASS with fixes

검토 기준: [plan.md](plan.md) R3, 1,041행, SHA-256 `4f00a9ff65bffd0e697ebc5d555371fdbf55540cbd03ef9dd5153f695778f2c9`. **PASS는 계획의 요구 포함 여부이며 구현·HW 시험 합격이 아니다.** plan.md는 수정하지 않았다. 사실 확인 보관본: [r4-facts.json](../../../spikes/uds/evidence/r4-facts.json), [Textual PyPI 조회](../../../spikes/uds/evidence/r4-textual.json). 아래 P1 반영 후 최종 확정 권고.

## A. 02 핵심 원칙 체크리스트

| 항목 | 판정 | plan 위치·근거 |
|---|---|---|
| 1. 맨 앞 핵심 루프4요소 표 | PASS | §0: 직접 접근/읽기/쓰기/사용자 표시 담당과 첫 수용 단계 명시 |
| 2. 구체적 E2E 명령·응답·표시시간·개입 | PASS | §2.0: connect→VIN→DID write/readback→user write→agent 재확인→takeover/abort→늦은 attach. 목업과50/100ms 목표 포함 |
| 3. 실시간 목표·고부하 정직한 표시 | PASS | §5.4–5.5: 렌더 p95≤100/p99≤200ms, 정상 부하·1,000회 표본, 집계/gap/drop/STALLED/lag/기록 장애. 구현 단계 배치의 모순은 E/F 참조 |
| 4. M1 걷는 뼈대 | PASS | §9.3·§11 M1: **실제 Textual 앱의 렌더 훅+사용자 입력 자동화**를 요구하고 단순 헤드리스 구독자 수신은 불합격. U-11에 U-10 의존성 반영 |
| 5. 같은 세션·스트림, 숨은 연결 금지 | PASS | §0 불변식1/5·§3.1/3.3·§4.4·§5.3·§10.9: CLI/Python API 직결 금지, 상주 소유, snapshot watermark 이후 live 합류. 남은 숨은 장치 연결 경로는 찾지 못함 |
| 6. 비기여 기능 후순위/비범위 | PASS | §1.2/1.3·§11: ODX/웹/시나리오/TLS·실차 범용 flash와 첫 loop 분리, CAN은 M1b |

6개 항목 모두 문서 수준 PASS이므로 FAIL 대체 문장은 없다. 아래 P1은 이 방향을 실행 가능한 계약·일정으로 맞추는 수정이다.

## B. 공통 브리프 §4의 15개 절 매핑

| 요구 절 | plan 절 | 비어 있지 않은 내용 / 판정 |
|---|---|---|
| 1 범위/비범위 | §1 | 단계별 포함·후순위·flash/CDD 비범위 — PASS |
| 2 사용 시나리오 | §2 | agent A1–A6, user U1–U6, E2E-1 — PASS |
| 3 아키텍처 | §3 | 프로세스 그림·소유권·흐름·타이밍·직렬화 — PASS |
| 4 CLI/API | §4 | 명령/인자/출력/종료 코드/Python RPC wrapper — PASS |
| 5 사용자 뷰어 | §5 | TUI 목업·입력·재접속·지연·과부하 — PASS |
| 6 데이터/기록 | §6 | 이벤트/JSONL/manifest/감사/DB/시나리오 — PASS |
| 7 라이브러리 | §7 | 버전·라이선스·Python·commit·채택 근거 — PASS, Textual 보강 필요 |
| 8 HW/OS | §8 | 세 OS·Vector/PCAN/SocketCAN/FD·계획/실측 구분 — PASS |
| 9 시뮬레이션/CI | §9 | 가상 ECU·스파이크·결함 주입·세OS Python lane — PASS |
| 10 안전 | §10 | 등급·승인·raw·dry-run·rate·plugin·스킬 — PASS |
| 11 마일스톤 | §11 | M1/M1b–M5와 수용 기준 — PASS |
| 12 작업 분해 | §12 | U-01–26, 담당·의존·병렬 그룹·M — PASS |
| 13 리스크 | §13 | 영향과 대응15개 — PASS |
| 14 질문/기본값 | §14 | Q1–Q10과 기본값·결정 영향 — PASS |
| 15 공통 계약/타팀 | §15 | core/CAN/viz 요구·필요 시점 — PASS |

§16 결정 기록과 최종 자가 점검도 존재한다. 코드 블록 안의 스킬 heading을 본문 절로 오인하지 않고 확인했다.

## C. UDS 브리프 커버리지

| 꼭 다룰 항목 | plan 위치 | 판정 |
|---|---|---|
| DoIP 탐색/UDP13400·라우팅·논리 주소·다중 ECU | §1.1·§3.4/3.5·§4.2·§9.1/9.3 | 포함. source LA 분배·단일 pump·다중 응답·socket 한도 시험도 있음 |
| TLS3496 | §1.2·§7·§8·§9.3·§11 M4·D13 | 포함. 요구 장비 첫 수용의 선행 gate, 인증 실패 시 평문 fallback 금지 |
| CAN-TP 데몬 경계·normal/extended/mixed·11/29bit·기능주소·FD | §1.1·§8·§11 M1b/M3·§15.2 | 포함. xteam v0.2/0.3 참조, raw 충돌 차단·주소 byte·rx_start 포함. CAN 팀 합의는 대기 상태로 표시 |
| 0x10/11/14/19/22/27/2E/2F/31/3E | §4.2·§10.3·§11 | 모두 포함. seed-key 사용자 플러그인·TP 자동 유지 포함 |
| 0x34/36/37 범위 판단 | §1.3·§11 M3/M5·D14 | 포함. 실차 범용 비범위, simulator/OEM 프로필 구분 |
| NRC0x78·P2/P2*·전체 deadline | §3.3/3.4·§9.3 | 포함. 첫 응답/완료/P2*·총 deadline 분리, 반복 제한·재송신 금지 |
| 진단 DB YAML/JSON·ODX/PDX·CDD | §1.3·§6.4·§7 | 포함. 타입/스케일/단위, strict import/coverage, CDD 우회 경로 |
| agent 명령·raw·시나리오·사람/JSON 출력 | §2·§4·§6.5 | 포함. 세부 옵션의 M 배치 수정 필요(E) |
| 사용자 live trace·NRC·타이밍·DID plot·개입 | §2·§5·§15.3 | 포함. M1 TUI, M3 viz/plot |
| 안전·라이브러리·가상 ECU/무HW CI | §7–§10 | 포함. 보장 한계·서브프로세스·세OS·결함 시험 명시 |

**항목 누락은 없다.** “포함”은 후순위·비범위 판단까지 문서화했다는 뜻이다.

## D. 사실 확인 결과

| 확인 대상 | 결과·근거 | plan 반영 |
|---|---|---|
| Central security 0xE0/E1, 판본 차이 | **공개 근거는0xE0**, doipclient1.3.0 enum만0xE1. [AUTOSAR R4.1.3](https://www.autosar.org/fileadmin/standards/R4.1.3/CP/AUTOSAR_SWS_DiagnosticOverIP.pdf#page=32) p32 및 [R24-11](https://www.autosar.org/fileadmin/standards/R24-11/CP/AUTOSAR_CP_SWS_DiagnosticOverIP.pdf#page=38) p38의 SWS_DoIP_00082–84가0xE0 경로를 규정. 후자는 p11 참고문헌[7]에 **ISO13400-2:2019** 명시. [Wireshark](https://raw.githubusercontent.com/wireshark/wireshark/master/epan/dissectors/packet-doip.c) L218, [Scapy2.8.0](https://raw.githubusercontent.com/secdev/scapy/v2.8.0/scapy/contrib/automotive/doip.py) L173도0xE0. **ISO2012/2019 정식 원문 전체는 확보하지 못했으며 판본 차이라는 근거는 확인하지 못함.** | §7의 R4 미결 문장을 F-P1-1로 정리. enum 값을 ISO 사실로 채택하지 말 것 |
| 같은 SA 두 번째 소켓→0x03 | **인용 요구사항은 맞지만 조건 생략.** [R24-11 SWS_DoIP_00105](https://www.autosar.org/fileadmin/standards/R24-11/CP/AUTOSAR_CP_SWS_DiagnosticOverIP.pdf#page=41) p41: **같은 DoIPInterface**의 다른 연결에 SA가 등록되어 있고 기존 연결이 alive timeout 안에 응답하면0x03 및 새 연결 폐쇄. 응답이 없으면 SWS_DoIP_00107로 진행. | §3.5의 무조건 거부 문구 한정(F-P2-2). 소켓 공유 기본 결정은 타당 |
| §7 패키지 버전·날짜·commit | [pypi.json](../../../spikes/uds/evidence/pypi.json), [github.json](../../../spikes/uds/evidence/github.json) 대조: **기재된11개 패키지 버전과 기재 날짜·commit 날짜 모두 일치**. udsoncan1.26.1, doipclient1.3.0, can-isotp2.0.7, python-can4.6.1, odxtools11.7.1 포함. 일부 행은 릴리스 날짜/Python 조건 생략 | 오류 대신 완전성 보강: scapy2026-10-02, doip-server2026-06-14, py-uds2026-09-28, pyvit2017-08-31, aioisotp2018-11-21, canmatrix2026-10-21(UTC). `GPL-3`/`BSD-2`는 각각 정확한 SPDX 표기로 보강 |
| Textual 최신·라이선스 | **8.2.8, MIT, Python≥3.9,<4.0**, 최초 파일 upload **2026-06-30 06:51:24 UTC**. [PyPI JSON](https://pypi.org/pypi/textual/json), [조회 보관본](../../../spikes/uds/evidence/r4-textual.json), 확인2026-11-09 14:35 UTC. | §5.1/§7의 “추정/미확인”을 확인값으로 교체(F-P2-1) |
| §9.2 스파이크 수치 | [roundtrip.json](../../../spikes/uds/evidence/roundtrip.json)와 **7행 모두 반올림 범위 내 일치**. Pipe p99 2.003208/max10.374083ms, CAN median0.921333/p95 2.178375/p99 3.087959/max5.972416ms, pending72.779708ms/2회, CAN timeout55.809458ms, DoIP activation1.013792/median0.305084/p99 0.850958ms, silent timeout2001.870750ms. TLS 예외는 [별도 JSON](../../../spikes/uds/evidence/tls-default-probe.json)과 일치 | PASS. 새 성능 시험을 실행하거나 UI 성능으로 확대 해석하지 않음 |
| functional SF 길이 | normal classic7/64-byte FD62B, 주소 byte1개인 extended/mixed는6/61B — **맞음**. [can-isotp 주소 소스](https://github.com/pylessard/python-can-isotp/blob/v2.0.7/isotp/address.py)의 prefix와 [ISO-TP 설명](https://docs.kernel.org/networking/iso15765-2.html#addressing) 대조. 다른 FD tx_dl에서는 해당 길이에서 overhead 차감 | §15.2 PASS. 4,095B는 제품 cap이며 FD의 표준 최대치라고 쓰지 않아 문제 없음 |
| NRC0x37 남은 시간 없음 | `7F 27 37`에는 남은 시간 필드 없음 — **맞음**. 로컬 `spikes/uds/.venv/lib/python3.14/site-packages/udsoncan/Response.py` L196–220, `ResponseCode.py`의0x37 정의. | §4.3/§10.7 PASS: ECU 지연 미상과 로컬 금지 시각 구별 |
| udsoncan 기본 타이밍·기능 | 로컬 `…/udsoncan/configs.py`의 P2=1s/P2*=5s/request_timeout=5s, `use_server_timing=True`. [v1.26.1 configs](https://github.com/pylessard/python-udsoncan/blob/v1.26.1/udsoncan/configs.py), [client 요청 루프](https://github.com/pylessard/python-udsoncan/blob/v1.26.1/udsoncan/client.py). `0x78`이 전체 request_timeout을 초기화하지 않음. `tester_present()`는 자동 scheduler 아님 | §3.3/§7 PASS. 프로필50/5000ms와 라이브러리 기본값 구별함 |
| can-isotp send 인자·virtual 경계 | 로컬 `…/isotp/protocol.py`는 `send_timeout`, [python-can virtual 문서](https://python-can.readthedocs.io/en/stable/interfaces/virtual.html)는 프로세스 내 공유 | §7/§8/§9.1 PASS |
| DTC format의 출처 | **§4.3 수정 필요:** `19 02` 응답은 status availability+3byte DTC/status record이며 DTCFormatIdentifier를 포함하지 않는다. `19 01` 계열 count 응답에는 포함. 로컬 `…/udsoncan/services/ReadDTCInformation.py` L465–485, L531–560, L646–653; [고정 버전 소스](https://github.com/pylessard/python-udsoncan/blob/v1.26.1/udsoncan/services/ReadDTCInformation.py) | format은 앞선 count 응답/검증된 프로필에서 확보하고, 없으면 raw24bit만 표시(F-P1-5) |

## E. 내부 일관성과 R2 P1 반영

| ID | 위치 | 남은 불일치 |
|---|---|---|
| E1 | §2.0·§4.2·§11·§12 | M1 E2E는 R2 승인 패널/뷰어 y까지 설명하지만 §1.1/§11/U-17은 패널을 M2로 둠. M1 스킬은 dry-run을 요구하나 구현 U-18은 M2. `connect`의CAN/FD/TLS, `status --probe`, `write-did --field`, raw functional도 명령표의 단일 M1 표기보다 실제 지원 단계가 늦음 |
| E2 | §7·D4 ↔ §9.3·§11·U-20 | “M1 안에 독립 doip-server 상호운용 관문 미통과면 전환”인데 해당 시험은 M2에 배정. M1 최소 gate와 M2 전체 회귀를 나눠야 함 |
| E3 | §4.2·§11 M2 ↔ U-19/U-21 | reset은 명령표/마일스톤에서 M2, 구현 작업은 U-21 M3에만 존재 |
| E4 | §4.2 ↔ §10.2/10.3 | `comm, dtc-setting`을 묶어 R2(vehicle R3)라 표기했지만 enable/on은R1, DTC setting off는R2이고 명시적 vehicle 승격 대상은 아님. §10.2의0x2F 전체 승격은 returnControl00=R1 표기와도 불일치 |
| E5 | §3.1/§15 C1/D3 | “분리 worker도 수용”과 “최소한 CAN/UDS 같은 프로세스”가 함께 존재. 같은 프로세스는 선호 배치, 물리 장치 단독 소유·API/stream 일치는 필수 계약으로 구분 필요 |
| E6 | §4.1·§4.3·스킬·D15 | **종료 코드0–7은 일치.** NRC JSON 예시에는 §4.1 필수 `schema_version/request_id/cursor/timing` 등이 생략되어 있어 축약 예시라고 표시하면 됨 |

| R2 지적 | 반영 판정 | 근거/잔여 |
|---|---|---|
| C1 실제 UI·개입·late attach·U-11 의존 | 반영 | §2.0·§5.3/5.4·§9.3·§11·U-11. 단순 수신자 시험을 대체함 |
| C2 bounded timing/queue·0x78/0x21 | 대체로 반영 | §3.4/3.5·§10.7:32개·총deadline·pending50회·읽기만1회 재시도. 전체deadline 시작점/각 단계 남은 예산과 byte cap 수치는 구현 계약에서 보강 권고 |
| C3 늦은 응답·epoch·no-wait | **부분 반영(P1)** | 중복 별칭/no-wait drain은 반영. 그러나 epoch는 재연결 때만 증가(§3.3)하며 **같은 연결에서 timeout 후 도착한 이전 응답의 회복 절차는 없다**. 테스트 목록만으로는 정책이 결정되지 않음(F-P1-2) |
| C4 기록 실패·과부하 | 설계 반영, 단계 모순 | §5.5/6.2/6.3은 bounded 기록·실패 시 송신 중단·완전성 확인을 명시. U-18에서 M2로 미룬 최소 fail-stop은 M1 쓰기/기록 불변식과 정렬 필요(F-P1-3) |
| C5 토큰 한계·재검사·abort/finally | 반영 | §10.2/10.4·§6.5·§10.7. D7의 “자기 승인 경로 차단”이라는 이유만 보장 한계보다 강함(P2) |
| C6 plugin 격리·idle pump | 반영 | §3.1·§7·§10.8:서브프로세스2s·출력 상한, 단일수신pump, blocking을 이벤트 루프에 직접 올리지 않음 |

§16 D2/D4의 선택 자체는 근거가 있다. 다만 BaseConnection 스파이크 성공은 첫-frame 타이밍·다중LA·UI까지 검증한 것은 아니므로 **되돌림도 동일 수용 시험을 통과해야 한다**. D10의 “02 때문에 VIN 무마스킹 필수”라는 근거는 약하다. 같은 마스킹 정책을 agent/viewer에 적용해도 같은 데이터 원칙을 만족하므로 원문 보존은 별도 제품 기본값으로 기록한다.

## F. 반드시 고칠 것(P1) / 고치면 좋은 것(P2)

### P1 — 최종 통합 전

1. **§7 central-security 미결 해소.** 대체: “공개 AUTOSAR R4.1.3/R24-11과 Wireshark/Scapy는 central-security 경로에0xE0를 사용한다. doipclient1.3.0의0xE1 enum과 불일치하며 이를 ISO2012/2019 판본 차이로 볼 근거는 확인하지 못했다. ISO 정식 원문 대조는 미완료다. 기본 activation은0x00, 비기본 값은 OEM 프로필 숫자를 사용하고 해당 enum을 표준 alias로 재사용하지 않는다.”
2. **§3.3–3.5 timeout 회복과 물리 ECU 잠금 범위.** 추가: “wire UDS 응답에는 Loom request_id/epoch가 없다. 송신 후 timeout/abort된 대상은 quarantine 상태로 두고 후속 요청·자동 재시도를 막는다. 수신 중 PDU를 drain한 뒤 전송별 검증된 회복 절차(DoIP 연결 재설정 또는 OEM 완료 확인)를 거쳐 새 epoch를 연다. CAN 채널 재open만으로 지연 ECU 응답이 사라졌다고 간주하지 않는다. 같은 물리 ECU를 다른 tester SA/별칭으로 열어도 회복 잠금을 우회하지 못한다.” `late_same_SID/DID` 시험을 U-06 gate에 포함한다.
3. **§11 M1·U-08/U-10/U-18의 최소 기능 정렬(E1/C4).** 대체: “M1에는 write/raw의 dry-run, 최소 대상/env/rate 검사와 감사·기록 불능 시 새 송신 차단을 포함한다. M2는 정책 일반화·해시 체인·승인 패널/R3 자격을 확장한다. M1 R2 승인은 채팅 토큰 재제출만 사용한다.” §2.0의 `[y]` 승인 패널 설명에는 **M2 이후**라고 표시하고 M1 수용 경로에서는 제거한다. `status --probe`는M2, CAN connect는M1b, FD/functional/struct `--field`는M3, TLS는M4/요구 장비 gate로 옵션별 표기한다. reset은U-19 M2에 추가하고 U-21 M3에서 뺀다.
4. **§7·D4·U-04/U-20의 gate 일정 정렬.** 대체: “M1 전송 수용은 핵심 프레이밍/activation/ACK·오주소·무응답 deadline·idle alive 결함 시험과 독립 구현 최소 왕복 smoke를 포함한다(U-04/U-05). M2 U-20은 전체 독립 상호운용 회귀를 확장한다. M1 최소 gate 미통과 시 pinned adapter 대안도 같은 gate를 통과해야 한다.”
5. **§4.3 DTCFormatIdentifier 오인 수정.** 대체: “19 02 응답 자체에는 DTCFormatIdentifier가 없다. 이전19 01 등 format 포함 응답 또는 검증된 대상 프로필에서 형식을 확보한 경우만 P/C/B/U로 변환한다. 형식 미확인 시 raw24bit만 표시한다.” M1 예시가 SAE 형식을 가정한다면 프로필 가정도 표시한다.
6. **§4.2·§10.2/10.3 등급 통일(E4).** 대체: “comm enable=R1, disable=R2(vehicle R3); dtc-setting on=R1/off=R2. IO returnControl00=R1, 01/02/03=R3(vehicle R4). vehicle 승격은 해당 위험 서브기능에만 적용한다.” 하나의 분류표에서 CLI/스킬 표시를 생성하도록 유지한다.

### P2 — 보강 권고

1. **§5.1/§7 Textual·메타데이터.** 교체 행: `Textual | 8.2.8 (2026-06-30 UTC) | MIT / >=3.9,<4.0 | 최신 commit 미조사 | TUI 후보, 타팀 통일 및 pin`. 누락 릴리스 날짜·Python 조건을 기존 pypi.json에서 채우고 pyvit은 고정 LICENSE 근거의 `GPL-3.0-or-later`, canmatrix는 `BSD-2-Clause`로 구체화한다.
2. **§3.5 SA 거부의 조건·§16 D3 배치 표현.** 교체: “같은 DoIPInterface의 다른 활성 연결에 동일 SA가 등록되어 있고 그 연결이 alive check에 응답하면0x03으로 새 연결을 거부한다.” C1의 “최소한 같은 프로세스”는 “인프로세스 배치를 선호하되 메인 결정에 따라 동일 API의 worker 분리도 허용”으로 정리한다.
3. **§16 D4/D7/D10 근거 한정.** 교체: “D4는 timer/TLS 기본경로 문제와 source LA별 수신 pump 보강 비용을 근거로 선택한다. doipclient의 `send_diagnostic_to_address`는 존재하므로 ECU별 소켓만 가능한 것은 아니다. D7은 승인 경로를 제한·감사하되 token이 인간 동의 증거임을 보장하지 않는다. D10의 로컬 원문 보존은 제품 기본값이며02의 필수 조건은 agent/viewer의 동일 표시 정책이다.”
4. **§3.4·§6.1·§9.3 구현 계약/예시.** 추가: “총deadline은 실행 RPC 수신부터 큐·송신·ACK·응답 완료까지 포함하며 각 대기는 남은 예산으로 제한한다. 관리 이벤트의 `dir` 의미는 core가 별도 정의하고 wire RX/TX 집계에서 제외한다. JSON 예시의 필수 필드 생략/hex 말줄임은 예시 전용이다. 0x78 100회 부하 시험은 상한50회에서 중단을 기대하거나 시험 프로필 상향을 명시한다.”
