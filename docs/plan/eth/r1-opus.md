# ETH R1 초안 — 이더넷 패킷 도메인 `eth` (explore-opus: 구조·CLI·UI·테스트)

작성: explore-opus, 2026-11-10. 기준: `02-core-principle.md` → 마스터 F1 > E > C > B > A > D, `eth/brief.md`.
성격: 계획 초안이다. 구현 코드는 없다. crate 버전·Npcap·Wireshark 필드 이름 같은 사실은 Sol R1(`r1-sol.md`)이 확인한다. 여기서 확인하지 못한 것은 **미확인**으로 표시한다.
협의 초안(리드가 `_xteam/`으로 옮김): `xteam-draft-eth-core.md`, `xteam-draft-eth-uds.md`, `xteam-draft-eth-ui.md`, `xteam-draft-test-domains-eth.md`.

## 핵심 루프

| 요소 | eth에서 무엇인가 | 구현 (명령 · 구성요소 · 화면) |
|---|---|---|
| ① 직접 접근 | 에이전트가 NIC를 골라 캡처를 시작한다. 사람의 손은 Npcap 설치·`CAP_NET_RAW` 부여 같은 **사전 설정 1회**뿐이다 | `loom eth ifaces` / `capture start <if> --bpf …`. `loomd` worker `eth-<if>`가 pcap 핸들을 단독 소유한다(Npcap/libpcap 로드 = W1) |
| ② 읽기 | tshark처럼 **실시간 한 줄 요약**, 계층 상세, 표시 필터, 경계 있는 대기를 쓴다 | `trace -Y <필터> [--follow --timeout]`, `last`, `show <ref>`, `wait-for -Y … --count --timeout`, `stats`, `follow tcp|udp`. worker가 필터를 먼저 걸러서 보내고 인덱스로 조회한다 |
| ③ 쓰기 | 원시 프레임·UDP·TCP 페이로드 송신. 등급은 표시만 한다(F1 자동) | `loom eth send raw|udp|tcp`. 같은 쓰기 파이프라인을 지나며 송신 프레임도 캡처 목록에 **AGT 배지**로 다시 나타난다 |
| ④ 사용자 실시간 UI + 개입 | 사용자는 **Wireshark처럼** 같은 패킷을 같은 순간에 본다(목록·상세 트리·hex·필터·스트림·UDS 링크). 재전송·편집 송신·캡처 정지·STOP으로 개입한다 | UI 패널 `eth.packets`(+ `eth.stream`, `eth.stats`). 사용자 송신은 origin=user(USR 배지). 에이전트는 `others_since_last`로 안다 |

**종단 간 시나리오(M1, sim)**:
1. 에이전트가 `loom eth capture start lo --bpf "port 13400"`과 `loom uds read-did F190 -t sim1`(가상 DoIP ECU)을 실행한다.
2. 패널에 DoIP 행 5개(공지·라우팅·진단 요청 AGT·ACK·응답 DEV)가 ≤ 200 ms 안에 뜬다. 진단 요청 행에는 `↔uds:t-000418` 칩이 붙는다.
3. 사용자가 요청 행을 우클릭해 "편집 후 송신"으로 DID를 `F18C`로 바꿔 보낸다. 이 행은 USR 배지로 표시된다.
4. 에이전트의 다음 `loom eth trace --since @last`에 `others_since_last{count:1, origins:[user]}`가 붙는다.

---

## 1. 범위 / 비범위

**범위 (P1 M1부터)**
- 캡처: NIC 목록·선택, BPF 캡처 필터, promisc, snaplen, 커널 버퍼, 드롭 통계 → `source.gap`.
- 디코드: Ethernet/802.1Q/ARP/IPv4/IPv6/ICMP(v6)/UDP/TCP(재조립·스트림 번호), **DoIP**, **DoIP 위 UDS**(UDS 코덱 공유), **SOME/IP·SOME/IP-SD**.
- **표시 필터**: Wireshark 부분집합 1종. CLI·UI·TEST 술어가 공유한다.
- 에이전트 CLI(tshark 대응), UI 패널(Wireshark 대응), 송신, pcapng 내보내기, UDS 상호 링크, 통합 테스트 증거·술어.

**후순위**
- tshark/sharkd 외부 dissector(M3, 선택, 사용자 설치).
- Wireshark extcap 브리지(M3, 선택).
- TLS 복호, 802.1AS·TSN 분석, 바이너리 열 전송(CORE M2 `loom.columns.v1`).

**비범위**
- Vector XL 이더넷 API(C4.1로 제외).
- 차량 네트워크 스위치 설정.
- 원격 캡처(rpcap).
- 모든 프로토콜을 Wireshark 수준으로 디코드하는 것 — 범위 밖은 tshark 연동 또는 hex로 둔다.

## 2. 사용 시나리오

**에이전트**
- (A1) DoIP 연결 실패 원인: `trace -Y 'doip || arp || icmp'` → "routing activation 응답 코드 0x06(SA 미등록)" 발견 → UDS 설정 수정.
- (A2) ECU 리셋 후 공지 확인: `wait-for -Y 'doip.type == 0x0004' --count 3 --within 5s --anchor uds:last-tx`.
- (A3) SOME/IP 서비스 확인: `stats --by conv -Y someip`, `follow udp 12`로 한 서비스 대화를 본다.
- (A4) UDS 요청이 실제로 NIC를 떠났는지: `trace -Y 'loom.request_id == "t-000418"'`.

