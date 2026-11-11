# VN5640 이더넷 위 DoIP 경로 — CAN ↔ UDS 합의 문서 (마스터 §2.3 C4)

docs/plan/02-core-principle.md 준수.

- 소유(C4): Vector XL 바인딩 crate(CAN FD + 이더넷 원시 프레임) = **CAN 팀**, DoIP 전송 = **UDS 팀**. 초안은 CAN 팀이 쓰기로 했다(`04-v2-tasks.md`). 2026-11-10까지 CAN 초안이 없어 uds-lead가 파일만 먼저 만들고 의견을 적었다. **CAN 팀은 아래 `## can 초안`에 자유롭게 쓰고, 이 머리말과 구조는 바꿔도 된다.**
- 원칙(02): 어느 경로든 에이전트의 DoIP 요청·ECU 응답이 같은 순간 Loom UI UDS 콘솔에 보여야 하고, 사용자 개입(STOP·takeover·승인)이 같은 파이프라인을 지나야 한다. 경로 선택은 그 지연·정직성 기준(응답 → UI p95 ≤ 100 ms, gap 표시)으로 판정한다.

## can 초안
(can-lead 작성란)

## uds 의견 (uds-lead, 2026-11-10)

근거: UDS `plan.md` v2.0 §8.2·D27, 조사 `docs/plan/uds/v2-sol.md` §4. 출처는 Vector VN5000 Hardware Manual v6.1 §2.1.3·§14.2–14.4와 Vector OS 지원표(2026-05-26)이고, 실측은 없다.

| 경로 | UDS 판단 | CAN 팀에 확인·요청 |
|---|---|---|
| (a) VN5640을 OS 네트워크 어댑터로 노출 → OS TCP/IP | **미확인**. 공개 매뉴얼에서 일반 NIC로 노출하는 설정을 찾지 못했다. `ETH16 Uplink`의 Ethernet Host는 측정 호스트 링크이고, `NetworkDriver.msi`는 PC NIC를 CANoe에 쓰는 드라이버다. 기본 경로로 약속하지 않는다 | 사용자 라이선스·드라이버(Vector Hardware Config/VHM)에서 이 모드가 실제로 있는지 확인 |
| (b) XL 원시 이더넷 프레임 → 사용자 공간 TCP/IP(`smoltcp` 0.14) → DoIP | 후보(Windows 전용, 구현량 큼). TCP/IP·DoIP는 UDS가 맡는다 | 프레임 인터페이스: bounded 송수신 큐(개수·바이트 상한), 단일 poll 소유자, 링크 up/down·속도 이벤트, HW/host 타임스탬프와 품질, MTU·VLAN·필터 설정, 송신 확인. 프레임 RPC는 CAN worker 안(XL DLL 격리, C2). USB/FFI/IPC 왕복 지연 p50/p99 실측 |
| (c) PC NIC → **VN5640 내장 독립형 미디어 컨버터**(VHM 설정 후 Deploy) → 100/1000BASE-T1 ECU | **조건부 기본 권장**. PC 쪽은 OS TCP/IP이므로 UDS 자체 DoIP 전송을 그대로 쓴다. Linux 사용은 설계 추론이며 미확인 | 컨버터 Deploy 상태에서 같은 VN5640의 CAN FD 채널을 CAN worker가 동시에 쓸 수 있는지. CANoe와 같은 포트를 쓰는 경우의 소유 규칙. activation line 제어 가능 여부 |

- **결정 제안**: M2 HIL에서 (c)를 Windows·Linux로 먼저 실측한다. 확인 항목은 VIN 읽기, 허가된 쓰기·되읽기 → 웹 UI, UDP 발견·다중 ECU, link flap, CAN FD 동시 부하다. (c)가 안 되는 벤치이거나, 같은 포트로 버스 관찰이 필요하면 (b) 스파이크(UDS U-31)로 간다. (a)는 Vector 확인이 있을 때만 쓴다.
- **화면 정직성**: 경로 (b)·(c)를 UDS 상태 바·결과에 `backend: doip` + `link: vn5640-conv|vn5640-xl`로 표시한다. (b)에서는 타임스탬프 출처(XL HW/host)를 `ts.source`에 그대로 쓴다.
- **벤치 준비와 에이전트 동작 구분**: VHM 설정·Deploy·activation line은 사람의 사전 작업이다(UDS §8.1 체크리스트). 에이전트가 반복하는 connect·read·write와 섞지 않는다.
- **CAN에 묻는 것**:
  1. XL 이더넷 원시 프레임 API를 바인딩 crate v0 범위에 넣는가, 넣는다면 시기는(UDS U-31 의존)?
  2. 사용자 VN5640의 1000BASE-T1 옵션 유무와 대상 ECU PHY를 아는가(UDS Q13)?
  3. 사용자가 쓰는 pyvxl 판으로 (b)의 이더넷 프레임 송수신을 교차 검증할 수 있는가(UDS Q14)?

## 메인 확정 (2026-11-10, 사용자 확인 → 마스터 C4.1)
- 사용자는 **PC 랜카드 + 미디어 컨버터**로 DoIP를 이미 쓰고 있다. **경로 (c) 확정** — PC 쪽은 OS TCP/IP, UDS 자체 DoIP 전송 그대로.
- 사용자: "pyvxl은 캔만... 벡터에서 이더넷 지원을 안 하는 것으로 알고 있음" → **경로 (b) XL 원시 이더넷 + `smoltcp`는 범위 밖**, UDS U-31 스파이크 취소. 경로 (a)도 쓰지 않는다.
- CAN Vector XL 바인딩 crate는 CAN/CAN FD API만. CAN 질문 1·3 닫힘. 질문 2(1000BASE-T1·PHY)는 컨버터 쪽 사람 설정 문제로 체크리스트에만 남긴다. `## can 초안`은 쓰지 않아도 된다.
