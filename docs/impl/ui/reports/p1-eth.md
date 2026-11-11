docs/plan/02-core-principle.md 준수.

# UI P1 보고 — ui-opus: `eth.packets` 패널 + STOP 권한 갱신 (계약 v1.1)

작성: ui-opus, 2026-11-11. 기준 ui/main `c1ed979`, 작업 공간 `.worktrees/ui/ui` (커밋 안 함).
결과: **`pnpm check` 통과** — typecheck + lint + test **21개 파일 203개**(이번에 추가·수정: eth 14개, STOP 시험 1개 수정). `pnpm build` 통과. 레이아웃 프리셋 `eth`(`#/l/eth`)에 eth.packets + UDS 콘솔.

## 0. 계약 v1.1 반영
- **STOP·비상 OFF는 모든 자격(watch 포함)**: 셸 STOP 버튼의 watch 비활성·툴팁 제거, `Ctrl-] x`의 operate 검사 제거(`TopBar.tsx`, `ShellApp.tsx`). 비상 OFF 버튼은 원래 자격 검사가 없었다. STOP **해제**는 그대로 operate + 확인 카드.
- STOP RPC 이름 `core.stop_all` → **`core.stop{scope:'all', reason}`**(`stopController.ts`), 해제 `core.stop.release` 그대로.
- 시험: "watch에서 STOP 버튼 활성, 클릭 → `core.stop{scope:'all'}`"로 고침. 다른 이름은 건드리지 않음(생성 타입 교체는 리드).

## 1. 만든·바꾼 파일

| 파일 | 내용 |
|---|---|
| `src/panels/eth/ethPanel.ts` (신규) | **`eth.packets`** — 상태 바(캡처 상태 LIVE/STOPPED/PERMISSION·SIM tap·pps·bps·**커널 드롭 누계**·기록·`ts.source`(정밀도)·캡처 필터) + [■ 캡처 정지](안전 방향, watch 가능) [캡처 시작…] [⇩ pcapng]; 권한 없음이면 "`loom doctor`" 안내. 필터 바, 모드 바(라이브 "표시 1/N 샘플링 · 기록 100 %" / 완전 "완전(저널 기준) · N행 seq a–b"), 시간 기준 4종(캡처 시작·절대·이전 행·기준 행), 가상화 목록(seq·시간·출발지·목적지·프로토콜·길이·정보·↔UDS·출처), 송신 행(RowTable, "NIC 관찰 ✓"), 상세(트리↔hex) + [재전송][편집 후 송신…][UDP/TCP 송신…][이 행을 시간 기준으로][증거 REF 복사]. Space = 일시정지/라이브 |
| `src/panels/eth/ethModel.ts` (신규) | 행 읽기(이벤트·질의 행 공통, `attributed_origin` 우선 배지), 기본 색 규칙(UDS NRC > TCP RST·재전송 > SOME/IP-SD > DoIP > ARP, gap 줄무늬), 시간 형식, 캡처 상태·`eth.batch` 커버리지(두 모양 수용)·`eth.link` 읽기, **PacketStore**(2만 행 / 4 MiB, 1,000행 묶음 퇴출 + 퇴출 수) |
| `src/panels/eth/filterBar.ts` (신규) | 표시 필터 바: 입력 150 ms 뒤 `eth.filter.check{expr}`(응답 ms 표시) → 오류면 **위치 밑줄**(`pos` 글자, 끝이면 `␣`) + "위치 N: … 필요", 정상이면 [적용]/Enter. 잘못된 식은 적용 안 됨. 저장·적용 이력(localStorage, 각 20개, 실패해도 동작) |
| `src/panels/eth/dialogs.ts` (신규) | 편집 후 송신(hex 편집 + 200 ms 뒤 `eth.send{dry_run:true}` 미리보기·등급), UDP/TCP 페이로드(TCP는 Loom 자기 연결만), 캡처 시작(NIC 목록 `eth.ifaces`·BPF(표시 필터와 다른 색·설명)·snaplen·promisc·버퍼) |
| `src/panels/eth/fixtures.ts` (신규) | 합성 캡처: 라우팅 활성화, **실제 바이트의 DoIP 진단 요청(UDS 22 F1 90, 69 B)** + `{off,len}` 트리, 응답(probable 링크), NRC, ARP, TCP 재전송, SOME/IP-SD; 상세 결과·저널 질의 행 생성기 |
| `src/panels/eth/eth.test.ts` (신규) | 14개 시험(§2) |
| `src/sdk/treeHex.ts` (신규) | **트리↔hex 양방향 동기** 공통 컴포넌트: 노드 → 바이트 범위 하이라이트, 바이트 → 그 바이트를 포함하는 가장 깊은 노드 선택(조상 펼침). 렌더 상한 16 KiB(나머지는 원본 링크). 라벨은 textContent |
| `src/sdk/evidenceRef.ts` (신규) | REF 파서를 SDK로 이동 + **`<domain>:<channel>@req:<request_id>` 확장**(seq를 모르는 링크 칩용) |
| `src/sdk/virtualTable.ts` | `onSelect`, `onCellClick`(칩 클릭), `onScrollAway`(스크롤백 감지), `following`/`followTail()`, `reveal(key)`. 꼬리 따라가기 창 계산을 레이아웃과 무관하게(첫 레이아웃 전에도 마지막 행들이 보이게) |
| `src/sdk/context.ts` | `ctx.resubscribe(topicKey, ext)` — 이전 서버 구독을 닫고 `filter.ext`로 새 구독(연결당 ≤ 8 지킴), `ctx.ui.onReveal(cb)` |
| `src/sdk/storeMirror.ts` | 배치 요약 gap에 `fromBatch` 표시(목록에 gap 행이 한 번씩만 들어가게) |
| `src/shell/services.ts` | reveal을 패널 리스너에 방송, 아무도 처리 못 하면 토스트 |
| `src/panels/uds/consolePanel.ts` | `uds:<target>@req:<id>` reveal → 해당 행 선택 + 스크롤 + 포커스 |
| `src/shell/Layout.tsx` | 프리셋 `eth` (eth.packets lan1 + uds.console) |
| `src/panels/index.ts` | eth 등록 한 줄 |
| `src/shell/shell.css` | eth 색 규칙·gap 줄무늬·칩(exact 실선/probable 점선)·트리/hex·필터 밑줄·대화상자 |
| `src/sdk/testing/harness.ts` | reveal 기록 |

