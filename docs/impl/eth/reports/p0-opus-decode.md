# P0 보고 — explore-opus: `crates/loom-eth-decode` (2026-11-11)

**완료.** worktree `.worktrees/eth`, 커밋 안 함. 관문 3개 green: `cargo fmt --check` · `cargo clippy --all-targets -D warnings` · `cargo test`(21개: decode 11, limits 6, 무작위 3, doc 1).

## 무엇을 만들었나
입출력 없는 패킷 디코더. 패킷 1개 → tshark식 요약(`Summary{proto, src, dst, len, info}`) + 계층 트리(`Vec<Layer>`, 필드는 frame/재조립 버퍼 위치 `Pos{buf, off, len}` 포함 → hex 하이라이트용) + 필터 값(`Decoded::field_values`).
- linktype: Ethernet(1), NULL(0)·LOOP(108), RAW(101)·IPv4(228)·IPv6(229), Linux SLL(113). 모르는 linktype = `frame` 층 + `unknown_linktype` 경고.
- L2–L4(etherparse 0.21.0 헤더 슬라이스): 802.1Q/QinQ(≤4), ARP, IPv4(fragment는 표시·경고만), IPv6(+확장 헤더), ICMP/ICMPv6, UDP, TCP(flags·상대 seq/ack·analysis 3종).
- TCP `StreamTable`: 양방향 키(VLAN+두 끝점), 세대(RST/양쪽 FIN 뒤 SYN, 다른 ISN → 새 `tcp.stream`), 방향별 순서 재조립(out-of-order 보관, 재전송 무시, overlap은 먼저 온 바이트 우선, wrap 비교, 중간 시작), 구멍 = `tcp_gap`, DoIP(13400)·SOME/IP(30490) 길이로 0·1·다수 PDU 절단, 여러 세그먼트 PDU는 마지막 패킷에 붙이고 `loom.pdu_first_seq` 기록.
- DoIP(`doip.rs`, 임시 — UDS no-IO 코덱으로 교체 주석): 0x0000–0x0008, 0x4001–0x4004, 0x8001–0x8003. UDS(`uds.rs`, 임시): SID 이름, ±응답, NRC 이름, 22/62/2E DID, 31 RID, 59 02 DTC 개수.
- SOME/IP·SD(`someip.rs`): **직접 구현**. `someip_parse` 0.7.2 평가 결과: SD iterator에 `unsafe`, `SdHeader`가 상한 없는 owned Vec, 바이트 위치 미제공 → 비채택(교차 비교 후보로만).
- 필드 레지스트리 `FIELDS`(163개, Wireshark 이름 + 별칭: `doip.payload_type`, `doip.futher_action`, `someip.service_id` 등, union: `ip.addr`·`tcp.port`·`eth.addr` …). JSON: 트리(`serde_json::to_value(&decoded)`)와 plan §4 compact(`CompactLayers(&d.layers)` → `{"doip":{"type":"0x8001"}}`).

## 공개 API 요약
`decode(linktype, &[u8], &FrameMeta, &mut StreamTable) -> Decoded` · `FrameMeta{seq, ts_ns, dir, attributed_origin, request_id, orig_len}`(orig_len 추가) · `Decoded::{field_values, has_protocol, bytes_at, layer_bytes, frame}` · `FIELDS`, `field_def(name)`(별칭 해석), `protocols()` · `FieldType{Protocol,U8..U64,Bool,Bytes,Str,Ipv4,Ipv6,Mac,TimeRel}` · `FieldValue` · `CompactLayers` · `linktype::*`, `ports::*`, `limits::*`.
- `frame.time_relative`·`frame.time_delta`는 `TimeRel` 타입으로 등록만 하고 값은 필터가 `meta.ts_ns`로 계산한다.

