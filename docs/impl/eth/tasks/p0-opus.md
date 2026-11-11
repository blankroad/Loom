# ETH P0 — explore-opus: `loom-eth-decode` → `loom-eth-filter`

먼저 `docs/impl/eth/tasks/p0-common.md`(필수 규칙)와 `docs/plan/eth/plan.md` §3.5–3.6·§4·§4.1·§6, `docs/plan/eth/r1-sol.md` §4–5를 읽는다. 참고 코드: `spikes/eth/r1-sol/src/main.rs`(DoIP·SOME/IP-SD 최소 파서와 샘플 프레임 생성).

배정 범위: worktree의 `crates/loom-eth-decode/`, `crates/loom-eth-filter/`만 수정한다. `loom-eth-pcapng`·`loom-eth-capture`는 explore-sol 담당이다(읽기만).

## 1. `crates/loom-eth-decode` (먼저)
패킷 한 개를 받아 tshark식 요약 + 계층 트리 + 필터용 필드를 만든다. 입출력이 없는 순수 라이브러리다.

### API (이름은 바꿔도 되나 기능 유지)
- `decode(linktype: u16, frame: &[u8], meta: &FrameMeta, streams: &mut StreamTable) -> Decoded`.
  - `FrameMeta{ seq: u64, ts_ns: u64, dir: Option<Dir>, attributed_origin: Option<Origin>, request_id: Option<String> }`
- `Decoded{ summary: Summary{proto, src, dst, len, info}, layers: Vec<Layer>, warnings: Vec<DecodeWarning> }`.
  - `Layer` = 이름 + 순서 있는 필드 목록(트리 출력, 하위 노드 허용). `serde::Serialize`로 plan §4 `--json`의 `layers` 모양을 만든다.
- **필드 레지스트리**: `FIELDS: &[FieldDef{name, ty, desc, aliases}]`.
  - ty: `U8..U64, Bool, Bytes, Str, Ipv4, Ipv6, Mac`.
  - 필드 이름은 plan §4.1 표의 **Wireshark 정본 + 별칭**으로 한다(SOME/IP는 `someip.serviceid` 등, DoIP에 `doip.further_action` + 별칭 `doip.futher_action`). Loom 필드 `loom.origin`, `loom.dir`, `loom.request_id`, `loom.seq`, 그리고 `frame.len`, `frame.time_relative`(필터 쪽 계산)도 둔다.
  - 프로토콜 존재 필드(`eth`, `ip`, `tcp`, `doip`, `uds`, `someip`, `someipsd` …)를 둔다.
- `Decoded::field_values(name) -> impl Iterator<Item=FieldValue>`(다중 값). 필터 crate가 이것만 쓴다.

### 디코드 범위
- **linktype**: Ethernet(1), NULL(0)·LOOP(108)(macOS lo0·Npcap loopback), RAW IPv4/IPv6(101, 228, 229), Linux SLL(113). 모르는 linktype은 `frame`만 두고 경고를 단다.
- **L2–L4**: `etherparse` 0.21.0 사용. 802.1Q·QinQ, ARP, IPv4(fragment는 표시만, 재조립은 P1), IPv6, ICMP/ICMPv6, UDP, TCP(flags·seq·ack·len).
- **TCP 스트림·재조립** (`StreamTable`, plan X8·E5):
  - 스트림 번호(`tcp.stream`)는 양방향 5-tuple + 세대로 매긴다.
  - 방향별 순서 재조립: out-of-order 버퍼링, 재전송·중복 제거, overlap은 먼저 온 데이터 우선, seq wrap, SYN/FIN/RST, 중간 시작(첫 seq 기준).
  - 구멍은 `gap`으로 표시한다.
  - 상한: 스트림당 4 MiB, 동시 1024 스트림, 유휴 120 s(ts 기준 eviction), 스트림당 대기 세그먼트 수. 넘치면 `reassembly_truncated` 경고(원본은 유지 — 디코드만 생략).
  - 재조립된 바이트에서 **DoIP PDU를 8바이트 헤더 length로 잘라** 0·1·다수 PDU를 낸다. 한 PDU가 여러 세그먼트에 걸친 경우 마지막 세그먼트 패킷에 PDU를 붙이고, 첫 세그먼트 seq를 `pdu_first_seq`로 기록한다.