**사용자**
- (U1) 패널 필터 바에 `doip.target_address == 0x1001`을 입력하면 자동완성되고, 오류는 빨간 밑줄로 표시된다.
- (U2) 진단 요청 행의 UDS 칩 → UDS 콘솔 해당 행으로 이동한다. 반대 방향도 된다.
- (U3) 행 우클릭 → 재전송 / 편집 후 송신 / 이 대화 따라가기 / 이 행부터 상대 시간 기준.
- (U4) 고부하에서 "표시 1/12 샘플링 · 기록 100 % · 커널 드롭 0" 배지를 본다. 일시정지하면 구간 전체가 인덱스 조회로 완전하게 보인다.

## 3. 아키텍처

### 3.1 배치
`eth-<if>` **worker 프로세스**, 인터페이스당 1개, 최대 4개다. W1(wpcap.dll/libpcap 네이티브 로드)·W4(고속 버퍼 메모리 예산)에 해당한다. DoIP 진단 소켓은 UDS 세션 관리자(gateway)가 그대로 소유하고, eth는 **관찰만** 한다(같은 장치를 두 번 열지 않음 — eth는 NIC 캡처, UDS는 OS TCP 소켓). UDS 링크기는 gateway 모듈 `eth-link`다(§3.5).

### 3.2 worker 내부 경로 (1 Gbps 가정)

```
NIC ─(Npcap / AF_PACKET TPACKET_V3 / BPF)─▶ [E1 커널 버퍼 64 MiB 요청] ── 드롭 ─▶ pcap_stats 100 ms 샘플 → source.gap{cause: kernel_drop|if_drop, count}
   └▶ 캡처 스레드: next_ex(비차단) → ts{pcap 시각, host mono} → source_seq
        └▶ [E2 원시 링 128 MiB 고정] ─┬─▶ 저널 writer 스레드 → CORE 세그먼트(codec pcap-epb/1)  ← 무손실 경로
                                     │      넘침 → record.gap + record.health failed (A12)
                                     ├─▶ 디코드 스레드(1–2): L2–L4 + DoIP/SOME-IP 헤더 + TCP 재조립 → 요약·인덱스
                                     │      뒤처짐 → 요약 건너뜀 + decode_lag 배지 (원본은 이미 저널에)
                                     └─▶ 발행 단계 → [E3 화면 미러 4 MiB] → data lane(크레딧) → hub
                                            예산 초과 → 관심 패킷만 + eth.batch 커버리지 + display.gap
에이전트·UI 범위 조회 ─▶ eth.query(worker) → 인덱스 + 세그먼트 디코드(필터 적용, 상한 있음)
```

**큐 상한·넘칠 때 정책 (C2, CORE §9.2 표에 추가 요청)**

| # | 큐 | 상한 | 넘칠 때 |
|---|---|---|---|
| E1 | 커널/드라이버 버퍼 | 요청 64 MiB(실제값 manifest 기록) | 커널 드롭 → `source.gap{kernel_drop}`. 위치는 100 ms 창 단위로만 앎(§8.4 TEST 주의) |
| E2 | 원시 링(캡처→저널·디코드 공유) | 128 MiB / 512k 패킷 | 저널 미기록 패킷이 밀려나면 `record.gap` + health failed. 디코드만 밀리면 요약 생략(원본 유지) |
| E3 | 화면 미러 | 4 MiB / 8192건 | 화면 드롭 + `display.gap{producer_range}` |
| E4 | 발행 예산 | 기본 5k pkt/s(설정 ≤ 20k) | 넘으면 **관심 패킷**(활성 구독·대기 필터 합집합에 맞는 것)만 개별 발행. 나머지는 100 ms마다 `eth.batch` |
| E5 | TCP 재조립 | 스트림당 4 MiB, 동시 1024 스트림, 유휴 120 s | 넘치면 그 스트림 `reassembly_truncated` 표시(원본 유지) |
| E6 | 인덱스(메모리) | 32 MiB | 세그먼트 sparse 인덱스로 내려보냄 |
| E7 | 송신 큐 | 64건 / 1 MiB, 기본 1000 pkt/s 레이트 | `E_BUSY`(exit 4). 송신은 `failed` |

- **세 경로 분리**: 저널은 무손실, 화면·에이전트 라이브는 예산 안에서 정직하게 줄인다. 범위 조회(일시정지·필터·`trace --since`)는 저널 인덱스에서 **완전하게** 읽는다.
- **1 Gbps의 현실**:
  - 64 B 패킷이면 약 1.48 Mpps, 1500 B면 약 125 MB/s다(산술).
  - 20 GiB quota는 선속도로 **약 3분** 만에 찬다. `capture start`는 `--snaplen`(헤더만 256 B)·BPF·`--ring <크기>`(오래된 eth 세그먼트 우선 회전)를 권장하고, "보존 가능 기간 추정"을 표시한다.
  - 차량 DoIP·SOME/IP 실제 부하는 대개 Mbps 이하다(추정).
- **worker 메모리 예산**: soft 256 MiB / hard 384 MiB(추정 초기값 — 링 128 + 재조립 64 + 인덱스 32 + 여유).

### 3.3 드롭 3층 (A13)

