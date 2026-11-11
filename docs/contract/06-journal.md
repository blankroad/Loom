# 06. 저널 · 기록 · 보존

## 6.1 원칙 (D7, C2)
- 모든 세션 자동 기록. **원본은 디스크로 흘려보내고 메모리에 누적하지 않는다**(허브 링 32 MiB는 화면용).
- 대량 원본은 **worker가 직접** 쓴다(`loom-journal` 라이브러리를 worker 프로세스 안 writer 스레드로). gateway 이벤트(승인·정책·UDS 트랜잭션·run/test·core)는 producer `gateway`로 같은 형식.
- `--no-record` 채널은 데이터 payload만 생략한다(`record.health=disabled`). 감사는 항상 기록. 그런 구간은 증거 완전성이 false라 원문 일치 PASS 불가.

## 6.2 세그먼트 형식 (E5)
```
~/.loom/records/<YYYY-MM-DD>/<daemon_session>/<producer_id>/<producer_epoch>/<seg_no:06>.lj
헤더:   magic "LOOMJ1\0\0" | u16 format_ver(1) | u16 codec | u16 checksum_alg | producer_id | producer_epoch | created wall_ns | clock_id
레코드: u32 len | u32 crc32c(payload) | u8 rec_type(event|index_hint|marker) | payload
```
- codec 레지스트리(헤더 `codec` id): `json/1`(**M1 기본**, 레코드 1개 = `SourceRecord` JSON 1개), `msgpack/1`(M2부터, full schema 실측 후), **`pcap-epb/1`**(eth: 레코드 = pcapng EPB 바이트, Loom 메타는 EPB `opt_comment`의 `loom1 …` — eth 결정 X10). checksum = **CRC32C**(Castagnoli) 고정.
- 큰 원시 바이트는 raw chunk 파일 + `payload.bytes.encoding="raw_ref"`.
- 회전 64 MiB 또는 60 s. 쓰는 중인 세그먼트 이름은 `<seg>.lj.open`, seal 후 `<seg>.lj`로 rename. payload를 해석하지 않는 codec(`pcap-epb/1`, `msgpack/1`)은 hint 레코드(rec_type 2)로 `source_seq`·`mono_ns`를 싣는다. CRC는 payload만 덮는다(rec_type 바이트 손상은 모르는 값일 때만 탐지 — v2 후보). seal: close → fsync → rename → 인덱스 commit → zstd 블록 압축본(seekable 1 MiB) 검증 → 원본 삭제.
- 크래시 복구: 열린 세그먼트의 마지막 유효 CRC까지 잘라내고 `record.gap{cause:"crash_tail", bytes}`. sealed 파일 중간 CRC 오류는 `record.gap{cause:"corruption"}` + quarantine(tail로 숨기지 않음).
- 세션 디렉터리 `manifest.json`: OS·빌드·worker 버전·정책/DB hash·채널 설정·드라이버 큐 실제값.
- JSONL은 **export 형식**(`loom record export --format jsonl`). 도메인 표준 포맷(BLF/ASC/MF4, pcapng, txt)은 도메인 export.

## 6.3 stream mapping과 인덱스
- **stream mapping 로그(정본)**: 허브가 배치마다 `(stream_epoch, seq_first, seq_last, producer_id, producer_epoch, source 범위/목록, mono_first, mono_last)`를 쓴다. 원본에는 stream seq가 없으므로 이 로그가 없으면 전역 순서를 복원할 수 없다 — **버리지 않는다**(밀리면 허브 발행이 backpressure). mapping이 durable하지 않은 구간의 커서는 `uncertain`으로 표시하고 source ref로 재조회한다.
- sparse 인덱스(SQLite `index.db`, WAL): 4096건 또는 1 s마다 `(source ref, file, offset, mono)`. 세그먼트에서 재구축 가능(`loom record verify --rebuild-index`).

## 6.4 내구성
| 대상 | OS로 write | 디스크 sync |
|---|---|---|
| 데이터 세그먼트 | ≤ 10 ms 또는 64 KiB | 그룹 ≤ 100 ms + seal |
| gateway 이벤트·mapping 로그 | ≤ 10 ms | 그룹 ≤ 100 ms |
| 감사 | 레코드마다 `write()` 성공 확인 후 진행 | R2+ 쓰기 intent는 송신 전 즉시, 그 외 그룹 ≤ 100 ms |
| index.db | WAL `synchronous=NORMAL` | 체크포인트 |
- worker `Health`에 `written_through_source_seq`·`durable_through_source_seq`를 보고한다. 프로세스 크래시와 전원 차단 내구성은 다르다. macOS `File::sync_data` = `F_FULLFSYNC`.

## 6.5 조회 · 재생
- 조회: 링 → mapping + 인덱스 → 세그먼트 블록 해제·필터. 상한: 스캔 256 MiB·2 s(기본), 결과 64 KiB(RPC) → 넘으면 `E_SCAN_LIMIT` + `resume_from`. 큰 결과는 HTTP GET.
- 재생(M3): `core.replay.open{range, speed}` → producer `replay:<session>`, `original_origin`·`purpose:"replay"` 보존, 재생 채널 쓰기 전부 거부, UI `REPLAY` 배지.

## 6.6 보존 (D7, E6)
- 기본 **30일 또는 20 GiB 중 먼저 도달**. GC 단위는 sealed 세그먼트(active 세그먼트 제외). **테스트 증거로 인용된 세그먼트는 자동 보호(pin)**.
- quota 80 % 경고, 90 % 오래된 unpinned sealed 제거, 100 % → `record.health=failed`(`04-safety.md` §4.7). 삭제는 retention manifest에 남기고 `record.gap`과 구분한다.
- **고부하에서 30일 무손실 보존은 불가하다**: 예) CAN 5k fps × JSON 약 580 B ≈ 2.9 MB/s → 20 GiB를 약 2시간에 채운다(압축 전). UI·`loom status`는 "보존 가능 기간 추정"을 표시한다.
- 감사는 별도 계정·무기한.
- 비밀값(보안 접근 key, 비밀번호 입력)은 기본 마스킹(길이·해시), 원문 보존은 opt-in.
