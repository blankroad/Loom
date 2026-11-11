# CAN v2 Rust 소규모 검증 결과

측정일: 2026-11-10(KST). 목적: Rust 전용 OS 스레드와 bounded channel의 개발 호스트 동작 확인. 제품 구현·실HW 시험·24h soak 완료 보고가 아니다.

## 재현

```sh
rustc --version
cargo --version
cargo run --release --locked --manifest-path spikes/can/rust/probe/Cargo.toml > spikes/can/rust/probe-result.json
```

- 실측 도구: rustc 1.93.1, cargo 1.93.1, macOS aarch64. 실제 숫자 정본: [probe-result.json](probe-result.json).
- `crossbeam-channel =0.5.17`, transitive `crossbeam-utils =0.8.23`; [Cargo.lock](probe/Cargo.lock) 고정. 전역 설치 없음.
- 초기 Cargo cache 쓰기는 sandbox에서 차단되어 승인된 실행으로 의존성을 받았다. 이후 `--locked --offline` 실행 성공(exit 0).
- 빌드 중 format 문자열 오류 1건을 수정한 뒤 아래 전체 실행을 완료했다. 결과 파일은 성공 실행의 출력만 담는다.

## 1. 10 ms 주기 전용 스레드 — 실측

기본 OS 우선순위, `Instant` 절대 deadline + `thread::sleep`, spin 없음. 매 case 1,000회, 약 10초. CPU 부하는 별도 스레드 1개에서 정수 연산을 연속 수행한다. deadline을 놓치면 건너뛰고 burst catch-up하지 않는다. vendor API를 호출하지 않는 host send-hook 시각이다. p99는 nearest rank.

| 조건 | deadline 지각 p99 / max | 절대 간격 오차 p99 / max | skip |
|---|---|---|---|
| idle | 4.839209 / 5.013542 ms | 4.527333 / 4.987417 ms | 0 |
| CPU 경쟁 스레드 1개 | 4.848625 / 16.452708 ms | 4.581000 / 12.535250 ms | 1 |

해석: 이 호스트의 기본 sleep만으로 기존 잠정 목표인 간격 오차 p99 <3 ms를 충족하지 못했다. Rust로 옮겼다는 이유로 ±1 ms 또는 FC p99 개선을 선언하지 않는다. 우선순위·Windows high-resolution waitable timer·실제 드라이버 전송·bus arbitration·journal 부하는 이번 시험에 없다.

v1.2 Python의 decode 경쟁 부하 간격 오차 p99 약47~60 ms, FF→FC p99 약52 ms와는 부하·계측점·샘플 수가 다르다. 언어별 공정한 속도 비교나 GIL 제거의 인과 증거로 사용하지 않는다. Rust FC→driver/wire 측정은 별도 필요하다.

## 2. bounded channel — 실측

고정 128 B `Frame`, capacity 8,192, producer/consumer 각 OS 스레드 1개, case당 1,000,000개, 5회. 모든 seq의 순서·개수를 assert한다. 합성 producer는 blocking `send`를 쓰므로 처리량 시험 중 손실 0은 RX에서 무손실을 보장한다는 뜻이 아니다. 설계의 driver RX는 `try_send`다. 지연 표본은 1,000개마다 1개이며 timestamp 생성부터 recv까지(대기 중 send 시간 포함)다.

| run | 처리량(frame/s) | 표본 지연 p99 / max(ms) | loss / reorder |
|---|---:|---|---|
| 1 | 11,599,964 | 0.040375 / 0.118750 | 0 / 0 |
| 2 | 11,917,205 | 0.059833 / 0.092791 | 0 / 0 |
| 3 | 11,609,059 | 0.038459 / 0.093292 | 0 / 0 |
| 4 | 11,621,561 | 0.032833 / 0.118042 | 0 / 0 |
| 5 | 11,641,003 | 0.067416 / 0.320291 | 0 / 0 |

`size_of::<Frame>() ==128` 확인. payload capacity 산술은 8,192×128=1 MiB지만 crossbeam 슬롯·동기화 구조와 allocator 비용은 별도다. 8칸 큐를 채운 뒤 9번째 `try_send`가 `Full`이고 기존 8개가 순서대로 남는 것도 assert했다. 실제 `source.gap`/`record.gap` 발행이나 프로세스 IPC는 구현하지 않았다. 이 처리량은 직렬화·디스크·USB·WS를 포함하지 않는다.

## 3. 외부 소스 확인

- [crates-evidence.json](crates-evidence.json): 공식 sparse registry의 버전·게시일·yank·checksum과 배포 `.crate`의 Cargo.toml 라이선스·repository. crates.io API는403이어서 registry index와 공식 crate archive를 사용했다. 전체 dependency tree의 라이선스 심사를 대신하지 않는다.
- [source-checks.json](source-checks.json): SDK 다운로드 URL·해시·확인한 함수·내부 파일 경로. PEAK 공식 5.1.0.1194 패키지의 `Samples/Samples.zip` 안에 `Console/Rust/*/src/PCANBasic.rs`가 있다. `InitializeFD`/`ReadFD`/`WriteFD` 선언을 확인했다. macOS에서 Windows SDK를 빌드하거나 DLL을 로드하지 않았다.
- Vector XL 26.30 공식 매뉴얼의 network/channel API와 VN5000 6.1의 media converter 표를 확인했다. XL 매뉴얼 p135 설정 화면도 렌더링해 확인했다. 장비·드라이버는 설치/접속하지 않았다.
- SDK ZIP·DLL·vendor header·PDF와 조사용 third-party archive/source 사본은 최종 산출물에 배포하지 않는다. URL·해시와 조사 결과만 남긴다.

## 다음 필수 검증 — 미실행

| 단계 | 시험 | 판정 |
|---|---|---|
| M0 | 같은 journal-on + 동일 CPU 부하에서 Rust FF ingest→FC submit / wire, BS0/8, STmin0/1ms/100µs, CC/FD | 기존52 ms와 같은 계측 조건 확보 후 비교; host p99≤3 ms는 목표 |
| M0 | 두 Rust 프로세스의 virtual bus, daemon IPC, 시퀀스·STOP·클라이언트 강제 종료 | 같은 프로세스 채널 결과로 대체 금지 |
| M2 | Windows x64 high-resolution timer·우선순위 전/후, PCAN FD 및 VN5640 | p99와 max·skip·TX ACK 품질 보고, hard RT 보증 없음 |
| M2 | Windows/Linux 24h 혼합 부하, journal fault·gateway lag·bus-off·DLL crash | v2-sol의 RSS·손실·지연 관문 전부 실행 |
| M2/M3 | BLF/ASC Rust exporter→CANoe/PCAN-View/Python reference 읽기, FD64·시간·종료/절단 fixture | 자기 writer-reader 왕복만으로 호환 통과 금지 |
