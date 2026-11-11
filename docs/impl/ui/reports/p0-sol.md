docs/plan/02-core-principle.md 준수.

# UI P0 보고 — ui-sol (net 계층 · mock 데몬 · fixture · E2E 하네스)

작성: ui-sol 대행(Claude 서브에이전트, 실제 ui-sol은 샌드박스 문제로 막힘), 2026-11-11.
작업 공간: `.worktrees/ui` (브랜치 `ui/main`), 커밋하지 않음. 중간에 리드 공지(`p0-proto-v1-update.md`, CORE 계약 v1)를 받아 **전부 v1 이름으로 맞췄다**(§3).

## 1. 결과 요약

| 항목 | 상태 |
|---|---|
| `cd ui && pnpm check` (typecheck + lint + vitest) | **통과** — 14 파일 124 시험 (ui-opus 것 포함) |
| 내 범위 vitest | **71개 통과** — net 49 (`src/net/*.test.ts` 6파일), mock·fixture 22 (`tools/mock/*.test.ts` 2파일) |
| `pnpm e2e` (Playwright, Chromium 158, `~/.cache/ms-playwright`) | **2개 통과** — 셸 마운트 + Worker가 Vite 프록시 거쳐 mock에 `open`, 데몬 측 강제 끊김 후 `reconnecting → open` |
| `pnpm mock` | 포트 7421, `/rpc` WS, `/healthz`, `POST /api/v1/stop`, 시나리오 3개 자동 재생 |

핵심 원칙 관점: 에이전트(mock.agent / 시나리오 타이머)가 장치(sim)에 쓰면 같은 허브의 같은 seq가 **뷰어 구독에 같은 순간** 실리고(출처 agent/user/device 서버 부여), 사용자 쓰기(`uart.input`, `can.tx.send`, `uds.request`)도 같은 파이프라인으로 들어가 같은 행 단계로 보인다. E-UI-1/2/3 흐름을 Node ws 클라이언트로 검증했다(§2.3). 브라우저 DOM 단의 확인은 셸·패널이 붙으면 리드가 확장.

## 2. 만든 파일

