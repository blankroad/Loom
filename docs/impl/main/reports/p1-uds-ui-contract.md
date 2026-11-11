docs/plan/02-core-principle.md 준수

# P1 UDS 실제 payload → UI 정합 — READY_TO_INTEGRATE

작업: docs/impl/main/tasks/p1-uds-ui-contract.md · 2026-11-12 · Herdr foreground Claude Opus (하위 agent 없음)
기준: main 37feca4 + 기존 WIP 보존. commit/push 없음.

## 수정 파일 (소유 범위만)

- `ui/src/panels/uds/udsModel.ts` — canonical 우선 읽기, legacy 호환 유지
- `ui/src/panels/uds/consolePanel.ts` — detail/서비스 열/dry-run 미리보기
- `ui/src/panels/uds/uds.test.ts` — 실제 Rust fixture 기반 테스트 추가 (기존 테스트 변경 없음)

SDK/store/browser E2E/native/Cargo/index/mock 미수정. 메모리 soak 고정 binary 미접촉.

## 찾은 불일치와 수정 (정본: crates/loom-uds/src/payload.rs)

| 항목 | 실제 payload | 수정 전 UI | 수정 후 |
|---|---|---|---|
| link | `link{state,cause}` | 문자열만 읽음, 항상 `●` | up `●`, down/connecting `○ down(cause)`, 미보고 `?` |
| session | `session{id,name,confidence}` | `session.value`만 → `sess=?` | `name` 우선 (`sess=extended✓`), legacy `value` 유지 |
| security | `level: number\|null`, `confidence` | 문자열만 → 항상 `🔒` | null/0 `🔒`, 숫자 `L1🔓`, 확신도 기호 표시, 미보고 `?` (잠금으로 가정하지 않음) |
| TP | `tester_present{active,ok_count}\|null` | `tp{on}`만 → 항상 off | canonical 우선, null = off |
| lease | `{owner_origin,actor_id,client_id}\|null` | `actor`만, `나` 판정 불가 | `actor_id` 표시, `client_id === conn_id` 이면 `제어: 나` |
| 요청/응답 bytes | `bytes{encoding,data}`, `resp_bytes` | `req_hex/resp_hex`만 → 공란 | `22 F1 A1` / `7F 22 31`, 무응답은 `—`, hex 아닌 encoding은 원문 그대로 |
| NRC detail | `nrc: 49` (숫자) | `49` (10진 그대로) | `0x31 requestOutOfRange` |
| 서비스 열 fallback | `sid: 34` | `34` | `SID 0x22` |
| 차단 행 | `block{class,reason}`, `policy_action:'deny'` | `✗` (일반 실패) | `⛔ 거부` + detail `차단` 줄 |
| DTC | `dtcs[{dtc,status}]` | 표시 없음 | detail `DTC 923400 st=0x09 · C10000 st=0x2F` |
| rtt | float (`0.9`, `2010.0`) | 그대로 | 소수 1자리 반올림 |
| 등급 detail | envelope `risk` (+payload `risk`) | envelope만 | envelope 우선, 없으면 payload |
| dry-run | `DryRunResult{policy_action,block,parse_error}` | 거부/구문오류 미표시 | `⛔ 거부 …`, `구문 오류: … (위치 n)`, `{data}` envelope도 허용 |

성공 거짓표시 방지 유지: final NRC(≠0x78)는 write_state와 무관하게 `✗`, timeout `⌛`, write timeout `⌛? 불확정`,
pending 0x78은 실패가 아님. uds.rx(late 포함)는 행을 만들지 않음.

## 검증 (코드 unit — browser PASS 아님)

실 fixture 직접 로드: `crates/loom-uds/fixtures/{state,read_ok,pending_then_positive,nrc,read_timeout,write_outcome_unknown,sprmib,blocked_2a,dtc_list,late_response,dry_run}.json`
(Rust `tests/fixtures.rs`가 payload.rs 타입에서 생성한 파일, 새 mock 형태 아님). 이벤트 envelope은 events.rs와 동일하게
channel=target, request_id, row 이벤트의 risk만 감쌈.

- `pnpm exec vitest run src/panels/uds` — 20/20 PASS (신규 9)
- `pnpm exec vitest run` (UI 전체 unit) — 33 files / 371 tests PASS
- `pnpm exec tsc -b` — 0, `pnpm exec eslint src/panels/uds` — 0

실제 브라우저 렌더 검사는 하지 않음(자동승인 거부 상태) → main 별도 확인 필요.

## main 확인/결정 필요 (기본값으로 진행함)

1. TP 카운터는 기존대로 `ok_count + 접힌 3E 행 수`. 현재 backend에는 주기 TP 송신이 연결돼 있지 않아
   (`on_tester_present_ok`는 테스트에서만 호출) 이중 계산은 없음. 주기 TP가 행도 발행하게 되면 이중 계산 → 그때 조정 필요.
2. `security.level: 0`은 잠금으로 표시 (ISO 0x00 예약값, mock `tools/mock/sim.ts`가 0을 잠금 의미로 사용).
3. legacy mock(`tools/mock/sim.ts`)은 여전히 구형 state 형태를 발행 — UI가 둘 다 읽으므로 동작함. mock 정본화는 소유 밖.
4. `uds.rx` late 응답은 행에 반영하지 않음(정본 설계대로). late 표시 UI가 필요하면 별도 작업.
