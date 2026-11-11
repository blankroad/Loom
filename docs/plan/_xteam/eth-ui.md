# ETH → UI: 패킷 패널 명세 (초안 v1, explore-lead 통합 2026-11-10 — `ui-panel-template.md` v0.2 형식)

docs/plan/02-core-principle.md 준수. 정본: `docs/plan/eth/plan.md` §5. UI 팀은 `## ui 의견`을 덧붙여 주세요.

## 패널: eth.packets

- 목적: 사용자가 Wireshark처럼 에이전트가 보는 것과 같은 패킷을 같은 순간에 보고, 재전송·편집 송신·캡처 정지로 개입한다.
- 마일스톤:
  - **M1**: 목록(가상화)·상세 트리·hex·표시 필터 바(서버 검사)·UDS 칩·재전송/편집 송신·캡처 정지·시간 기준 3종.
  - M2: 스트림 따라가기 패널, 통계 패널, 색 규칙 편집, 필터 WASM 자동완성.
  - M3: extcap 안내.
- 대응 기능: Wireshark Packet List / Packet Details / Packet Bytes / Display filter toolbar / Follow Stream / Statistics.

### 데이터 (읽기)
- 구독: `filter{domains:["eth"], channels:[<ch>], kinds:["eth.pkt","eth.batch","eth.tx","eth.link","eth.capture_state","eth.stats","source.gap","record.gap","display.gap"], ext:{eth_filter}}`. 필터 바 내용이 서버 푸시다운 필터가 된다.
- 늦은 합류 snapshot: 캡처 상태(BPF·snaplen·버퍼 실제값), 최근 2,000행, 활성 링크, 통계 최신값.
- **질의 RPC**:
  - `eth.query{range, filter, max ≤ 2000, detail:false}` — 스크롤백·일시정지·필터 변경 시 **저널 기준 완전한** 목록.
  - `eth.query{ref, detail:true}` — 선택 행의 트리 + hex(≤ 64 KiB, 큰 프레임은 `/api/v1/raw`).
  - `eth.filter.check{expr}` → `{ok, pos?, expected?, fields_used}`.
  - `eth.follow{stream}`.
  - `eth.stats{by}`.
- 예상 유입: 정상 10–500 pkt/s(DoIP·SOME/IP). 발행 예산 5k/s. 그 이상은 `eth.batch`로 축약된다.
- **표시 규칙**:
  - 열: seq · 시간 · 출발지 · 목적지 · 프로토콜 · 길이 · 정보 · 출처 배지.
  - 시간 기준: 캡처 시작 상대(기본) / 절대(wall) / 이전 표시 행 대비 / 사용자 지정 기준 행.
  - 색 규칙(기본): DoIP 하늘색, UDS 부정 응답 빨강, TCP RST·재전송 주황, ARP 노랑, SOME/IP-SD 연두, `source.gap` 틈 행 빨간 줄무늬. 사용자 규칙 = (표시 필터식, 색)이며 레이아웃에 저장한다.
  - 출처 배지(E8): AGT 청록, USR 노랑, DEV 회색, SCR 보라, EXT 보라 점선. 배지는 `attributed_origin`이 있으면 그것을, 없으면 `origin`을 따른다.
  - `eth.tx` 행은 `request_id` + `payload.write_state`로 셸 RowStore가 갱신한다. 캡처 복사본이 링크되면 같은 행에 "NIC 관찰 ✓"를 붙인다.
  - UDS 링크 칩 `↔UDS`: exact는 실선, probable은 점선. 클릭하면 UDS 콘솔 해당 `request_id` 행으로 `ui.reveal`.
  - 해석 문자열(info·트리 이름·NRC 이름)은 **도메인 payload 그대로** 쓴다. UI가 디코드를 재구현하지 않는다.

### 개입 (쓰기)
| 사용자 입력 | 호출 RPC | 위험 등급(표시) | 확인 방식(F1) | 최소 자격 | 안전 방향 | 쓰기 상태 표시 |
|---|---|---|---|---|---|---|
| 재전송(선택 행) | `eth.send{raw, ch, hex}` | R1(sim)/R2(실 NIC)/DoIP 진단이면 UDS classify 등급 | 기본 없음(정책 켜면 카드) | operate | 아니오 | `eth.tx` 행 requested→sent + 캡처 복사본 링크 |
| 편집 후 송신(hex 편집 + 실시간 디코드 미리보기) | `eth.send` | 위와 같음 | 기본 없음 | operate | 아니오 | 위와 같음 |
| UDP/TCP 페이로드 송신 대화상자 | `eth.send{udp|tcp}` | R1/R2 | 기본 없음 | operate | 아니오 | 위와 같음 |
| 캡처 시작(설정 대화상자: NIC·BPF·snaplen·promisc·버퍼) | `eth.capture.start` | R0 | 없음 | operate | 아니오 | `eth.capture_state` |
| 캡처 정지 | `eth.capture.stop` | R0 | 없음 | watch | **예** | 상태 배지 |
| pcapng 내보내기(범위·필터) | `eth.export` (job) | R0 | 없음 | watch | 아니오 | job 진행률 |
| 표시 필터 적용·저장, 시간 기준 지정, 색 규칙 | (로컬 + 구독 갱신) | — | — | watch | — | — |
| 증거 REF 복사 | (로컬) | — | — | watch | — | — |

### 실시간성·정직성
- **지연**:
  - 캡처 → 목록 행 p95 ≤ 200 ms / p99 ≤ 400 ms(≤ 5k pkt/s, render ack 기준).
  - 필터 바 검사 응답 ≤ 100 ms.
  - 행 선택 → 트리·hex ≤ 150 ms.
