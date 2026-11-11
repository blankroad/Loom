# UI P1 — loom-ui-assets 구현 보고

작성: ui-sol, 2026-11-11. `docs/plan/02-core-principle.md` 준수.
작업 공간: `/Users/ctmctm/workspace/Loom/.worktrees/ui`, 브랜치 `ui/main`.
코드는 `crates/loom-ui-assets/**`만 수정했다. 커밋·stash·네트워크·신규 crate 추가 없음. 루트 Cargo.toml·Cargo.lock 변경 없음.

## 1. 구현 API

| API | 계약 |
|---|---|
| `pub fn router() -> axum::Router` | `/ui` → `/ui/` 308(query 보존), `/ui/`·`/ui/{*path}` GET/HEAD. 다른 gateway 경로는 처리하지 않음 |
| `pub fn security_headers() -> HeaderMap` | CSP·Permissions-Policy·Referrer-Policy·nosniff 헤더 목록. UI 라우터에는 이미 middleware로 적용됨 |
| `pub fn canonical_origin_redirect(host: &str, port: u16, path: &str) -> Option<Location>` | 정확한 `127.0.0.1:<port>` authority를 `http://localhost:<port><path>`로 변환. `Location = HeaderValue` |

- 정적 파일 MIME은 `mime_guess`로 판정한다. text MIME에는 UTF-8 charset을 붙인다.
- ETag는 rust-embed metadata의 **콘텐츠 SHA-256**, 따옴표로 감싼 64자리 소문자 hex다.
- `If-None-Match` strong/weak·목록·`*`를 처리하고 304에는 본문이 없다. 304에도 ETag·cache·보안 헤더를 유지한다.
- Vite 기본 `assets/<이름>-<8자 base64url hash>.<확장자>`만 `public, max-age=31536000, immutable`.
- index·일반 파일은 `no-cache`. 없는 `/ui/*` 경로는 실제 index의 MIME·ETag·`no-cache`로 대체한다. 없는 해시형 asset 경로가 HTML로 대체되어도 immutable로 캐시하지 않는다.
- GET과 HEAD를 지원하고 그 외 메서드는 405. traversal·백슬래시·NUL 경로는 400.
- Host/Origin allowlist·인증은 CORE 책임이다. canonical helper의 `None`을 허용 판정으로 해석하면 안 된다. query와 percent encoding을 보존하고 CR/LF·scheme-relative·absolute URL 입력은 거부한다.

## 2. 결정: dist 없음 — 빌드 스크립트 없이 처리

`rust-embed 8.13.0`의 설치된 macro 소스에서 **`#[allow_missing = true]`** 지원을 확인했다.

```rust
#[derive(rust_embed::Embed)]
#[folder = "../../ui/dist"]
#[allow_missing = true]
struct Distribution;
```

- `build.rs`·새 build dependency·dist 생성/복사 과정이 없다.
- crate 내 `fallback/index.html`은 별도 Embed로 항상 내장한다.
- distribution에 index가 없으면 router 전체가 안내 페이지 source를 선택한다. 빈 dist·일부 asset만 있는 dist도 안내 페이지가 된다.
- 안내에는 `cd ui && pnpm build` 후 loomd 재빌드 절차와 **실시간 데이터 표시가 아니라는 문구**가 있다. HTTP 200·`no-cache`로 제공하며 UI 기능 수용 시험 통과를 뜻하지 않는다.
- `debug-embed`를 켜 debug/release 모두 내장 바이트를 사용한다. 요청 시 파일시스템 읽기·무한 cache·추가 큐가 없고 응답 본문은 static bytes를 참조한다.
- UI 변경 반영 순서는 **UI build → loomd/crate rebuild**다. debug에서도 디스크 파일 hot reload는 하지 않는다.
- 테스트에서 실제로 존재하지 않는 폴더에 동일 `allow_missing` derive를 적용해 컴파일과 안내 응답을 검증했다. 공유 `ui/dist`를 삭제·이동하는 시험은 하지 않았다.

## 3. 시험 결과

| 명령 | 결과 |
|---|---|
| `cargo test --offline -p loom-ui-assets` | **11개 통과**, 실패 0 |
| `cargo fmt -p loom-ui-assets -- --check` | 통과(다른 팀 crate를 고치지 않도록 package 한정) |
| `cargo clippy --offline -p loom-ui-assets --all-targets -- -D warnings` | 통과, 테스트 target 포함 |
| `git diff --check -- crates/loom-ui-assets` | 통과 |
| `cd ui && pnpm build` | 통과, Vite 8.3.7. 큰 chunk 경고 1개(실패 아님) |
| `cd ui && pnpm check` | schema drift·typecheck·lint·Vitest 통과, **30 files / 317 tests** |

