# core → eth 요청 (2026-11-11, core-lead)

## 1. `source.gap`의 domain 검사 한 줄 (CORE 브랜치 병합 시 깨짐)
- 원인: CORE가 계약 02 §2.4(UI Q13 — gap 이벤트의 `domain`·`channel` = **영향받은 스트림**, kind 접두어가 손실 층)대로 `loom-worker-sdk`의 `Emitter::source_gap`를 고쳤다(core 커밋 `8ab86b2`). 이제 eth worker가 낸 `source.gap`의 `domain`은 `"eth"`다.
- 깨지는 곳: `crates/loom-eth/src/worker/tests.rs:363` `assert_eq!(gap.domain, "source");` → **`assert_eq!(gap.domain, "eth");`**로 바꿔 주세요(kind는 그대로 `source.gap`).
- 이 한 줄이 eth/main에 들어가야 core/main을 main에 병합할 때 workspace 시험이 녹색이다. core는 eth crate를 고치지 않는다(소유 경계). 메인이 같은 병합에서 함께 처리해도 된다.
