docs/plan/02-core-principle.md 준수 (최우선)

# v2.0 검증 — uart-sol (20분 내외, 수정 금지·점검만)
리드가 docs/plan/uart/plan.md 를 v2.0으로 통합했다(네 v2-sol.md와 uart-opus의 v2-opus.md 반영). 기준: docs/plan/00-master-plan.md §2.3 C1–C8(우선), docs/plan/04-v2-tasks.md, 네 v2-sol.md.
1. C1·C2·C3·C8 중 UART 해당 항목 준수/위반/누락 + 위치 + 고칠 문안.
2. 네 사실(§7, §9.4, §3.13, §9.5)이 plan.md 다른 절과 모순되는 곳: 특히 §3.5·§3.11·§4.5·§8·§8.1·§9.1·§11·§12·§13에 남은 pyserial/Python 시절 서술, 수치 불일치(큐 상한·메모리·구독 상한 DR-V21·웹 패널 메모리 DR-V22), regex 선형성 서술 과장.
3. 리드 결정 DR-V21(구독 채널당 4·전체 16)과 DR-V22(패널 2 MiB 등)가 네 예산과 맞는지.
출력: docs/plan/uart/v2-check-sol.md, 맨 끝 "리드 수정 목록"(필수/권장, 각 항목에 plan.md 위치와 대체 문안). 짧게. 완료 시 "V2 CHECK DONE" 한 줄.
