docs/plan/02-core-principle.md 준수.

# UI P1 보고 — ui-opus: `can.trace`를 CAN 스키마 v1에 맞춤

작성: ui-opus, 2026-11-11. 기준 ui/main `2aa7e59`, 작업 공간 `/Users/ctmctm/workspace/Loom/.worktrees/ui/ui` (커밋 안 함). mock(`ui/tools/**`)은 손대지 않음.
근거: `docs/impl/requests/can-ui.md` §1–6, CORE fixture 게이트웨이 `crates/loom-gateway/src/replay/conn.rs`(`can.send → can.tx` 행 payload = `{write_state, echo_of:<요청 인자>}`).

## 0. 결과
- typecheck 0, 내 영역 lint 0, **테스트 27개 파일 298개 전부 통과**(CAN 46개 = 기존 갱신 + 신규 13개: 모델 9 · 패널 5 중 `sendLine` 갱신 포함), build 통과.
- **`pnpm check`는 lint 단계에서 멈춤 — 내 파일 아님**: `src/net/core.test.ts:247` `'_u' is defined but never used`(net 담당의 커밋 안 된 변경). 그 한 줄이 고쳐지면 check 전체가 통과한다(테스트·타입은 위처럼 이미 통과).

## 1. 바꾼 파일

| 파일 | 내용 |
|---|---|
| `src/panels/can/traceModel.ts` | **v1 읽기**: `readFrame`(payload `id` 숫자·`ext`·`fd`·`brs`·`rtr`·`dlc`=DLC 코드 0..15·`len`=바이트 0..64·`bytes{hex\|base64}`), `txFrame`(`can.tx`의 요청 = `payload.echo_of` / `payload.frame`(cansend 문자열 또는 객체) / payload 자체의 구조 필드), `parseCansend`(`300#01`, `18DAF110#…`, `123##1…`, `300#R`). 새 열 `dlc`(u8)·`fidx`(u16, frame_index), 새 플래그 `F_RTR`·`F_REQ`(요청 소속 행)·`F_NOID`(요청에 id 없음). **한 줄 병합**: 요청마다 `ReqInfo{rows[], echoed, txRow, state, risk}` — 에코 `frame_index` 0은 `can.tx` 행에 합쳐 **시각·바이트·플래그를 에코 값으로 교체**, 1 이상은 행 추가, 같은 index 재수신은 무시(`dupEchoes`), 에코가 먼저 오면 에코 행이 요청 행이 됨. 상태·등급은 요청 단위로 그 요청의 모든 행에 반영(`setReqState`/`raiseState` 요청 전체). 쓰기 행과 그 에코는 표시 예산으로 숨기지 않음. id 없는 요청도 행은 남김(`?`). 헤더 주석을 v1 기준으로 고침. `dlcForLen`·`lenForDlc`·`rowsOf`·`dlcAt`·`frameIndexAt` |
| `src/panels/can/traceView.ts` | 열 이름 `dlc` → **`len`**(바이트 수), 툴팁 `12 B · DLC 코드 9 · CAN FD BRS`, RTR은 `R 2` + data `RTR`, id 툴팁 `29bit (ext) id`/`11bit id`(표시는 기존 `18DAF110x`), id 없음 `?`, 다중 프레임 비고 `#1`. 셀 툴팁도 바뀐 것만 다시 씀. 행 속성 `data-len`·`data-dlc`·`data-frame-index` |
| `src/panels/can/sendLine.ts` | 결과 = **`can.send` 구조 인자 `{channel, id, ext, fd, brs?, data}`**(`data_hex` → `data`). FD는 `brs`를 항상 명시(데몬 기본이 true라 `fd 0x123#…`는 `brs:false`), Classic은 `brs`를 빼서 보냄(주면 `E_USAGE`) |
| `src/panels/can/canTrace.ts` | manifest 액션 `can.send`, 질의 목록에 `can.send`(SDK가 `dry_run:true`일 때만 허용). **등급 미리보기**: 입력이 유효하면 250 ms 디바운스 후 `can.send{…, dry_run:true}` → 배지 `R1`, 툴팁 `R1 — sim channel · 정책 auto · 1 B`. 확인 중 `R…`, 입력 무효·빈칸·실패는 `R?`(이유 툴팁). 새 입력이면 이전 요청 abort + 늦은 응답 무시. 미리보기 줄에 DLC 코드 표시. 결과는 D5 봉투(`{data:{…}}`)와 맨 객체 둘 다 읽음 |
| `src/sdk/context.ts` | `ctx.act` 결과의 `request_id`를 맨 객체 또는 `data.request_id`에서 읽음(v1 §3 결과가 `data{request_id, …}`) |
| `src/sdk/sdk.test.ts` | 예시 RPC 이름 `can.tx.send` → `can.send` |
| 시험 | `traceModel.test.ts`(+9), `canTrace.test.ts`(+5, 이름 갱신), `sendLine.test.ts`(인자 이름·brs 규칙) |

## 2. 시험 (과제 2 대응)