### 2.1 net 계층 `ui/src/net/` (api.ts는 읽기만)
| 파일 | 내용 |
|---|---|
| `wire.ts` | **wire 필드 이름을 읽고 쓰는 유일한 곳**(어댑터). `parseInbound`(응답/`core.events`/제어 구독/snapshot_part/end/heartbeat/`core.closed`), `core.hello` 인자, `core.subscribe` 인자(`after_seq`), `core.time` 결과, 배치 재구성·병합, 로컬 합성 이벤트(`display.gap`, `core.heartbeat`), 구독 공유용 필터 비교(`filterCovers`, `matchesFilter`) |
| `rpc.ts` | `RpcClient`: id 매칭, **요청 64 KiB(UTF-8 바이트) 검사 후 미전송 거절**, timeout(기본 10 s), **재시도 없음**, 미연결이면 큐잉 없이 `not_connected`, 끊김 시 진행 중 전부 `disconnected{maybeSent:true}`. `NetError`(code/rpcCode/dataCode/maybeSent, Worker 경계 직렬화) |
| `mux.ts` | `SubscriptionMux`: 같은 (filter, options) = 서버 구독 1개 + refcount, snapshot_part* → end{H} → live(>H) 순서(그 전 live는 보관, ≤H 중복 제거), 늦은 합류 핸들에 **replay**(snapshot+이후 live), 재연결 시 `after_seq` 재개 / epoch 변경·재개 거절·snapshot 중 끊김·서버 주도 재snapshot → `onResync`. **연결당 ≤ 8 구독**(아래 §5) |
| `budget.ts` | `DeliveryQueue`: 메인 스레드가 아직 안 가져간 디코드 결과. 구독별로 프레임당 1 배치로 병합. **데이터 예산 8 MiB / 128 배치** 초과 시 그 구독 데이터만 버리고 구멍 자리에 **로컬 `display.gap{cause:"client_decode_budget"}`**(연속 드롭은 한 gap을 넓힘) + onControl. 제어 이벤트는 예산 밖. 전체 hard cap 32 MiB/4096건 초과 시 그 구독 purge + resync |
| `clock.ts` | `ClockSync`: theta=((d1−b0)+(d2−b3))/2, delta=(b3−b0)−(d2−d1), 최근 60 s 최소 delta, clock_id/epoch 변경 시 폐기, BigInt ns(`msToNs`로 epoch 규모 ms → ns) |
| `core.ts` | `NetCore`: WS 소유, `core.hello`, 재연결 지수 백오프 **0.5→8 s**(연속 실패 기준), `core.closed` → 재연결, 제어 구독(`sub==='control'`)과 데이터 구독의 제어 kind를 **즉시 단건 onControl**(seq 기준 dedup, 4096 FIFO), heartbeat → 합성 `core.heartbeat` 제어 이벤트, `core.time` 연결 직후 8회·5 s 주기·가시성 복귀 시, `core.viewer.presence`, pull 기반 전달. 낡은 소켓 콜백은 세대 번호로 무시 |
| `protocol.ts` | 메인 ↔ core 메시지(`CoreIn`/`CoreOut`) |
| `client.ts` | `MainNetClient implements NetClient`: 상태·onControl·call(AbortSignal)·subscribe·clock·setVisible·**stopViaHttp(`POST /api/v1/stop`, 재시도 없음)**·close. 데이터는 **pull 방식**: 메인이 프레임마다 1회 `pull` → core가 그동안 모인 것을 한 번에. 메인이 바쁘거나 탭이 숨으면 pull이 멈추고 예산(budget.ts)이 정직하게 gap을 만든다. 핸들러 예외는 격리 |
| `browser.ts`, `net.worker.ts` | 브라우저 바인딩(WebSocket·performance·타이머), Worker 진입점 |
| `index.ts` | `createNetClient(url='/rpc', opts?)`: Worker 기본, Worker 없으면 inline. **`import.meta.env.DEV`일 때만 `window.__loomNet`** 설정 |
| `testing/fixtures.ts` | net 시험 도우미(이벤트·배치 생성, 수동 타이머) |
| `*.test.ts` | rpc 9, clock 7, budget 8, mux 14, core(통합: NetCore+MainNetClient inline + 가짜 소켓) 11 |

