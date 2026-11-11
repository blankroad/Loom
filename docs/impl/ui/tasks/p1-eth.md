docs/plan/02-core-principle.md 준수.

# UI P1 — ui-opus: `eth.packets` 패널(Wireshark식) + STOP 권한 갱신

작성: ui-lead, 2026-11-11. 규칙은 `p0-common.md`·`p0-proto-v1-update.md` 그대로(커밋 안 함, 리드 소유 파일 수정 금지, `cd ui && pnpm check` 통과). 기준 커밋 ui/main `c1ed979`(당신의 P0·P1 선행분 포함).

## 0. 먼저 — 계약 v1.1 반영(작게)
- CORE 답변(`docs/impl/requests/ui-core.md` `## core 답변`): **STOP·비상 OFF는 watch에서도 허용**(계약 v1.1). STOP **해제**는 operate. 셸 STOP 버튼·`Ctrl-] x`·비상 OFF의 watch 비활성/툴팁을 걷어 내고 시험을 고쳐라.
- STOP RPC 이름은 `core.stop`(해제 `core.stop.release`) — 지금 `core.stop_all`을 부르는 곳이 있으면 `core.stop`으로. (생성 타입 교체는 메인이 CORE 태그를 병합한 뒤 리드가 따로 한다 — 다른 이름은 건드리지 마라.)

## 1. `eth.packets` (F2 — UI-M1 범위)
근거: 내용 명세 **`docs/plan/_xteam/eth-ui.md`**(+ `## ui 의견`), 도메인 계획 `docs/plan/eth/plan.md` §4–5, UI `plan.md` §0.4 F2·§5.2·§6.5.

- **3분할**: 목록(가상화) / 상세 트리 / hex. 트리 필드 선택 ↔ hex 바이트 범위 하이라이트 **양방향 동기**(도메인 `layers` 각 필드 `{off, len}` 전제).
- **목록 열**: seq · 시간 · 출발지 · 목적지 · 프로토콜 · 길이 · 정보 · 출처 배지(`attributed_origin` 우선). 시간 기준 3종(캡처 시작 상대 기본 / 절대 wall / 이전 표시 행 대비) + 사용자 기준 행. 기본 색 규칙(DoIP 하늘, UDS NRC 빨강, TCP RST·재전송 주황, ARP 노랑, SOME/IP-SD 연두, `source.gap` 빨간 줄무늬). 해석 문자열은 도메인 payload 그대로.
- **보관**: 패널 4 MiB / 2만 행 상한(넘치면 오래된 것 버림 + "보관 범위 밖 → 질의").
- **표시 필터 바**: 입력 → `eth.filter.check{expr}`(≤ 100 ms 응답 표시) → 오류면 위치 밑줄 + 메시지, 정상이면 구독 `filter.ext{eth_filter}` 갱신. 필터 저장·적용 이력.
- **정직성**: `eth.batch`(축약) → "표시 1/N 샘플링 · 기록 100 %" 배지. **일시정지(Space)·스크롤백·필터 변경 시 라이브 링이 아니라 `eth.query{range, filter, max ≤ 2000}`로 완전한 범위**를 보여 주고 "완전(저널 기준)" 표시. 커널 드롭 `source.gap` → 빨간 틈 행 + 배지 누계 = `count`.
- **선택 → 상세**: `eth.query{ref, detail:true}` (≤ 150 ms 목표), 큰 프레임은 `/api/v1/raw` 링크.
- **`eth.tx` 행**: RowTable/RowStore(`request_id` + `payload.write_state`), 캡처 복사본이 링크되면 "NIC 관찰 ✓".
- **`↔UDS` 칩**: exact 실선 / probable 점선, 클릭 → `ctx.ui.reveal(ref)`(UDS 콘솔 해당 request_id 행).
- **개입**: 재전송(선택 행) · 편집 후 송신(hex 편집 + 실시간 디코드 미리보기 — 서버 dry-run이 없으면 보고서에 요청) · UDP/TCP 페이로드 송신 대화상자 → `eth.send`; 캡처 시작 대화상자(NIC·BPF·snaplen·promisc·버퍼) → `eth.capture.start`; **캡처 정지 = 안전 방향**(watch 허용) → `eth.capture.stop`; pcapng 내보내기 → `eth.export`(job 진행률). 위험 등급 배지 항상 표시(F1 기본 자동).
- **상태 바**: 캡처 상태(LIVE/STOPPED/PERMISSION), pps·bps, 커널 드롭 누계, 발행 비율, record health, `ts.source`·정밀도.
- 스트림 따라가기·통계·색 규칙 편집은 M2(자리만).

## 2. mock
`ui/tools/mock`는 서브에이전트(CAN 작업 중) 영역이다 — **고치지 마라**. eth 이벤트·RPC가 mock에 없으므로 fakeNet + 당신의 `src/panels/eth/fixtures.ts`(합성 DoIP·UDS·ARP·TCP 패킷, `layers{off,len}` 포함)로 개발·시험한다. mock에 넣을 eth 시나리오 요구는 보고서에 적어라(리드가 배정).

## 3. 파일 범위
`ui/src/panels/eth/**`, `ui/src/sdk/**`, `ui/src/shell/**`, `ui/src/store/**`, `ui/src/panels/index.ts`(eth 등록 한 줄 + 레이아웃 프리셋 `eth`). **수정 금지**: `ui/src/panels/can/**`, `ui/src/net/**`, `ui/tools/**`, `ui/fixtures/**`, `ui/e2e/**`(CAN 서브에이전트 작업 중), 리드 소유 파일.

## 4. 시험
트리↔hex 동기, 필터 오류 밑줄·적용, 샘플링 배지와 일시정지 시 완전 질의 전환, gap 틈 행 count, tx 행 단계·NIC 관찰 링크, UDS 칩 reveal, watch로 캡처 정지 가능·재전송 불가, 상한 퇴출.

## 보고
`docs/impl/ui/reports/p1-eth.md` — 파일, 시험 수, ETH 팀에 물을 것(payload·RPC 모양), mock 시나리오 요구, 리드 요청. 끝나면 "완료: <경로>"만.