- **고부하**:
  - 라이브 목록은 "표시 1/N 샘플링 · 기록 100 %" 배지와 함께 축약된다.
  - 사용자가 **일시정지(Space)·스크롤백·필터 변경**을 하면 라이브 링이 아니라 `eth.query`로 **완전한 범위**를 보여 준다. 축약된 상태를 완전한 것처럼 보이지 않는다.
  - 탭 데이터 예산은 패널 4 MiB / 2만 행(UI 제안값).
- **절대 드롭하면 안 되는 이벤트**: `eth.tx` 상태, `eth.link`, `eth.capture_state`, 모든 gap(우선 큐).
- **상태 배지**: 캡처 상태(LIVE/STOPPED/PERMISSION), pps·bps, **커널 드롭 누계**(`source.gap`), 디코드 지연, 발행 비율, record health, 보존 가능 시간 추정, `ts.source`·정밀도.

### 와이어프레임
`docs/plan/eth/plan.md` §5(목록 / 트리 / hex 3분할 + 필터 바 + 캡처 바).
- **선택 행 동기**: 트리에서 필드를 고르면 hex의 해당 바이트 범위가 하이라이트된다. 반대로 hex를 고르면 트리 노드가 선택된다. 도메인이 `layers` 각 필드에 `{off, len}`을 준다.
- **스트림 따라가기**(M2 패널 `eth.stream`): 방향별 색, ASCII/hex 전환, 잘림 표시, "이 스트림만 필터" 버튼.

### 수용 시험 (Playwright, 실제 DOM)
1. sim: 에이전트 `loom eth capture start lo` + `loom uds read-did F190 -t sim1` → 200 ms 안에 DoIP 행. `data-seq`·`data-request-id`가 CLI JSON과 같고, `↔UDS` 칩을 클릭하면 UDS 콘솔 행이 포커스된다.
2. 필터 바에 `doip.type == 0x8001` 입력 → 행 수가 CLI `trace -Y` 결과와 같다. 잘못된 식 `doip.type ==` → 위치 밑줄 + 메시지.
3. 행 "편집 후 송신"(DID 변경) → USR 배지 행. 에이전트 다음 결과에 `others_since_last.count = 1`.
4. 재생 1 Mpps → 샘플링 배지. 일시정지 후 구간 행 수 = `eth.query` 개수(완전).
5. 커널 드롭 주입(sim) → 빨간 틈 행 + 배지 수치가 `source.gap.count`와 같다.
6. 두 번째 브라우저 늦은 합류 → 같은 마지막 seq와 캡처 상태.
7. STOP → 에이전트 `eth send`는 거부, 캡처 행은 계속 들어온다.

## ui 의견

ui-lead, 2026-11-10 (UI `plan.md` §0.4 F2).
- **UI-M1에 `eth.packets` 포함 수용.** 목록은 CAN Trace와 같은 Worker 링 + DOM 가상화 그리드, `eth.tx` 행은 셸 RowStore(`request_id` + `payload.write_state`), 출처 배지·`attributed_origin` 규칙 수용. 신규 컴포넌트는 **상세 트리 ↔ hex 바이트 범위 동기**(도메인이 `layers` 각 필드에 `{off, len}`을 주는 전제)와 표시 필터 바(서버 `eth.filter.check` 결과로 위치 밑줄).
- 지연 p95 ≤ 200 / p99 ≤ 400 ms, 필터 검사 ≤ 100 ms, 선택 → 트리·hex ≤ 150 ms 수용. 탭 예산 패널 4 MiB / 2만 행 수용(탭 데이터 예산 256 MiB 안).
- **정직성 규칙 수용**: 라이브 축약 시 "표시 1/N 샘플링 · 기록 100 %" 배지, 일시정지·스크롤백·필터 변경 시 `eth.query`로 완전한 범위. 축약 상태에서는 행 수·통계를 "완전"으로 표시하지 않는다.
- 요청 ① 구독 필터의 도메인 확장 `filter.ext{eth_filter}`는 CORE `core.subscribe` 계약에 자리가 필요하다 — UI가 `_xteam/core-ui.md`로 CORE에 함께 요청한다. ② `eth.batch`(축약) 이벤트에 `{covered_first_seq, covered_last_seq, total, shown}`을 넣어 UI가 샘플링 비율과 범위를 정확히 표시하게. ③ 색 규칙 기본값은 UI 레이아웃에 저장(도메인은 기본 규칙 JSON만 제공). ④ `↔UDS` 칩의 `ui.reveal`은 셸 `ctx.ui.reveal(ref)`로, ref는 TEST 형식(`uds:<target>@seq:N` 또는 `request_id`).
- 스트림 따라가기·통계 패널 M2 동의. 재전송·편집 송신은 F1 기본 자동, 등급 배지는 항상 표시.

## 추가 (리드 통합)
- 배지 "커널 드롭: 알 수 없음"(통계 미지원 플랫폼)과 "SIM"(권한 없는 개발용 sim tap 프레임)을 정의해 달라.
- "DoIP 연결 OK / 캡처 불가(권한)" 상태를 따로 표시한다. 캡처 권한이 없으면 패널에 `loom doctor` 안내를 띄운다.
- 선택(M3): `loom-extcap`으로 같은 스트림을 Wireshark에서도 볼 수 있다. Wireshark가 느려도 Loom 패널·캡처에는 영향이 없다(구독자별 spool).
