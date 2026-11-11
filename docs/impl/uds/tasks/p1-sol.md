docs/plan/02-core-principle.md 준수

# UDS P1 — uds-sol (DoIP·runner 독립 검토 + 부하·메모리 측정 crate)

공통 규칙: `docs/impl/uds/tasks/p0-common.md`(worktree `/Users/ctmctm/workspace/Loom/.worktrees/uds`, **질문 창 금지**, 커밋 금지, C2, 보고). 옛 Desktop 경로 금지.

상황: 네가 재시작되기 전에 리드 하위 작업자가 P0 sol 몫을 대신 만들었다 — `crates/loom-uds-doip`, `crates/loom-uds-runner`(커밋 fb28cf4, 보고서 `docs/impl/uds/reports/p0-sol-*.md`). 지금 **runner는 다른 하위 작업자가 우선순위·abort를 추가 중**(`tasks/p1-runner-sub.md`), **`crates/loom-uds`는 uds-opus가 P1-b로 고치는 중**이다. 이 셋은 **읽기만** 한다.

## 빌드 (샌드박스: 저장소 밖 쓰기 불가)
- `CARGO_HOME=.cargo-home CARGO_TARGET_DIR=target` + `--offline` (worktree 루트 기준, 둘 다 gitignore. 리드가 registry를 `.cargo-home`에 복사해 둠). 새 crate가 없어 받을 수 없으면 쓰지 말고 보고.
- 다른 작업자가 crate를 고치는 중이라 workspace 로드가 잠깐 실패하면 기다렸다 다시 한다. 남의 crate를 고치지 않는다.

## 1. 독립 검토 (코드 수정 없음)
네 v2 조사(`docs/plan/uds/v2-sol.md`, `spikes/uds/rust/`)와 plan §3.4·§3.8·D19 기준으로 `loom-uds-doip`·`loom-uds-runner`를 검토한다. 볼 것: ISO 13400 코드값·활성화·alive check, 길이 검사·할당, 상한 누락(무한 큐·무한 대기), 취소 안전성(tokio select/drop 시 상태 누수), P2/P2*/0x78 타이밍, quarantine 판정의 반례, `write_state` 매핑의 거짓 성공 가능성. 결과는 `docs/impl/uds/reports/p1-sol-review.md`: 심각도(P1 고쳐야 함 / P2 개선)·파일:줄·재현 조건·제안. 리드가 반영을 배정한다.

## 2. `crates/loom-uds-load` (새 crate, 네 소유 — plan U-30·M1 수용 (2)(3))
sim(`loom-uds-sim`) + `DoipEntity` + DoIP 클라이언트 + runner를 한 프로세스에서 돌리는 부하·측정 도구(`src/bin/uds-load.rs` + 라이브러리). sim 핸들러 어댑터는 이 crate 안에 자체로 둔다(`loom-uds`의 것을 기다리지 않음).
- 시나리오: (a) 대상 N개(기본 64) × 요청 주기(기본 20 Hz) 읽기 폴링, (b) 0x78·지연·Drop 결함 섞기, (c) 링크 끊김·재연결 반복.
- 측정: 요청→최종 응답 지연 p50/p95/p99/max(mono), 처리량, 오류 코드별 개수, **RSS 시계열**(외부 crate 없이 — 자기 pid로 `ps -o rss=`를 주기 호출하거나 OS별 분기), 시작 대비 RSS 증가.
- 출력: JSON 요약 1개(`--out <path>`, 기본 표준 출력). 실행 시간 인자(`--duration`), **개발 중 측정은 10분 이내**.
- C2: 결과 표본은 고정 크기 히스토그램(무한 Vec 금지).
- 관문 3개 + 단위 시험(히스토그램, 인자, 짧은 1–2 s 실행 smoke).
- 측정 1회: 64 × 20 Hz, 결함 섞기, 5분 → 결과 요약을 보고서에(macOS, release). M1 기준 참고: 30분 RSS 증가 ≤ 10 MiB — 5분 기울기로 외삽해 판정 근거로 적는다(외삽임을 표시).

## 보고
- `docs/impl/uds/reports/p1-sol-review.md`(검토, 60줄 이하)
- `docs/impl/uds/reports/p1-sol-load.md`(40줄 이하: API·사용법, 상한 표, 시험, 5분 측정 결과, 결정·질문)
둘 다 끝나면 멈춘다.