| 층 | 원인 | 이벤트 | 화면 |
|---|---|---|---|
| `source.gap` | 커널·드라이버 드롭(`ps_drop`, `ps_ifdrop`), 캡처 재시작 공백, worker 재기동 | `cause: kernel_drop / if_drop / capture_restart / gateway_restart`, `count` 또는 `unknown` | 빨간 "수신 전 손실 N" 배지 + 목록에 틈 행 |
| `record.gap` | E2 넘침, 디스크 지연·용량 | 범위·count | "기록 손실" 배너(A12: agent 쓰기 중단) |
| `display.gap` | E3·E4·구독 큐 | producer 범위 | "표시 1/N 샘플링 · 기록 100 %" |

### 3.4 기록 정본 — **CORE 저널 세그먼트 + codec `pcap-epb/1`** 제안

| 안 | 장점 | 단점 |
|---|---|---|
| (A) pcapng 세그먼트를 별도 정본으로 | 표준, Wireshark 바로 열림 | 보존·GC·pin·tail 복구·mapping을 CORE와 이중 구현. 증거 pin이 어긋날 위험 |
| (B) CORE 저널 프레임(JSON) + pcapng export | 저장 체계 하나 | M1 JSON codec은 패킷당 수백 B + base64 → 고속 불가 |
| **(C) CORE 저널 프레임, 레코드 = pcapng EPB(Enhanced Packet Block) 바이트** | E5(길이+CRC32C+codec id)를 그대로 쓰므로 보존·pin·tail 복구·mapping이 CORE 것. 레코드는 원시라 작다. **pcapng export = 프레임 벗기기 + SHB/IDB 앞에 붙이기**(스트리밍, 복사만) | CORE codec 레지스트리에 도메인 codec 1종 추가 필요 |

**권고 (C)**:
- EPB 옵션에 `source_seq`, `dir`, 송신 링크(`request_id`)를 넣는다(pcapng custom option 또는 `opt_comment` — 표기는 미확인, Sol).
- 요약 이벤트(`eth.pkt`)는 저널에 따로 쓰지 않는다. 같은 `source_seq`의 **파생**이며 허브 mapping 로그로 연결된다.
- 상세는 `xteam-draft-eth-core.md`.

### 3.5 UDS 링크 (상세 `xteam-draft-eth-uds.md`)
- UDS는 `uds.tx/rx`에 `payload.wire_key{transport:"doip", local, remote, sa, ta, uds_sha256_8}`를 넣는다(요청).
- gateway 모듈 `eth-link`가 `uds.tx/rx`와 `eth.pkt`(doip 진단 0x8001/0x8002/0x8003)를 구독한다. (5-tuple, SA/TA, UDS 바이트 해시, 시각 창 −5 ms…+2 s)가 일치하면 `eth.link{eth_ref, request_id, role: request|ack|nack|response, confidence: exact|probable}`를 발행한다.
- **중복 규칙**:
  - 같은 바이트를 두 도메인이 기록하는 것은 허용한다(관점이 다르다: `uds.tx` = 스택에 넘김, eth 패킷 = NIC에서 관찰).
  - 어느 쪽도 상대 이벤트를 다시 만들지 않는다.
  - 판정·화면의 "전송 증거"는 eth 관찰이 더 강하다. UDS `tx_state`에 선택 값 `nic_observed`를 추가하자고 제안한다(M2).

### 3.6 송신 프레임의 출처
- 캡처된 패킷 레코드의 `origin`은 기본 `device`다(관찰).
- `eth.send`(또는 UDS DoIP 요청)와 링크되면 `payload.attributed_origin = agent|user|script`를 붙인다. 배지는 attributed_origin을 따른다(AGT/USR).
- TEST 검증기는 eth 관측 증거를 고를 때 **attributed_origin이 있는 패킷을 device 관측으로 세지 않는다**. 에이전트 자신의 송신만으로 pass되는 것을 막는다(TEST R-8).

## 4. CLI / API 명세 초안 (tshark 대응)

D5 봉투·출력 상한(기본 100행, 하드 1,000행/64 KiB)·커서(`@last | now | seq:N | mark:X | -5s | …`)·`others_since_last`를 따른다.

