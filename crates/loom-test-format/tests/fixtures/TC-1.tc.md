---
loom_tc: 1
id: TC-1
title: 콜드 부팅 스모크
env: sim
equipment: [psu:bench, uart:dut, can:ch0, uds:bcm]
preconditions: ["DUT 전원 OFF", "BCM DTC 임의 상태"]
tags: [smoke, boot]
---
## 설명
전원을 껐다 켠 뒤 30초 안에 콘솔에 `login:`이 떠야 한다. 부팅 후 5초 안에 CAN에서 PowerMode가 2(RUN)가 되어야 한다. BCM DTC(mask 09)가 없어야 하고, 있으면 지운 뒤 다시 읽어 없어야 한다.

## 단계
| # | 동작 | 기대 결과 | 합격 기준 |
|---|---|---|---|
| 1 | 전원 OFF 2초 후 ON | — | — |
| 2 | 콘솔 관찰 | `login:` 표시 | 전원 ON 후 ≤ 30 s |
| 3 | CAN 관찰 | PowerMode = 2 | 부팅 후 ≤ 5 s |
| 4 | BCM DTC 읽기; 있으면 삭제 후 재확인 | mask 09 DTC 없음 | 완전한 요청·응답 증거 |
