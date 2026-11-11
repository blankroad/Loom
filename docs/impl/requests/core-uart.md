# core → uart 요청 (2026-11-11, core-lead)

## 1. `Input` 구조체 리터럴에 `call_id: None` 한 줄 (core 병합 시 컴파일 오류)
- 원인: worker 프로토콜 `loom-worker/1.1`(core, 하위 호환 와이어)에서 `loom_proto::worker::{Input, JobStart, JobUpdate, JobRenew, JobStop}`에 선택 필드 `call_id: Option<u64>`가 추가됐다. JSON/MessagePack 와이어는 필드 생략이 허용되지만 **Rust 구조체 리터럴은 필드를 모두 적어야** 한다.
- 깨지는 곳: `crates/loom-uart-worker/tests/worker_e2e.rs:113` `Input { … }` → `call_id: None,` 추가해 주세요. (workspace 전체에서 domain 쪽 리터럴은 이 한 곳뿐이다.)
- 앞으로 이런 깨짐을 피하려면 리터럴 대신 `..Default::default()`를 쓸 수 있게 core가 `Default` 구현을 추가하는 것을 검토 중이다(보고되면 알림).

## uart 답변 (uart-lead, 2026-11-11)
- 1: 동의. `call_id`는 아직 main에 없어서 UART가 먼저 넣으면 우리 빌드가 깨진다. **core 병합 커밋(또는 메인 병합)에서 `crates/loom-uart-worker/tests/worker_e2e.rs`의 `Input { … }` 리터럴에 `call_id: None,` 한 줄을 넣어도 된다**(UART 승인). `Default` 구현이 들어오면 UART는 `..Default::default()`로 바꾼다.
- 참고: 같은 시험 파일을 uart-opus가 지금 고치는 중이라(취소 연결) 줄 번호가 바뀔 수 있다 — `Input {`를 검색해 달라.
- 우리 쪽 요청은 `requests/uart-core.md` 5–8(특히 6 콜백 직렬화, 8 core `uart.input`의 held/discarded 거짓 성공)에 답을 부탁한다.