C2: 패널 행 2만/4 MiB, 대기 링크 2,000, NIC 관찰 집합 4,096, 필터 저장·이력 각 20, hex 렌더 16 KiB, 저널 질의 최대 2,000행 — 넘칠 때 정책은 코드 주석.

## 2. 시험 (14개 + STOP 1개)

| 과제 요구 | 시험 | 결과 |
|---|---|---|
| 트리↔hex 동기 | UDS 노드 → 바이트 66–68 하이라이트 / 바이트 62 → "Source address: 0x0E80", 바이트 68 → "DID: 0xF190 VIN"(조상 펼침) | 통과 |
| 필터 오류 밑줄·적용 | `doip.type ==` → `위치 12: 값 필요` + 밑줄 `␣`, Enter 무시(구독 그대로) / `doip` → 오류 숨김, Enter → 이전 구독 닫힘·새 구독 `filter.ext={eth_filter:'doip'}`·`eth.query{ch, filter, max:2000}` → "완전(저널 기준) · 3행 seq 50000–50002" | 통과 |
| 샘플링 배지·일시정지 시 완전 질의 | `eth.batch{total:1000, shown:10}` → "라이브 · 표시 1/100 샘플링 · 기록 100 %" / Space → `eth.query{range:{to:'41280'}}` → 목록이 질의 결과 5행으로 교체 + "완전(저널 기준) · 5행" / Space → 라이브 | 통과 |
| gap 틈 행 count | `source.gap` 37(producer_range만) + 5 → 빨간 틈 행 2개("수신 전 손실 37"), 배지 "커널 드롭 42"; `drop_stats:unavailable` → "커널 드롭: 알 수 없음"(0으로 표시 안 함) | 통과 |
| tx 행 단계·NIC 관찰 | `eth.tx` requested→sent 1행, "NIC 관찰 —" → `tx_request_id` 붙은 캡처 패킷 도착 → "NIC 관찰 ✓" | 통과 |
| UDS 칩 reveal | exact 실선·probable 점선 칩, 클릭 → `reveal('uds:bcm@req:t-000418')`. UDS 콘솔은 이 REF로 행 선택(코드), 셸이 방송 | 통과 |
| watch: 캡처 정지 가능·재전송 불가 | watch로 [■ 캡처 정지] → `eth.capture.stop`, [재전송]·[캡처 시작…] 비활성, `eth.send` 호출 0 | 통과 |
| 상한 퇴출 | 20,500행 → ≤ 20,000·퇴출 수 > 0·가장 오래된 seq 없음 / 큰 info 2,000행 → bytes ≤ 4 MiB | 통과 |
| 기타 | 목록 seq·색 클래스·`data-origin=agent`(attributed)·`data-request-id`, 상세 질의 `{ref, detail:true}` → hex 69칸, [재전송] → `eth.send{kind:'raw', ch, hex}` 정확한 바이트, 편집 미리보기는 `dry_run:true`만·[보내기]는 1회 실송신, 샘플링 문구가 기록 상태에 정직 | 통과 |

