# ETH R1 — explore-sol: 사실 확인 + Rust 스파이크

배경: `docs/plan/eth/brief.md`를 먼저 읽는다. Rust로 이더넷 패킷 도메인을 만든다(Python 금지, C1). 에이전트가 tshark처럼 보고, 사용자가 Wireshark처럼 보는 것이 목표다. 실HW는 Windows x64(Npcap)·Linux x64, macOS는 개발용이다. 이 Mac에는 tshark·Wireshark가 없다.

## 산출물
`docs/plan/eth/r1-sol.md`, 250줄 이하. 항목마다 지킬 것:
- 버전·릴리스일·라이선스·W/L/M 지원을 적는다.
- **출처 URL**을 붙인다.
- 못 한 것은 "미확인"으로 적는다.
- 끝에 권고 표를 둔다.

## 확인할 것
1. **캡처**
   - `pcap` crate(libpcap/Npcap 바인딩)와 `pnet`(datalink) 비교. Linux AF_PACKET TPACKET_V3·mmap 링 crate가 있는지.
   - 커널 버퍼 크기 설정, 드롭 통계(`pcap_stats` ps_drop/ps_ifdrop), 비차단·타임스탬프 정밀도(나노초, `pcap_set_tstamp_precision`).
2. **Npcap**
   - 라이선스(무료판의 재배포 제한, OEM).
   - 설치 옵션: WinPcap 호환 모드, "관리자만 캡처" 옵션, loopback 어댑터.
   - **Loom이 Npcap을 번들할 수 없고 사용자가 설치해야 하는가**를 판단한다.
3. **Linux 권한**: `CAP_NET_RAW`·`CAP_NET_ADMIN`(setcap) 대 그룹. 사람이 사전 설정해야 하는 것.
4. **macOS 개발 캡처**: `/dev/bpf*` 권한(ChmodBPF / access_bpf 그룹). 이 머신에서 sudo 없이 가능한지만 확인하고, 시스템 설정은 바꾸지 않는다.
5. **파서·포맷**
   - `etherparse`, `pnet_packet`, `pcap-file`, `pcap-parser`, pcapng 쓰기 crate.
   - SOME/IP(`someip_parse` 등)와 DoIP Rust crate 현황.
   - TCP 재조립 crate(예: `pdu`, `smoltcp`는 해당 없음, 직접 구현 범위).
6. **tshark/sharkd(선택 dissector)**
   - sharkd JSON 인터페이스 안정성, `tshark -T ek|json`·`-l` 실시간 출력, 라이선스(GPL-2.0, 외부 프로세스).
   - Windows·Linux 설치 경로.
   - Loom 필드 이름을 Wireshark 필드 이름과 같게 둘 수 있는지.
7. **extcap**: Wireshark extcap 인터페이스(인자 `--extcap-interfaces`, `--capture`, fifo). 외부 바이너리로 Loom 스트림을 Wireshark에 보여 줄 수 있는지.
8. **처리량**: 1 Gbps(약 81k pps @1500 B, 최악 1.48 Mpps @64 B)에서 Rust 사용자 공간 캡처의 현실적 상한에 대한 공개 근거를 찾는다.
9. **스파이크**: 위치는 `spikes/eth/`, **10분 이내**.
   - 샘플 pcapng를 만든다: DoIP 차량 공지(UDP 13400), 라우팅 활성화, 진단 메시지에 UDS `22 F1 90` 요청·`62` 응답, SOME/IP-SD offer, VLAN 1개.
   - Rust로 읽어 etherparse + 자체 DoIP/SOME/IP 최소 파서로 한 줄 요약을 출력한다.
   - 빌드·실행 시간과 출력을 기록한다.

## 규칙
- 재개 공지 05를 지킨다.
  - `CARGO_HOME`은 `~/.cache/loom-eth-cargo`, `CARGO_TARGET_DIR`은 `~/.cache/loom-eth-target`으로 둔다(프로젝트 폴더 밖).
  - 대형 다운로드를 하지 않는다. 파일 읽기가 30초 넘게 멈추면 중단하고 결과 파일에 `BLOCKED: file io`를 쓴다.
- 전역 설치와 sudo를 하지 않는다. 시스템 설정을 바꾸지 않는다.
- 쓰기는 `docs/plan/eth/`와 `spikes/eth/`만 한다.

끝나면 대기한다.
