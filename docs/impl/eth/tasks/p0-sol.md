# ETH P0 — explore-sol: `loom-eth-pcapng` → `loom-eth-capture`

먼저 `docs/impl/eth/tasks/p0-common.md`(필수 규칙, 특히 X10)와 다음을 읽는다:
- `docs/plan/eth/plan.md` §3.2–3.4·§6·§8·§9
- `docs/plan/eth/r1-sol.md`(네가 쓴 조사)
- 참고 코드: `spikes/eth/r1-sol/`

배정 범위: worktree의 `crates/loom-eth-pcapng/`, `crates/loom-eth-capture/`만 수정한다. `loom-eth-decode`·`loom-eth-filter`는 explore-opus 담당이다(읽기만).

## 1. `crates/loom-eth-pcapng` (먼저)
pcapng 읽기·쓰기와 저널 레코드 codec `pcap-epb/1`을 만든다. 입출력은 `std::io::Read/Write`만 쓴다.
- **선택**:
  - `pcap-file` 2.0.0을 먼저 확인한다: 나노초 `if_tsresol`, 다중 IDB, EPB `opt_comment`, big-endian 읽기, 블록 크기 상한.
  - 하나라도 안 되면 **자체 블록 reader/writer**로 간다. 형식이 단순하니 직접 구현을 권장한다.
  - 판단 근거를 보고서에 쓴다.
- **쓰기 `PcapNgWriter<W: Write>`**:
  - 블록: SHB, IDB 여러 개(`linktype, snaplen, if_name, if_tsresol` — 마이크로·나노), EPB(`if_id, ts_ns, caplen, origlen, data, comments`), 필요하면 ISB(드롭 통계 `isb_ifdrop`).
  - little-endian으로 쓰고, 4바이트 정렬을 지킨다.
- **읽기 `PcapNgReader<R: Read>`**:
  - 지원: 두 endianness, 여러 section, 모르는 블록 skip, `if_tsresol` 10의 거듭제곱·2의 거듭제곱 해석, **고전 pcap(마이크로·나노 magic) 읽기도 지원**(replay 호환).
  - 상한: 블록 최대 크기(기본 1 MiB, 넘으면 오류 — 할당 전 검사), IDB 수 상한 256.
  - 잘린 파일은 오류 + 마지막 정상 블록 위치를 돌려준다.
- **X10 Loom 메타데이터**: `LoomMeta{seq, dir, req, org}` ↔ `opt_comment` `loom1 …`. 사용자 주석은 보존한다.
- **레코드 codec `pcap-epb/1`**(plan X1):
  - `encode_record(&Packet) -> Vec<u8>`: EPB 블록 바이트 하나, little-endian.
  - `decode_record(&[u8]) -> Result<Packet>`.
  - IDB 정보는 레코드 밖(캡처 세션 헤더)에 따로 두는 구조다: `SessionHeader{interfaces}`를 serde로 직렬화할 수 있게 한다.
  - **export**: `export(header, records: impl Iterator<Item=&[u8]>, w)`는 SHB+IDB를 쓰고 레코드 바이트를 그대로 붙인다(스트리밍, 재인코딩 없음).
- **테스트**:
  - 왕복: 나노·마이크로, 다중 IDB, 주석, 큰/작은 패킷, snaplen 잘림.
  - `spikes/eth/r1-sol/` 샘플 pcapng 읽기.
  - big-endian 손수 만든 fixture.
  - export 결과를 다시 읽어 패킷 수·바이트 일치.
  - 무작위 바이트 10만 회 panic 없음.
- tshark가 이 Mac에 없으므로 Wireshark 교차 확인은 "미확인"으로 보고한다.

## 2. `crates/loom-eth-capture` (pcapng 끝난 뒤)
캡처 소스와 캡처 스레드를 만든다. 아직 CORE worker SDK가 없으므로 출력은 이 crate 안의 타입으로 둔다.
- **`trait CaptureSource`**:
  - `interface() -> Interface`(linktype·snaplen·tsresol·name)
  - `next(&mut self, timeout) -> Result<Option<RawFrame{ts_ns, caplen, origlen, data}>>`
  - `stats() -> DropStats{availability: Available|Partial|Unavailable, recv, kernel_drop, if_drop}`
  - `send(&mut self, frame) -> Result<()>`(지원 안 하면 오류)
- **`ReplaySource`**: pcapng·pcap 파일. 속도는 `Realtime{factor}`(원본 시각 간격 × factor) 또는 `Max`. 반복 재생 옵션, `ts_source=replay`.
- **`SimTapSource`**(권한 없는 개발용, plan §9):
  - `push_tcp(conn, dir, payload)`, `push_udp(src, dst, payload)`로 Ethernet/IPv4/TCP·UDP 프레임을 합성한다(etherparse builder).
  - TCP는 연결별 seq/ack를 추적하고 SYN/FIN 합성 옵션을 둔다.
  - `ts_source=sim`, 내부 큐 상한을 둔다.
- **`LiveSource`(cargo feature `live`, 기본 꺼짐)**:
  - `pcap` 2.5.0 사용. 활성화 전에 `buffer_size`·`snaplen`·`promisc`·`precision(Nano)`(실패 시 마이크로로 내리고 실제 값 기록)·`immediate_mode`를 설정한다.
  - `stats()` 가용성 매핑은 r1-sol §1을 따른다(0 = 무손실 금지).
  - `send`는 `sendpacket`이다.
  - 이 Mac은 BPF 권한이 없다. `--features live`로 **컴파일과 clippy만** 통과시키고, 실제 열기 시험은 `#[ignore]`로 둔다. 권한 오류는 `CaptureError::Permission`으로 분리한다.
- **`CaptureLoop`**(캡처 스레드):
  - 소스에서 읽어 `source_seq`(단조 증가)와 host mono 시각을 붙인 뒤 **`BoundedFrameQueue`**에 넣는다.
  - 큐 상한: 바이트 128 MiB, 개수 512k(상수, 설정 가능). 넘치면 **새 프레임을 버리고**(drop-newest) 잃은 seq 범위를 모아 `GapReport{kind: Record, first_seq, last_seq, count}`를 낸다.
  - 100 ms마다 `stats()`를 증분으로 비교해 `GapReport{kind: Source, cause: KernelDrop|IfDrop|StatsUnavailable, count: Some|None}`를 낸다.
  - 정지 핸들(즉시 깨움)과 재시작 시 `CaptureRestart` gap을 둔다.
  - gap 보고 채널도 상한(1024)을 두고, 넘치면 합쳐서(coalesce) 하나로 만든다.
- **테스트**:
  - 샘플 pcapng를 Max 재생 → 소비자가 받은 프레임 수 = 파일 수, seq 연속.
  - 느린 소비자로 큐를 넘치게 해 gap 보고의 범위·count 정확성 확인.
  - stats Unavailable 경로.
  - SimTap TCP 연결(SYN·데이터·FIN)을 loom-eth-decode 없이 etherparse로 다시 파싱해 seq 정합 확인.
  - 정지 지연 < 200 ms.
- **간단한 처리량 측정**(10분 이내): 64 B·1500 B 합성 프레임 Max 재생 → 큐 → 소비 pps. 결과는 보고서에 "macOS arm64, 캡처 제외 경로 실측"으로 적는다.

## 보고
crate마다 `docs/impl/eth/reports/p0-sol-pcapng.md`, `p0-sol-capture.md`를 쓴다. pcapng가 끝나면 보고서를 쓰고 바로 capture로 넘어간다.
