MERGED into main (47a792e, 11-11 19:48) — 이전: MERGE READY: aa74f0e — CAN M2a 앞당김 묶음 3(f9887b8 이후 추가): loom-can-export BLF/ASC(5c6a8e4), UI 데이터 질의 can.db.catalog·encode.preview·signal.query·signal.at(856e374), ISO-TP cancel·recv·barrier worker+gateway(856e374·0d4c13a). loom-can 시험 114·workspace fmt·clippy -D warnings 통과(0d4c13a 기준; aa74f0e는 과제 문서만). 다음: loom-can을 gateway/worker/cli feature로 나누기(CLI I1), can.open/demo(진행 중), 실제 loomd sim 연결·웹 E2E.
- 남은 P1 M1: CORE 런타임 main 병합 + 요청 10(domains.rs feature `can`) → 웹 CAN Trace E2E.
- CORE 요청 can-core.md 1–11(11 = E_CONFLICT 코드). UI 답변 can-ui.md §1–8.
- 커밋: 5c6a8e4 export, 856e374 UI 질의·ISO-TP worker 계약, 0d4c13a ISO-TP gateway 등록. 진행: can-opus = can.open/close/node RPC·loom can demo(M1), can-sol = can_sim 부하·메모리 시험(M1 ④⑧).
