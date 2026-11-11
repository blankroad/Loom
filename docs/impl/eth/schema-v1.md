# eth 스키마 v1 (P1 M1 정본, explore-lead 2026-11-11)

정본 Rust 타입은 `crates/loom-eth/src/model.rs`이다. 이 문서는 그 요약이고, UI 질문(`docs/impl/requests/ui-eth.md` 1–10)에 대한 답이다.

## 원칙
- **64비트 정수(seq·카운터·ns·바이트 수)는 JSON에서 10진 문자열**이다(계약 v1 §6.1 `U64`). 예: `"total":"9"`. (v1.1, 2026-11-11)
- 패킷 레코드 = 이벤트 `eth.pkt`. 하나의 패킷은 저널·CLI·UI가 같은 seq로 본다. 에이전트만 보는 패킷은 없다.
- **REF는 payload에 넣지 않는다.** worker는 stream seq를 모른다. REF `eth:<ch>@seq:<stream seq>`는 레코드의 seq로 gateway·CLI·UI가 만든다.
- replay 프레임의 `bytes`에는 `comments`(사용자 pcapng 주석 원문)와 `loom_comment`(원래 X10 문자열)가 함께 올 수 있다. export는 둘 다 EPB 주석으로 되돌려 쓴다. (v1.2)
- M1 원시 바이트: `payload.bytes = {encoding:"base64", data}`(CORE `BytesPayload`). inline 상한 `MAX_INLINE_FRAME = 16 KiB`(점보 9 KiB 포함). 넘으면 앞 16 KiB + `bytes_truncated: true`(M1에는 실프레임이 이보다 작다). M2에 CORE raw store가 오면 `raw_ref`로 바꾸고, 저널 codec `pcap-epb/1`은 CORE 요청으로 유지한다.

## 이벤트 kind
| kind | payload 주요 필드 |
|---|---|
| `eth.pkt` | `ch, linktype, len(원래 길이), caplen, dir(rx\|tx\|null), attributed_origin?(agent\|user\|script), ts_src(pcap_host\|pcap_adapter\|replay\|sim), proto, src, dst, info, stream?(tcp.stream/udp 대화 번호), layers(계층별 핵심 필드, ≤ 4 KiB), link?{domain, request_id, role, confidence, target?}, pdu_first_src_seq?, bytes, bytes_truncated?` |
| `eth.batch` | `ch, covered_first_src_seq, covered_last_src_seq, total, shown, unrecorded, by_proto{proto: count}` — 100 ms마다, 개별 발행이 생략된 구간의 커버리지. `unrecorded` = 원본 바이트가 어디에도 저장되지 않은 수(M1은 worker 저널이 없어 `total - shown`) |
| `eth.tx` | 송신 행(E8): `ch, write_state, kind(raw\|udp\|tcp), to?, len, hex_preview(≤ 64 B), observed_src_seq?`(캡처에서 다시 본 복사본) |
| `eth.link` | `eth_ref, domain, request_id, role(request\|ack\|nack\|response\|tx), confidence(exact\|probable), target?` — gateway가 나중에 연결한 링크 |
| `eth.capture_state` | `ch, state(starting\|live\|stopped\|error\|permission_denied\|replay_done), backend(live\|replay\|sim), iface, bpf?, snaplen, promisc, buffer_bytes_requested, buffer_bytes_actual?, ts_precision(micro\|nano), drop_stats(available\|partial\|unavailable), linktype, started_mono_ns, error?` |
| `eth.stats` | 1 s: `ch, pps, bps, kernel_drop?, if_drop?, decode_lag_ms, shown, total` |
| 손실 | CORE `source.gap`(cause `kernel_drop\|if_drop\|stats_unavailable\|capture_restart`), `record.gap`, `display.gap` |