| 명령 | tshark 대응 | 요지 / 종료 코드 |
|---|---|---|
| `loom eth ifaces [--json]` | `tshark -D` | 이름·별칭·MAC·IP·링크·속도·캡처 권한 여부(`E_PERMISSION` 안내) |
| `loom eth capture start <if> [--name ch] [--bpf EXPR] [--snaplen N] [--promisc] [--buffer 64MiB] [--ring 2GiB]` | `-i -f -s -B` | 7 `E_PERMISSION`/`E_NOT_FOUND`, 2 BPF 문법 오류. 캡처 = R0(관찰) |
| `loom eth capture stop|status [<ch>]` | — | status에 pps·bps·커널 드롭·디코드 지연·발행 비율·보존 추정 |
| `loom eth trace [<ch>] [-Y FILTER] [--since CUR] [--max N] [--follow --timeout T] [--detail] [--json]` | `-Y -c -l` / `-V` | 한 줄 요약. `--follow`는 ≤ 600 s |
| `loom eth last [<ch>] [-Y FILTER] [-n N]` | — | 최근 N개(링 기준) |
| `loom eth show <REF> [--hex]` | `-V -x` (1개) | 계층 트리 + hex |
| `loom eth wait-for [<ch>] -Y FILTER [--count N[..M]] [--within T --anchor REF] --timeout T` | — | 0 만족 / **3 `E_TIMEOUT`·`E_COUNT_OUT_OF_RANGE`**(E2) / 8 창 안 gap(inconclusive) |
| `loom eth stats [<ch>] --by proto|conv|endpoint|io [-Y F] [--since]` | `-z io,phs/conv` | 상위 N |
| `loom eth follow tcp|udp <stream#> [--as ascii|hex] [--max-bytes 16KiB]` | `-z follow` | 방향별 색, 잘림 표시 |
| `loom eth send raw <ch> --hex …` · `send udp <ch> --to IP:PORT [--from-port P] --hex|--text` · `send tcp --to IP:PORT --hex [--keep-open]` | — | raw = pcap inject. udp·tcp = worker의 OS 소켓(TCP는 **자기 연결만** — 남의 TCP 스트림에는 끼어들지 않음). `--dry-run`, `--rate` |
| `loom eth export pcapng [<ch>] --since --until [-Y F] -o FILE` | `-w` | job, 진행률. 필터 시 EPB만 선택 |
| `loom eth filter check EXPR` | `-Y` 검사 | 2 `E_USAGE{detail:{pos, expected}}` |

**한 줄 요약 출력 (사람용)**
```
$ loom eth trace lan1 -Y doip --max 5
    seq   time      source         destination      proto     len  info                                                  org
  41201  12.004113  192.168.0.10   255.255.255.255  DoIP       90  Vehicle announcement (0x0004) VIN=WVWZZZ1KZAW000001 LA=0x1001  DEV
  41230  12.104883  192.168.0.2    192.168.0.10     DoIP       64  Routing activation request (0x0005) SA=0x0E80 type=0x00      AGT
  41231  12.106120  192.168.0.10   192.168.0.2      DoIP       69  Routing activation response (0x0006) SA=0x0E80 code=0x10    DEV
  41240  12.110002  192.168.0.2    192.168.0.10     DoIP/UDS   62  Diag 0x0E80→0x1001 22 F1 90 ReadDataByIdentifier ↔uds:t-000418 AGT
  41242  12.118400  192.168.0.10   192.168.0.2      DoIP/UDS   81  Diag 0x1001→0x0E80 62 F1 90 "WVWZZZ1KZAW000001" ↔uds:t-000418 DEV
(5 rows · source.gap 0 · display 1/1 · next: --since seq:41243)
```
열은 tshark 기본(No. Time Source Destination Protocol Length Info)을 따르고, Loom 열 `seq`·`org`를 더한다. `time`은 기본 캡처 시작 상대시간이고 `--time abs|delta|rel:REF`로 바꾼다.

**`--json` 상세 (발췌)**
```json
{"schema_version":1,"ok":true,"cmd":"eth.trace","data":{"rows":[
 {"ref":"eth:lan1@seq:41240","seq":"41240","ts":{"wall_ns":"…","mono_ns":"…","source":"pcap_host","quality":"ok"},
  "dir":"tx","origin":"device","attributed_origin":"agent","len":62,"proto":"DoIP/UDS",
  "info":"Diag 0x0E80→0x1001 22 F1 90 ReadDataByIdentifier",
  "layers":{"eth":{"src":"…","dst":"…","type":"0x0800"},"ip":{"src":"192.168.0.2","dst":"192.168.0.10","proto":6},
   "tcp":{"srcport":50112,"dstport":13400,"stream":3,"seq_rel":145,"len":12},
   "doip":{"version":2,"type":"0x8001","length":8,"source_address":"0x0E80","target_address":"0x1001"},
   "uds":{"sid":"0x22","service":"ReadDataByIdentifier","did":"F190","req_hex":"22F190"}},
  "link":{"domain":"uds","request_id":"t-000418","role":"request","confidence":"exact"},
  "bytes":{"encoding":"raw_ref","ref":"eth:lan1/src:7/88120"}}]},
 "meta":{"truncated":false,"next_cursor":"seq:41243","range":{"from":"41201","to":"41242"},"display_gap":null,"record_health":"ok"},
 "warnings":[],"others_since_last":{"count":0}}
```
- 대기형 결과에는 `data.evidence[] = [{ref, kind:"eth.match", filter, count, window, gaps}]`가 들어간다(TEST TC-C5).
- 바이트 원문은 `show --hex` 또는 `raw_ref` HTTP로만 준다. 에이전트 컨텍스트를 보호하기 위해서다.

### 4.1 표시 필터 문법 — Wireshark 부분집합 `loom-pktfilter`
- **필드**: `프로토콜[.필드…]`. 프로토콜 이름만 쓰면 존재 검사다(`doip`, `tcp`).
- **필드 이름**: Wireshark 이름을 정본으로 하고, 다른 표기는 별칭으로 받는다.

