---
id: TC-2
title: DLT 레벨 변경과 LA UART 물리 확인
env: sim
equipment: [dlt:ecu1, la:sal, uart:dut]
---
## 설명
CAM 컨텍스트를 debug로 올리고 콘솔에 `cam stat`을 보내면, DLT에 debug 로그가 2초 안에 나와야 하고, UART TX 선에 실제 `cam stat\r`이 115200으로 나가야 한다.
## 단계
| Step | Action | Expected | Pass criteria |
|---|---|---|---|
| 1 | CAM debug 레벨 설정 | effective_verified | DLT 제어 ACK |
| 2 | LA 캡처 시작 후 UART 송신 | `cam stat\r` | 동일 실행 단계 기록 |
| 3 | LA 디코드 대기 | UART TX 바이트 일치 | 115200 baud |
| 4 | DLT debug 로그 조회 | CAM debug 1건 이상 | UART 송신 후 ≤ 2 s |
