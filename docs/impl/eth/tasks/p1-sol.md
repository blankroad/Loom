# ETH P1 — explore-sol: `loom-eth` worker (캡처 파이프라인)

먼저 `p1-common.md`, `p0-common.md`를 읽는다. 다음도 읽는다:
- `docs/plan/eth/plan.md` §3.2–3.3·§3.6·§9·§10
- `docs/impl/eth/schema-v1.md`
- `crates/loom-echo/src/worker.rs`
- `crates/loom-worker-sdk`(Worker·Emitter·EventDraft·MemorySink·SimClock·LatchFlag·RateGuard)
- 계약 `docs/contract/05-worker-protocol.md`·`02-events.md`(gap kind)

배정 범위: `crates/loom-eth/src/worker/`만. 리드 소유 `src/lib.rs`·`src/model.rs`는 고치지 않는다. gateway·cli는 explore-opus 담당이다.

## 만들 것: `EthWorker: Worker` (채널 1개 = worker 인스턴스 1개)
- **on_call**:
  - `eth.ifaces`: feature `live`면 pcap 장치 목록 + `can_capture` 판정(권한 오류 이유), 아니면 `sim`/`replay` 가상 항목.
  - `eth.capture.start`(`CaptureStartParams`): backend `live`(feature) / `replay` / `sim`.
  - `eth.capture.stop`, `eth.capture.status`.
- **on_write `eth.send`**(`SendParams`):
  - `raw`: `CaptureSource::send`. replay·sim에서는 **가상 탭**(보낸 프레임이 같은 스트림에 다시 나타남).
  - `udp`: OS UDP 소켓(loopback이면 이 Mac에서도 권한 없이 됨).
  - `tcp`: M1은 `E_NOT_IMPLEMENTED`로 해도 되나, 가능하면 자기 연결만 하고 상한을 둔다.
  - STOP latch 중이면 agent·script 송신을 거부한다(echo와 동일).
  - `eth.tx` 행 `write_state`는 E8 규칙을 따른다(`sent` = 드라이버에 넘김).
  - 레이트 상한 기본 1000 pkt/s(`RateGuard`), 크기 상한 9 KiB.
- **파이프라인 스레드**(plan §3.2, C2 상한을 코드 상수로):
  1. `CaptureLoop`(loom-eth-capture): source_seq, 경계 큐 E2
  2. 디코드 스레드: `loom-eth-decode` `StreamTable`로 요약·layers를 만든다(`MAX_LAYERS_JSON`). 뒤처지면 `decode_lag`를 표시한다.
  3. 발행 단계: **E4 예산** 기본 5k pkt/s(설정 ≤ 20k).
     - 예산 안이면 `eth.pkt`를 개별 발행한다(`Emitter::rx`, 실패는 세기만 — 막지 않음).
     - 넘으면 개별 발행을 생략하고 100 ms마다 **`eth.batch`{covered_first_src_seq, covered_last_src_seq, total, shown, by_proto}**를 낸다.
     - 관심 필터 푸시다운(`eth.interest.set` on_call, 필터식 목록 ≤ 16)이 있으면 예산을 넘어도 그 필터에 맞는 패킷은 개별 발행한다.
  4. 1 s `eth.stats`, 상태 변화 시 `eth.capture_state`.
  5. 손실: capture `GapReport` → CORE `source.gap`(cause kernel_drop·if_drop·stats_unavailable·capture_restart)·`record.gap`, 발행 생략은 batch로 커버. Emitter 가득 참은 `display.gap`.
- **원본 기록**: CORE `loom-journal` writer가 아직 없다. M1은 `eth.pkt.bytes`(base64, ≤ `MAX_INLINE_FRAME`)가 원본을 겸한다. **개별 발행이 생략된 패킷은 원본이 어디에도 안 남는다** → 이 공백을 보고서에 명확히 적는다. CORE 요청: worker 저널 writer / `pcap-epb/1` codec. 생략분을 숨기지 말 것: batch의 `total - shown`이 곧 기록 안 된 수라는 점을 `eth.batch`에 `unrecorded: true` 같은 필드로 표시할지 리드에게 제안한다(model 변경은 리드).
- **송신 귀속**:
  - 최근 송신 표: 프레임 해시 → `request_id`·origin, 상한 256, TTL 2 s.
  - 캡처에서 같은 프레임을 보면 `attributed_origin` + `link{domain:"eth", request_id, role:"tx", confidence:"exact"}`를 붙인다.
  - `eth.tx` 행 갱신 `observed_src_seq`.
- **sim 백엔드 발생기**(권한 없는 M1·CI용, plan §9):
  - SOME/IP-SD OfferService 주기(기본 1 s, 지터 설정)
  - DoIP vehicle announcement 3회(시작 시, 그리고 `eth.sim.reset` on_call 시 — TC 예 "리셋 후 5 s 안 공지 3회"용)
  - 받은 raw DoIP 라우팅 요청에 응답(최소)
  - 모든 sim 프레임은 `ts_src=sim`.
  - UDS 가상 ECU와의 연결은 나중에 SimWorld로 한다(지금은 하지 않음).
- `Health`용 지표: 큐 사용량, 디코드 지연, 발행 비율.

## 시험 (worker-sdk `MemorySink`/`SimClock`, 실 NIC 없이)
- replay(샘플 pcapng, `spikes/eth/r1-sol/` 또는 P0 fixture) → `eth.pkt` 수 = 파일 수, src seq 연속, 요약 필드 채움
- 예산 초과 합성 부하 → batch `total`·`shown` 정확, 관심 필터 패킷은 개별 발행
- sim: 공지 3회·SD 주기
- raw send on sim → 캡처 복사본 `attributed_origin=agent` + link
- STOP latch 중 agent send 거부, user send 허용
- 정지 지연 < 200 ms
- gap 매핑(stats unavailable → `source.gap` cause)

## 보고
`docs/impl/eth/reports/p1-sol.md`(60줄 이하, CORE 요청·model 변경 제안 포함). 커밋하지 않는다.