| 계층 | 필드(Loom 정본, Wireshark와 같게 — 실제 Wireshark 이름은 **미확인**, Sol 대조) | 별칭 |
|---|---|---|
| frame | `frame.len`, `frame.time_relative`, `frame.time_delta` | |
| eth/vlan/arp | `eth.src`, `eth.dst`, `eth.addr`, `eth.type`, `vlan.id`, `vlan.priority`, `arp.opcode`, `arp.src.proto_ipv4` | |
| ip/ipv6/icmp | `ip.src`, `ip.dst`, `ip.addr`, `ip.proto`, `ip.ttl`, `ipv6.src/dst/addr`, `icmp.type`, `icmpv6.type` | |
| udp/tcp | `udp.port/srcport/dstport/length`, `tcp.port/srcport/dstport/stream/len/seq/ack`, `tcp.flags.syn/fin/rst`, `tcp.analysis.retransmission` | |
| DoIP | `doip.version`, `doip.type`, `doip.length`, `doip.source_address`, `doip.target_address`, `doip.logical_address`, `doip.vin`, `doip.response_code`, `doip.ack_code` | `doip.payload_type` = `doip.type` |
| UDS (DoIP 위) | `uds.sid`, `uds.reply`, `uds.did`, `uds.nrc`, `uds.subfunction` | |
| SOME/IP(-SD) | `someip.service_id`, `someip.method_id`, `someip.client_id`, `someip.session_id`, `someip.message_type`, `someip.return_code`, `someipsd.entry.type`, `someipsd.entry.service_id`, `someipsd.entry.ttl` | Wireshark 실제 표기(`someip.serviceid` 등) 별칭 |
| Loom | `loom.origin`, `loom.dir`, `loom.request_id`, `loom.seq` | |

- **연산자**: `== != < <= > >=`(별칭 `eq ne lt le gt ge`), `contains`(바이트·문자열), `matches`(Rust `regex`, 백트래킹 없음), `in {a, b, 10..20}`.
- **의미**:
  - 다중 값 필드(`ip.addr`)의 `==`는 "어느 하나"다.
  - `a != b`는 `not (a == b)`로 **정의**한다(Wireshark 버전별 의미 차이를 피함).
- **논리**: `and or not && || !`, 괄호. **슬라이스**: `eth.src[0:3] == 00:1b:21`, `frame[12:2] == 08:00`.
- **리터럴**:
  - 10진·`0x` 정수, MAC `aa:bb:…`, IPv4/IPv6, **CIDR**(`ip.addr == 10.0.0.0/8`), 문자열 `"…"`, 바이트열 `aa:bb:cc`.
- **비지원(M1)**: 함수(`len()`, `upper()`), 산술, 매크로, 필드 참조 비교(`ip.src == ip.dst`), 레이어 연산자(`ip.addr#2`). 쓰면 위치와 함께 오류를 낸다.
- **한 구현, 세 곳에서 사용**:
  - Rust crate `loom-pktfilter`(파서 → AST → 타입 검사 → 평가기)를 worker 푸시다운·`eth.query`·TEST 검증기가 같이 쓴다.
  - CLI는 `filter check`를 서버 RPC로 부른다.
  - UI는 같은 crate의 WASM 빌드로 자동완성·밑줄을 처리한다(M2). M1은 `eth.filter.check` RPC다.
- **캡처 필터(BPF)와 구분**: `--bpf`는 libpcap 문법이고 커널에서 걸러 버린다(기록에도 없음). 바꾸려면 캡처를 다시 시작해야 한다. `-Y`는 표시 필터이고 기록은 그대로 둔 채 보기만 줄인다. UI에서도 두 입력을 다른 색·위치에 둔다(캡처 설정 대화상자 vs 필터 바).

## 5. 사용자 UI (상세 `xteam-draft-eth-ui.md`)

```
┌ eth.packets ─ lan1 ● LIVE  pps 312  bps 1.2M  kernel drop 0  record OK  display 1/1 ──── [■ 캡처 정지] [⇩ pcapng] ┐
│ 필터: doip && doip.target_address == 0x1001                                   ✓ [적용] [저장]  캡처: port 13400  │
├──────┬───────────┬──────────────┬──────────────┬──────────┬─────┬──────────────────────────────────────────┬─────┤
│ seq  │ 시간(rel) │ 출발지       │ 목적지       │ 프로토콜 │ 길이│ 정보                                     │ 출처│
│41230 │ 12.104883 │ 192.168.0.2  │ 192.168.0.10 │ DoIP     │  64 │ Routing activation request SA=0x0E80     │ AGT │
│41240 │ 12.110002 │ 192.168.0.2  │ 192.168.0.10 │ DoIP/UDS │  62 │ 22 F1 90 ReadDataByIdentifier  ↔UDS     │ AGT │
│41242▶│ 12.118400 │ 192.168.0.10 │ 192.168.0.2  │ DoIP/UDS │  81 │ 62 F1 90 "WVWZZZ1KZAW000001"  ↔UDS      │ DEV │
│41260 │ 13.002011 │ 192.168.0.2  │ 192.168.0.10 │ DoIP/UDS │  62 │ 22 F1 8C ReadDataByIdentifier            │ USR │
├──────┴───────────┴──────────────┴──────────────┴──────────┴─────┴──────────────────────────────────────────┴─────┤
│ ▾ Frame 41242: 81 bytes, rx, pcap_host ts            │ 0000  00 1b 21 aa bb cc 00 1b 21 11 22 33 08 00 45 00 │
│ ▸ Ethernet II  ▸ IPv4 192.168.0.10 → 192.168.0.2     │ 0010  00 43 …                                          │
│ ▸ TCP 13400 → 50112, stream 3, len 25                │ 0030  02 fd 80 01 00 00 00 11 10 01 0e 80 [62 f1 90] │
│ ▾ DoIP v2 Diagnostic message (0x8001) 0x1001→0x0E80  │ 0040  57 56 57 5a 5a 5a …                              │
│   ▾ UDS 0x62 ReadDataByIdentifier positive  [UDS 행 →]│                                                       │
└──────────────────────────────────────────────────────┴───────────────────────────────────────────────────────┘
```
- 행 우클릭: 재전송 · 편집 후 송신(hex 편집 + 디코드 미리보기) · TCP/UDP 스트림 따라가기 · 이 행을 시간 기준으로 · 이 값으로 필터 · 증거 REF 복사.
- 개입은 모두 같은 RPC를 쓴다(`eth.send` origin=user, right=operate, `eth.capture.stop` operate). STOP·캡처 정지는 `safetyDirection: reduce`다.

