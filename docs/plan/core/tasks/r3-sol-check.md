docs/plan/02-core-principle.md 준수

# CORE R3 사실 확인 — core-sol (짧게, 전체 10분 이내)

재부팅 메모: `docs/plan/05-resume-after-reboot.md`를 먼저 읽을 것. 경로는 `/Users/ctmctm/workspace/Loom`. 새 결정 C4.1: DoIP = PC 랜카드 + 미디어 컨버터(OS TCP/IP), VN5640은 CAN FD 전용.
**재발 방지 규칙**: 각 실행 10분 이내, cargo 빌드 산출물은 각 스파이크 폴더의 `target/` 안, 큰 다운로드 금지. 파일 읽기가 30초 넘게 멈추면 기다리지 말고 결과 파일에 그 사실만 적고 끝낸다.

리드가 R3에서 `plan.md`를 쓰는 중이다. 당신 리뷰(r2-sol-review.md) §7의 확인 질문 중 아래 두 가지만, 작게 실측해 달라. 나머지(Windows/Linux, full schema 4 client)는 구현 단계 관문으로 넘긴다.

## 1. P0 스키마 이중 표현 골든 (리뷰 §7-5, F13)
`spikes/core/r3-schema/` 새 cargo 프로젝트 (기존 `r2-contract`는 build.log만 남고 미완 — 필요하면 코드 재사용):
- `U64`(JSON = 10진 문자열, MessagePack = 정수), `Bytes`(JSON = hex 문자열, MessagePack = bin) newtype을 `Serializer::is_human_readable()`로 한 타입에서 구현. **fallible decoder**: 홀수 길이·비ASCII·비hex·상한 초과(예: 64 KiB)를 panic 없이 오류로.
- 축약 `EventRecord`(schema_version, seq, epoch, producer_id, producer_epoch, source_seq, ts{mono_ns,wall_ns}, dir: Option, origin enum, kind, payload: Option<Bytes>, data: Map) + `GapCount`(Exact(U64) | "unknown").
- schemars 1.x로 JSON Schema 내보내기 — 실제 `$schema` draft 값, U64/Bytes가 `{"type":"string","pattern":...}`로 나오는지, Option/enum/untagged 표현을 기록.
- 골든: 정상 5건(최대 u64, None 필드, 빈 bytes, Unicode data, unknown 필드 포함) JSON↔msgpack 왕복 일치, 비정상 8건(홀수 hex, 비hex, 비ASCII, 음수/소수 U64 문자열, 2^64, 상한 초과 bytes, 잘린 msgpack, 잘못된 enum)이 전부 Err.
- TS/Python 생성기는 **실행하지 않는다**(다운로드 금지). 생성기 입력이 될 schema 파일만 저장.

## 2. 감사 intent 동기 쓰기 비용 (리뷰 §7-6 축약)
`spikes/core/r3-audit/` (또는 같은 프로젝트의 두 번째 bin): append 전용 파일에 300 B JSON 레코드를 해시 체인(SHA-256)으로 1만 건 쓰기, 세 정책 비교 — (a) `write()`만, (b) 매 건 `sync_data`, (c) 100 ms 그룹 `sync_data`. 건당 p50/p95/p99 지연과 처리량. macOS `sync_data` = F_FULLFSYNC임을 결과에 명시.

## 출력
- `docs/plan/core/r3-sol-check.md`: 두 실험의 방법·결과 표·결론(각 5줄 이내)·한계. 그리고 맨 끝에 "plan.md에 넣을 권고" 5줄 이내.
- 원본 결과 JSON은 각 스파이크 폴더에.
- 끝나면 화면에 파일 경로와 3줄 요약만. 다른 에이전트에게 메시지 금지, 전역 설치·사용자 설정 수정 금지.
