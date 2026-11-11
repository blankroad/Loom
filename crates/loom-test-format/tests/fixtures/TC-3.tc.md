---
id: TC-3
title: 전원 사이클 부팅 시간
env: sim
equipment: [psu:bench, uart:dut, dlt:ecu1]
---
## 설명
전원 ON부터 `boot complete` DLT 로그까지 8초 이하. 5회 반복해 최대값으로 판정한다. 창에 손실이 있으면 inconclusive다.
## 단계
| 번호 | 동작 | 기대 결과 | 합격 기준 |
|---|---|---|---|
| 1 | PSU 전원 사이클·DLT 로그 대기 5회 | `boot complete` 관측 | 최대 부팅 시간 ≤ 8000 ms |
