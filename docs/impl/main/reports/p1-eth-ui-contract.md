# P1 ETH canonical payload → UI 정합 — 반환 보고

상태: **완료 (jsdom 단위 검증까지)**. 실브라우저 검증은 **미실시** — browser 권한 거부 미해결이므로 아래 결과를 실브라우저 PASS로 기록하지 말 것.

## 변경 범위 (소유 파일만)

- `ui/src/panels/eth/ethModel.ts`
- `ui/src/panels/eth/ethPanel.ts`
- `ui/src/panels/eth/eth.test.ts`

다른 소스 / Cargo / lock / index / commit / push 없음. soak binary 미접촉.

## 확인한 실제 계약 (`crates/loom-eth/src/model.rs`, `worker/pipeline.rs`, `gateway.rs`)

| payload | 필드 | 실제 | 수정 전 UI |
|---|---|---|---|
| `eth.capture_state` | `backend` | `live\|replay\|sim` | `sim: boolean`만 읽음 → SIM 표시 누락 |
| | `buffer_bytes_requested` / `buffer_bytes_actual?` | U64 문자열, actual은 OS 미보고 시 필드 없음 | `buffer_bytes` 숫자만 읽음 |
| | `state` | snake_case (`permission_denied` 등) | `'PERMISSION'`만 비교 → 권한 배너 안 뜸 |
| | `error?` | 문자열 | 미표시 |
| `eth.stats` | `pps`/`bps`/`decode_lag_ms`/`shown`/`total` | U64 문자열 | `typeof number`만 → 항상 `?` |
| | `kernel_drop?`/`if_drop?` | U64 문자열, None = 알 수 없음 | 미사용 |
| `source.gap` | `count` | U64 문자열 또는 `"unknown"`; `stats_unavailable` cause 존재 | `Number()` 변환(2^53 초과 시 부정확), `stats_unavailable` 무시 |

## 수정 내용

1. **canonical 우선 + legacy 호환** — `readCaptureState`: `backend` → `sim`, U64 버퍼 2종을 정확한 bigint로, state 소문자 정규화(legacy `PERMISSION`/`LIVE` 수용). 요청값만 있고 actual 없으면 actual=null(요청값으로 대체하지 않음). dev mock(`ui/tools/mock/eth.ts`)의 legacy 모양도 계속 읽음.
2. **stats** — 새 `readStats`가 U64 문자열/legacy 안전정수 모두 읽음. pps는 정확한 정수(천단위 구분), bps는 bigint 기반 축약(k/M/G, 소수 1자리).
3. **거짓 0 방지 (드롭)** — 표시값 = `max(source.gap 누계, stats 초당 누계)` (같은 드라이버 카운터라 합산하지 않음). 다음이면 `커널 드롭: 알 수 없음`(누계 0일 때) 또는 `커널 드롭 ≥N`(하한):
   capture_state 미수신, `drop_stats: unavailable`, 최근 stats에 kernel_drop·if_drop 모두 없음, 한쪽만 보고됨, `partial`, count `unknown`인 드롭 gap, `stats_unavailable` gap.
4. **U64 안전 경계** — U64 > u64::MAX 또는 2^53 초과 JS number는 추정하지 않고 무시(직전 값 유지 / unknown). gap count는 bigint로 보존해 행 문구에도 정확한 값 표시.
5. **같은 화면 불일치** — batch 요약 전에도 stats `shown/total`(같은 1초 창)로 샘플링 배지를 갱신 → 상태바는 대량 수신인데 배지는 `표시 1/1`인 불일치 제거. `permission_denied` → 권한 배너 + 기존 CSS 훅(`data-state="PERMISSION"`) 유지. `error` 상태면 도메인 오류 문구 표시.

## 검증

- `vitest run src/panels/eth` — 27/27 PASS (신규 13: Rust serde 출력과 필드 단위로 같은 canonical payload 사용 — 기존 fixture에 canonical 모양이 없어 테스트 내 `canonCapture`/`canonStats` 정의).
- `vitest run` 전체 UI — 33 files / 384 tests PASS.
- `pnpm typecheck` PASS, `eslint src/panels/eth` PASS.
- **jsdom 전용**. 실브라우저 미검증.

## 자체 리뷰 / 남은 사항 (타 소유)

- **ETH 계약 불일치**: `gateway.rs` 스키마는 `/payload/bps` 단위를 `"B/s"`로 선언하지만 `pipeline.rs`는 bytes×8(bits/s)을 보냄. UI는 값 그대로 `bps`(bits)로 표시. ETH 소유자가 단위 선언 또는 값 중 하나로 정리 필요.
- `ui/src/shell/shell.css`의 `.eth-status[data-state='PERMISSION']` 은 legacy 이름 — UI가 호환 값으로 넘기므로 지금은 동작. 정리 시 `permission_denied`로 바꿀 수 있음(shell 소유).
- `ui/tools/mock/eth.ts` 는 여전히 legacy 모양 송출(동작함). canonical로 바꾸면 dev 화면도 실제와 같아짐(소유 밖).
- 버퍼 크기(requested/actual)는 모델에서 정확히 읽지만 화면 표시는 추가하지 않음(최소수정 범위).
- 상태 표시는 state를 대문자로 표시(`LIVE`, `PERMISSION_DENIED`) — 기존 화면과 동일한 형태 유지.

## 메인 통합 보완

- None인 통계 구간이 지난 뒤 정상 카운터가 회복돼도 누계는 하한으로 남긴다. `unknown → known` 회귀를 추가했고 ETH 28개 단위 검사가 통과했다(`/private/tmp/loom-p1-eth-recovery-test.log`).
- gateway `/payload/bps` 단위 선언을 실제 값과 일치하는 `bit/s`로 바로잡았다. 전송 값은 바꾸지 않았다.