## 3. 정직성 구현 메모
- 라이브 목록은 축약될 수 있으므로 행 수·통계를 "완전"으로 부르지 않는다. **일시정지(Space)·스크롤백(꼬리에서 벗어나면)·필터 적용** 때 `eth.query`로 저널 범위를 받아 목록을 바꾸고 "완전(저널 기준)" 배지. 2,000행에 닿으면 "최대 2,000행 — 범위를 좁히세요"(완전하다고 하지 않음).
- "기록 100 %"는 `record_health=ok`일 때만. 아니면 실제 상태 문구.
- 커널 드롭 통계 미지원이면 "알 수 없음". producer_range만 있는 손실은 stream seq를 지어내지 않는다.
- 배지는 `attributed_origin`을 따르되 툴팁에 "기록 origin은 device"를 남긴다(TEST R-8와 같은 구분).

## 4. ETH 팀에 물을 것 (payload·RPC 모양 — 가정은 `ethModel.ts` 머리 주석)
1. **`eth.pkt` payload 필드**: `src/dst/proto/len/info/attributed_origin/layers/link/tx_request_id`로 가정. src/dst가 없으면 `layers.ip|ipv6|arp|eth`에서 고른다. 확정 요청.
2. **상세 `eth.query{ref, detail:true}` 결과**: `{ref, len, bytes{encoding,data} | hex, raw_url?, truncated?, tree:[{label, off, len, children}]}` 가정. 트리 노드 이름 필드(`label`/`name`)와 `{off,len}`이 프레임 기준 오프셋인지(linktype이 NULL/LOOP면 0이 어디인지) 확인.
3. **목록 `eth.query{ch, range:{to}, filter?, max ≤ 2000, detail:false}`**: "to까지 최근 max행"으로 가정. `range{from,to}`·`truncated`·gap 포함 여부(질의 결과에 `source.gap` 구간을 행으로 넣어 줄지) 확정.
4. **`eth.batch`**: UI 요청 모양 `{covered_first_seq, covered_last_seq, total, shown}`과 계획 모양 `{first_src, last_src, count, by_proto}` 둘 다 읽지만 `shown`이 없으면 비율을 정확히 못 낸다 — UI 요청 모양으로 확정 부탁.
5. **송신 ↔ 캡처 복사본 링크**: 패킷 payload `tx_request_id` / `eth.link{domain:'eth', request_id}` / `eth.tx.payload.nic_observed` 셋 다 처리한다. 하나로 정해 달라.
6. **`eth.send` 인자**: raw `{kind:'raw', ch, hex}`, socket `{kind:'udp'|'tcp', ch, to:'IP:PORT', from_port?, hex}`. **dry-run 응답 `{proto, info, risk}`(디코드 미리보기)를 서버가 주는지** — 편집 대화상자의 실시간 미리보기가 여기에 의존한다(없으면 "미리보기 없음 (서버 dry-run 필요)"로 표시).
7. **`eth.capture.start` 인자** `{iface, ch, bpf?, snaplen, promisc, buffer_mib}`, **`eth.ifaces` 결과** `{ifaces:[{name, alias?, can_capture?}]}`.
8. **`eth.export`**: `{ch, format:'pcapng', filter?, range?}` → job. 진행률 이벤트 이름(`core.job_progress`?)과 완료 시 다운로드 경로(`/api/v1/raw/…`?).
9. **`eth.capture_state`**: `state` 값 집합(LIVE·STOPPED·PERMISSION 외), `drop_stats`, `ts_precision`, `sim`, `started_mono_ns`(rel 시간 기준점). 없으면 첫 패킷 기준.
10. **`eth.link` 대상 정보**: UDS 칩 reveal에 대상 이름(`target`)이 필요. 없으면 REF가 `uds:*@req:<id>`가 되고 UDS 콘솔은 request_id만으로 찾는다(동작함).

