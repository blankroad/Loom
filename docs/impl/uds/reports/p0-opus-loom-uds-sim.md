# P0 보고 — uds-opus · `crates/loom-uds-sim`

**만든 것**: 결정적 가상 UDS ECU 상태 모델과 결함 주입. 순수 crate(tokio·I/O·시계 없음, 시간은 인자로만 받는다). 의존은 `loom-uds-codec`, thiserror뿐이다.

**공개 API 요약**
- `SimEcu::handle(now: Duration, req) -> Replies` / `handle_addressed(now, req, Addressing)`. `Replies`는 `Reply{delay, bytes}`의 상한 있는 목록이다(`delay`는 요청 도착 시점 기준, 오름차순). 빈 목록은 무응답이다(SPRMIB, Drop, 기능 주소에서 억제된 NRC).
- 상태 조회: `session()`, `security_level()`, `did(id)`, `dtcs()`. 구성: `SimEcuConfig::bench()`(기본), `SimEcu::new(cfg)`가 검증한다.
- bench 프로필:
  - DID: `F190`="LOOMSIM0000000001"(읽기 전용), `F1A0`=[0x01](쓰기 가능), `F18C` serial, `F40D`=0. `F186`은 현재 세션을 동적으로 돌려준다.
  - DTC 2개. P2 50 ms / P2* 5000 ms, S3 5 s.
- 서비스별 동작:
  - 0x10: 01/03만 지원, 나머지는 0x12. 세션이 바뀌면 보안이 잠긴다. S3를 넘기면 다음 요청 때 01로 돌아간다.
  - 0x22: 다중 DID 가능. 없는 DID는 0x31, 길이 오류는 0x13, PDU 초과는 0x14.
  - 0x2E: 기본 세션이면 0x7F, 읽기 전용 DID는 0x31, 길이 변경은 0x13.
  - 0x19 02 마스크(다른 서브기능은 0x12), 0x14(없는 그룹은 0x31), 0x11 01–03(리셋 후 세션 01·보안 잠금).
  - 0x3E(SPRMIB면 무응답).
  - 0x27: seed 4바이트, key = seed XOR `SECURITY_XOR_DEFAULT`. 기본 세션이면 0x7F, seed 없이 key를 보내면 0x24, 틀린 key는 0x35, 3회째는 0x36과 10 s lockout(그동안 0x37). 이미 해제된 레벨의 seed 요청에는 0을 돌려준다.
  - 그 외 서비스(0x2A, 0x86, 0x31 포함)는 0x11.
- 기능 주소: ISO §7.5에 따라 NRC 0x11/0x12/0x31/0x7E/0x7F를 억제한다.
- 결함 주입 `FaultRule{matcher: FaultMatch{sid, id}, action, remaining}`(먼저 일치한 규칙 1개가 적용된다). `id`는 DID, RID 또는 bit7을 지운 서브기능이다.
  - `Delay(d)`: 응답 지연.
  - `ForceNrc(n)`: 요청을 실행하지 않고 NRC만 보낸다.
  - `PendingThen{count, every}`: 0x78 n회 후 최종 응답.
  - `Drop`: 실행은 하고 응답만 버린다. outcome_unknown 시험용.
  - `Late{after}`: P2를 지나 늦게 응답.
  - `WrongEcho`: 식별자 에코를 뒤집는다. codec 판정은 Foreign이 된다.
- `SimNetwork`: 논리 주소 → `SimEcu`. `handle_physical(addr, …) -> Option<Replies>`, `handle_functional(…) -> Vec<(addr, Replies)>`(응답 없는 ECU는 빠진다).

**C2 상한**
| 상수 | 값 | 넘칠 때 |
|---|---|---|
| `MAX_REPLIES_PER_REQUEST` | 64 | `PendingThen` 횟수를 63으로 자른다, push는 버린다 |
| `MAX_FAULT_RULES` | 32 | `add_fault` → `SimError::TooMany` |
| `MAX_SIM_ECUS` | 64 | 새 주소면 `insert` → `TooMany`(기존 주소 교체는 허용) |
| `MAX_SIM_DIDS` / `MAX_SIM_DTCS` | 256 / 256 | `new`/`validate` → `TooMany` |
| DID 데이터 | 1..=4092 B | `Config` 오류. 응답이 `MAX_PDU_LEN`을 넘으면 NRC 0x14 |

**시험**: 19개 모두 통과.
- services 9: 서비스별 긍정·부정, S3 만료, 보안 lockout, 리셋, 미지원 서비스, 구성 한도. 모든 긍정 응답을 codec `classify_response`로 교차 검증한다.
- faults 9: 결함별 응답 순서·시각, 규칙 소진, 상한, 네트워크 기능 주소 분배, 기능 주소 NRC 억제.
- fuzz 1: 무작위 요청 10만 회를 무작위 시각에 보낸다. panic 없음, 응답 수 상한 유지, 모든 응답이 자기 요청에 대해 Positive/Negative/Pending이다(정의되지 않은 SID나 너무 짧은 요청 제외).

관문: fmt --check OK · clippy `-D warnings` 경고 0 · test 19 passed.

**임시 정의·통합 시 교체**: 없음. 시나리오 YAML의 `sim:` 절은 loom-uds 연결 단계(P1)에서 이 타입으로 변환한다.
**CORE 추가 요청**: 없음. `loom-uds-codec`은 형제 crate라 path 의존으로 걸었다. 루트 `[workspace.dependencies]`에 넣을지는 리드가 판단한다.
**미해결**:
- 0x31 루틴은 기본 미지원(0x11)이다. plan의 "FF01 0x78×4" 시나리오는 결함 주입 `ForceNrc`/`PendingThen`으로만 만들 수 있다. 0x31 상태기가 필요하면 M3에 추가한다.
- 세션 02(programming)는 미지원이다.
