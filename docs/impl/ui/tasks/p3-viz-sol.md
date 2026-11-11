docs/plan/02-core-principle.md 준수.

# UI P3 선행 — ui-sol: `crates/loom-viz` 데이터셋 엔진 (데몬 쪽, 순수 Rust)

작성: ui-lead, 2026-11-11. 규칙: `docs/impl/ui/tasks/p0-common.md` — 커밋 안 함, git stash 금지, **대화형 질문 금지**(정해서 보고서에, 못 정하면 질문을 쓰고 끝냄), 경로 `/Users/ctmctm/workspace/Loom/...`만. 작업 공간 `/Users/ctmctm/workspace/Loom/.worktrees/ui`. `-a never`: 네트워크 불가 → **Cargo.lock에 이미 있는 crate만** 쓴다(`cargo tree --offline`로 확인). 없으면 직접 구현하거나 보고서에 요청.

## 근거
UI `docs/plan/ui/plan.md` **§4.1.1(`loom viz` 명령·자동 렌더 규칙)**, **§6.3.1(데이터셋 계약)**, UD-29(처리 위치 = 데몬 `loom-viz` crate), `docs/plan/_xteam/viz-requirements.md`(UART·UDS 요구). C2: 모든 버퍼 상한.

## 만들 것 (M3 선행 — gateway 연결·CLI는 나중, 여기선 라이브러리 + 시험)
1. **데이터셋 모델**: series(이름·단위·value_type number|enum|bool|string·enum_map·time_source·quality·source{domain,channel,seq,rx_offset?,request_id?}) + 샘플(ts_ns u64, value, quality). 메모리 상한(데이터셋당·전체 바이트) + 넘칠 때 정책.
2. **ingest**: long JSONL/CSV(`ts, series, value, unit, quality, source`), wide CSV(시간 열 탐지: `ts|time|timestamp`·ISO8601·epoch s·`{mono_ns,wall_ns}`·단조 수치), 공통 이벤트 envelope JSONL의 `metrics[]`. 스트리밍(한 줄씩), 잘못된 줄은 위치와 함께 오류 집계(조용히 버리지 않음).
3. **자동 종류 판정**(`--kind auto`, plan §4.1.1 규칙 ②–⑤): 시간 열·수치 → line, 정수 & 고유값 ≤ 16 → step, 문자열/enum 고유값 ≤ 32 → state, 그 외 table, 수치 열 > 8이면 8개만 기본 표시.
4. **축소 질의** `query(series[], t0, t1, px, mode:minmax)` → series당 ≤ min(4·px, 20000) 점, first/last/min/max를 시각 순서로(극값 보존, LTTB 아님). **gap·quality≠valid 구간은 끊긴 구간으로 표시**(보간 금지). 원본 개수·반환 개수 함께. 큰 데이터는 min/max 피라미드(층별 고정 크기)로.
5. **summary**(원본 기준): series별 min/max/mean/count/구간, gap 구간 수.
6. 시험: 각 입력 형식, 시간 열 탐지, 자동 판정, minmax가 극값을 보존(스파이크 1점), gap에서 선 끊김, 상한 초과 정책, 잘못된 줄 오류 위치. `cargo test --offline -p loom-viz`, `cargo clippy --offline -p loom-viz --all-targets -- -D warnings`, `cargo fmt -p loom-viz -- --check`.

## 파일 범위
`crates/loom-viz/**`만(새로 만든다). 루트 `Cargo.toml`은 `members = ["crates/*"]`라 수정 불필요. 다른 crate·`ui/**` 수정 금지.

## 보고
`docs/impl/ui/reports/p3-viz-sol.md` — API, 시험 결과, 결정 사항, 필요한 crate 요청. 끝나면 "완료: <경로>"만.