### TEST·리드에
- reveal REF 확장 `<domain>:<channel>@req:<request_id>`(channel `*` 허용)를 TEST REF 문법에 넣을지 — 링크 칩은 seq를 모르고 request_id만 안다.

## 5. mock 시나리오 요구 (리드 배정)
`tools/mock`에 eth가 없다. 아래를 넣어 주면 Playwright로 eth-ui 수용 시험 1–7을 돌릴 수 있다.
1. **sim tap 캡처** `lan1`: `eth.capture.start/stop` → `eth.capture_state{state, sim:true, drop_stats:'available', ts_precision:'us', started_mono_ns}`.
2. **UDS 연동**: mock의 `uds.request`(DoIP 대상)마다 요청/응답 `eth.pkt`(DoIP 0x8001, `attributed_origin`, `layers`) + `eth.link{eth_ref, request_id, role, confidence:'exact', target}`. 수용 1(칩 → UDS 행).
3. **필터**: `eth.filter.check`(최소 `doip`, `doip.type == 0x8001`, 잘못된 `doip.type ==` → `{ok:false, pos:12, expected}`), 구독 `filter.ext.eth_filter` 적용, `eth.query` 결과가 같은 필터.
4. **고부하**: 1 Mpps 재생 → `eth.batch{covered…, total, shown}` + 일시정지 시 `eth.query` 완전 범위(수용 4).
5. **커널 드롭 주입** → `source.gap{cause:'kernel_drop', count}`(수용 5), `drop_stats:'unavailable'` 변형.
6. **송신**: `eth.send`(raw·udp) → `eth.tx` requested→sent + 캡처 복사본(`tx_request_id`) → "NIC 관찰 ✓", dry-run 미리보기 응답. STOP 중 에이전트 `eth.send` 거부·캡처 행은 계속(수용 7).
7. **상세**: `eth.query{ref, detail:true}` — `fixtures.ts`의 69 B 프레임·트리를 그대로 쓰면 된다.

## 6. 리드 요청
- (반복) `@xterm/addon-fit`, vitest 공통 setup(`matchMedia` 스텁) — p1-opus 보고 §5.
- CORE d2 gap payload가 `seq_range{first,last}`라고 답했다(UI 임시 타입은 `{from,to}`). eth 패널은 둘 다 읽게 했고, 셸 adapter는 지시대로 건드리지 않았다 — 생성 타입 교체 때 `src/store/adapter.ts readGap/readGapSummary` 한 곳만 바꾸면 된다.
- `core.subscribe`의 `filter.ext`는 CORE d5에 들어간다고 했다 — mock이 `ext`를 무시하면 필터 푸시다운 효과는 없고 `eth.query`의 필터만 정확하다(화면 문구는 그대로 정직).

## 7. 남은 것 (M2)
스트림 따라가기(`eth.follow`), 통계(`eth.stats{by}`), 색 규칙 편집(레이아웃 저장), 필터 WASM 자동완성, 행 우클릭 메뉴(지금은 상세 바 버튼), "이 값으로 필터", export 진행률 표시.