## 6. 데이터 / 기록 포맷

- **이벤트 kind**:
  - `eth.pkt`: 요약 + `layers` 일부 + `bytes.raw_ref`(EPB 위치).
  - `eth.batch`: 커버리지 — `{first_src, last_src, count, by_proto}`. 개별 발행되지 않은 패킷도 stream 좌표로 인용할 수 있게 하는 것이다(§8).
  - `eth.tx`: 송신 요청 행. `request_id` + `payload.write_state`, E8 행 갱신.
  - `eth.link`, `eth.capture_state`(시작·정지·BPF·snaplen·커널 버퍼 실제값), `eth.stats`(1 s 집계).
  - 손실은 core `source.gap`/`record.gap`/`display.gap`.
- **ts**: `ts.source = pcap_host | pcap_adapter | replay`, `ts.quality`. Npcap/libpcap 정밀도(마이크로/나노)는 `eth.capture_state`에 기록한다. 실제 정밀도는 미확인(Sol).
- **저널**: §3.4 (C). 레코드 = EPB 바이트 + Loom 옵션.
- **export**: `pcapng`(SHB + IDB + EPB, Loom 옵션 유지). Wireshark에서 읽힐 때 Loom 옵션은 주석으로 보인다(형식 미확인).
- **TCP 스트림 번호**: 캡처 세션 안에서 증가하는 정수(`tcp.stream`). 재시작하면 epoch가 바뀐다. REF에는 epoch가 들어 있어서 섞이지 않는다.

## 7. 라이브러리 선택과 근거 (Rust, C1 — 버전·라이선스·OS 지원은 Sol R1 확인 전이라 모두 미확인)

| 용도 | 후보 | 메모 |
|---|---|---|
| 캡처 | `pcap`(libpcap/Npcap 바인딩) 기본, Linux 고속은 AF_PACKET TPACKET_V3 직접 검토 | 드롭 통계·나노초 정밀도·비차단 지원 여부 확인 |
| L2–L4 파싱 | `etherparse` 또는 `pnet_packet` | 할당 없는 슬라이스 파서 선호 |
| pcapng 읽기/쓰기 | `pcap-file` / `pcap-parser` | EPB custom option 쓰기 지원 여부 |
| TCP 재조립 | 직접 구현(작게: 순서·중복·구멍·상한) | 범위 밖 기능(윈도 스케일 분석 등)은 안 함 |
| DoIP·UDS | **UDS 팀 `loom-uds` 코덱의 no-IO 부분 공유**(공개 crate는 UDS D26에서 비채택) | `xteam-draft-eth-uds.md` |
| SOME/IP(-SD) | `someip_parse` 검토, 아니면 직접 | |
| 필터 | 직접 구현 `loom-pktfilter`(파서는 `winnow`/`chumsky` 등 후보) + `regex` + `ipnet` | WASM 빌드 가능해야 함 |
| 외부 dissector(선택) | tshark/sharkd 외부 프로세스(GPL-2.0, 사용자 설치) | 선택 패킷 상세만, M3 |

Python은 쓰지 않는다.

## 8. HW / OS 지원 매트릭스

| 항목 | Windows x64 (실HW) | Linux x64 (실HW) | macOS (개발) |
|---|---|---|---|
| 캡처 드라이버 | **Npcap — 사용자가 설치**(번들 가능 여부는 Sol, 미확인). "관리자만 캡처" 옵션이면 데몬 권한 문제 | libpcap/AF_PACKET. **`CAP_NET_RAW`(+`CAP_NET_ADMIN` promisc)를 사람이 `setcap`으로 부여** | BPF `/dev/bpf*` 권한(ChmodBPF 등). 시스템 설정은 바꾸지 않음 |
| loopback 캡처(M1 sim) | Npcap loopback 어댑터(설치 옵션, 미확인) | `lo` | `lo0` |
| 송신 raw | Npcap `pcap_sendpacket` | AF_PACKET | BPF write |
| DoIP 실HW | PC NIC + 미디어 컨버터(C4.1) | 동일 | — |
| CI | replay 백엔드만(권한 불필요) + 가능하면 loopback | replay + `lo` 캡처(컨테이너에 CAP_NET_RAW) | replay |

**HW 사실(질문으로)**: 미디어 컨버터는 **PC↔ECU 구간만** 보여 준다. ECU 간 트래픽(다른 ECU의 SOME/IP, 게이트웨이 뒤 DoIP)을 보려면 **TAP 또는 스위치 미러 포트**가 필요하다. 1000BASE-T1 TAP은 별도 장비다(모델·가격 미확인).

