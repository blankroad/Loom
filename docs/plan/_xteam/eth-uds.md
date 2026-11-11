# ETH ↔ UDS: DoIP 디코드 · request 링크 · 코덱 공유 (초안 v1, explore-lead 통합 2026-11-10)

docs/plan/02-core-principle.md 준수. 정본: `docs/plan/eth/plan.md` §3.5–3.6. UDS는 `## uds 의견`을 덧붙여 주세요.

## 1. 코덱 공유 (no-IO)
- UDS `loom-uds` crate에서 **I/O가 없는 부분**을 분리해 eth가 의존할 수 있게 해 달라. 예: `loom-uds-codec` 또는 feature `codec-only`.
  - `doip::frame` — 헤더 파서(버전·역버전·payload type·length, **할당 전 길이 검사**), payload type별 구조체(0x0004 공지, 0x0005/0x0006 라우팅, 0x8001 진단, 0x8002 ACK, 0x8003 NACK, 0x0007/0x0008 alive check, 0x4001–0x4004 entity/power status).
  - `uds::decode(bytes, dir, db?) → {sid, service, positive, nrc, nrc_name, decoded}`. UDS 콘솔과 **같은 해석**을 쓴다.
- eth는 tokio 소켓·러너·정책에 의존하지 않는다. 코덱 버전은 두 도메인이 같은 workspace 빌드라 항상 일치한다.
- 진단 DB(DID 이름)는 UDS가 로드한 대상 DB를 eth 디코드에도 쓸 수 있으면 좋다(M2, 선택).

## 2. 링크 키 `wire_key`
UDS가 `uds.tx`·`uds.rx` payload에 넣어 달라:
```json
"wire_key":{"transport":"doip","local":"192.168.0.2:50112","remote":"192.168.0.10:13400",
            "sa":"0x0E80","ta":"0x1001","uds_sha256_8":"3fa1c2…","tcp_conn_gen":4}
```
- CAN-TP 대상은 해당 없다. CAN 쪽 ISO-TP 주석(V-UDS-5)과 같은 방식은 CAN 팀 몫이다.

## 3. 링커 `eth-link` (gateway 모듈, ETH 소유)
- 구독: `uds.tx`, `uds.rx`, `uds.timeout` + `eth.pkt`(doip 0x8001/0x8002/0x8003).
- **일치 규칙**:
  - 요청: 5-tuple + SA/TA + UDS 바이트 해시 + `eth ts ∈ [uds.tx.ts − 5 ms, + 2 s]` → `role: request`.
  - 그다음 같은 TCP 스트림의 ACK/NACK → `role: ack|nack`.
  - TA→SA 진단 메시지 중 응답 SID(긍정 `SID+0x40` 또는 `7F SID`)가 맞는 첫 것 → `role: response`.
    - `0x78` pending이면 계속 기다린다.
    - UDS 격리(D19 `late`)로 표시된 응답과 짝이면 `role: late`.
- 발행: `eth.link{eth_ref, request_id, role, confidence: exact|probable, reason?}`. 같은 키의 반복 요청(폴링) 때문에 모호하면 `probable`로 두고 UI 칩을 점선으로 그린다.
- **양방향 화면**: eth 행 칩 `↔UDS` → UDS 콘솔 `request_id` 행. UDS 행 "패킷 보기" → eth 패널 `loom.request_id == "<id>"` 필터.

## 4. 중복·정합 규칙
1. 같은 바이트를 두 도메인이 기록하는 것은 의도된 일이다.
   - `uds.tx` = 스택(소켓)에 넘긴 시점, `write_state: sent`.
   - eth 패킷 = NIC 캡처에서 관찰한 시점.
   
   둘을 합치지 않고 서로 다시 만들지 않는다. 링크만 둔다.
2. 판정·지연 계산은 각 도메인의 정의를 따른다. UDS P2는 UDS 타임스탬프로 계산한다. eth 시각은 참고로 표시만 한다(같은 host 시계, `ts.source` 다름).
3. **(M2 선택)** eth가 요청 패킷을 관찰하면 gateway가 UDS 행에 `tx_state: nic_observed`를 덧붙인다. 관찰이 없다고 UDS 상태를 낮추지는 않는다(캡처가 꺼져 있을 수 있음).
4. eth의 `attributed_origin`은 UDS 요청의 origin을 따른다. 링크된 응답 패킷은 `device`다.
5. 캡처에만 있고 UDS에 없는 진단 메시지(다른 테스터·CANoe·외부 도구)는 `external_actor: "unknown_tester"`로 표시하고 UDS 상태에는 반영하지 않는다. 대신 UDS 뷰에 "외부 진단 트래픽 감지" 경고를 띄운다(UDS §3.5-6과 같은 취지).

## 5. raw 송신 가드
`loom eth send raw|tcp`로 DoIP 0x8001을 만들면, eth는 **UDS `classify` 훅으로 내부 UDS 바이트의 등급을 계산해** 표시·감사한다(F1 기본 자동이지만 등급 표시는 정직하게). UDS가 classify 훅을 eth에서 부를 수 있게 공개해 달라.

## uds 의견
(uds-lead 작성란)

## 추가 (리드 통합)
- 공개 crate `doip-codec` 2.0.9·`simple_doip` 0.7.0(MIT 등)이 있다. 그래도 UDS 콘솔과 같은 해석을 위해 **UDS 코덱 no-IO crate를 공유**한다(UDS D26 존중). 공개 crate는 fuzz 교차 비교에만 쓴다.
- C4.1: DoIP는 PC NIC + 미디어 컨버터(OS TCP/IP)이므로 eth는 **같은 NIC를 관찰만** 한다. UDS 소켓과 캡처는 독립이다. UI와 `status`는 "DoIP 연결 OK / 캡처 불가(권한)"를 따로 표시한다.
- 한 UDS PDU가 여러 TCP 세그먼트에 걸칠 수 있다. 링크는 `request_id ↔ eth_ref[]`(목록)로 둔다.
