# UI → ETH(EXPLORE) 요청 (eth.packets 구현 중, 2026-11-11)

docs/plan/02-core-principle.md 준수. UI는 `eth.packets`를 합성 fixture 위에서 먼저 만들었다(ui/main e12a1da, 가정은 `ui/src/panels/eth/ethModel.ts` 머리 주석). 아래 payload·RPC 모양을 ETH 스키마로 확정해 주세요. 이름이 다르면 UI가 맞춘다.

1. **`eth.pkt` payload 필드**: `src/dst/proto/len/info/attributed_origin/layers/link/tx_request_id`로 가정. src/dst가 없으면 `layers.ip|ipv6|arp|eth`에서 고른다. 확정 요청.
2. **상세 `eth.query{ref, detail:true}` 결과**: `{ref, len, bytes{encoding,data} | hex, raw_url?, truncated?, tree:[{label, off, len, children}]}` 가정. 트리 노드 이름 필드(`label`/`name`)와 `{off,len}`이 프레임 기준 오프셋인지(linktype이 NULL/LOOP면 0이 어디인지) 확인.
3. **목록 `eth.query{ch, range:{to}, filter?, max ≤ 2000, detail:false}`**: "to까지 최근 max행"으로 가정. `range{from,to}`·`truncated`·gap 포함 여부(질의 결과에 `source.gap` 구간을 행으로 넣어 줄지) 확정.
4. **`eth.batch`**: UI 요청 모양 `{covered_first_seq, covered_last_seq, total, shown}`과 계획 모양 `{first_src, last_src, count, by_proto}` 둘 다 읽지만 `shown`이 없으면 비율을 정확히 못 낸다 — UI 요청 모양으로 확정 부탁.
5. **송신 ↔ 캡처 복사본 링크**: 패킷 payload `tx_request_id` / `eth.link{domain:'eth', request_id}` / `eth.tx.payload.nic_observed` 셋 다 처리한다. 하나로 정해 달라.
6. **`eth.send` 인자**: raw `{kind:'raw', ch, hex}`, socket `{kind:'udp'|'tcp', ch, to:'IP:PORT', from_port?, hex}`. **dry-run 응답 `{proto, info, risk}`(디코드 미리보기)를 서버가 주는지** — 편집 대화상자의 실시간 미리보기가 여기에 의존한다(없으면 "미리보기 없음 (서버 dry-run 필요)"로 표시).
7. **`eth.capture.start` 인자** `{iface, ch, bpf?, snaplen, promisc, buffer_mib}`, **`eth.ifaces` 결과** `{ifaces:[{name, alias?, can_capture?}]}`.
8. **`eth.export`**: `{ch, format:'pcapng', filter?, range?}` → job. 진행률 이벤트 이름(`core.job_progress`?)과 완료 시 다운로드 경로(`/api/v1/raw/…`?).
9. **`eth.capture_state`**: `state` 값 집합(LIVE·STOPPED·PERMISSION 외), `drop_stats`, `ts_precision`, `sim`, `started_mono_ns`(rel 시간 기준점). 없으면 첫 패킷 기준.
10. **`eth.link` 대상 정보**: UDS 칩 reveal에 대상 이름(`target`)이 필요. 없으면 REF가 `uds:*@req:<id>`가 되고 UDS 콘솔은 request_id만으로 찾는다(동작함).

### TEST·리드에
- reveal REF 확장 `<domain>:<channel>@req:<request_id>`(channel `*` 허용)를 TEST REF 문법에 넣을지 — 링크 칩은 seq를 모르고 request_id만 안다.


## eth 답변 (explore-lead, 2026-11-11)
전부 확정했다. 정본은 `docs/impl/eth/schema-v1.md`(요약)와 `crates/loom-eth/src/model.rs`(Rust 타입, eth/main). 요지:
1. `eth.pkt`: `ch, linktype, len, caplen, dir, attributed_origin?, ts_src, proto, src, dst, info, stream?, layers, link?, pdu_first_src_seq?, bytes, bytes_truncated?`. **`ref`는 payload에 없다** — 레코드 seq로 `eth:<ch>@seq:<seq>`를 만든다.
2. `eth.show{ref}` → `tree:[{name, label, off, len, children}]`. `off`는 저장된 캡처 바이트 0번 기준(NULL/LOOP면 family 헤더 4바이트 포함).
3. `eth.query`: "to까지 최근 max행", 결과 `{rows, range{from,to}, truncated, gaps[], next_cursor}`. gap은 `gaps[]`로 따로 준다.
4. `eth.batch`: UI 모양 확정 — `{covered_first_src_seq, covered_last_src_seq, total, shown, by_proto}`(source seq).
5. 송신 링크는 `eth.pkt.payload.link` + 나중 링크는 `eth.link` 이벤트, 두 가지만. `tx_request_id`·`nic_observed`는 없다. `eth.tx.observed_src_seq`가 역링크다.
6. `eth.send` 인자 그대로. **dry-run `{proto, info, len, risk}`를 서버가 준다**(행·감사 없음).
7. `capture.start`에 `backend: live|replay|sim`과 `replay{path, speed, repeat}` 추가. `ifaces` 항목에 `up, loopback, can_capture, reason?`.
8. `eth.export`: `core.job_progress`, 완료 `{path, packets, bytes}`(raw HTTP 경로는 CORE가 생기면 `raw_url`).
9. `capture_state.state ∈ starting|live|stopped|error|permission_denied|replay_done`, `drop_stats`, `ts_precision`, `backend`, `started_mono_ns`.
10. `eth.link.target`은 UDS가 주면 채운다.
- `@req:` REF는 **이동 전용**으로 채택(증거는 seq 범위만) — TEST 계획에 반영.

## eth 변경 알림 v1.1 (explore-lead, 2026-11-11)
1. **64비트 값은 JSON 문자열**이다(계약 v1 §6.1 `U64`): `eth.batch`의 `covered_*_src_seq, total, shown, unrecorded, by_proto` 값, `eth.stats`의 `pps, bps, kernel_drop, if_drop, decode_lag_ms, shown, total`, `eth.capture_state`의 `buffer_bytes_*, started_mono_ns`, `eth.tx.observed_src_seq`, `eth.pkt.pdu_first_src_seq`. 숫자로도 읽히게 두면 둘 다 동작한다.
2. `eth.batch.unrecorded` 추가: 원본 바이트가 저장되지 않은 패킷 수(M1은 worker 저널이 없어 `total - shown`). 표시 비율 배지와 별도로 "원본 미기록 N"을 보여 달라 — 이 구간은 증거로 쓸 수 없다.
3. `eth.query` 최대 행 2000 → **1000**(D5 하드 상한).
4. gateway 결과 추가 필드(p1-opus 보고): `eth.query`에 `prev_cursor, matched, complete, gaps_omitted`, gap `{kind: source|record|scan|unpublished, cause, count, after_seq}`; `eth.show`에 `summary, warnings`; dry-run에 `reason, dry_run`; reducer snapshot = 채널별 `eth.capture_state` + `{kind:"eth.last_packets", rows[≤200]}`.
5. 자기 송신 패킷은 레코드 `dir`이 SDK 제약으로 `rx`일 수 있다 — **`payload.dir`·`payload.attributed_origin`을 우선**해 달라(CORE에 SDK 수정 요청함).
