# ETH ↔ CORE: eth worker · 저널 codec · 질의 경로 (초안 v1, explore-lead 통합 2026-11-10)

docs/plan/02-core-principle.md 준수. 정본: `docs/plan/eth/plan.md` §3–§6. CORE는 `## core 의견`을 덧붙여 주세요.

## 1. worker 배치
- `WorkerSpec{kind:"eth", instances: PerChannel{max:4}, reopen_after_crash: None}`.
  - 근거: W1(wpcap.dll/libpcap 로드), W4(고속 버퍼).
  - 크래시 뒤 캡처는 자동으로 다시 열지 않는다. 사람이 다시 시작하고, 그 공백은 `source.gap{cause:"capture_restart"}`로 남긴다.
- 메모리 예산(추정): soft 256 MiB / hard 384 MiB.
- 스레드: 캡처 1 + 디코드 1–2 + 저널 writer 1 + SDK IPC 1.

## 2. 큐 (CORE §9.2에 추가 요청)

| # | 큐 | 상한 | 넘칠 때 |
|---|---|---|---|
| E1 | 커널 버퍼 | 요청 64 MiB(실제값 manifest) | `source.gap{kernel_drop|if_drop}`(pcap_stats 100 ms 샘플) |
| E2 | 원시 링 | 128 MiB / 512k pkt | 저널 미기록분 축출 → `record.gap` + health failed(A12) |
| E3 | 화면 미러 | 4 MiB / 8192 | `display.gap` |
| E4 | 발행 예산 | 5k pkt/s(설정 ≤ 20k) | 관심 패킷만 + `eth.batch` |
| E5 | TCP 재조립 | 4 MiB/스트림, 1024 스트림 | `reassembly_truncated` |
| E6 | 인덱스 메모리 | 32 MiB | sparse 인덱스로 |
| E7 | 송신 큐 | 64건 / 1 MiB, 1000 pkt/s | exit 4 `E_RATE_LIMIT` |

## 3. 저널 codec `pcap-epb/1` (E5 확장)
- 세그먼트 헤더의 codec id = `pcap-epb/1`. 레코드 = `u32 len | u32 crc32c | u8 rec_type | EPB 바이트`(E5 프레이밍 그대로).
- **EPB 옵션으로 넣을 Loom 메타**:
  - `source_seq`
  - `dir`
  - `request_id`(송신 링크)
  - `ts.mono_ns`
  
  표기(pcapng custom option PEN vs `opt_comment`)는 Sol이 확인한다(미확인).
- 세그먼트 헤더에 IDB 정보를 넣는다: linktype·snaplen·`if_tsresol`·인터페이스 이름.
- **export**: `record export --format pcapng` = 세그먼트를 순서대로 읽으며 프레임을 벗기고, 앞에 SHB + IDB를 붙인다. 스트리밍 복사이고 디코드는 하지 않는다. 필터 export는 `loom-pktfilter`로 EPB를 고른다.
- 보존·GC·pin·tail 복구·`record verify`는 CORE 것을 그대로 쓴다. 증거로 인용된 세그먼트 pin(E6)도 같다.
- 요약 이벤트 `eth.pkt`·`eth.batch`는 **저널에 따로 쓰지 않는다**. 같은 `source_seq`의 파생이므로 mapping 로그로 연결된다. CORE 의견 요청: 파생 이벤트를 mapping 정본에 넣는 방식이 맞는가?

## 4. 발행과 커버리지
- 개별 발행은 예산(E4) 안에서만 한다. 넘으면 관심 패킷(활성 구독 필터 ∪ 대기 중 `wait-for` 필터)만 개별로 보내고, 나머지는 100 ms마다 `eth.batch{first_src, last_src, count, by_proto}` 1건으로 보낸다.
- 이렇게 하면 **모든 원본 패킷이 어떤 stream seq에 덮인다**. TEST 증거 범위 `eth:<if>@seq:a..b`가 개별 발행되지 않은 패킷까지 가리킬 수 있다.
- 구독 필터 푸시다운: `core.subscribe{filter{domains:["eth"], channels, kinds, ext:{eth_filter:"<표시 필터식>"}}}` → gateway가 worker에 필터 AST를 전달한다. CORE 의견 요청: `filter.ext` 도메인 확장 슬롯을 둘 수 있는가?

## 5. 질의 경로
- `eth.query{channel, range, filter, max, detail}` → gateway가 worker ctl로 라우팅한다. worker가 인덱스 + 세그먼트를 디코드한다.
- 상한: CORE 질의 풀 규칙(스캔 256 MiB·2 s, 결과 64 KiB, 넘으면 `E_SCAN_LIMIT` + resume). 큰 결과는 `/api/v1/query`.
- 바이트 원문: `/api/v1/raw/<raw_ref>`, `raw_ref` = `eth:<if>/src:<producer_epoch>/<source_seq>`.

## 6. 이름·값 추가
- `source.gap` cause: `kernel_drop`, `if_drop`, `capture_restart`.
- `ts.source`: `pcap_host`, `pcap_adapter`, `replay`.
- 이벤트 kind: `eth.pkt`, `eth.batch`, `eth.tx`, `eth.link`, `eth.capture_state`, `eth.stats`.
- RPC: `eth.ifaces`, `eth.capture.start/stop/status`, `eth.query`, `eth.wait_for`, `eth.stats`, `eth.follow`, `eth.send`, `eth.export`, `eth.filter.check`.

## core 의견
(core-lead 작성란)

## 추가 (리드 통합, `eth/plan.md` 결정 X1–X9)
- **별도 실행 파일 `loom-eth-worker`**: Linux에서 사람이 이 파일에만 `cap_net_raw`를 준다. worker manifest에 실행 경로·필요 capability를 선언하고, `loom doctor`가 capability를 확인한다(재빌드 후 사라짐).
- **드롭 통계 가용성**: `pcap_stats`는 플랫폼에 따라 미구현 값이 0으로 나온다. `eth.capture_state.drop_stats = available|partial|unavailable`을 두고, unavailable이면 "커널 드롭: 알 수 없음"으로 표시한다(0 = 무손실 금지). `source.gap` cause 추가 요청: `kernel_drop`, `if_drop`, `capture_restart`, `stats_unavailable`.
- **오류 코드**: 새 코드 없이 D5를 재사용한다 — 7 `E_NOT_OPEN{detail: permission|not_found|npcap_missing}`, 4 `E_RATE_LIMIT`. CR-C4 목록에 `detail` 값만 등록해 달라.
- **`ts.source`** 추가: `pcap_host`, `pcap_adapter`, `replay`. 나노초 단위와 정확도는 다르므로 `eth.capture_state`에 실제 precision을 기록한다.
- **pcapng export**: pcap-file `PcapNgWriter`(스파이크 확인, 1 IDB·µs)를 쓴다. 나노초 `if_tsresol`·다중 IDB·custom option은 M0 관문이고, 안 되면 자체 블록 writer로 간다. IDB linktype(loopback NULL DLT 포함)을 보존한다.
- **처리량**: 1 Gbps 무손실은 보증하지 않는다(미측정). 20 GiB quota는 선속도로 약 172초다. `capture start --ring`과 보존 추정 표시를 CORE 보존 정책(E6)과 맞춘다.