### 2.2 mock 데몬 `ui/tools/mock/`
| 파일 | 내용 |
|---|---|
| `hub.ts` | seq/epoch 부여, 링 20,000건, 연결별 구독, **적응형 배치**(조용한 뒤 첫 이벤트 즉시, 바쁘면 `max_batch_ms`(기본 40)·200건·60 KB), **연결마다 자동 제어 구독**(`sub:"control"`, 데이터보다 먼저 별도 프레임), snapshot(파트 n=0..N−1, 200건·60 KB 단위, state는 part 0) → `snapshot_end{stream_seq:H}`, `after_seq` 재개(링 안이면 놓친 것 재전송, 밖이면 `display.gap{resume_out_of_ring}` + snapshot), 느린 클라이언트(`bufferedAmount` > 4 MiB) 데이터 드롭 + `display.gap{slow_client}` + 배치 `display_gap` 요약, 제어 > 16 MiB면 `core.closed{slow_consumer}`, heartbeat 1 s(v1 필드), **응답 전 모든 대기 배치 flush(publish-before-reply)** |
| `sim.ts` | 쓰기 파이프라인 `requested→accepted→sent→(장치)→confirmed|failed|outcome_unknown`, 출처는 연결에서(viewer=user, mock.agent=agent), STOP 중 agent 쓰기 `requested→failed{stop_latched}` + RPC 오류, 정책 승인(켰을 때만) `core.approval_requested/resolved`, `core.agent_cursor`·`core.activity`. 장치: UART bootshell(에코, `uname -a`·`dmesg`·`reboot`, 사용자 입력권 2 s 유휴 해제, 사용자 입력 중 agent send는 deferred ≤ 10 s), CAN(0x300 → sim BCM 0x301), UDS(22 F190 → 0x78 ×3 → VIN, 10/3E, 그 밖 NRC 0x11) |
| `scenario.ts` | `fixtures/scenarios/*.jsonl` 재생(emit / agent / 주기 `every_ms`) |
| `server.ts` | HTTP·WS·RPC 디스패치·CLI. `?rights=watch|operate|approve`(mock 전용), 쓰기 RPC 자격 검사(`data.code:"rights"`), 구독 ≤ 8(`sub_limit`), `core.hello` 먼저(`hello_required`), batch 배열 거절(−32600). 시험용 `mock.agent{method,params}`, `mock.inject{kind}`(`display_gap`/`record_gap`/`source_gap`/`stall_heartbeat{ms}`/`disconnect`/`slow_consumer`/`epoch_change`/`flood_can{fps≤10k,seconds}`/`flood_uart{bytes_per_s,seconds}`/`burst_can{count}`/`policy_confirm{on}`/`outcome_unknown`), `mock.emit` |
| `gen-fixtures.ts` | `pnpm exec tsx tools/mock/gen-fixtures.ts` — mock을 카운터 시계로 돌려 wire 녹취 fixture 생성 |
| `server.test.ts` (11) | hello·rights, **E-UI-1**(agent uart.send → 뷰어에 단계·tx·장치 에코, 쓰기 이벤트 전부가 응답보다 먼저, 데이터 구독 seq 순서, 제어 구독의 agent_cursor), user origin 서버 부여·watch 거절, **E-UI-3** CAN 0x300→0x301, **E-UI-2** UDS pending ×3 → VIN, STOP(latch·agent 거절 행·user 허용·해제), 늦은 합류 1,000건, resume(링 안/밖), 구독 ≤ 8, 손실 3층·outcome_unknown·승인, heartbeat·stall |
| `fixtures.test.ts` (11) | fixture 형식·순서 관문: 필수 필드·u64 정규형·**null 금지(생략)**·kind 2토큰·gap count 문자열·쓰기 이벤트 env, snapshot n 연속/N 일치, live > H, 쓰기 단계 단조, publish-before-reply, 배치 ≤ 64 KiB |

### 2.3 fixture `ui/fixtures/`
- `scenarios/uart-bootshell.jsonl`, `can-bcm.jsonl`(0x100 10 Hz·0x200 20 Hz 배경 + agent 0x300), `uds-read-did.jsonl` — mock 입력(사람이 읽고 고치는 스크립트).
- `wire/*.jsonl` — 뷰어 쪽 wire 녹취(줄 1 = 헤더, 이후 `{t_ms, dir:"c2s"|"s2c", msg}`): `uart-normal`, `can-bcm`, `uds-pending`, `gap`(3층), `outcome-unknown`, `late-join`(1,000건 후 snapshot), `stop-latch`. **스토어 단위 시험 입력으로 그대로 쓸 수 있다**(ui-opus).

### 2.4 E2E `ui/e2e/smoke.spec.ts`
`data-testid="loom-shell"` 보임 + `window.__loomNet.state.status === 'open'` + `hello.env === 'sim'` + page error 0. 두 번째 시험은 `mock.inject{disconnect}`로 데몬 측 끊김 → `reconnecting` → `open` 확인. Playwright Chromium은 `~/.cache/ms-playwright`에 설치(프로젝트 밖).

## 3. 계약 v1 맞춤 (리드 공지 반영)
`core.hello{proto:"loom/1.0", client{kind:"web",…}}`(token 없음) · 결과 단일 `rights` · 이벤트 `epoch/seq` + `producer_*` + 선택 필드 생략 · `payload.bytes{encoding,data}` · `GapData{seq_range|producer_range, count:"문자열"}` · `EventsBatch{sub, first, last, display_gap?, lag_ms}` · `snapshot_part{sub,n,N,state?,events?}` / `snapshot_end{sub,stream_seq}` · heartbeat `daemon_mono_ns/daemon_wall_ns` · `core.time{recv_mono_ns, send_mono_ns}` · `after_seq` · 제어 구독 = `core.events` + `sub===CONTROL_SUB` · `core.closed{reason}` → 재연결 · 연결당 구독 ≤ 8. mock과 fixture도 같은 이름.

