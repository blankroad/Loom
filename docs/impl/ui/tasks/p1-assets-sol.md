docs/plan/02-core-principle.md 준수.

# UI P1 — ui-sol: `crates/loom-ui-assets` (웹 UI 내장 제공)

작성: ui-lead, 2026-11-11. 규칙: `docs/impl/ui/tasks/p0-common.md` 그대로 — 커밋 안 함, git stash 금지, **대화형 질문 금지**(정해서 보고서에 적거나 질문을 쓰고 끝냄), 경로는 `/Users/ctmctm/workspace/Loom/...`만. 작업 공간 `/Users/ctmctm/workspace/Loom/.worktrees/ui`(브랜치 ui/main). 당신은 `-a never` 모드: 저장소 밖 쓰기·네트워크는 실패한다 — 의존성은 리드가 이미 `cargo fetch`했다(`rust-embed 8.13.0`, `mime_guess 2.0.5`, dev `tower 0.5`, `http-body-util 0.1`). 새 crate가 필요하면 보고서에 요청.

## 근거
`docs/contract/01-processes-ipc.md` §1.2(오리진 정본 `http://localhost:<port>`, 127.0.0.1 → 리디렉트, CSP·`Permissions-Policy: serial=(), usb=(), hid=(), bluetooth=()`·`Referrer-Policy: no-referrer`, `/ui/*` 정적), UI `docs/plan/ui/plan.md` §3.1·§4.3·§10.2. UI 빌드 산출물 `ui/dist`(Vite base `/ui/`).

## 만들 것 — `crates/loom-ui-assets` (뼈대 있음: Cargo.toml, 빈 lib.rs)
1. `rust-embed`로 `ui/dist`를 내장(`#[folder = "../../ui/dist"]`). **dist가 없으면 빌드가 깨지지 않게**: 빌드 스크립트 없이 처리할 방법을 고르고(예: rust-embed의 `debug-embed` 미사용 시 debug에선 런타임 파일 읽기, release에선 dist 필수 → `build.rs`로 dist 없으면 안내 페이지 하나만 내장) 결정을 보고서에.
2. `pub fn router() -> axum::Router`(CORE gateway가 `/ui`에 mount): `GET /ui/*path` 정적 파일(MIME, `ETag` = 내용 해시, `Cache-Control`: 해시된 `assets/*`는 immutable, `index.html`은 no-cache), 없는 경로는 SPA 대체로 `index.html`, `/ui` → `/ui/` 리디렉트.
3. `pub fn security_headers()`(axum middleware 또는 헤더 목록 함수): CSP(`default-src 'self'; connect-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; frame-ancestors 'none'; base-uri 'none'; form-action 'self'`), Permissions-Policy, Referrer-Policy, `X-Content-Type-Options: nosniff`. 빌드된 dist에 인라인 스크립트·스타일이 있으면 CSP와 충돌하니 확인하고(ui/dist를 `cd ui && pnpm build`로 만들어 볼 수 있다) 결과를 보고.
4. `pub fn canonical_origin_redirect(host, port, path) -> Option<Location>`: `127.0.0.1:<port>` 요청을 `http://localhost:<port><path>`로(WebAuthn RP ID). Host 허용 목록 검사는 CORE gateway 몫이니 이 함수는 순수 함수로.
5. 시험(`cargo test -p loom-ui-assets`): MIME·ETag·304, SPA 대체, 헤더 존재, 리디렉트, dist 없음 처리. 품질: `cargo fmt --check`, `cargo clippy -p loom-ui-assets -- -D warnings`.

## 파일 범위
`crates/loom-ui-assets/**`만. `ui/**`·다른 crate·루트 `Cargo.toml` 수정 금지(루트·`Cargo.lock` 변경이 필요하면 보고서에 — Cargo.lock은 cargo가 새 항목만 더하는 것은 허용).

## 보고
`docs/impl/ui/reports/p1-assets-sol.md` — 만든 API, 시험 결과, CSP 확인 결과, CORE gateway가 mount할 방법(예시 코드). 끝나면 "완료: <경로>"만.