## 9. 시뮬레이션 & 테스트 전략

- **백엔드 `eth-sim`**(권한 불필요, 3 OS CI):
  - (1) **pcapng 재생** — 파일 시각대로 속도 조절(×1, ×N, max). `ts.source=replay`.
  - (2) **가상 탭** — `eth.send`를 같은 스트림에 attributed_origin과 함께 되돌린다.
  - (3) **SOME/IP-SD 발생기** — 서비스 offer를 주기·지터를 설정해 낸다. TC용.
- **loopback 실캡처**: UDS 가상 DoIP ECU(127.0.0.1:13400)를 실제 pcap으로 캡처한다. Linux CI 필수, macOS 개발, Windows는 Npcap loopback이 있을 때.
- **부하 시험**: 재생 max 속도(작은 패킷 1 Mpps 합성 파일)로 E2–E4 거동을 본다(`record` 건수 = 파일 건수, display 비율 배지, RSS 상한). 24 h soak 프로필에 "DoIP 폴링 + SOME/IP-SD 10 Hz + 배경 50 Mbps"를 추가한다.
- **필터 적합성**: 필터식 200개 golden(파싱·평가)으로 본다. Wireshark와의 결과 대조는 tshark가 있는 환경에서만 하는 선택 시험이다.
- **디코더 fuzz**: DoIP 길이 필드·SOME/IP 길이·TCP 구멍(cargo-fuzz)에서 panic이 없어야 한다.

## 10. 안전 / 가드레일 (F1: 기본 자동, 등급은 표시·감사)

| 동작 | 표시 등급 | 비고 |
|---|---|---|
| ifaces·capture·trace·wait-for·export | R0 | promisc도 관찰. 단 캡처 시작·정지는 감사 기록 |
| `send udp|tcp` → loopback/sim | R1 | |
| `send raw|udp|tcp` → 실 NIC | R2 | |
| 브로드캐스트·멀티캐스트 > 100 pkt/s, `--rate` 상향 | R3 | 망 폭주 위험 |
| **DoIP 진단 메시지(0x8001)를 raw로 송신** | **UDS classify로 내부 UDS 바이트를 분류한 등급**(예: 11 01 → R2, 31 … → R3) | eth로 UDS 가드 판정을 **우회하지 못하게** 등급을 정직하게 표시. 정책을 켠 사용자에게는 같은 규칙이 적용됨 |

- 레이트 상한(기본 1000 pkt/s)과 크기 상한(점보 9 KiB)은 F1에서도 유지한다(설정 가능).
- STOP(A3)은 eth 송신과 재생 job을 latch한다. **캡처는 계속한다**(관찰은 STOP 대상이 아님).
- 개인정보: 캡처에는 VIN·IP 등이 들어간다. D7 마스킹 규칙(export `--redact`)을 따른다.

## 11. 마일스톤

| M | 내용 | 수용 기준 |
|---|---|---|
| **M0** (P0) | `loom-pktfilter` AST·golden 200, `pcap-epb/1` codec 합의(eth-core), UDS 코덱 공유 경계 합의(eth-uds), 패널 명세 v0 | golden green, 협의 파일 3개 서명 |
| **M1** (P1, sim) | eth-sim(재생·가상 탭·SD 발생기) + loopback 캡처, CLI 전 명령, 웹 패널 `eth.packets`(목록·트리·hex·필터 바·UDS 칩·재전송/편집 송신·캡처 정지) | **Playwright E2E**: ① `capture start lo` → `uds read-did F190`(가상 ECU) → DOM 행 5개, `data-seq`·`data-request-id`가 CLI JSON과 같음, 캡처→행 p95 ≤ 200 ms(render ack) ② 사용자 편집 송신 → USR 행 + 에이전트 `others_since_last.count=1` ③ `wait-for -Y 'doip.type == 0x0004' --count 3 --within 5s` 0/3 두 경로 ④ 재생 1 Mpps에서 저널 건수 = 파일 건수, 표시 샘플링 배지, RSS ≤ 예산 ⑤ STOP 중 에이전트 send 거부·캡처 계속 ⑥ 늦게 연 두 번째 브라우저가 snapshot+live로 같은 행 ⑦ pcapng export 파일 재생 시 동일 패킷 수 |
| **M2** (P2, 실HW) | 실 NIC + 미디어 컨버터 Windows(Npcap)·Linux(setcap), 실 ECU DoIP 관찰 + UDS 링크, 스트림 따라가기·통계 패널, 필터 WASM | 두 OS 체크리스트, 실 ECU 리셋 공지 TC pass, 커널 드롭 `source.gap` 표시 |
| **M3** | tshark/sharkd 선택 dissector, extcap 브리지(Loom 스트림 → Wireshark, 읽기 전용), 색 규칙 편집 | 선택 기능 |

## 12. 작업 분해 (병렬 단위 · 담당 · 의존)

