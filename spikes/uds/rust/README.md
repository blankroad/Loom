# UDS v2 Rust 조사 스파이크

후보 API 확인과 최소 DoIP TCP loopback을 한 실행에 묶었다. 프로덕션 전송/UDS 러너가 아니다.

```sh
cd spikes/uds/rust
CARGO_HOME="$PWD/.cargo-home" cargo run --release --locked --manifest-path Cargo.toml
```

의존성은 로컬 Cargo.toml/Cargo.lock, 캐시는 로컬 `.cargo-home`, 빌드는 로컬 `target`에 둔다. 전역 설치는 하지 않는다.

- `automotive_diag`의 `uds`, `doip` 기능을 켜야 해당 모듈이 공개된다.
- `doip-codec`의 STD Decoder는 분할 프레임을 기다리고 연속 프레임 두 개를 분리한다.
- `automotive_diag` central security 값은 0xE0, `doip-definitions` 값은 0x02이다. 후자에 의존하는 후보 코덱은 0xE0 요청을 거부했다.
- 후보 코덱은 u32::MAX 길이 헤더만 받으면 크기 오류 대신 `None`을 반환했다. Loom에서 버퍼 누적 전에 상한 검사가 필요하다.
- 왕복 측정은 별도 최소 코덱(할당 전 4096바이트 상한), tokio 단일 스레드, loopback, 워밍업 20회 + 측정 1000회다. ACK와 응답을 한 write로 보낸다.
- 결과는 `../evidence/v2-rust.json`. IPC, TLS, UI, HW, 완전한 UDS 러너, Windows/Linux, 24시간 soak는 검증하지 않았다.