## 4. CORE 계약에 물어볼 것 (애매했던 점)
1. **snapshot_part 모양 불일치**: 계약 01 §1.4 본문에 `{n,N}`과 `{n, total, items:[{domain, channel?, kind:"<domain>.snapshot", state}]}` 두 형태가 함께 있다. 임시 타입(`{sub,n,N,state?,events?}`)을 따랐다. n이 0부터인지 1부터인지도 미기재(0으로 가정).
2. **구독 응답과 snapshot_part 순서**: 응답(sub_id) 전에 part가 올 수 있나? 클라이언트는 응답 전 알림을 상한 있게 보관(256건/8 MiB, 넘치면 resync)하지만 "응답 먼저"를 계약으로 보장해 주면 단순해진다.
3. **`after_seq` 재개 시 `snapshot` 값**: 재개 요청에 `snapshot:false`를 보내고, 링 밖·epoch 변경이면 서버가 gap + snapshot을 스스로 보낸다고 가정(라이브 중 n=0 part가 오면 `onResync("server_resnapshot")` 후 새 snapshot 처리). 맞는지, 그리고 `after_seq`만으로 서버가 epoch 불일치를 어떻게 아는지(인자에 epoch 없음).
4. **제어 이벤트의 데이터 구독 중복**: 우선 큐로 먼저 온 제어 이벤트가 데이터 구독에도 순서대로 다시 오는가? 현재 클라이언트는 둘 다 처리(onControl은 epoch:seq로 중복 제거, 데이터 구독에는 그대로 둠). 우선 이벤트가 데이터보다 **앞 seq를 추월**할 수 있으므로 재개 커서는 데이터 구독이 받은 seq만으로 계산한다.
5. **heartbeat를 제어 경로로 넘길지**: 새 `CONTROL_KIND_PREFIXES`에서 `core.heartbeat`가 빠졌지만, 셸(ui-opus)은 STALLED 계산에 onControl의 `core.heartbeat`를 쓴다. net이 heartbeat 알림을 합성 이벤트(`kind:"core.heartbeat"`, payload = heartbeat 필드)로 onControl에 넘긴다. 기록 대상이 아니므로 계약 문제는 없지만 UI 내부 규약으로 리드 확인 필요.
6. **로컬 합성 이벤트의 origin**: 클라이언트가 만든 `display.gap`/heartbeat에 맞는 origin이 4종에 없다. 지금은 `origin:"device"`, `producer_id:"ui-net"`, `ts.source:"synthetic"`, `detail:{local:true}`.
7. **gap 이벤트의 domain/channel**: `display.gap`의 `domain`이 kind 접두(`display`)인지 영향받은 스트림(`can`)인지. mock·로컬 gap 모두 **영향받은 스트림**을 domain/channel에 넣었다(ui-opus `readGap`과 같은 가정).
8. **UART 성공 종단 = `sent`**(계약 02 §2.3): 작업 지시의 "…→confirmed"와 다르다. mock은 계약대로 UART 쓰기를 `sent`로 끝낸다. `sent`는 최종 단계가 아니라 제어 경로가 아니다 — UART 쓰기 완료가 제어 경로로 즉시 보이게 할지 CORE·UART와 정해야 한다.
9. **STOP 권한**: 계약은 operate. mock도 operate로 막는다(`POST /api/v1/stop`은 mock에서 인증 없음). 리드가 CORE에 질의 중인 건과 같다.
10. **`core.closed` 뒤 재연결 지연**: 지금은 일반 백오프(0.5 s부터). slow_consumer면 서버가 권하는 지연(`retry_after_ms`?)이 있으면 좋다.
11. `uart.data`·`can.frame` payload 필드(`bytes`, `id` 숫자, `input_seq`)와 `uart.send` 인자(`data`/`data_hex`)는 도메인 스키마가 아직 없어 mock이 정한 값이다.

