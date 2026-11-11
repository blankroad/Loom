docs/plan/02-core-principle.md 준수.

# UI 팀 R2 공통 지시 — 교차 리뷰 (ui-opus, ui-sol)

작성: ui-lead, 2026-11-10 (재부팅 후). 한국어.

## 재부팅 공지 요약 (`docs/plan/05-resume-after-reboot.md`)
- 프로젝트 경로가 `/Users/ctmctm/workspace/Loom`으로 바뀌었다(옛 Desktop 경로는 링크). 새로 쓰는 문서·명령은 새 경로.
- 장시간 soak·벤치마크는 **10분 이내**. 브라우저·대형 바이너리·캐시는 **프로젝트 폴더 밖**(`~/.cache/...`).
- 파일 읽기가 30초 넘게 멈추면 기다리지 말고 작업을 멈추고 "BLOCKED: file io"라고 답한다.
- C4.1: DoIP는 PC 랜카드 + 미디어 컨버터, VN5640은 CAN FD 전용(UI 영향 거의 없음).

## 읽을 것
1. 상대 초안: `docs/plan/ui/r1-opus.md`, `r1-sol.md` (+ `spikes/ui/sol/RESULTS.md`)
2. `docs/plan/ui/r1b-opus-xteam.md` — 도메인 명세·CORE·TEST 초안과의 정합 점검, **§5 쟁점 15개**
3. R1 이후 새로 생긴 타팀 문서: `_xteam/core-ui.md`의 **`## core 의견`** 절(CORE의 답), `_xteam/explore-ui.md`, `_xteam/ui-test.md`, `_xteam/core-test.md`, `_xteam/test-domains.md`, `docs/plan/test/plan.md`(테스트 실행 화면 관련 절). 도메인 명세 `_xteam/can-ui.md`·`uart-ui.md`·`uds-ui.md`는 R1b가 요약했으니 필요한 부분만.

## 산출물
`docs/plan/ui/r2-<이름>-review.md` (ui-opus → `r2-opus-review.md`, ui-sol → `r2-sol-review.md`). 200~400줄, 표 위주.

1. **틀린 사실** — 상대 초안의 버전·API·수치·브라우저 동작 오류(근거 첨부).
2. **빠진 것** — 02 원칙 체크리스트·브리프 범위·타팀 요구(위 새 문서들) 중 상대 초안에 없는 것.
3. **더 나은 선택** — 상대안보다 나은 대안과 이유. 당신 초안이 틀렸다고 판단되면 그것도 적는다.
4. **합의 가능한 부분** — 두 초안이 같은 결론인 항목(짧게 목록).
5. **쟁점 의견** — r1b §5의 쟁점 1~15 각각에 동의/반대/수정안 한 줄 + 새 쟁점이 있으면 16번부터 추가. 특히 CORE 의견(`core-ui.md`)과 다른 지점.

규칙은 `tasks/r1-common.md` §4와 같다(쓰기는 자기 출력 파일과 `spikes/ui/<이름>/`만). 끝나면 "완료: <경로>"만 답한다.
