DONE
- 산출물: `docs/plan/eth/plan.md`(핵심 루프 표 + 15절 + 결정 기록 X0–X9). 입력은 `r1-opus.md`(구조·CLI·UI·테스트)와 `r1-sol.md`(사실 확인 + Rust 오프라인 스파이크 `spikes/eth/r1-sol/`: DoIP·UDS·SOME/IP-SD·VLAN 9패킷 pcapng 생성·파싱 PASS, 첫 빌드 10.8 s). 짧은 라운드(R1 → 리드 통합).
- 에이전트는 tshark처럼 쓴다: `loom eth ifaces | capture start/stop | trace -Y … [--follow] | last | show | wait-for | stats | follow | send | export pcapng`. 한 줄 요약(tshark 열) + `--json` 계층 트리. 사용자는 Wireshark처럼 본다: 웹 패널 `eth.packets`(목록·상세 트리·hex·필터 바·스트림·UDS 칩·재전송/편집 송신). 같은 seq·같은 패킷이고, 에이전트만 보는 패킷은 없다.
- 표시 필터 `loom-pktfilter`는 Wireshark 부분집합이다(필드 이름 = Wireshark 정본 + 별칭). 한 구현을 CLI·UI·TEST 술어가 함께 쓴다. BPF 캡처 필터와는 분리한다.
- 기록 정본은 CORE 저널 프레임 + 레코드 = pcapng EPB(codec `pcap-epb/1`)이고, export는 프레임 벗기기다. 저널(무손실)·화면·에이전트 경로를 분리하고, 드롭은 `source/record/display.gap` 3층으로 표시한다. 통계 미지원이면 "알 수 없음"으로 표시한다(0 = 무손실 금지).
- 캡처는 `pcap` crate 단일 백엔드다. Windows는 **Npcap을 사용자가 설치**(EULA상 번들 불가), Linux는 별도 `loom-eth-worker`에만 `CAP_NET_RAW`. macOS(이 Mac은 BPF 권한 없음)는 replay + sim tap이고, **M1 E2E 정본은 Linux `lo` 실캡처**다.
- UDS 링크: DoIP 진단 패킷 ↔ `uds.tx/rx request_id` 양방향, UDS 코덱 no-IO crate 공유. 자기 송신 패킷은 `attributed_origin`으로 표시하고 TEST 증거에서 device 관측으로 세지 않는다.
- 협의 파일: `_xteam/eth-core.md`, `_xteam/eth-uds.md`, `_xteam/eth-ui.md`, `_xteam/test-domains.md` `## eth`. **각 팀 의견 대기.**
- 미확인·관문: 1 Gbps 전체 경로 무손실 상한(M2 W/L NIC 벤치), pcapng 나노초·다중 IDB·custom option 쓰기, TCP 재조립 후보 선택, Wireshark 필드 이름 전수 대조(tshark 필요). 사용자 질문은 미디어 컨버터·NIC 모델, ECU 간 트래픽용 TAP·미러 포트 유무, Npcap 설치 상태, VLAN·SOME/IP 사용 여부.
