# P0 sol — loom-eth-pcapng
- 위치: `.worktrees/eth/crates/loom-eth-pcapng` (eth/main), 커밋 없음.
- 자체 bounded reader/writer 채택: pcap-file 2.0.0 EPB는 raw ticks를 `Duration::from_nanos`/`as_nanos`로 처리해 IDB `if_tsresol`을 적용하지 않는다. ReadBuffer 기본도 8,000,000 B여서 요구한 할당 전 1 MiB 제한에 맞지 않는다.
- 근거: 로컬 registry `pcap-file-2.0.0/src/pcapng/blocks/enhanced_packet.rs`·`src/read_buffer.rs`; https://docs.rs/pcap-file/2.0.0/src/pcap_file/pcapng/blocks/enhanced_packet.rs.html
- 구현: LE SHB/다중 IDB/EPB/ISB writer, LE/BE·다중 section·unknown block skip·고전 pcap 4 magic reader. Read/Write만 요구한다.
- X10: `LoomMeta` ↔ `loom1 seq=… dir=rx|tx org=agent|user|script [req=…]`; 사용자/잘못된 loom 주석 보존, 중복 메타데이터 거부.
- 공개 API: `PcapNgReader::{new,with_block_limit,next_block,next_packet,interfaces,last_good_offset}`, `PcapNgWriter::{new,start_section,write_interface,write_packet,write_statistics,flush}`.
- `SessionHeader` serde Serialize 제공. 입력은 타입 생성 후 validate; 무제한 serde Deserialize API는 제공하지 않는다.
- `encode_record`/`try_encode_record`/`decode_record`, `export(header, iterator, Write)` 제공. canonical codec의 ticks는 나노초이고 export IDB를 나노초로 정규화한다. EPB는 재인코딩 없이 복사한다.
- API 조정: 잘못된 Packet·상한·timestamp overflow를 조용히 쓰지 않도록 `encode_record`는 `Result<Vec<u8>>`이다(작업 파일의 무오류 Vec 서명에서 변경).
- ts_ns는 u128: u64 microsecond tick 전체 범위와 기존 R1 fixture의 잘못 표기된 큰 시각도 overflow 없이 보존한다. canonical EPB는 u64 ns 범위 밖이면 오류다.
- R1 fixture는 IDB가 microsecond인데 pcap-file writer가 ns raw ticks를 썼다. 이번 reader는 파일대로 해석하며 최초 ts_ns=1,700,000,000,000,000,000,000을 확인한다. 기존 조사 roundtrip은 외부 포맷 적합성 증거가 아니었다.

| C2 대상 | 상한 | 초과 정책 |
|---|---|---|
| block/record | 1 MiB, reader 설정으로 낮출 수 있음 | 할당 전 오류 |
| IDB | section당 256 | 오류, 새 section에서 reset |
| packet | block보다 작음, MAX_PACKET_BYTES | 오류, caplen/origlen/snaplen 검증 |
| options / comments | 1024 / 64 | 오류 |
| comment/name/request UTF-8 bytes | 4096 / 255 / 512 | 복사 전 오류 |
| reader 보관 | 현재 block+현재 section IDB만 | 스트리밍; 파일 전체 누적 없음 |

- 관문: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` 모두 통과.
- 테스트: 14개 통과; 무작위 바이트 100,000회 reader/record/meta panic 없음 포함. micro/nano·다중 IDB·큰/작은/snaplen 잘림·BE 수제 fixture·binary/decimal 해상도·4 pcap magic·다중 endian section·ISB·export 원본 EPB 일치·truncation/마지막 정상 offset 확인.
- timestamp raw wire 숫자도 직접 확인해 writer/reader가 같은 단위 오류를 감추는 왕복을 방지했다.
- 미해결: Wireshark/tshark 교차 확인 미확인(설치 없음); if_tsoffset은 조용히 오해하지 않도록 명시 오류, SPB 등 비필수 block은 skip. Binary의 sub-ns 부분은 floor-to-ns, coarse 해상도 쓰기에서 정밀도 손실은 오류.
- CORE workspace.dependencies 추가 요청: serde =1.0.228 (derive), serde_json =1.0.149 (dev). 나머지는 std만 사용한다.
- 다음: 지시대로 `loom-eth-capture` 구현으로 진행.