| 요구 | 시험 | 결과 |
|---|---|---|
| 병합(같은 request_id의 can.tx + 에코) | CORE fixture 모양(`echo_of.frame:"300#01"`, id 없음)으로 요청 행 생성 → 에코 1개 → 1줄, 시각 = 에코 `ts.mono_ns`, 바이트 = 에코 `A5`, 상태 `confirmed` / 구조 요청(29bit FD 12 B) / 에코 먼저 → 쓰기 행 나중 = 1줄 / 패널에서 `data-request-id`·상태 | 통과 |
| frame_index 다중 줄 | count 2·3: 줄 = 프레임 수, 사이에 rx 끼어도 유지, 모든 줄 `confirmed`, 중복 index 무시, 패널 비고 `#1` | 통과 |
| dlc≠len | FD 12 B `dlc:9, len:12` → len 12·DLC 9, dlc 없으면 len에서 9 계산, raw_ref + `dlc:9`(FD) → len 12·`?`, 패널 셀 `FB 12` + 툴팁 `DLC 코드 9` | 통과 |
| rtr | `rtr:true, dlc 4, len 4, data ""` → len 4·데이터 없음, 패널 `R 2`·`RTR` | 통과 |
| dry-run 디바운스 | 연속 입력 3번 → 249 ms까지 호출 0, 250 ms에 정확히 1번(`dry_run:true`, 마지막 입력 인자), 배지 `R1`·사유 / 무효 입력은 호출 없음 + `R?` / 응답 대기 중 입력이 바뀌면 늦은 `R3` 무시 | 통과 |
| 송신 | Enter → `can.send{channel, id, ext, fd, brs:false, data}`(FD), Classic은 `brs` 없음, D5 봉투 결과에서 request_id `u-9` | 통과 |
| 기타 | 표시 예산(강등) 중에도 쓰기 행·에코는 숨지 않음, 요청에 id가 없어도 행 유지 | 통과 |

## 3. 정한 것 (질문 창 없이)
1. **송신은 구조 인자**로 보낸다(`frame` 문자열 아님) — 29bit 여부와 BRS가 명시적이어야 해서. 송신줄 문법은 그대로(cansend식 + `fd`/`brs` 접두, `x` 표시).
2. **에코 병합 시 행 위치(seq)는 첫 수신(`can.tx`) 그대로**, 시각·바이트·플래그만 에코 값. 시각 열이 에코 시각이 되므로 시각 순서가 아주 조금 어긋날 수 있다(요청 → 에코 사이 다른 프레임이 있으면). seq 순 정렬이 기록·증거 REF와 맞기 때문에 위치를 옮기지 않았다.
3. 다중 프레임 상태 갱신은 요청당 1,024줄까지(그 뒤 줄은 추가 시점 상태로 고정) — C2 상한, count가 그만큼 큰 송신은 M1에 없음.
4. RTR **송신**은 M1 UI에서 계속 막는다(v1 구조 인자에 `rtr`가 없음). RTR **표시**는 지원.
5. 등급 배지는 데몬 dry-run 값만 쓴다. 미리보기 전·실패면 `R?`(지어낸 R1을 보이지 않음).

## 4. 물을 것

### CAN
1. `can.send` dry-run 결과가 D5 봉투(`data{risk, reason, bytes, policy}`)인지 맨 객체인지 — 둘 다 읽게 했다.
2. `can.tx` 쓰기 행 payload에 요청 프레임을 어디에 싣는지 확정: 지금 CORE fixture는 `echo_of`(요청 인자 그대로). 실제 CAN worker는 `payload.frame`(문자열?)인지 구조 필드인지. 세 곳 모두 읽는다.
3. 에코가 아예 안 오는 실패(`failed`/`outcome_unknown`)에서 행 시각은 `can.tx` 시각으로 남는다 — 맞는지.
4. `frame_index`가 있는 에코의 `count` 총수를 `can.tx`에 실어 주면 "2/3 프레임 송신됨" 표시가 가능하다(지금은 받은 줄 수만 보임).

### CORE
- fixture `can.frame`의 `dlc:8`은 코드·길이가 같아 모호 — v1대로 `len`도 실어 달라(CAN이 이미 요청). UI는 `len`이 없으면 바이트 수에서 계산한다.

## 5. mock 요청 (리드가 서브에이전트에 전달)
- RPC 이름 `can.send`, 쓰기 행 `can.tx{echo_of|frame}` + 에코 `can.frame{dir:"tx", request_id, frame_index}`, payload `dlc`(코드)·`len`(바이트), `can.send{dry_run:true}` → `{risk:"R1", reason:"sim channel", bytes, policy:"auto"}`. 지금 작업 공간의 mock(`tools/mock/server.ts`)은 `can.send`와 옛 `can.tx.send`를 둘 다 받는 전환 중 상태이고, `e2e/can.spec.ts`는 아직 `can.tx.send`를 언급한다 — 패널은 이제 `can.send`만 보내므로 e2e 쪽 이름도 맞춰야 한다. 구조 인자(`data`, FD의 `brs` 명시)와 dry-run 응답 모양을 mock이 받는지 확인 필요.
