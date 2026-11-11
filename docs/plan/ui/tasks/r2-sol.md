docs/plan/02-core-principle.md 준수.

# R2 — ui-sol 지시

## 먼저: R1 마무리 (짧게)
1. 30분 soak는 **750 s 지점에서 파일 시스템 정지(재부팅)로 중단**됐다. 재실행하지 말 것(새 규칙: 벤치마크 10분 이내). `spikes/ui/sol/RESULTS.md`와 `r1-sol.md`의 soak 서술을 "720 s 표본까지 완료, 750 s에서 외부 사유로 중단"으로 확정한다. 그 이후 `run.log`의 EPERM 오류는 정지 당시 부산물이라고 적는다.
2. 캐시를 프로젝트 밖으로 옮긴다: `spikes/ui/sol/.browsers`, `.cargo-home`, `.npm-cache` → `~/.cache/loom-ui-sol/` 아래로 **이동**(삭제 아님). RESULTS.md 재현 절차의 경로(`PLAYWRIGHT_BROWSERS_PATH` 등)를 새 위치로 고친다. `node_modules`는 그대로 둔다.

## 그다음: R2
`docs/plan/ui/tasks/r2-common.md`를 읽고 따른다. 리뷰 대상은 **`r1-opus.md` + `r1b-opus-xteam.md`**. 출력 `docs/plan/ui/r2-sol-review.md`.

추가로 볼 것:
- r1-opus의 지연 예산 표(§3.5)·메모리 상한(§6.4)·M1 수용 기준(UI-T01~T10)이 당신 스파이크 수치와 맞는지, 측정 가능하게 정의됐는지.
- r1-opus/r1b의 브라우저 동작 주장(xterm 키·질의 응답·decoration API, Permissions-Policy 디렉티브, Keyboard Lock, WebAuthn localhost, `chrome.debugger` WebAuthn)의 사실 확인 — 출처 첨부, 확인 못 하면 [미확인].
- CORE 의견(`core-ui.md`)의 전송 형식·큐 크기·`loom.columns.v1` 헤더가 당신 S1 결과와 맞는지, UI 쪽 decoder 계약에 필요한 것.