| ID | 작업 | 담당 | 의존 |
|---|---|---|---|
| H1 | `loom-pktfilter` 문법 명세 + golden 200 + 필드 표 | Opus | — |
| H2 | 필드 이름 Wireshark 대조, crate 조사(pcap·etherparse·pcapng·someip) | Sol | — |
| H3 | `eth-<if>` worker: 캡처 스레드·E1–E7 큐·`pcap_stats`→source.gap | Sol | CORE worker SDK |
| H4 | 저널 codec `pcap-epb/1` + export 스트리밍 | Sol | CORE E5 codec 레지스트리(eth-core) |
| H5 | 디코더: L2–L4·TCP 재조립·DoIP·SOME/IP(-SD) + fuzz | Sol | UDS 코덱 crate 분리(eth-uds) |
| H6 | 발행 단계: 관심 패킷 푸시다운·`eth.batch`·예산 | Opus 설계 / Sol 구현 | H1, H3 |
| H7 | `eth.query`(인덱스 조회)·`trace/last/show/stats/follow` | Sol | H4, H5 |
| H8 | CLI 출력 형식(한 줄 요약·JSON 트리)·D5 봉투 | Opus | H7 |
| H9 | `eth-link` gateway 모듈 + UDS `wire_key` | Opus | UDS 합의 |
| H10 | `eth-sim`(재생·가상 탭·SD 발생기) + loopback CI | Sol | H3 |
| H11 | 패널 명세 → `_xteam/eth-ui.md`, Playwright 시나리오 | Opus | UI 패널 SDK |
| H12 | TEST 술어(`eth.match/period/absent`) + 검증 훅 | Opus | TEST 검증기, H1 |
| H13 | `skills/loom-eth/SKILL.md`(필터 문법 요약, BPF vs -Y, 출력 상한, 증거 REF) | Opus | H8 |

## 13. 리스크

| 리스크 | 대응 |
|---|---|
| Windows Npcap 설치·권한("관리자만" 옵션)으로 데몬이 캡처 못 함 | `loom doctor` 탐지 + 안내, `E_PERMISSION` hint. 번들 불가 시 사용자 설치 전제 |
| 고속 캡처에서 커널 드롭 | 버퍼 확대·snaplen·BPF 권장, 드롭은 `source.gap`으로 정직하게, M2 실측 |
| 저널 용량 급소진 | `--ring` 회전, 보존 추정 표시, 증거 pin은 CORE E6 |
| 필터 문법이 Wireshark와 미묘하게 다름 | 의미를 문서에 정의(`!=`, 다중 값), 비지원은 명시 오류, 선택 대조 시험 |
| UDS 링크 오매칭(같은 요청 반복, 폴링) | 해시 + 5-tuple + 순서, 모호하면 `probable` 표시만 |
| SOME/IP-SD 주기 판정이 host 타임스탬프 지터에 묻힘 | 타임스탬프 출처·정밀도 표시, 허용오차 < 2×불확실성이면 inconclusive |
| macOS 개발 BPF 권한 | replay 백엔드로 개발 진행 |

## 14. 사용자에게 물어볼 질문 (기본값)

| # | 질문 | 기본값 |
|---|---|---|
| Q1 | 미디어 컨버터 모델·속도(100/1000BASE-T1), NIC 모델 | 100BASE-T1, 일반 USB/PCIe NIC |
| Q2 | ECU 간 트래픽(게이트웨이 뒤, 다른 ECU SOME/IP)을 볼 필요가 있나? TAP·미러 포트가 있나? | 없음 → PC↔ECU 구간만 |
| Q3 | Windows에 Npcap이 이미 설치돼 있나("관리자만 캡처" 옵션 여부)? Linux에서 `setcap` 허용? | 설치·허용 가정, 없으면 안내 |
| Q4 | VLAN을 쓰나(DoIP VLAN ID)? | 미사용 |
| Q5 | SOME/IP를 쓰나? 서비스 ID·FIBEX/ARXML이 있나? | 헤더만 디코드, 페이로드 hex |
| Q6 | Wireshark를 함께 쓰고 싶나(extcap으로 Loom 스트림 보기)? | M3 선택 |

## 15. 공통 계약 변경 요청 / 타팀 의존성

| ID | 대상 | 요청 |
|---|---|---|
| ETH-C1 | CORE | 저널 codec 레지스트리에 `pcap-epb/1` 등록(레코드 = EPB 바이트), export 훅. worker kind `eth` PerChannel ≤ 4 (`xteam-draft-eth-core.md`) |
| ETH-C2 | CORE | `eth.query`를 worker로 라우팅하는 질의 경로(인덱스 스캔, 상한), `eth.batch` 커버리지 이벤트를 mapping 정본에 포함 |
| ETH-C3 | CORE | `source.gap` cause 추가: `kernel_drop`, `if_drop`, `capture_restart`. `ts.source` 값 `pcap_host`·`pcap_adapter`·`replay` |
| ETH-C4 | CORE | §9.2 큐 표에 E1–E7, 프로세스 예산에 eth worker |
| ETH-U1 | UDS | DoIP·UDS 코덱 no-IO crate 공유, `uds.tx/rx` `wire_key`, (M2 선택) `tx_state: nic_observed` (`xteam-draft-eth-uds.md`) |
| ETH-UI1 | UI | 패널 `eth.packets`·`eth.stream`·`eth.stats` (`xteam-draft-eth-ui.md`), 필터 WASM 모듈 탑재(M2) |
| ETH-T1 | TEST | 증거 REF `eth:<if>@seq:a..b`, 술어 `eth.match/period/absent`, 관측 증거에서 attributed_origin 제외 (`xteam-draft-test-domains-eth.md`) |