## 5. 구현 결정 · 스토어 쪽(ui-opus) 통합 메모
- **구독 상한 ≤ 8**: 같은 (filter, options)는 공유. 8개가 찬 뒤에는 필터를 **포함하는** 기존 구독에 "좁힌" 핸들로 붙고 메인 스레드가 그 핸들만 걸러 준다(gap 이벤트는 통과). 포함하는 구독도 없으면 핸들을 대기시키고 `onResync("subscription_cap")`을 즉시 알린 뒤, 구독이 하나 닫히면 자동으로 배치·snapshot 한다. 그래도 **스토어가 도메인·채널 단위로 구독을 합쳐 패널끼리 공유하는 것이 우선**이다.
- **`onResync(reason)` 뒤에는 새 snapshot(part…end)이 자동으로 이어진다**(snapshot 옵션이면). 스토어는 resync에서 그 구독 상태를 비우고 다음 snapshot을 기다리면 된다. reason 예: `disconnected`, `epoch_changed`, `resume_failed:*`, `server_resnapshot`, `client_queue_overflow`, `subscribe_failed:*`(5 s 뒤 재시도), `subscription_cap`.
- **늦은 합류 패널**은 같은 구독의 replay를 받는다(snapshot + 그 뒤 live). replay 로그 상한(256건/4 MiB)을 넘긴 바쁜 구독(CAN 등)에 늦게 붙으면 서버 구독이 하나 더 열린다(상한 안에서).
- `onControl`은 쓰기 최종 단계·승인·STOP·gap 등을 **즉시 1건씩** 준다. 같은 이벤트가 데이터 구독 배치에도 있으므로 RowStore의 단조 갱신(중복 무해)에 기댄다. 연결 직후 latch는 `hello.latch`로, 이후는 `core.latch` 이벤트로.
- `call()`은 연결 전·끊김 중 즉시 `NetError{code:'not_connected'}` — **대기열 없음**. STOP은 `state.status !== 'open'`이면 `stopViaHttp()`(ui-opus stopController가 이미 그렇게 함).
- 로컬 gap: `kind:"display.gap"`, `payload{seq_range{from,to}, count:"N", cause:"client_decode_budget", detail:{local:true}}`. 같은 이벤트가 그 구독 배치(구멍 자리)와 onControl 양쪽에 간다.
- `setVisible(false)`면 서버에 알리고, 탭이 숨으면 rAF가 멈춰 pull도 멈춘다 → 데이터는 예산 안에서 버려지고 gap 1개로 합쳐진다(제어는 계속). 복귀 시 시계 재측정.
- inline 모드(테스트·Worker 없는 환경)는 객체를 복제하지 않고 참조로 넘긴다 — 핸들러는 받은 배치를 고치지 말 것.
- `main.tsx`(ui-opus)의 `import.meta.glob` 우회는 이제 `import { createNetClient } from './net'`로 바꿔도 된다.

## 6. 리드에게 요청 (설정·의존성)
- 의존성 추가 없음. 설정 변경 없음.
- 선택: `e2e/`는 tsconfig.node(DOM lib 없음)로 검사되어 `page.evaluate` 안에서 `window`·`location`을 `globalThis` 캐스트로 썼다. e2e용 tsconfig에 `"lib": ["ES2023","DOM"]`을 주면 더 읽기 쉽다.
- 선택: Ajv 경계 검증(plan §6.3)은 CORE 생성 스키마가 나오면 `wire.ts`의 `parseInbound`에 넣는다(지금은 최소 모양 검사).

## 7. 남은 것 / 비범위
- `loom.columns.v1` binary 디코더(M2) — 지금은 항상 `encoding:"json"`을 요청하고 binary 프레임은 세기만 한다.
- `core.viewer.presence`의 `rendered_through_seq`·`oldest_pending_age_ms`, `core.metrics.report{ui.render}` — 렌더 스케줄러(ui-opus) 값이 필요해 미구현.
- 시계 표시값 완화(5 ms 넘는 변화를 1 s에 걸쳐), sleep 복귀 감지 — 셸 쪽.
- 지연·soak 측정(plan §9.3 P1–P8)은 P0 범위 밖.