## 상한 (C2)
| 상수 | 값 | 넘칠 때 |
|---|---|---|
| `MAX_FRAME_BYTES` | 256 KiB | 앞부분만 디코드, `frame_too_large` |
| `MAX_LAYERS` / `MAX_FIELDS` / `MAX_WARNINGS` / `MAX_BUFFERS` | 64 / 4096 / 16 / 64 | 이후 버림 + `limit_*` 경고 |
| `MAX_STREAMS` | 1024 | idle 먼저 축출, 없으면 LRU 축출 + `stream_evicted` |
| `MAX_STREAM_BUFFERED_BYTES` / `MAX_TOTAL_BUFFERED_BYTES` | 4 MiB(양방향 합) / 64 MiB | 그 방향 앱 디코드 중단 `reassembly_truncated`(원본 패킷은 그대로) |
| `MAX_PENDING_SEGMENTS` / `MAX_OOO_DISTANCE` | 256 / 4 MiB | 구멍 포기 `tcp_gap`, 다음 헤더로 재동기 또는 `app_desync` |
| `MAX_PDU_BYTES` / `MAX_PDUS_PER_PACKET` | 1 MiB / 64 | `reassembly_truncated` / 나머지는 다음 패킷에 |
| `STREAM_IDLE_NS` / `SWEEP_INTERVAL_NS` | 120 s / 1 s (패킷 ts 기준) | idle 축출 |
| SD 엔트리·옵션 / 데이터그램당 메시지 / UDS DID / VLAN 태그 | 64·64 / DoIP 16·SOME/IP 64 / 64 / 4 | 경고 후 생략 |

## 테스트
fixture는 모두 Rust(etherparse PacketBuilder + 수작업 바이트): 공지, 라우팅 요청·응답, 22 F1 90 → 62 + VIN, NRC 7F 22 31, VLAN SD offer, ARP, ICMP echo, IPv6 UDP, TCP 3경우(3분할 / 한 세그먼트 PDU 2개 / 순서 뒤바뀜+재전송+overlap), RST 뒤 새 stream 번호, NULL·RAW·IPv4·SLL·미지 linktype, JSON 두 모양, 레지스트리 일관성(중복·별칭·union·프로토콜 존재, 방출 필드 전부 등록 확인). 상한: 1025번째 스트림 LRU 축출, idle 축출, PDU 1 MiB 초과, out-of-order 4 MiB 초과, SD 65 엔트리, 256 KiB 초과 프레임. 무작위: 랜덤 바이트×8 linktype 10만, 정상 fixture 비트 변조 10만, DoIP 포트 랜덤 TCP 2만 — panic 0, 매 회 상한 단언.

## 미해결 / 요청
- **Wireshark 대조 필요**(`tshark -G fields`, 이 Mac에 tshark 없음): `doip.vin_gid_sync`, `doip.header_nack_code`, `doip.nack_code`, `doip.node_type`/`max_sockets`/`current_sockets`/`max_data_size`/`power_mode`, `uds.reply`, `uds.err.sid`, `uds.rid`, `uds.dtc_count`, `someipsd.option.*`, `someip.protoversion`/`interfaceversion`, `sll.etype`, `vlan.dei`. 별칭으로 흡수 가능.
- IPv4/IPv6 fragment 재조립은 P1(etherparse에 있음). DoIP over TLS(3496)는 디코드 안 함. UDP SOME/IP는 30490 + 엄격 휴리스틱(포트 ≥ 1024, 메시지가 데이터그램을 정확히 채움, proto ver 1, 알려진 type).
- 같은 PDU의 첫 패킷이 out-of-order로 먼저 보관된 경우 `pdu_first_seq`는 그 보관 세그먼트의 seq다(정확). 단 gap 뒤 재동기 시 이전 부분은 버린다.
- **CORE workspace.dependencies 추가 요청**: `etherparse = "=0.21.0"`(새 외부 crate). serde 1.0.228·serde_json 1.0.149(dev)는 CORE와 같은 판으로 lock 고정.
