docs/plan/02-core-principle.md 준수.

# UI P1 — ui-opus: `can.trace`를 CAN 스키마 v1 답변에 맞춤

작성: ui-lead, 2026-11-11. 규칙은 `p0-common.md` 그대로(커밋 안 함, **질문 창 금지** — 정해서 보고서에 적기, 경로는 `/Users/ctmctm/workspace/Loom/...`). 기준 ui/main `2aa7e59`.

근거: **`docs/impl/requests/can-ui.md`**(CAN 답변 §1–6). 요점:
- 송신 RPC = **`can.send`**(`can.tx.send` 아님). 인자 `{channel, frame:"300#01"}` 또는 구조 `{id, ext, fd, brs, esi, data}`(둘 중 하나), `count`, `gap_ms`, `expect`, `dry_run`. 결과 `data{request_id, write_state, source_seq, risk, reason}`.
- 쓰기 행 `can.tx`와 버스 에코 `can.frame(dir:"tx")`가 **같은 top `request_id`** → 한 줄, 상태 칸만 갱신, 시각·바이트는 에코 값. 다중 프레임이면 `payload.frame_index`마다 줄, 상태는 요청 단위.
- `can.frame` payload: `id`(숫자), `ext`, `fd`, `brs`, `esi`, `rtr`, **`dlc` = DLC 코드(0..15)**, **`len` = 바이트 수(0..64)**, `bytes{encoding:"hex", data}`, `device_ts?`, `isotp?`(M2 주석), `frame_index?`.
- 위험 등급 배지: 송신줄 입력이 바뀌면 디바운스해서 `can.send{…, dry_run:true}` → `{risk, reason, bytes, policy}`. M1 sim은 R1.
- 출처 색은 E8 통일안(이미 반영).

## 할 것
1. `ui/src/panels/can/**`: 송신 RPC 이름·인자, payload 읽기(dlc/len 구분 — 표시 열은 `len`, 툴팁에 DLC 코드, `rtr` 표시), 한 줄 병합 규칙(+ `frame_index`), dry-run 등급 미리보기, `ext` 표시. 기존 가정 주석을 v1 기준으로 고친다.
2. 시험 갱신·추가: 병합(같은 request_id의 can.tx + 에코), frame_index 다중 줄, dlc≠len(FD 12바이트 = DLC 9), dry-run 디바운스.
3. mock은 수정하지 마라(`ui/tools/**`는 서브에이전트 작업 중 — 리드가 같은 변경을 따로 전달함).

## 파일 범위
`ui/src/panels/can/**`만(필요하면 `ui/src/sdk/**`). 보고 `docs/impl/ui/reports/p1-can-v1.md`. 끝나면 "완료: <경로>"만.
