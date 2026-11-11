# 스파이크 opus-cli-rtt — `loom` CLI 1회 호출의 바닥 비용 (core-opus R2, 2026-11-10)

목적: 에이전트가 `loom <domain> <verb>`를 부를 때 **코어 처리 전** 고정 비용(프로세스 생성 + clap + 포트 파일 읽기 + 동기 WS 연결 + hello + RPC 1회 + 종료). R1 두 초안의 CLI 목표(opus: Linux/macOS p95 ≤ 30 ms·Windows ≤ 60 ms, sol: warm p95 ≤ 50 ms)의 근거. core-sol R1 스파이크 4번(미실행)을 대신한다. 재부팅 전(2026-11-10 08:28) 실행, 원본 `result-macos.json`.

구성: `srv` = axum 0.8 WS 서버(hello·RPC에 즉시 JSON 응답, 허브·정책·저널 없음). `cli` = clap derive + tungstenite 0.26 동기 클라이언트(tokio 없음), release·strip, 실행 파일 1,019,088 B. `bench` = `std::process::Command`로 `cli`를 300회(워밍업 20회 제외) 실행해 생성→종료 벽시계.

환경: macOS, Apple M1, rustc 1.93.1, release. 재현: `CARGO_TARGET_DIR=target cargo build --release && ./target/release/bench`.

| 측정 | p50 | p95 | p99 | max |
|---|---:|---:|---:|---:|
| `cli --help` (생성 + clap + 종료) | 2.19 ms | 2.81 ms | 2.97 ms | 3.04 ms |
| 생성 + WS 연결 + hello + RPC 1회 + 종료 | 2.78 ms | **3.30 ms** | 3.53 ms | 4.28 ms |

해석 [확인 — macOS만]
- CLI·WS 고정 비용 ≈ 3 ms. 30·50 ms 목표 어느 쪽에서도 병목이 아니다. 실제 지연은 gateway(정책·감사 fsync·worker 홉·발행 순서)가 결정한다.
- 참고: core-sol 저널 스파이크의 Apple `F_FULLFSYNC` 1회 ≈ 3.9–5.9 ms. 쓰기마다 감사 intent를 강한 fsync로 내리면 그것만으로 CLI 고정 비용보다 크다.
- 한계: 따뜻한 캐시, 로컬 TCP, 서버 처리 0. Windows(프로세스 생성·Defender)와 Linux 미측정.
