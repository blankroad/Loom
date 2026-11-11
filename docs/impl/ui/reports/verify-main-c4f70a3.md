# main c4f70a3 UI 검증 (ui-lead, 2026-11-11)

- 대상: main `c4f70a3e64d2e772f68fb9b9b1f7cd61e91179bf` 그대로의 깨끗한 트리 — `git worktree add --detach ~/.cache/loom-verify/c4f70a3 c4f70a3`(미커밋 변경 0). main 코드는 수정하지 않았다.
- 환경: macOS(M1), Node v25.6.1, pnpm 11.11.0(`pnpm install --frozen-lockfile`), Playwright 1.65.0 Chromium(`~/.cache/ms-playwright`), 리드 셸(샌드박스 아님). 검증 전 포트 4790–4792의 고아 `loomd --fixture-replay`(리드의 이전 실행, ui worktree에서 기동)를 종료해 다른 트리의 서버가 재사용되지 않게 했다. ui-opus에게 검증 중 Playwright 실행 금지를 알렸다.
- 로그: `~/.cache/loom-verify/logs/*.log`

| # | 명령(`ui/`에서) | 결과 |
|---|---|---|
| 1 | `pnpm check` (check:schema + tsc + eslint + vitest) | **통과** — schema 32개 sha256 2d68ea8a…, vitest **33 파일 / 358 통과**, exit 0 |
| 2 | `pnpm e2e --workers=1` (기본 설정: smoke 2 + can.spec 2 + perf 3) | **4 통과 · 3 skip**(perf 3건은 `LOOM_PERF=1` 전용, 의도) · 실패 0, exit 0 |
| 3 | `pnpm e2e:mock --workers=1` | **미실행 — 리드 사용량 한도**. 직전 ui/main 기준 결과: 44 통과 · 1 skip(replay 전용) · T07 CSP는 `test.fail`(예상 실패, Vite dev에 CSP 없음) |
| 4 | `pnpm e2e:replay --workers=1` | **미실행 — 리드 사용량 한도**. 직전 ui/main 기준 결과: 11 통과(그중 2건은 CORE 차이 C-4·C-5로 `test.fail` 예상 실패) |

재개 시 3·4를 같은 트리(`~/.cache/loom-verify/c4f70a3/ui`)에서 실행하고 이 파일을 갱신한다. 끝나면 `git worktree remove ~/.cache/loom-verify/c4f70a3`.
