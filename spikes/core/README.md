# CORE R1 성능 스파이크

최종 실행: `runs/20261109T231749Z`. 환경: Apple M1 8core(4P+4E),16 GiB, macOS27.0.1 arm64, rustc1.93.1, cargo release. 실제 toolchain·시각은 [환경 JSON](runs/20261109T231749Z/environment.json). 개발 머신의 합성 실험이며 Windows/Linux·실장치·실브라우저·24 h soak 결과가 아니다. 다른 사용자/에이전트의 머신 부하는 통제하지 못했다.

## 파일·재현법

- `worker-ipc/`: std UDS 두 프로세스, postcard/bincode2/MessagePack 요청응답 및100k/s 단방향. 프레이밍 상한64 KiB, 64 B payload, 200건 배치. MessagePack bytes는 serde_bytes bin, compact tuple이며 제품 추천 map 형식과 다르다.
- `ws-batch/`: 별도 axum 서버 프로세스 + Rust 클라이언트1/4개. JSON 배열/행 postcard/고정64 B 열 프레임. 연결별 8배치 큐와100 ms send timeout. 느린 클라이언트는250 ms마다1배치 읽는다.
- `journal/`: 합성 JSONL 100k건(약580.8 B/건) append/zstd1 vs bundled rusqlite WAL. 입력은 타이밍 전에 생성하며 segment 복사/압축/쓰기·sync는 포함한다.
- `collect_evidence.py`, `library-evidence.md`, `evidence/`: 공개 API·공식 문서 원본/메타데이터. 전역 설치 없음.
- `run_benchmarks.py`: 독립 결과 폴더를 만들고 벤치를 순차 실행. IPC3회, WS 조합별2회, 저널1회. `summarize.py`: 전체 반복범위 요약.

저장소 루트에서 실행:

```sh
env CARGO_HOME="$PWD/spikes/core/.cargo" CARGO_TARGET_DIR="$PWD/spikes/core/worker-ipc/target" cargo build --release --locked --manifest-path spikes/core/worker-ipc/Cargo.toml
env CARGO_HOME="$PWD/spikes/core/.cargo" CARGO_TARGET_DIR="$PWD/spikes/core/ws-batch/target" cargo build --release --locked --manifest-path spikes/core/ws-batch/Cargo.toml
env CARGO_HOME="$PWD/spikes/core/.cargo" CARGO_TARGET_DIR="$PWD/spikes/core/journal/target" cargo build --release --locked --manifest-path spikes/core/journal/Cargo.toml
python3 spikes/core/run_benchmarks.py
python3 spikes/core/summarize.py
python3 spikes/core/validate_results.py
python3 spikes/core/collect_evidence.py
```

crate별 Cargo.lock이 측정 버전을 고정한다. 각 target/과 Cargo cache는 spikes/core/ 안이다. `worker-ipc`는 Unix 전용 검증 코드, 서버 RSS 단위는 macOS 기준이므로 이 코드를 그대로 Windows 지원 구현이라고 쓰지 않는다. 저널 직접 재실행은 새 작업 디렉터리에서 해야 한다(기존 DB 덮어쓰기 방지). 공개 자료 재수집은 시점에 따라 버전/다운로드 수를 갱신한다.

검증: `validate_results.py`는 최종 실행의 IPC/WS 수신 건수·중복·프레임 cap, SQLite9개 integrity/100k행, 비압축 segment3개의100k 연속 source seq, source/Cargo.lock hash, 문서 링크·0–15절을 점검했다. [검증 JSON](runs/20261109T231749Z/validation.json)에 결과를 보존한다. zstd 내용 readback·실제 crash 복구 검증은 미실시다. `worker-ipc/results.json`은 빌드와 겹친 초기 smoke이며 serde_bytes 수정 전 결과여서 결정 근거에서 제외했다.

## 1. worker IPC

RTT: codec마다 warmup1000회 후10,000회, 한 요청을 직렬화→소켓→상대 decode/encode→응답 decode까지. stream: 300,000건을200건/2 ms 목표로 주입. CPU는 해당 stream 구간 user+system 합계이며 한 코어100% 기준이다. percentile은 반복별 값의 최솟값–최댓값으로, 표본을 합친 분위수가 아니다.

| codec | RTT p50 µs | RTT p99 µs | 수신 events/s | stream p99 ms | 양 프로세스 CPU%/1core | wire B/event |
|---|---:|---:|---:|---:|---:|---:|
| postcard | 6.500–7.750 | 17.792–23.208 | 100041–100042 | 0.197–0.252 | 5.30–6.45 | 75.97–75.97 |
| bincode2 | 7.667–7.792 | 17.917–23.000 | 100042–100043 | 0.175–0.187 | 5.42–5.70 | 78.59–78.59 |
| msgpack | 7.000–8.125 | 21.000–29.125 | 100040–100044 | 0.171–0.213 | 4.96–5.92 | 80.60–80.60 |

