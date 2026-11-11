# ETH R1 — explore-opus: 계획 초안 (구조·CLI·UI·테스트)

새 도메인 `eth`(이더넷 패킷)를 1차(P1 M1)에 넣는다. 사용자 지시는 이렇다: "UI는 와이어샤크나 tshark 사용하는 것처럼 에이전트가 실시간으로 패킷을 볼 수 있어야 함." 계획만 쓰고, 한국어로 쓰고, 구현 코드는 쓰지 않는다.

## 먼저 읽기
- `docs/plan/eth/brief.md`: 범위 1–7과 협의 파일이 요구사항이다.
- `docs/plan/02-core-principle.md`
- `docs/plan/00-master-plan.md`:
  - §2.3 C1–C8
  - §2.4 E1–E12: 종료 코드 E2·E3, 저널 E5, 출처 색 E8
  - §2.5 **F1(기본 정책 = 모두 자동, 등급 표시·감사·STOP은 유지)**
  - §3.0 단계
- `docs/plan/core/plan.md`: worker 프로토콜·저널·이벤트·허브.
- `docs/plan/ui/plan.md`: 패널 프레임워크와 `_xteam/ui-panel-template.md`.
- `docs/plan/uds/plan.md`: DoIP 전송, UDS 코덱, `request_id`.
- `docs/plan/test/plan.md` §3.1: 증거 인용·검증 모델.
- `docs/plan/_xteam/vn5640-eth.md`: C4.1, DoIP = PC NIC + 미디어 컨버터.

## 산출물
`docs/plan/eth/r1-opus.md`, 450줄 이하. 맨 위에 **핵심 루프 표**(① 직접 접근 ② 읽기 ③ 쓰기 ④ 사용자 실시간 UI + 개입)를 두고, 그 아래에 `00-brief.md` §4의 **15절**을 쓴다.

## 반드시 다룰 것
1. **에이전트 CLI (tshark 대응)**
   - 명령: `loom eth ifaces | capture start/stop/status | trace [--follow] [--filter] [--max] [--json] | last | wait-for | stats | follow tcp|udp <stream> | send | export pcapng`.
   - 한 줄 요약 형식(tshark 기본 열과 비슷하게)과 `--json` 상세(계층 트리)를 출력 예로 보인다.
   - D5 봉투, 출력 상한, 커서(`@last|seq:N|now`), 종료 코드(E2: 3 = 조건 불충족)를 따른다.
2. **표시 필터 문법**
   - Wireshark 문법의 부분집합을 고른다. 필드 이름(`ip.addr`, `udp.port`, `doip.payload_type`, `uds.sid`, `someip.service_id` 등), 연산자, 논리, 슬라이스·`contains`·`matches` 범위를 정한다.
   - **캡처 필터(BPF)**와 구분한다.
   - 같은 필터식을 CLI·UI·테스트 술어에서 공유한다.
3. **아키텍처**
   - eth worker(캡처 스레드 → 링 → 디코드 → 허브)와 C2 상한·넘칠 때 정책을 정한다. 1 Gbps를 가정하고 화면·에이전트·저널 경로를 분리한다.
   - 드롭은 3층(`source.gap` = 커널·드라이버 드롭, `record.gap`, `display.gap`)으로 표시한다.
   - 기록 정본: pcapng 세그먼트 대 CORE 저널 프레임 + pcapng export. 안을 내고 `_xteam/eth-core.md` 초안을 쓴다.
4. **디코드 범위**
   - Ethernet/VLAN/ARP/IPv4/IPv6/ICMP/UDP/TCP(재조립·스트림 번호)
   - DoIP: 헤더·payload type, 차량 공지 0x0004, 라우팅 활성화 0x0005/0x0006, 진단 메시지 0x8001/0x8002/0x8003
   - **DoIP 위 UDS**: UDS 팀 코덱을 공유한다.
   - SOME/IP·SOME/IP-SD
   - 나머지 프로토콜은 tshark/sharkd 연동(선택, 외부 프로세스) 여부. 사실 확인은 Sol이 한다.
5. **UDS 상호 링크**
   - Loom UDS가 보낸 요청(`uds.tx` `request_id`)과 캡처된 DoIP 패킷을 연결한다(양방향 링크).
   - 같은 바이트를 두 도메인이 기록할 때 중복·정합 규칙을 정한다.
   - `_xteam/eth-uds.md` 초안을 쓴다.
6. **UI 패널 (Wireshark 대응)**
   - 구성: 패킷 목록(가상화, 색 규칙, 출처 배지), 상세 트리, hex(선택 필드 하이라이트), 표시 필터 바(자동완성·오류 표시), 스트림 따라가기, 시간 기준(절대·상대·이전 패킷 대비), 통계 요약, pcapng 내보내기, UDS 링크.
   - `ui-panel-template.md` 형식으로 `_xteam/eth-ui.md` 초안을 쓴다(지연 목표, 고부하 시 동작, 수용 시험).
   - 선택 기능: Wireshark extcap로 Loom 스트림을 Wireshark에서 보기.
7. **쓰기(`send`)**
   - 원시 프레임·UDP·TCP 페이로드를 보낸다. 위험 등급 분류는 표시만 한다(F1: 기본 자동).
   - 송신 프레임도 캡처에 보여야 한다(origin=agent 배지).
   - 사용자도 UI에서 패킷을 재전송·편집 송신할 수 있다(origin=user).
8. **통합 테스트**
   - 증거 REF `eth:<if>@seq:a..b`, 술어(필터식 재사용 + count·within_ms).
   - TC 예 2개: "ECU 리셋 후 5 s 안에 DoIP vehicle announcement 3회", "SOME/IP-SD offer 주기 1 s ± 100 ms".
   - `_xteam/test-domains.md`에 붙일 `## eth` 초안을 쓴다.
9. **M1·M2와 작업 분해**
   - M1(sim): pcap 재생 + loopback에서 UDS 가상 ECU DoIP 트래픽 캡처 → CLI 실시간 + 웹 패널 E2E(Playwright).
   - M2: 실 NIC + 미디어 컨버터(Windows·Linux).
   - HW 질문: 미디어 컨버터는 PC↔ECU 구간만 보인다. ECU 간 트래픽에는 TAP·미러 포트가 필요하다.

## 규칙
- 쓰기는 `docs/plan/eth/`만 한다. `_xteam` 초안은 `docs/plan/eth/` 안에 `xteam-draft-*.md`로 두면 리드가 옮긴다.
- 불확실한 사실은 "미확인"으로 표시한다.

끝나면 대기한다.
