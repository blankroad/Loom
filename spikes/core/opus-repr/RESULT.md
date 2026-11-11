# 스파이크 opus-repr — 이벤트 레코드 이중 표현 (core-opus, 2026-11-10)

목적: 스키마 정본 = Rust serde 타입 하나(C3)로 **외부 JSON**(u64 = 문자열, 바이트 = hex — D3)과 **내부 worker 프로토콜 MessagePack**(u64 = 정수, 바이트 = bin)을 동시에 만족할 수 있는지, 그리고 이벤트 1건의 크기·인코딩 속도.

방법: `U64`/`Bytes` newtype이 `Serializer::is_human_readable()`로 분기(serde_json = true, rmp-serde = false). D3 필드를 모두 가진 `can.frame` 이벤트(8바이트 payload) 20만 건 인코딩.

환경: macOS, Apple M1, rustc 1.93.1, `--release`, 단일 스레드. 실행: `CARGO_TARGET_DIR=target cargo run --release`.

| 형식 | 1건 크기 | 인코딩 속도 | 비고 |
|---|---|---|---|
| JSON (serde_json) | 501 B | 1.05 M ev/s (529 MB/s) | `"seq":"1840201"` 문자열 확인 |
| MessagePack named (map) | 351 B | 1.65 M ev/s | 필드 이름 포함 — Python 어댑터 디버깅 쉬움 |
| MessagePack array (tuple) | 120 B | 3.18 M ev/s | 필드 순서 의존 — 스키마 버전 고정 필요 |

확인 사항
- 세 형식 모두 왕복(decode == encode 원본) 성공 [확인].
- MessagePack 바이트열 안에 seq의 10진 문자열이 없음 → 정수로 인코딩됨 [확인].
- 결론: 한 타입으로 두 표현이 가능하다. JSON 인코딩만으로도 CAN 설계 최대치(채널당 20k fps × 4)의 10배 이상 여유(M1, 단일 코어) [확인 — macOS 수치, Windows/Linux 미측정].
- 한계: 디코드 속도, 할당 횟수, 배치 인코딩, 압축 후 크기는 측정 안 함 → core-sol 스파이크 1·2(worker 왕복·WS 처리량)에 위임.
