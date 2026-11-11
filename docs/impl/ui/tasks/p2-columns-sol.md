docs/plan/02-core-principle.md 준수.

# UI M2 선행 — ui-sol: `loom.columns.v1` 제안 + TS 디코더

작성: ui-lead, 2026-11-11. 규칙: `docs/impl/ui/tasks/p0-common.md` — 커밋 안 함, git stash 금지, **대화형 질문 금지**, 경로 `/Users/ctmctm/workspace/Loom/...`만. 작업 공간 `/Users/ctmctm/workspace/Loom/.worktrees/ui`. `-a never`: 네트워크 불가 — 이미 설치된 패키지만(`ui/node_modules`). 브라우저 시험 불가(Node vitest만).

## 배경
계약 01 §1.4: 고속 binary(`encoding:"binary"`, `loom.columns.v1`)는 **M2 초반에 확정**. CAN 팀은 M2a(PCAN FD 실HW·24 h soak) 전에 `can.frame` 열 블록이 있어야 한다고 했다. 형식 정본은 CORE 소유지만 UI가 소비자이므로 **제안서 + 디코더 + 골든 corpus**를 먼저 내 합의를 빠르게 한다.

## 근거
- `docs/plan/_xteam/core-ui.md`: v0.2 요약 CR-UI-01, **`## core 의견`의 `loom.columns.v1` 헤더 후보**(magic/version/schema_hash/sub_id/batch_id/stream_epoch/count/first_seq/last_seq/hub_publish_mono_ns/daemon_wall_ns/flags/lengths + 공통 메타 사전 + CAN 열)
- 당신의 R1 §6.2와 스파이크 `spikes/ui/sol/`(열 블록 24 B/행 측정), `docs/plan/ui/plan.md` §6.2-2·4(decoder 계약)
- CAN 스키마 v1 `docs/impl/requests/can-ui.md` §2(`can.frame` payload), UART `uart.data`(바이트 청크), 계약 02 §2.2(D3 필드)

## 만들 것
1. **제안서** `docs/impl/ui/proposals/loom-columns-v1.md`: 바이트 배치(LE), 헤더, 공통 메타 사전(session·domain·channel·kind·producer·client 문자열 사전), 열 정의(CAN frame, UART data 청크 — 최소 이 둘), seq 표현(delta/명시), nullable = validity bitmap, 가변 길이 = offsets+values, u64 처리, `display_gap` 요약, 최대 크기(64 KiB 프레임 전체), 버전·schema_hash 불일치 처리, **JSON(D3 레코드)과 손실 없는 왕복 규칙**(origin·seq·quality·request_id·source identity 보존). 결정마다 근거 한 줄.
2. **TS 디코더** `ui/src/codec/columns.ts`(새 디렉터리, net·proto 수정 금지): `decode(buf: ArrayBuffer) → { header, rows: LoomEvent[] view 또는 열 접근자 }` — 길이·offset·정렬·count·최대 메모리 검사 후 view, unknown version·잘린 offset·overflow는 **오류 반환(throw 아님 또는 명시 오류 타입)**, 최대 u64 BigInt 경계. 열 접근자로 행 객체 없이 읽는 경로 + 필요 시 D3 레코드로 펼치는 경로.
3. **골든 corpus + 퍼즈 시험** `ui/src/codec/columns.test.ts`: 손으로 만든 인코더(시험 전용, TS)로 골든 바이트 생성 → 디코드 → D3 JSON과 같음. 가변 DLC·FD 64 B·빈 배치·혼합 domain·최대 u64·request_id 있음/없음·gap 요약. 퍼즈: 무작위 잘림·비트 뒤집기 1만 회에서 크래시·무한 루프 없음(오류로 끝남).
4. 성능 메모(측정은 Node에서만): 10k 행 디코드 시간, 행 객체 펼치기 유무 비교. 브라우저 수치는 아님을 명시.

## 파일 범위
`ui/src/codec/**`, `docs/impl/ui/proposals/**`. 그 밖 수정 금지(ui-opus가 ui/src 나머지 작업 중).

## 보고
`docs/impl/ui/reports/p2-columns-sol.md`(제안 요약, 시험 수, CORE에 물을 것). 끝나면 "완료: <경로>"만.