모든 codec의 source_seq 연속성과300,000건 수신을 assert했다. 100k/s는 paced 부하 달성이며 최대 지속 처리량을 탐색한 수치가 아니다.
MessagePack가 compact tuple/bin에서 충분한 여유를 보였지만 제품의 map·풍부한 envelope·2lane·감사·저널 비용은 제외다. Rust/Python 공통 생태계 때문에 worker v1 후보로 추천하고 P0 golden corpus에서 확정한다. bincode2.0.1은 비교만 하며 bincode3.0.0 유지보수 종료 공지로 신규 채택하지 않는다.

## 2. WebSocket 배치

300,000 events/3 s 목표(100k/s),200건 배치, payload64 B. timestamp는 batch 생성 때 찍으므로 **batch를 모으는2 ms 대기·장치 ingest·journal·JSON-RPC envelope·브라우저 decode/렌더는 미포함**이다. 서버 RSS는 child getrusage peak이며 CLI/클라이언트 RSS는 제외한다. 이벤트별 수신 지연은 누락된 이벤트를 포함하지 않으므로 수신 건수와 함께 해석한다.

| codec·연결 | 송신 events/s | 정상 client 수신 범위 | 정상 client p99 ms | 서버 peak RSS MiB | 서버 CPU%/1core | 최대 frame B | full/drop batch·timeout |
|---|---:|---:|---:|---:|---:|---:|---|
| JSON / 1 | 100033–100036 | 300000–300000 | 0.818–0.873 | 3.61–3.61 | 10.96–11.21 | 57519 | [0, 0] / [0, 0] |
| JSON / 4 | 99988–100020 | 300000–300000 | 0.895–1.124 | 3.94–4.08 | 14.80–15.59 | 57519 | [0, 0] / [0, 0] |
| JSON / 4 (1개 slow) | 99992–100032 | 300000–300000 | 1.044–1.230 | 4.52–4.53 | 14.66–14.87 | 57519 | [42, 42] / [1, 1] |
| postcard 행 / 1 | 100018–100038 | 300000–300000 | 0.328–0.346 | 3.34–3.39 | 6.71–7.47 | 15202 | [0, 0] / [0, 0] |
| postcard 행 / 4 | 100017–100023 | 300000–300000 | 0.385–0.426 | 3.56–3.62 | 10.60–12.10 | 15202 | [0, 0] / [0, 0] |
| postcard 행 / 4 (1개 slow) | 100025–100028 | 300000–300000 | 0.381–0.428 | 3.64–3.64 | 11.06–11.19 | 15202 | [42, 41] / [1, 1] |
| 고정 열 / 1 | 100035–100046 | 300000–300000 | 0.293–0.321 | 3.33–3.38 | 5.69–6.35 | 14416 | [0, 0] / [0, 0] |
| 고정 열 / 4 | 100030–100047 | 300000–300000 | 0.319–0.431 | 3.48–3.52 | 9.32–11.36 | 14416 | [0, 0] / [0, 0] |
| 고정 열 / 4 (1개 slow) | 100003–100008 | 300000–300000 | 0.323–0.364 | 3.64–3.66 | 8.44–10.74 | 14416 | [42, 42] / [1, 1] |

`binary`는 postcard **행** 직렬화, `column`은 count/first seq/timestamp열/고정64 B raw열을 가진 짧은 실험 형식이다. full schema·가변DLC·nullable·origin dictionary·불연속 seq를 다루지 않아 제품 형식으로 사용할 수 없다. JSON은 bytes를 숫자 배열로 표현하므로 hex/base64 JSON과 크기비가 달라진다. WS frame header/마스킹/TCP overhead는 wire_payload_bytes에서 제외다.

느린 구독자는 send timeout으로 그 연결만 종료되고 정상 클라이언트는 계속 받는다. 이 스파이크는 gap 카운터만 수집하며 실제 display.gap 알림·우선 제어 이벤트의 무손실 전달은 구현하지 않았다. 제품에서는 연결별 데이터큐/제어큐·overflow snapshot·gap 필드가 필요하다.

첫 실행 `runs/20261109T230951Z/ws-column-4-2.json`에서 **정상 4개 중3개가299,600건**, queue full6배치를 기록했다. 기본8배치 큐는 host scheduling spike를 항상 흡수하지 못했다. 원시 실패를 보존하며 최종 실행이 통과하더라도 안정성 증명으로 지우지 않는다. 첫 실행의 느린 클라이언트 deadline은 buffered read가 즉시 완료될 때 timeout_at만으로 엄격히 끝나지 않는 결함이 있어, 최종 코드는 매 read 전 절대 deadline을 직접 검사하도록 수정했다. 첫 실행의 slow 지연은 최종 요약에 섞지 않는다.

판단: M1 JSON으로 시작 가능하며 고속 data 구독은 바이너리 opt-in을 추천한다. 단순 열 형식의 크기/CPU 이득은 방향 근거이며 실제 브라우저·full schema·4클라이언트24 h 관문 뒤 확정한다. permessage-deflate는 이 실험에서 사용하지 않았다.

## 3. 저널