- **DoIP**(TCP/UDP 13400 + 판별):
  - 대상: 헤더(version·inverse 검사), 공지 0x0004, 라우팅 0x0005/0x0006, alive check 0x0007/0x0008, 진단 0x8001/0x8002/0x8003, entity·power status 0x4001–0x4004.
  - `doip` 모듈 하나로 격리하고 상단 주석에 "UDS 팀 no-IO 코덱 crate가 나오면 교체(_xteam/eth-uds.md)"라고 쓴다.
- **UDS**(DoIP 0x8001 페이로드):
  - 범위: SID 이름표, positive(SID+0x40)/negative(0x7F, NRC 이름), 22/62의 DID, 19 02 DTC 개수 정도의 최소 해석.
  - 이것도 임시 모듈로 격리한다(UDS 공유 코덱으로 교체 예정).
- **SOME/IP·SOME/IP-SD**(UDP·TCP 30490 및 헤더 판별): `someip_parse` 0.7.2를 평가하고, 맞지 않으면 직접 구현한다. SD 엔트리(type·serviceid·instance·ttl)와 IPv4/IPv6 endpoint 옵션을 다룬다.
- **요약 Info 문자열**: plan §4 예시처럼 tshark 열 의미를 따른다. 예: `Vehicle announcement (0x0004) VIN=… LA=0x1001`, `Diag 0x0E80→0x1001 22 F1 90 ReadDataByIdentifier`, `SOME/IP-SD OfferService 0x1234.1 ttl=3`, TCP는 `50112 → 13400 [SYN] Seq=0 Len=0`.

### 테스트
- 바이트 fixture를 Rust 코드로 만든다(etherparse builder 사용 가능):
  - 공지, 라우팅 요청·응답, 22 F1 90 → 62 F1 90 + VIN, NRC 7F 22 31, VLAN SOME/IP-SD offer, ARP, ICMP echo, IPv6 UDP
  - TCP 세 경우: DoIP PDU 하나가 3세그먼트로 쪼개짐 / 한 세그먼트에 PDU 2개 / 순서 뒤바뀜 + 재전송
  - loopback NULL linktype
- 상한 시험: 1025번째 스트림, 4 MiB 초과, idle eviction.
- 무작위 바이트 ≥ 10만 회 panic 없음.

## 2. `crates/loom-eth-filter` (decode 끝난 뒤)
Wireshark 표시 필터 부분집합 `loom-pktfilter`를 구현한다. plan §4.1이 문법 정본이다.
- **구조**: 파서(직접 재귀 하강, 외부 파서 crate 없이 시작해도 됨) → AST → 타입 검사(decode의 필드 레지스트리 기준, 모르는 필드·타입 불일치 = 위치 있는 오류) → 평가기(`Decoded::field_values`).
- **연산자**:
  - 비교: `== != < <= > >=`와 `eq ne lt le gt ge`
  - `contains`(바이트·문자열), `matches`(`regex` crate)
  - `in {a, b, 10..20}`
  - 논리 `and or not && || !`, 괄호
  - 슬라이스 `eth.src[0:3]`, `frame[12:2]`
  - 리터럴: 10진·0x 정수, MAC, IPv4/IPv6, CIDR(`ipnet`), 문자열, 바이트열 `aa:bb:cc`
- **의미**:
  - 다중 값 필드의 `==`는 "어느 하나"다. `a != b`는 `not (a == b)`로 정의한다.
  - 프로토콜 이름만 쓰면 존재 검사다.
  - 비지원 문법(함수, 산술, 필드끼리 비교, `#` 레이어 연산자)은 위치와 함께 명시적 오류를 낸다.
- **API**: `Filter::compile(&str) -> Result<Filter, FilterError{pos, expected, msg}>`, `Filter::matches(&Decoded) -> bool`, `Filter::fields_used()`(푸시다운용).
- **상한(C2)**: 식 길이 4 KiB, AST 깊이 64, 노드 1024, `in` 원소 256, 정규식 크기 상한(`RegexBuilder::size_limit`).
- **테스트**: golden ≥ 200개(파싱 성공·실패 위치·평가 결과)를 표 형식 상수 배열로 둔다. 무작위 식 문자열 10만 회 panic 없음.

## 보고
crate마다 `docs/impl/eth/reports/p0-opus-decode.md`, `p0-opus-filter.md`를 쓴다. decode가 끝나면 보고서를 쓰고 바로 filter로 넘어간다. 리드는 decode 보고서를 보는 대로 커밋한다.
