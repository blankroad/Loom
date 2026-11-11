# ETH R1 — Rust 캡처·파서 사실 확인과 오프라인 스파이크

2026-11-10 조사. `brief.md`, 재개 공지 05, 마스터 C1/C2/C4.1/C6·E1–E8·F1을 기준으로 했다. 실HW 경로는 **PC NIC → 미디어 컨버터 → ECU**, OS TCP/IP이다. VN5640 XL Ethernet·smoltcp 경로는 범위 밖이다.
아래 버전은 조사 시 확인한 기준 버전이며, 날짜는 crates.io 게시일(UTC)이다. W/L/M = Windows x64/Linux x64/macOS. `문서`는 지원 주장·API 확인, `추론`은 OS 독립 코드에 근거한 판단, `실측`은 이 Mac arm64 실행이다. W/L 빌드·실HW·장시간 성능은 모두 미확인이다.

## 1. 캡처 백엔드

| 후보 | 버전 / 게시일 | 라이선스 | W / L / M | 근거·판정 |
|---|---|---|---|---|
| pcap | 2.5.0 / 2026-08-15 | MIT OR Apache-2.0 | Npcap / libpcap / libpcap, 문서 | [메타데이터](https://crates.io/api/v1/crates/pcap), [API](https://docs.rs/pcap/latest/pcap/struct.Capture.html). 필터·통계·정밀도·파일 읽기·송신을 공통 API로 제공. 기본 권고 |
| libpcap 기준판 | 1.10.7 / 2026-09-05 | BSD 계열, 파일별 고지 확인 | Npcap 별도 / 지원 / 지원 | [공식 배포 목록](https://github.com/the-tcpdump-group/tcpdump-htdocs/blob/master/index.html), [소스](https://github.com/the-tcpdump-group/libpcap/tree/libpcap-1.10.7). 1.11.0(2026-09-13)도 배포됨; 호스트 실제 링크 버전 미확인 |
| pnet / pnet_packet | 0.35.0 / 2024-05-30 | MIT OR Apache-2.0 | 지원 문서 / 지원 문서 / BPF 지원 문서 | [pnet 메타](https://crates.io/api/v1/crates/pnet), [packet 메타](https://crates.io/api/v1/crates/pnet_packet), [Config](https://docs.rs/pnet/latest/pnet/datalink/struct.Config.html). Windows 문서는 WinPcap 의존; Npcap 호환 모드 조합의 실측 미확인 |
| af_packet | 0.3.1 / 2020-06-02 | GPL-3.0 | 불가 / 지원 문서 / 불가 | [메타](https://crates.io/api/v1/crates/af_packet), [TPACKET3](https://docs.rs/af_packet/latest/af_packet/tpacket3/). mmap 링·통계·fanout 존재; 오래된 게시일·라이선스 때문에 기본 제외 |
| netring | 0.32.0 / 2026-10-18 | MIT OR Apache-2.0 | 불가 / 지원 문서 / 불가 | [메타](https://crates.io/api/v1/crates/netring), [해당 판 README](https://docs.rs/crate/netring/0.32.0). AF_PACKET TPACKET_V3·AF_XDP; README Rust 1.97+ 요구로 현재 1.93.1 도구체인과 불일치. 컴파일 미확인 |
| netdump-afpacket | 0.1.1 / 2026-06-30 | MIT | 불가 / 지원 문서 / 불가 | [메타](https://crates.io/api/v1/crates/netdump-afpacket), [구현](https://docs.rs/netdump-afpacket/latest/src/netdump_afpacket/lib.rs.html). 작은 TPACKET_V3 후보; 유지보수·실성능 미확인 |

- pnet datalink는 `DataLinkReceiver::next()`로 프레임 바이트를 주는 저수준 채널이다. `read_buffer_size`·timeout·fanout은 백엔드별 **hint**이며, 통합 `pcap_stats`·패킷 타임스탬프 정밀도 선택 API를 대체하지 않는다. 따라서 raw frame 송수신에는 후보지만 증거용 캡처의 기본으로는 pcap이 간단하다. [datalink](https://docs.rs/pnet/latest/pnet/datalink/), [Config](https://docs.rs/pnet/latest/pnet/datalink/struct.Config.html)
- Linux libpcap도 PACKET_MMAP을 쓰며 TPACKET_V3는 libpcap 1.5.0부터 지원한다. 별도 Rust mmap crate가 곧 더 빠르다는 근거는 없다. mmap은 커널/사용자 공유 링으로 syscall·복사를 줄이는 방식이며 NIC DMA부터 종단까지 무복사를 보장하지 않는다. [커널 문서](https://www.kernel.org/doc/html/latest/networking/packet_mmap.html)
- `pcap::Capture::from_device(...).buffer_size(bytes).snaplen(...).timeout(...).precision(Precision::Nano).open()`처럼 **활성화 전** 설정한다. `immediate_mode(true)`는 지연 감소와 배치 효율 사이 절충이며 실측으로 선택한다. 버퍼 요청이 OS 한도 때문에 그대로 반영되지 않을 수 있다. [Capture API](https://docs.rs/pcap/latest/pcap/struct.Capture.html), [pcap_set_buffer_size](https://www.tcpdump.org/manpages/pcap_set_buffer_size.3pcap.html)
- 활성화 후 `setnonblock()` 가능; 빈 읽기는 패킷 부재로 처리하고 busy-spin하지 않는다. OS별 poll/event 또는 전용 capture thread·중단 wakeup을 검증한다. timeout만으로 무트래픽 중 종료 지연이 제한된다고 가정하지 않는다. [API](https://docs.rs/pcap/latest/pcap/struct.Capture.html), [pcap nonblock](https://www.tcpdump.org/manpages/pcap_setnonblock.3pcap.html)
- `stats()`의 `ps_drop`은 OS 캡처 버퍼 부족, `ps_ifdrop`은 NIC/드라이버 손실이다. 플랫폼별 미구현 값이 0일 수 있어 **0 = 무손실**로 해석하면 안 된다. `ps_recv`의 필터 전후·미독출 포함 여부도 다르다. 누적값·증분·backend·가용성·앱 큐 drop·저널 gap을 각각 기록한다. [정본](https://www.tcpdump.org/manpages/pcap_stats.3pcap.html)
- 나노초 **단위**와 나노초 **정확도**는 다르다. `pcap_set_tstamp_precision`은 activate 전 호출, 미지원 오류를 처리하고 실제 precision·timestamp source를 기록한다. libpcap 1.5.1부터 있는 API이며 장치/드라이버가 원하는 정밀도를 제공하는지는 별도 확인이다. [정본](https://www.tcpdump.org/manpages/pcap_set_tstamp_precision.3pcap.html)

## 2. Npcap 배포·설치

기준 **1.90 / 2026-10-21 / Npcap EULA(독점, OEM 별도) / W 지원·L/M 불가**. [변경 이력](https://npcap.com/changelog), [EULA](https://github.com/nmap/npcap/blob/master/LICENSE)

- 무료판은 일반 재배포 권리를 주지 않는다. 보통 5 copy/computer/user 제한이 있고 Nmap·Wireshark·Defender for Identity와 **그 소프트웨어만** 함께 쓰는 예외가 있다. Wireshark가 설치되어 있다는 사실만으로 Loom 사용까지 예외가 된다고 볼 수 없다. [EULA](https://github.com/nmap/npcap/blob/master/LICENSE)
- 결론: **현재 별도 계약이 확인되지 않은 Loom은 Npcap을 번들하지 않고 사용자 직접 설치·자격 확인을 안내한다.** OEM Redistribution 계약을 취득하면 번들·silent install이 가능하다. OEM Internal-Use는 조직 밖 재배포 허가가 아니다. [OEM redist](https://npcap.com/oem/redist), [OEM 유형](https://npcap.com/oem/)
- `/winpcap_mode`: WinPcap 호환 설치. pnet의 기존 WinPcap 연동을 평가할 때 필요 여부를 확인한다. Loom pcap 경로는 Npcap DLL의 검색 위치·실제 `pcap_lib_version`을 확인해 오래된 WinPcap을 잘못 로드하지 않게 한다. [설치 옵션](https://npcap.com/guide/npcap-users-guide.html), [개발 안내](https://npcap.com/guide/npcap-devguide.html)
- `/admin_only`: 캡처·송신 장치 ACL을 SYSTEM/관리자로 제한한다. 일반 사용자에게도 무조건 캡처가 된다고 가정하지 않는다. 설치·옵션 선택·필요한 UAC는 사람의 사전 작업이다. `/S` silent installer는 OEM 기능이다. [설치 안내](https://npcap.com/guide/npcap-users-guide.html)
- 현대 loopback은 `NPF_Loopback` pseudo-interface이다. 오래된 Microsoft KM-TEST 기반 “Npcap Loopback Adapter”는 legacy 옵션이며 필수 어댑터가 아니다. loopback DLT는 Ethernet II와 다를 수 있으므로 IDB linktype별 파싱이 필요하다. [개발 안내](https://npcap.com/guide/index.html)
- 1.81은 NDIS metadata 기반 802.1Q 복원을 추가했고 1.88은 다중 VLAN 관련 수정을 포함한다. `/vlan_support`는 폐기 옵션이다. 실제 NIC VLAN/offload·GRO/LRO/checksum offload가 캡처 표현에 미치는 영향은 W/L 벤치에서 확인한다. [이력](https://npcap.com/changelog)

## 3. Linux 권한과 macOS 개발 환경

OS 권한 정책은 별도 crate가 아니므로 버전·릴리스일은 호스트 OS/커널에 종속(이 조사에서 W/L 호스트 미확인), 자체 라이선스 항목은 해당 없음이다. Linux 커널 API 근거와 macOS Wireshark 패키지 근거를 아래에 구분한다.

- Linux AF_PACKET 생성은 해당 network namespace를 지배하는 user namespace의 `CAP_NET_RAW`가 필요하다. `CAP_NET_ADMIN`은 인터페이스 설정·전역 promiscuous 설정·`SO_RCVBUFFORCE` 등에 필요하다. socket membership 방식 promiscuous와 전역 `SIOCSIFFLAGS`는 구분하며 backend별 최소 권한을 검증한다. [packet(7)](https://man7.org/linux/man-pages/man7/packet.7.html), [capabilities(7)](https://man7.org/linux/man-pages/man7/capabilities.7.html)
- 사람이 배포한 **작은 capture worker**에 `setcap cap_net_raw=ep /absolute/path/loom-eth-worker`를 적용하고, backend가 정말 요구하는 경우에만 `cap_net_admin`을 추가하는 방식을 권고한다. 명령은 예시이고 이번에 실행하지 않았다. 파일 교체·재빌드 뒤 capability 재확인, 컨테이너 bounding set·nosuid·namespace 제약도 확인한다. 일반 `wireshark` 그룹 가입만으로 임의 Loom 프로세스에 capability가 생기지 않는다. 그룹은 privileged helper 실행 ACL과 결합할 때 의미가 있다. [capabilities](https://man7.org/linux/man-pages/man7/capabilities.7.html), [Wireshark 권한 분리](https://wiki.wireshark.org/CaptureSetup/CapturePrivileges)
- Linux의 NIC 선택·IP/VLAN·TAP/미러·필요한 offload 조정·capability/그룹 배포는 사람의 벤치 사전 설정이다. 일반 TCP/UDP DoIP socket(13400)은 raw capture 권한 없이도 동작할 수 있다. **DoIP 통신 가능과 관찰 캡처 가능은 별도 상태**로 표시한다.
- macOS: 공식 Wireshark 패키지의 ChmodBPF launch daemon이 BPF 접근을 준비한다. access_bpf 그룹·장치 소유/권한이 맞아야 한다. ChmodBPF 개별 버전·릴리스일·개별 라이선스는 미확인(W/L 대상 아님, M 전용). [공식 설치 안내](https://www.wireshark.org/docs/wsug_html_chunked/ChBuildInstallOSXInstall.html), [패키지 설명](https://formulae.brew.sh/cask/wireshark-chmodbpf)
- **이 Mac 실측 2026-11-10 21:58 KST**: arm64, uid 501; `/dev/bpf0`·`bpf1` 모두 uid/gid 0, mode 0600, read-only open **EACCES**. access_bpf 그룹 없음. tshark/Wireshark/dumpcap PATH 검색 결과 없음. 따라서 현재 계정의 sudo 없는 BPF 캡처는 불가. 설정 변경·sudo·전역 설치·NIC 송신은 하지 않았다. [기계 판정 기록](../../../spikes/eth/r1-sol/machine.json)

## 4. 파서·pcapng·프로토콜

아래 순수 Rust 파서의 W/L은 OS 독립성에 근거한 가능성(별도 교차 빌드 미확인)이며 M `실측` 표시는 이번에 빌드·실행한 것만 뜻한다.

| 후보 | 버전 / 게시일 | 라이선스 | W / L / M | 기능·제한·출처 |
|---|---|---|---|---|
| etherparse | 0.21.0 / 2026-07-21 | MIT OR Apache-2.0 | 추론 / 추론 / 실측 | Ethernet II, VLAN, ARP, IP, ICMP, UDP/TCP; IP fragment 재조립도 있지만 TCP stream 재조립은 별개. [메타](https://crates.io/api/v1/crates/etherparse), [API](https://docs.rs/etherparse/latest/etherparse/) |
| pnet_packet | 0.35.0 / 2024-05-30 | MIT OR Apache-2.0 | 추론 / 추론 / 추론 | 프레임별 accessor·builder; pnet datalink와 독립적으로 사용 가능. [메타](https://crates.io/api/v1/crates/pnet_packet), [API](https://docs.rs/pnet_packet/latest/pnet_packet/) |
| pcap-file | 2.0.0 / 2023-02-01 | MIT | 추론 / 추론 / 실측 | pcap·pcapng reader/writer, SHB/IDB/EPB/ISB 등. `PcapNgWriter`를 이번에 사용. [메타](https://crates.io/api/v1/crates/pcap-file), [API](https://docs.rs/pcap-file/latest/pcap_file/) |
| pcap-parser | 0.17.0 / 2025-07-25 | MIT OR Apache-2.0 | 추론 / 추론 / 추론 | streaming/zero-copy 읽기, pcapng block 직렬화도 후보. 전용 writer 사용성은 pcap-file 쪽 선택. [메타](https://crates.io/api/v1/crates/pcap-parser), [API](https://docs.rs/pcap-parser/latest/pcap_parser/) |
| someip_parse | 0.7.2 / 2026-07-19 | MIT OR Apache-2.0 | 추론 / 추론 / 추론 | SOME/IP header·SD·SOME/IP-TP. 애플리케이션 payload 의미는 제외; 메시지 iterator·TP는 TCP stream 재조립 대체 아님. [메타](https://crates.io/api/v1/crates/someip_parse), [API](https://docs.rs/someip_parse/latest/someip_parse/) |
| doip-codec | 2.0.9 / 2026-02-17 | MIT | 추론 / 추론 / 추론 | DoIP encode/decode 후보. UDS 서비스 의미 해석·passive TCP 재조립은 별도. [메타](https://crates.io/api/v1/crates/doip-codec), [API](https://docs.rs/doip-codec/latest/doip_codec/) |
| doip-definitions | 3.0.14 / 2026-02-17 | MIT | 추론 / 추론 / 추론 | DoIP 공통 정의; codec과 최신 major를 임의 조합하지 말고 Cargo dependency 호환 판 확인. [메타](https://crates.io/api/v1/crates/doip-definitions) |
| simple_doip | 0.7.0 / 2026-10-19 | MIT OR Apache-2.0 | 추론 / 추론 / 추론 | no_std zero-copy core·tester/entity 후보. 자체 TCP socket 수신의 framing과 passive packet 재조립은 다른 문제. [메타](https://crates.io/api/v1/crates/simple_doip), [API](https://docs.rs/simple_doip/latest/simple_doip/) |

- pcapng writer는 raw captured bytes, original/captured length, interface별 linktype·snaplen·timestamp resolution을 보존해야 한다. 여러 interface/section, truncated packet, unknown block/options를 시험한다. 이번 스파이크는 한 IDB·기본 microsecond 해상도만 확인했다. 나노초·다중 IDB는 미확인이다. [pcapng 포맷](https://www.ietf.org/archive/id/draft-ietf-opsawg-pcapng-05.html), [pcap-file 소스](https://github.com/courvoif/pcap-file)
- 제품에서는 UDS 팀의 **동일 codec**을 진단 payload에 사용한다. 이번 자체 DoIP/SOME/IP parser는 샘플을 읽는 최소 실험 코드이며 완전한 ISO/AUTOSAR 적합성 구현이 아니다. UDS request_id ↔ 관련 packet_id 목록은 공유 이벤트에 붙이고 한 PDU가 여러 패킷에 걸칠 수 있게 한다.

## 5. TCP 재조립 선택

| 후보 | 버전 / 게시일 | 라이선스 | W / L / M | 판정·출처 |
|---|---|---|---|---|
| pdu | 1.5.1 / 2026-08-04 | Apache-2.0 | 추론 / 추론 / 추론 | L2/L3/L4 프레임 parser, TCP 재조립 엔진 아님. [메타](https://crates.io/api/v1/crates/pdu), [API](https://docs.rs/pdu/latest/pdu/) |
| smoltcp | 0.14.0 / 2026-08-17 | 0BSD | host backend별 / host backend별 / host backend별, 실측 미확인 | TCP/IP **endpoint stack**, passive analyzer의 재조립 라이브러리 대체 아님. C4.1 경로에도 불필요. [메타](https://crates.io/api/v1/crates/smoltcp), [소스](https://github.com/smoltcp-rs/smoltcp) |
| protolens | 0.2.3 / 2025-06-19 | MIT OR Apache-2.0 | 지원 문서 / 지원 문서 / 지원 문서 | 실제 out-of-order·재전송 stream 재조립 후보. DoIP/SD 적합성과 자원 상한 보장 미확인. [메타](https://crates.io/api/v1/crates/protolens), [README](https://github.com/chunhuitrue/protolens) |
| packet-dissector-reassembly | 0.6.3 / 2026-11-08 | MIT OR Apache-2.0 | 추론 / 추론 / 추론 | gap tracking·offset buffer·table building block. TCP connection lifecycle 전체를 대신하지 않는다. 공개 docs 검색에는 옛 0.4.2도 남아 있어 0.6.3 API·컴파일은 미확인. [메타](https://crates.io/api/v1/crates/packet-dissector-reassembly), [설계 설명](https://docs.rs/packet-dissector-reassembly/latest/packet_dissector_reassembly/) |

직접 구현 시 단순 payload concatenate로 끝내면 안 된다. 양방향 5-tuple+interface/VLAN+generation, SYN/FIN/RST, sequence wrap, 재전송/중복/충돌 overlap 정책, out-of-order, midstream 시작, 누락·truncation, idle eviction을 다룬다. IP fragment 재조립을 선행하며 DoIP 8-byte header의 length로 0/1/다수 PDU를 잘라낸다. SOME/IP TCP framing도 packet 경계와 분리한다.
최대 flow 수·방향별 bytes·총 bytes·PDU 길이·idle timeout을 모두 제한하고 초과/구멍은 `incomplete/gap`으로 노출한다. 수치는 C2 worker 예산과 함께 확정한다. 후보는 이 정책을 붙여 비교 시험하고 M1 전 선택한다. 이 스파이크는 **한 TCP packet에 완전한 PDU**만 넣었으며 TCP 재조립을 검증하지 않았다.

## 6. 선택 dissector: tshark / sharkd

기준 Wireshark suite **4.6.9 / 2026-09-23 / GPL-2.0 계열 / W·L·M 지원 문서**. 개별 파일의 “or later” 표기는 배포판 소스에서 확인한다. 외부 프로세스로 호출하고 libwireshark 직접 링크는 기본 설계에서 제외한다. 이 선택이 모든 배포 라이선스 의무를 없앤다는 뜻은 아니다. [공식 발표](https://www.wireshark.org/news/), [공식 라이선스 FAQ](https://www.wireshark.org/faq)

- `tshark -n -i <interface> -l -T ek`는 실시간 NDJSON 계열 출력이다. `ek`의 Elasticsearch bulk metadata/data 행을 parser가 구분해야 한다. `-T json`은 JSON 문서 출력이므로 NDJSON 한 줄=한 패킷이라고 읽지 않는다. `-l`은 패킷마다 flush하며 캡처·dissection·OS buffering 지연까지 보장하지 않는다. `-2`의 2-pass는 live/pipe에 쓸 수 없다. [매뉴얼](https://www.wireshark.org/docs/man-pages/tshark.html)
- 정본은 Rust capture worker가 소유한다. 선택 tshark에는 같은 raw stream을 `-i -` 입력으로 공급하거나 확정된 pcapng 조각을 넘긴다. tshark가 별도로 NIC를 열면 필터·시점·drop이 달라져 CLI/UI의 동일 데이터 원칙을 깨뜨린다. stdout/stderr·IPC 큐와 child RSS를 제한하고 느리면 dissector gap을 표시한다.
- sharkd는 파일을 load하고 frames/frame/follow 등을 요청하는 분석 daemon이다. 현재 man page는 newline JSON-RPC 2.0, 위키에는 구형 `{"req":"..."}` 형식도 남아 있다. 장기 고정 schema 호환 보장은 확인하지 못했다. **판 고정+startup capability/fixture 검사+어댑터**를 둔다. 계속 자라는 파일의 live 분석 의미·latency·Windows 배포물 포함 여부는 미확인이다. [man page](https://www.wireshark.org/docs/man-pages/sharkd.html), [옛 위키 형식](https://wiki.wireshark.org/Development/sharkd)
- 일반 설치 위치 후보: W `C:\Program Files\Wireshark\tshark.exe`, L distro package의 `/usr/bin/tshark`·`/usr/bin/dumpcap`; sharkd는 별도 package/빌드일 수 있다. M app bundle `Wireshark.app/Contents/MacOS/` 또는 패키지 PATH. 고정 경로 대신 사용자 지정 executable→PATH→후보 탐색 후 `--version`으로 확인한다. 이 Mac의 설치 실행 검증은 불가. [Windows 위치 근거](https://www.wireshark.org/docs/man-pages/extcap.html), [Debian 패키지 파일](https://packages.debian.org/trixie/amd64/tshark/filelist), [Mac 설치](https://www.wireshark.org/docs/wsug_html_chunked/ChBuildInstallOSXInstall.html)
- 필드 이름은 `eth.src`, `ip.src`, `tcp.srcport`, `doip.type`, `someipsd.entry.serviceid`처럼 **같은 의미의 subset**을 채택할 수 있다. `tshark -G fields` registry로 판별하고 native field와 decoder_version을 기록한다. 동일 이름이 곧 Wireshark 전체 display-filter 문법·reassembly·값 타입 호환은 아니다. 실제 `doip.futher_action` 오타는 4.4에서 `doip.further_action`으로 바뀌었다. alias/version 처리가 필요하다. [DoIP registry](https://www.wireshark.org/docs/dfref/d/doip.html), [SD registry](https://www.wireshark.org/docs/dfref/s/someipsd.html), [fields 매뉴얼](https://www.wireshark.org/docs/man-pages/tshark.html)

## 7. extcap

Wireshark suite 버전·릴리스·라이선스·W/L/M 범위는 §6과 같다. 별도 Loom Rust 바이너리의 실제 extcap 연결은 미확인이다. Python은 필요 없다.

- Rust executable이 `--extcap-interfaces`에 `interface {value=loom}{display=Loom stream}`을 출력하고 `--extcap-dlts --extcap-interface loom`에는 Ethernet DLT=1을 응답할 수 있다. `--extcap-config`로 세션 선택을 제공한다. [공식 개발 안내](https://www.wireshark.org/docs/wsdg_html_chunked/ChCaptureExtcap.html)
- `--extcap-interface loom --capture --fifo <path> [--extcap-capture-filter ...]` 호출에서 텍스트가 아닌 pcap/pcapng를 FIFO에 쓴다. Windows named pipe 포함 OS별 open/close·중단을 시험해야 한다. pcapng SHB+IDB 후 EPB를 보낸다. [동일 안내](https://www.wireshark.org/docs/wsdg_html_chunked/ChCaptureExtcap.html)
- 경로는 `capture worker → canonical stream/journal → bounded subscriber → loom-extcap → Wireshark`이다. GUI backpressure가 캡처 worker/STOP을 막지 않게 subscriber별 spool/drop·gap을 둔다. extcap 필터는 그 subscriber에만 적용한다. pcapng custom metadata를 Wireshark가 UDS request_id 링크로 자동 해석한다고 약속하지 않는다.

## 8. 처리량: 공개 근거와 확정할 수 없는 상한

- 산술: 1 Gb/s 단방향에서 1500 B payload + Ethernet header/FCS/preamble/IFG = 1538 B이면 **81,274 pps**; 최소 64 B frame + 20 B preamble/IFG = **1,488,095 pps**. 64 B의 18배 이상 packet rate 차이다. 양방향 포화는 합산 부하가 두 배다. VLAN·실제 프레임 길이·offload 조건을 명시해야 한다. 이 값은 측정이 아니다.
- 공개 Rust 근거 A: **ixy.rs**, MIT, 조사 README 기준판·릴리스일 미확인, L Intel NIC 전용(W/M 미지원). 저자는 Xeon E3-1230v2 3.3GHz 단일 core·64B·20Gb/s 양방향 forwarder에서 **26 Mpps 초과**를 제시한다. 자체 userspace NIC driver/batching이며 Npcap/libpcap·저널·JSON/UI 경로가 아니므로 Loom 상한으로 전용할 수 없다. [저자 저장소](https://github.com/ixy-languages/ixy.rs), [연구 논문, 2019-09-13](https://arxiv.org/abs/1909.06344)
- 근거 B: protolens(§5)은 Rust 1.87·Mac mini M4/Ubuntu Xeon 환경에서 합성 payload·프로토콜 처리 throughput을 공개한다. NIC ingress·drop-free wire rate 시험과 다르며 byte/s를 minimum-frame pps 보장으로 환산하지 않는다. [방법·수치](https://github.com/chunhuitrue/protolens)
- 근거 C: Linux 공식 문서는 mmap 링·batch·NIC interrupt mitigation·CPU pinning의 효과와 환경 의존성을 설명한다. netring README도 capture와 dispatch 성능을 구분하지만 링크된 상세 성능 문서는 이번 조회에서 404였다. 해당 crate의 재현 가능한 수치 상한은 **미확인**이다. [커널 문서](https://www.kernel.org/doc/html/latest/networking/packet_mmap.html), [netring 해당 판](https://docs.rs/crate/netring/0.32.0)
- 결론: **Rust 자체가 1Gb/s의 병목이라는 근거는 없지만, 선택 backend+전체 Loom 경로의 무손실 상한은 아직 미확인**. 이번 9 packet offline 시간은 처리량 벤치가 아니다. 81k/1.488Mpps 무손실을 제품 보증으로 쓰지 않는다.
- M2 W/L NIC 벤치: 64/128/512/1518B·VLAN·양방향·burst·DoIP+SD+재조립 mix, snaplen/BPF/OSbuffer/IRQ/offload/CPU/SSD 조건을 기록한다. generator seq와 NIC stats·pcap drop·앱 queue drop·journal gap·UI lag를 대조한다. raw ingress→decode→journal→CLI/UI 단계를 각각 재고 bounded byte queues/RSS·disk spill/rotation을 검증한다. 1Gb/s 원시량은 약 125MB/s, 20GiB는 약 172초이므로 E6 “30일 또는 20GiB 먼저”를 고부하 30일 보관으로 해석하지 않는다.

## 9. 스파이크 결과 — 완료, 10분 이내

위치: [`spikes/eth/r1-sol/`](../../../spikes/eth/r1-sol/). Rust 1.93.1, etherparse=0.21.0·pcap-file=2.0.0 exact pin, Cargo.lock 포함. Node는 실행 시간·파일 기록만 담당하며 packet 생성/읽기/해석은 모두 Rust이다.
시작 **12:54:11.057 UTC**, 성공 결과 **12:58:00.027 UTC**, **228.970초**(준비·fetch·작성·빌드·실행 포함). `CARGO_HOME=~/.cache/loom-eth-cargo`, `CARGO_TARGET_DIR=~/.cache/loom-eth-target`. 소규모 crate 13개만 fetch, 프로젝트 안 target 없음. 파일 읽기 30초 stall 없음.

| 측정/검증 | 결과 |
|---|---|
| 최초 debug build (fetch 제외) | 10.788초 wall / cargo 표시 10.66초 / exit 0 |
| 최초 process 실행 | 309.507ms wall / 프로그램 내부 생성+읽기 11.459ms / exit 0 |
| 파일 | sample.pcapng; SHB+Ethernet IDB+EPB 9개; 기본 microsecond timestamp |
| 포함 | UDP13400 vehicle announcement, TCP 3-way handshake, routing activation request/response, UDS22F190/62F190+VIN, VLAN42 SOME/IP-SD offer+IPv4 UDP endpoint |
| assertion | 9 packet·6 application PDU·VLAN 1개, roundtrip raw bytes·timestamps 일치, 잘린 DoIP·inverse 오류·SOME/IP 길이 오류 3건 거부 |
| 미검증 | 라이브 NIC, W/L, Wireshark 교차 decode, full TCP 재조립·IP fragment, checksum 독립 재검산, nanosecond/multi-IDB, UI·CLI 통합·성능 |

실제 요약 출력 일부(전체는 파일):
```text
#1 UDP 13400->13400 DoIP vehicle-announcement VIN=LOOM0000000000001 logical=1000
#5 TCP 50000->13400 DoIP routing-activation-request tester=0e00
#6 TCP 13400->50000 DoIP routing-activation-response code=10
#7 TCP 50000->13400 DoIP diagnostic 0e00->1000 UDS=[22, F1, 90]
#9 UDP 30490->30490 VLAN=42 SOME/IP-SD OfferService service=0x1234 instance=1 ttl=3 endpoint=192.0.2.2:30500
PASS packets=9 application_pdus=6 vlan=1 roundtrip_bytes/timestamps=ok malformed_rejections=3 elapsed_us=11459
```
근거: [소스](../../../spikes/eth/r1-sol/src/main.rs), [출력](../../../spikes/eth/r1-sol/run.stdout.txt), [build 로그](../../../spikes/eth/r1-sol/build.stderr.txt), [측정 JSON](../../../spikes/eth/r1-sol/timings.json), [재현 안내](../../../spikes/eth/r1-sol/README.md). 외부 정형 ECU fixture에 대한 적합성 확인은 미확인이다.

## 10. 최종 권고

| 항목 | 권고 | 다음 관문 |
|---|---|---|
| 기본 캡처 | pcap + W Npcap / L·M libpcap; 단일 worker 소유 | W/L compile·NIC drop/ts/중단 시험 |
| Windows 배포 | Npcap 미번들, 사람의 직접 설치·사용 자격 확인 | 필요 시 OEM Redistribution 계약 |
| Linux 권한 | 작은 capture helper에 최소 CAP_NET_RAW; 그룹 ACL, NET_ADMIN은 필요 입증 시 | 배포 환경별 capability·promisc 검증 |
| macOS 개발 | 지금은 offline replay; live는 사람의 ChmodBPF 설정 후 | 현재 BPF EACCES, 시스템 변경 없음 |
| 파싱·저장 | etherparse + pcap-file pcapng, someip_parse 평가, UDS 공통 codec 재사용 | malformed·truncation·multi-IDB·재조립 fixture |
| TCP 재조립 | protolens/building block 평가 후 bounded 정책 적용; pdu·smoltcp는 제외 | gap/overlap/wrap/재전송/eviction 시험 |
| tshark/sharkd | optional 외부 process, 판 고정·어댑터; 핵심 루프 필수 의존으로 두지 않음 | 설치 탐지·schema fixture·child 자원 상한 |
| Wireshark live | Rust extcap subscriber; canonical stream 공유 | W FIFO/named pipe·backpressure 검증 |
| 고성능 Linux | libpcap부터 측정; 부족할 때 mmap 후보 평가 | netring MSRV·license·복잡도·실측 비교 |
| 핵심 루프 | packet_id/raw evidence를 journal·CLI·UI에 공유, request_id 연결, gap 표시·STOP 경로 보존 | replay+가상 DoIP M1, W/L NIC+converter M2; F1 기본 자동·정책은 opt-in |