동일100,000건 입력, segment는 length prefix+JSONL 또는 zstd1 독립 chunk. 여기에는 제품 CRC/index/recovery가 없다. `nominal_sync_interval_ms_at_100k`는 **100k/s라면 해당 건수가 쌓이는 시간**이며 실제 timer를 돌린 값이 아니다. SQLite WAL autocheckpoint=0, commit 타이밍과 마지막 checkpoint를 따로 기록했다.

**동기화 차이:** [Rust1.93.1 Unix fs 소스](https://github.com/rust-lang/rust/blob/1.93.1/library/std/src/sys/fs/unix.rs)에서 Apple `sync_data`가 `fcntl(F_FULLFSYNC)`를 호출함을 확인했다. 일반 macOS fsync와 다르다. SQLite `FULL` 기본 fullfsync=OFF와 동등한 내구성 비교가 아니므로 `FULL_FULLFSYNC`(fullfsync=ON, checkpoint_fullfsync=ON)를 추가했다. [SQLite 공식 PRAGMA](https://sqlite.org/pragma.html#pragma_fullfsync)·[synchronous](https://sqlite.org/pragma.html#pragma_synchronous)를 참고한다. 실제 전원 차단은 시험하지 않았다.

| 형식 | batch events (명목 ms@100k) | events/s | 파일 MiB | checkpoint ms | segment sync ms/회 |
|---|---:|---:|---:|---:|---:|
| segment-jsonl | 200 (2) | 46,138 | 55.389 | 0.00 | 4.046 |
| segment-zstd1 | 200 (2) | 46,627 | 1.124 | 0.00 | 3.934 |
| sqlite-wal-NORMAL | 200 (2) | 458,500 | 65.266 | 133.93 | — |
| sqlite-wal-FULL | 200 (2) | 431,443 | 65.266 | 140.70 | — |
| sqlite-wal-FULL_FULLFSYNC | 200 (2) | 41,424 | 65.266 | 99.83 | — |
| segment-jsonl | 2000 (20) | 368,461 | 55.387 | 0.00 | 4.291 |
| segment-zstd1 | 2000 (20) | 341,656 | 0.680 | 0.00 | 4.141 |
| sqlite-wal-NORMAL | 2000 (20) | 474,353 | 65.266 | 130.31 | — |
| sqlite-wal-FULL | 2000 (20) | 543,216 | 65.266 | 113.80 | — |
| sqlite-wal-FULL_FULLFSYNC | 2000 (20) | 230,384 | 65.266 | 156.09 | — |
| segment-jsonl | 10000 (100) | 882,191 | 55.387 | 0.00 | 5.882 |
| segment-zstd1 | 10000 (100) | 878,756 | 0.616 | 0.00 | 4.816 |
| sqlite-wal-NORMAL | 10000 (100) | 497,148 | 65.266 | 124.18 | — |
| sqlite-wal-FULL | 10000 (100) | 522,661 | 65.266 | 143.96 | — |
| sqlite-wal-FULL_FULLFSYNC | 10000 (100) | 419,790 | 65.266 | 157.88 | — |

입력 크기: 58,077,776 B, 평균 580.778 B/event. SQLite live DB+WAL 크기는 원본 JSON에 별도 보존했다. checkpoint 이후 파일만 비교하면 순간 디스크 사용량을 과소평가한다.

zstd 압축률은 고정 채널/시각/규칙적 payload의 낮은 엔트로피 때문에 매우 높다. 실제 CAN FD/UART/암호화 데이터의 용량비로 일반화하지 않는다. SQLite는 seq primary-key만 있고 제품의 추가 인덱스/row 해석 비용은 제외다. 바이트/시간 인덱스·CRC·실제 crash 복구는 후속 관문이다. 최초 실행의 “sync_data와 F_FULLFSYNC가 다름” 메모는 **잘못된 설명**으로 본 보고서에서 정정했으며 원본 수치와 당시의 잘못된 메모는 경위 확인을 위해 보존한다.

판단: 짧은 배치마다 강한 sync를 하면 비용이 지배적이다. 100 ms 그룹 sync와 durability watermark를 기본 후보로 두고 JSONL+raw segment 정본/SQLite catalog 혼합을 추천한다. SQLite가 모든 조건에서 느리다거나 append가 항상 빠르다는 결론은 내리지 않는다.

## 4. 미실행·후속 관문

- 선택4번 최소 clap CLI cold/warm+WS JSON-RPC1회 기동 시간은 미실행. 계획의 p95≤50 ms는 목표다.
- Python adapter 교차 codec, interprocess/named pipe·Job/cgroup 강제 상한, full-schema JSON/map codec·가변 열 형식은 미검증.
- 실장치/브라우저 렌더·정책/감사·STOP 우선 전달·원자 snapshot·실제 기록 손실·디스크 full·24 h RSS plateau 시험은 미실행.
- 구현 관문/정확한 큐·메모리 예산은 [CORE 독립 초안](../../docs/plan/core/r1-sol.md), UI/TEST 계약은 docs/plan/_xteam/ 참조.

실험 프로그램은 계획의 선택 근거이며 loom/loomd 제품 구현이 아니다.
