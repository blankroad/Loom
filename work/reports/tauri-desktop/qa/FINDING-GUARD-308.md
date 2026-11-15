# FINDING (desktop, QA-owned) — UI identity guard refused its own fresh current daemon (HTTP 308). Fixed. 2026-11-15

## Observed (first native run on the final build, evidence/native-final-attempt1-308/)
- `smoke-native.sh start` (Loom-QA.app, fresh private home) → the bundled loomd started (pid 33336) → the app showed the Failed view:
  `/ui/index.html 읽기 실패: GET /ui/index.html returned HTTP 308`. The viewer never attached. The daemon was kept alive (not auto-stopped).
  Screenshot `wk/05-error-view.png` was opened and assessed.
- Root cause: loomd (`crates/loom-gateway/src/server.rs` ~L104) answers every GET outside `/rpc` and `/api/` whose
  `Host` is `127.0.0.1` with **308 → http://localhost:<port>…** (canonical origin). The new public GET in
  `desktop/host/src/http.rs` sent `Host: 127.0.0.1:<port>`. Bearer `/api/` calls are exempt, which is why they always worked.
  The unit stub server did not model this rule, so it slipped through. Manual check with the canonical host: `/ui/` 200 395 B ==
  ui/dist/index.html; `/ui/index.html` 200 (same); `/ui` 308.
- Second defect (UX truthfulness, from the same screenshot): an unreadable/non-200 response was worded as
  "다른 UI 빌드 / 이전 버전의 데몬" (a different UI build / an older daemon), which is false for a fresh current daemon.

## Fix (no guard relaxation, no redirect following)
- `host/src/http.rs`: public GETs send `Host: localhost:<port>` (the socket still connects to 127.0.0.1). The bearer request bytes are unchanged.
- `host/src/lib.rs` `daemon_ui_index`:
  - GETs **`/ui/`**, the exact document the window loads (`http://localhost:<port>/ui/#boot=…`);
  - any non-200, including 308, is an error;
  - the result is compared byte-exact with the staged index.
- `src-tauri/src/ui_identity.rs` + `main.rs`: Mismatch is split by cause. Both keep the daemon running and recordings preserved.
  - Different bytes → `mismatch_message` (different UI build, explicit stop command, Retry).
  - Unreadable → new `unreadable_message` ("UI 화면을 확인하지 못해 열지 않았습니다", Retry, log hint; never "이전 버전").
- Tests:
  - unit `uses_canonical_localhost_host`, `unreadable_is_not_called_an_old_build`;
  - **real-daemon integration** `real_loomd.rs` step 2b: `daemon_ui_index` against the actual loomd returns exactly the
    `--ui-dir` index bytes (so a stub-only pass is no longer possible).

## Results (evidence/final/desktop-targeted-tests.log)
- loom-desktop 16/16; host unit 4/4; real_loomd 1/1 (staged loomd 8ed2ef33…, CLI same pid). clippy clean, rustfmt (2024) clean.
- UI/daemon were not rebuilt (`stage-loomd --check` current). The rebuilt release dist + e2e app passed verify-dist/verify-bundle, and both embed the
  current index.html. Fingerprints are in evidence/final/fingerprints.txt.
- Native positive/negative re-proof: see STATUS.md (next run).
