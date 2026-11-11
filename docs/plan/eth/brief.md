# ETH 브리프 (v2.2 신규, EXPLORE 팀 담당) — 이더넷 패킷 도메인 `eth`

먼저 읽기: `02-core-principle.md`(최우선), `00-master-plan.md`(§2.3 C, §2.4 E, **§2.5 F1 모두 자동**, §3.0 단계), `core/plan.md`(worker 프로토콜·저널·이벤트), `ui/plan.md`(패널 프레임워크), `uds/plan.md`(DoIP 전송), `test/plan.md`(증거 술어), `_xteam/vn5640-eth.md`(C4.1).

## 사용자 지시 (원문)
"이더넷까지 1차에 포함. UI는 와이어샤크나 tshark 사용하는 것처럼 에이전트가 실시간으로 패킷을 볼 수 있어야 함."

## 핵심 원칙
에이전트가 **tshark처럼** 명령으로 패킷을 실시간으로 읽고(필터·디코드·대기·검증), 필요하면 패킷을 보낸다. 사용자는 **Wireshark처럼** 같은 패킷을 같은 순간 UI에서 본다(목록·상세 트리·hex·필터·스트림 따라가기). 에이전트만 보는 패킷이 없어야 한다. **1차(P1 M1)에 포함**된다.

## 환경 사실
- 실HW: DoIP는 **PC 랜카드 + 미디어 컨버터**(100/1000BASE-T1)로 ECU에 연결(C4.1). Windows x64(Npcap)·Linux x64(AF_PACKET/libpcap), macOS는 개발(BPF). 이 Mac에는 tshark/Wireshark가 없다.
- 언어: Rust, Python 금지(C1). 외부 프로세스 tshark/sharkd는 선택 dissector로 검토 가능(GPL 프로세스 분리, 설치는 사용자).
- 기본 정책은 모두 자동(F1).

## 범위
1. 캡처: NIC 선택, BPF 캡처 필터, 프로미스큐어스, 커널 버퍼, 드롭 통계 → `source.gap`. 권한(Windows Npcap 설치, Linux `CAP_NET_RAW` — 사람이 사전 설정).
2. 디코드: Ethernet/VLAN/ARP/IPv4/IPv6/ICMP/UDP/TCP(재조립) + **DoIP + DoIP 위 UDS**(UDS 코덱 재사용) + **SOME/IP·SOME/IP-SD** 우선. 나머지 프로토콜은 tshark 연동(선택) 여부 판단. crate 조사(`pcap`, `etherparse`, `pnet`, `pcap-file` 등).
3. 에이전트 CLI(tshark 대응): `loom eth ifaces`, `capture start/stop`, `trace --follow --filter <표시 필터>`(한 줄 요약 + `--json` 상세), `last`, `wait-for <술어>`, `stats`, `follow tcp|udp <stream>`, `send`(원시 프레임·UDP·TCP 페이로드). 표시 필터 문법 결정(Wireshark 문법 부분집합 권장).
4. UI 패널(Wireshark 대응): 패킷 목록(가상화·색 규칙), 상세 트리, hex, 표시 필터 바, 스트림 따라가기, 시간 기준, **UDS 요청 `request_id`와 DoIP 패킷 상호 링크**, pcapng 내보내기. 선택: Wireshark extcap로 Loom 실시간 스트림을 Wireshark에서도 보기.
5. 기록: 원본 = pcapng 세그먼트(또는 CORE 저널 프레임 + pcapng export) — CORE와 결정. 고속(1 Gbps) 시 C2 큐·링 상한과 넘칠 때 정책.
6. 통합 테스트: 증거 REF `eth:<if>@seq:a..b` + 술어(필드 비교), TC 예(“ECU 리셋 후 5 s 내 DoIP vehicle announcement 3회”).
7. M1(sim): pcap 재생 + loopback에서 UDS 가상 ECU DoIP 트래픽 캡처 → CLI 실시간 + 웹 패널 E2E(Playwright). M2: 실 NIC + 미디어 컨버터(Windows·Linux). 미디어 컨버터는 PC↔ECU 구간만 보인다 — ECU 간 트래픽 관찰에는 TAP·미러 포트가 필요하다는 점을 HW 질문으로.

## 협의 파일
`_xteam/eth-core.md`(worker·저널·pcapng), `_xteam/eth-ui.md`(패킷 패널 명세), `_xteam/eth-uds.md`(DoIP 디코드·request 링크·코덱 공유), `_xteam/test-domains.md`에 `## eth`.

## 산출물
`docs/plan/eth/plan.md`(00-brief §4의 15절, 맨 위 핵심 루프 표). 이번은 **짧게**(R1 두 워커 병렬 → 리드 통합, R2·R4 생략 가능). 스파이크는 `spikes/eth/`. 완료 시 `eth/STATUS.md` 첫 줄 `DONE`.