## RPC
| method | params | result |
|---|---|---|
| `eth.ifaces` | `{}` | `{ifaces:[{name, alias?, desc?, mac?, ips[], up, loopback, can_capture, reason?}]}` |
| `eth.capture.start` | `{iface, ch?, backend:"live"\|"replay"\|"sim", bpf?, snaplen=65535, promisc=false, buffer_mib=64, replay?{path, speed:"max"\|factor, repeat}}` | `{ch, state}` |
| `eth.capture.stop` / `eth.capture.status` | `{ch}` / `{ch?}` | `{ch, state, …capture_state}` |
| `eth.query` | `{ch, range{from?, to?}, filter?, max(기본 100, ≤ 1000 — D5 하드 상한), detail=false}` | `{rows[], range{from,to}, truncated, gaps[], next_cursor}`. `from`이 없으면 "to까지 최근 max행". `gaps[] = {kind, cause, count\|"unknown", first_seq?, last_seq?, after_seq}` — UI가 틈 행으로 그린다 |
| `eth.show` | `{ref}` | `{ref, len, caplen, linktype, bytes{encoding,data}, truncated, tree:[{name, label, off, len, children}]}`. **`off`는 저장된 캡처 바이트의 0번부터**(NULL/LOOP면 4바이트 family 헤더가 0–3) |
| `eth.wait_for` | `{ch, filter, count{min, max?}, within_ms?, anchor?(REF), timeout_ms}` | `{matched, count, window, evidence[]}` / exit 3·8 |
| `eth.stats` | `{ch, by:"proto"\|"conv"\|"endpoint"\|"io", filter?, range?}` | 상위 N 표 |
| `eth.follow` | `{ch, proto:"tcp"\|"udp", stream, max_bytes ≤ 64 KiB}` | `{segments[{dir, off, data}], truncated, gaps[]}` |
| `eth.send` | raw `{ch, kind:"raw", hex}` / socket `{ch, kind:"udp"\|"tcp", to:"IP:PORT", from_port?(udp만), hex, keep_open?(tcp만, 자기 연결 재사용 ≤ 4·유휴 30 s)}` + `dry_run?` | 실행: `eth.tx` 행. **dry-run: `{proto, info, len, risk}`(서버가 디코드 미리보기를 준다, 행·감사 없음)** |
| `eth.export` | `{ch, format:"pcapng", filter?, range?}` | job. 진행은 CORE `core.job_progress`, 완료 결과 `{path, packets, bytes}`(M1 로컬 경로, CORE raw HTTP 경로가 생기면 `raw_url` 추가) |
| `eth.filter.check` | `{expr}` | `{ok, error?{pos, expected, msg}, fields_used[]}` |

## UI 질문 답 요약 (ui-eth.md 번호)
1. **eth.pkt**: 위 표와 같다. `src/dst/proto/info`는 항상 채운다(L2만이면 MAC). `tx_request_id`는 없앴다(5번 참고).
2. **eth.show**: 노드 이름은 `name`(필드 id, 예 `doip.type`)과 `label`(표시 문자열)이다. `off`·`len`은 저장된 캡처 바이트의 0번 기준이다.
3. **eth.query**: "to까지 최근 max행"이 맞다. 결과에 `range{from,to}`·`truncated`가 있고, gap은 `gaps[]` 배열로 따로 준다.
4. **eth.batch**: UI 요청 모양 `{covered_first_src_seq, covered_last_src_seq, total, shown}` + `by_proto`로 확정. 이름에 `_src_`가 붙은 이유: 아직 stream seq로 바꾸지 않은 worker 쪽 source seq이기 때문이다. 허브 mapping으로 stream seq가 필요하면 CORE에 요청한다.
5. **송신 링크**: 하나로 정한다 — **`eth.pkt.payload.link{domain:"eth"|"uds", request_id, role, …}`**(발행 시점에 알면) + 나중에 알게 된 링크는 **`eth.link` 이벤트**. `tx_request_id`와 `eth.tx.nic_observed`는 쓰지 않는다. `eth.tx.observed_src_seq`는 송신 행에서 캡처 복사본으로 가는 역링크다.
6. **eth.send**: 인자 그대로 받는다. dry-run 미리보기를 제공한다.
7. **capture.start / ifaces**: 위 표와 같다(`iface` + `backend` 추가).
8. **export**: job, `core.job_progress`, 완료 `{path}`.
9. **capture_state**: 위 표와 같다. 시간 기준점은 `started_mono_ns`다.
10. **eth.link**: UDS가 대상 이름을 주면 `target`을 채운다.
- **REF `@req:` 확장**: `<domain>:<channel>@req:<request_id>`를 **이동(reveal) 전용 REF**로 받아들인다. 테스트 판정 증거는 여전히 seq 범위 REF만 인정한다(증거는 stream 좌표가 필요). TEST 계획 §3.1에 이 구분을 추가했다.