Rust 시험: MIME·해시 자산/일반 자산 cache·독립 SHA-256 기준값·304 strong/weak/list/wildcard·ETag 차이·SPA 대체·missing dist·HEAD·보안 헤더·308 query 보존·traversal/405·gateway merge·순수 canonical redirect·Vite hash 문법.

UI 시험은 jsdom의 Canvas getContext 미구현 메시지를 출력했지만 모두 통과했다. 브라우저 렌더/CSP 실제 적용 시험과는 구분한다. 실행 시점에 다른 UI 워커의 변경이 함께 존재했으며 해당 소스 파일은 수정하지 않았다.

## 4. CSP 확인 결과·남은 통합 사항

요구한 헤더를 약화하지 않고 그대로 적용했다.

```text
Content-Security-Policy: default-src 'self'; connect-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; frame-ancestors 'none'; base-uri 'none'; form-action 'self'
Permissions-Policy: serial=(), usb=(), hid=(), bluetooth=()
Referrer-Policy: no-referrer
X-Content-Type-Options: nosniff
```

실제 `pnpm build` 후 `ui/dist/index.html` 검사:

- 외부 module script 1개: `/ui/assets/index-DxckHjaE.js`.
- 외부 CSS link: `/ui/assets/index-xMbgRxX7.css`.
- 인라인 script 본문 0개, `<style>` 0개, `style=` 속성 0개.
- Worker도 `/ui/assets/net.worker-BleoIIWk.js`의 같은 origin 파일이다.
- fallback HTML에도 script/style이 없다. **정적 HTML 검사 통과**.

**정적 검사와 별개로 UI 리드 확인 필요:** 설치된 xterm 6 소스와 생성 JS bundle에 런타임 inline style 생성 코드가 있다.

- `ui/node_modules/@xterm/xterm/src/browser/Viewport.ts:79–83`: `<style>` 생성·추가 후 theme CSS를 textContent로 설정.
- `ui/node_modules/@xterm/xterm/src/browser/renderer/dom/DomRenderer.ts:138–158`: dimensions/theme style 생성.
- 실제 `index-DxckHjaE.js`에도 `_styleElement.textContent`, scrollbar theme CSS, DOM theme CSS가 남아 있음을 확인했다.

따라서 **index.html에 inline이 없다는 결과만으로 UART까지 CSP 호환이라고 판단하지 않는다**. 요청된 `style-src 'self'`와 이 런타임 style 경로의 충돌 가능성이 있어, UI 리드가 실제 브라우저 CSP violation/터미널 표시를 확인하고 CSP를 지키는 style 주입 방식을 정해야 한다. 이 작업에서는 브라우저 런타임 검증을 수행하거나 UI 코드를 수정하거나 `'unsafe-inline'`을 추가하지 않았다. 해결을 위해 새 crate는 필요하지 않다.

## 5. CORE gateway mount 예시

반환 router는 `/ui` prefix를 이미 소유하므로 **`merge`** 한다. `nest("/ui", router())`로 중복 prefix를 만들지 않는다. gateway 상태가 있다면 먼저 `with_state(state)`로 제공한 뒤 merge할 수 있다.

```rust
use axum::{Router, routing::get};

let gateway: Router = Router::new()
    .route("/health", get(|| async { "ok" }));
let app = gateway.merge(loom_ui_assets::router());
// 기존 /rpc·/api/v1/* routes도 gateway에 두면 그대로 유지된다.
```

Host/Origin 검사 이후 canonical redirect를 적용하는 CORE middleware 내부 예시:

```rust
use axum::{http::{header, StatusCode}, response::IntoResponse};

// port = 실제 loopback listener port, req = Host/Origin 검증을 통과한 요청.
let host = req.headers().get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("");
let path = req.uri().path_and_query().map(|v| v.as_str()).unwrap_or("/");
if let Some(location) = loom_ui_assets::canonical_origin_redirect(host, port, path) {
    let mut response = StatusCode::PERMANENT_REDIRECT.into_response();
    response.headers_mut().insert(header::LOCATION, location);
    response.headers_mut().extend(loom_ui_assets::security_headers());
    return response;
}
// 나머지는 next.run(req).await. 다른 gateway 응답에도 같은 보안 헤더 적용.
```

canonical helper는 Host를 읽거나 포트·프로세스 상태를 조회하지 않는 순수 함수다. `127.0.0.1:<다른 port>`·localhost·IPv6·다른 host는 None이며 별도 허용 여부는 CORE가 결정한다.
