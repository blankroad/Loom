# ETH P1b — explore-sol: live 목록·BPF, TCP 송신, replay 메타 보존

먼저 `p1-common.md`를 읽는다(맨 끝 추가 절 포함: 질문 창 금지, Codex 빌드 경로). 리드가 P1을 커밋했다(021181a, b497a9e). b497a9e는 model u64를 `loom_proto::num::U64`로 바꾼 커밋이다 — 네 worker 코드도 그에 맞춰 리드가 고쳤으니 먼저 확인할 것.

배정 범위:
- `crates/loom-eth-capture/`
- `crates/loom-eth/src/worker/`
- `crates/loom-eth-pcapng/`(필요 시)

`model.rs`·`lib.rs`·gateway·cli는 고치지 않는다. 바꿔야 하면 보고서에 적는다.

## 할 일 (네 P1 보고서의 요청 2·1과 미구현 항목)
1. **live 인터페이스 목록**: `loom-eth-capture`에 공개 `list_interfaces()`(feature `live`)를 만든다.
   - 이름·설명·주소·up·loopback·**캡처 가능 판정과 실패 이유**를 낸다. 열기를 시도해 권한 오류를 분류해도 되나, 열자마자 닫고 promisc는 켜지 않는다.
   - worker `eth.ifaces`가 이것을 쓰게 한다.
2. **BPF 캡처 필터**: `LiveSource`에 BPF setter(`pcap` `filter(expr, optimize=true)`)를 넣는다.
   - 문법 오류는 `CaptureError::Bpf{msg}` → exit 2 `E_USAGE`.
   - replay·sim은 BPF를 **지원하지 않음을 명시 오류**로 낸다(조용히 무시 금지). 대신 표시 필터 `-Y`를 쓰라는 hint를 준다.
3. **TCP 송신(자기 연결만)**: `eth.send{kind:"tcp", to, hex}`.
   - worker가 연결을 열고 보내고 닫는다. 옵션 `keep_open`이면 채널당 연결 ≤ 4, 유휴 30 s로 닫는다.
   - 남의 스트림에 끼어들기(raw TCP 주입)는 하지 않는다.
   - 연결 실패는 `failed` + `E_NOT_OPEN`이다.
   - 가상 백엔드(sim/replay)에서는 실제 소켓 대신 SimTap TCP 합성으로 같은 스트림에 나타나게 한다.
4. **replay 메타 보존**: pcapng `opt_comment` `loom1 …`(X10)을 `RawFrame`까지 전달하고, replay 프레임의 `attributed_origin`·`link`·dir을 복원한다. 사용자 주석은 버리지 않는다.
5. 시험: 위 각각.
   - live는 이 Mac에서 열 수 없으므로 목록·판정 함수의 순수 부분 시험 + `#[ignore]` 실제 열기.
   - 모든 상한(C2)은 상수로 둔다.

## 관문·보고
- 관문: p1-common의 4개 명령(`--features live` clippy 포함)을 통과한다.
- 보고서: `/Users/ctmctm/workspace/Loom/docs/impl/eth/reports/p1b-sol.md`(40줄 이하, 결정·질문 목록). 커밋하지 않는다.
