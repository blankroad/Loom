# Sol 독립 리뷰 — logic UI / backend (진행 중)

2026-11-14 KST. 기준: `CONTRACT.md` §8 및 현재 `ui/src/panels/logic/{logicModel,waveform}.ts`. 읽기 전용 정적 검토만 수행했다. 제품 수정, Cargo/npm, 테스트 실행, 하위 에이전트 없음. 아래 재현 입력은 코드 경로 추론이며 실행 성공을 주장하지 않는다. P1=통합 전 수정, P2=기능 정확성 수정.

## 확인된 결함

### R1 — P1: 무전이/저전이 capture의 chunk 보관이 무제한

- 증거: `logicModel.ts:276–305` 매 chunk 및 `seenChunk`를 추가하지만, `:330–339` eviction 조건은 `edgeCount > 400_000`뿐이다. `:316–327`은 매 추가 시 전체 chunk를 다시 순회한다.
- 영향: `edges:[]`인 정상 idle pin capture에서는 sample 범위가 계속 증가해도 단 한 chunk도 퇴출되지 않는다. chunk 객체/seq set가 무한 증가하고 누적 gap 재계산 비용도 제곱으로 커진다. edge 배열 상한은 chunk 보관 상한이 아니다.
- 수정: chunk 수 또는 실제 bytes/sample-window 상한을 추가하고 `chunks/seenChunk/unknown`을 함께 퇴출한다. 무전이 장기 capture를 수용 기준에 포함한다.

### R2 — P2: invalid sample/rate가 유효 geometry로 들어옴

- 증거: `logicModel.ts:86–89`는 유한한 모든 number(음수/소수 포함) 및 unsafe integer decimal string을 수락한다. `:153–156,167–170`은 이를 sample index로 사용한다. `:193–206`은 역순 annotation을 거부하지 않고 end만 clamp하며 tEnd는 원래 end로 계산한다. rate도 `:181,197`에서 양수 검증이 없다.
- 예: raw `sample_start=-1`, `sample_end=2.5`, edge `[0.5,1]`이 통과한다. annotation `{start_sample:100,end_sample:10}`은 `[100,100]` geometry와 `tEnd<tStart`를 동시에 갖는다.
- 수정: sample용 `Number.isSafeInteger(v) && v>=0`, 양수 유한 rate 검증을 분리한다. end<start 및 정수/범위를 벗어난 sample은 명시적으로 거부/unknown 처리한다. 손실 카운트도 음수로 보정을 상쇄하지 못하게 한다.

### R3 — P1: 중간 malformed edge를 삭제한 뒤 그 구간을 정상 level로 그림

- 증거: `logicModel.ts:162–187`에서 bad edge를 skip하고 `dropped`에 합산한다. `:297–301`은 원인과 무관하게 **마지막 정상 edge 뒤**만 unknown으로 만든다.
- 예: `[0,100)`에서 `initial=0`, edges `[[10,1],[20,"invalid"],[90,0]]`이면 sample20 이후 상태를 알 수 없는데도 `[10,90)`을 확정 HIGH로 표시한다. unknown은 91 이후에만 생긴다. bad edge 위치와 tail cap은 다른 손실이다.
- 수정: malformed edge가 있는 chunk를 통째로 거부/unknown 처리하거나 오염 시작점부터 다음 검증 가능한 상태까지 unknown으로 보존한다. backend의 prefix-retained cap과 parser corruption을 같은 tail-drop 변수에 합치지 않는다.

### R4 — P2: dense pixel이 실제 chunk 끝/unknown 영역까지 침범

- 증거: `waveform.ts:90–94`는 dense 종료를 항상 `col+1`로 잡고 piece 끝 `p.b`나 viewport로 clip하지 않는다. `:115–118`은 실제 끝에서 unknown을 별도로 추가한다.
- 예: chunk `[0,5)`, transitions sample1/2, `spp=10`, `s0=0`이면 dense가 x=0.1…1로 그려진다. 실제 chunk 끝은 x=0.5이므로 이후 unknown/빈 구간에 dense 데이터가 있다고 표시한다.
- 수정: dense geometry를 `[xOf(p.a),xOf(p.b))` 및 viewport에 clip한다. 인접 chunk의 같은 pixel에 전이가 나뉘는 경우도 전체 pixel 기준으로 합산하거나, 해당 근사를 명시한다.

### R5 — P2: zoom/pan 시 앞에서 시작한 긴 annotation이 사라짐

- 증거: `logicModel.ts:347–350`은 start 기준 정렬한다. `:386–397`은 lower_bound(start)에서 직전 annotation의 end가 view 밖이면 역탐색을 멈춘다. `waveform.ts:123–135`는 그 index부터만 그린다.
- 예: annotation `[0,1000]`, `[10,20]` 두 개에서 view start=50이면 index=2를 반환하여 여전히 겹치는 `[0,1000]`을 누락한다. decoder별/nested frame이 섞일 수 있는 계약에서 start 정렬은 end 정렬을 보장하지 않는다.
- 수정: prefix-max-end index/interval 조회 또는 bounded 배열의 정확한 overlap scan을 사용한다. zoom 후 긴 annotation 유지 사례를 검증한다.

### R6 — P2: snapshot/backlog의 부분 중첩 chunk를 받아 binary lookup 전제를 깨뜨림

- 증거: `logicModel.ts:286–295`는 완전 동일한 prev range만 거부하고 다른 overlap은 삽입한다. `:363–372`의 binary search는 chunk end가 단조 증가한다고 전제한다. `waveform.ts:52`도 같은 index를 쓴다.
- 예: 먼저 `[0,100)`, 이후 다른 seq의 `[20,30)`을 추가하면 end 배열은 `[100,30]`. `chunkAt(50)`은 2를 반환해 원래 존재하는 sample50 데이터가 사라진다.
- 수정: overlap을 명시적으로 거부하거나 interval coverage를 정규화/교체하여 lookup 전제를 유지한다. snapshot 재적용에는 원본 event seq와 이미 evicted된 coverage를 구분하는 경계도 유지한다.

## 확인된 계약 일치 / 아직 미완료

- sample 기반 raw/decode 축이 분리돼 있고 host timestamp는 chunk metadata로 남는다. `class:null`은 `logicModel.ts:201–202`에서 그대로 null이며 `uart-1` 등 decoder 문자열을 보존한다. §8 all-8 acquisition 및 fx2lafw만 허용하는 UI 설정은 `:614–622,659–665`에 반영돼 있다.
- 동일 capture state는 `:419–430`에서 seq가 같거나 오래되면 재적용하지 않는다. retained raw/annotation의 동일 event key dedup도 있다. 다만 stream epoch reset과 snapshot/live 실제 합류는 이 두 파일 밖의 integration이 필요하며 현재 성공으로 판정하지 않았다.
- `waveform.ts`는 실제 canvas painter/axis/annotation track을 구현한다. 원시 gap hatch와 dense를 level과 구별하지만 R4 경계 문제가 있다. `visibleAnnotations`의 2,000행 상한(:123–135)은 반환값에 truncation 정보를 제공하지 않으므로 패널에서 visible omission을 명시할 계약을 추가할 필요가 있다. 패널의 최종 표시가 아직 없어 이를 별도 확정 UI 결함으로 세지는 않았다.
- 검토 시 `LogicPanel`/waveform interaction component가 아직 없다. 실제 pan/zoom/live-follow, selection/hit-test, gap/retention 배지, priority state/snapshot/epoch 연결은 **미완료·미검증**이며 존재하지 않는 구현에 추정 결함을 부여하지 않았다.
- `crates/loom-logic/src/worker.rs`는 아직 없다(`lib.rs`, Cargo manifest만 존재). 생성되면 같은 범위로 process/raw/decoder/STOP/contract 독립 리뷰를 확장한다. 제품 소유 파일 수정은 하지 않는다.

## Backend model 확장 리뷰 (model.rs 생성 후)

정적 검토 범위: `crates/loom-logic/src/model.rs` 전체 및 현재 UI의 실제 요청 생성 경로. 아직 worker/gateway/CLI 소스가 없어 이들의 누락을 결함으로 세지 않았다. 아래 행 번호는 이 확장 검토 시점 파일 기준이다. 기존 UI R1–R6의 수정 여부 재판정은 이번 확장과 분리한다.

### B1 — P1: 큰 clock_hz로 설정 검증 자체가 panic 가능

- 증거: `model.rs:478–484,632`는 JSON u64 전 범위를 받는다. `:955–961`은 상한 검사 전에 `2 * clk`를 두 번 계산하고 나눗셈에 쓴다. `:658–660`은 start config 검증 단계에서 이 함수를 호출한다.
- 재현 입력(정적): `{"source":"synthetic","decoders":["spi:clk=D0:mosi=D1"],"clock_hz":9223372036854775808}`. debug arithmetic에서는 곱셈 overflow, wrapping 설정에서는 0으로 감긴 divisor에 의한 나눗셈 panic이 가능하다. 검증 오류로 반환되지 않는다.
- 수정: 곱셈 전에 `clock_hz > 0 && clock_hz <= samplerate/2`로 검사하거나 checked multiplication 후 오류를 반환한다. u64::MAX/2+1, u64::MAX, 0 경계를 검증한다. 수정/실행은 backend 소유자 담당.

### B2 — P1: 기본 synthetic UI 요청이 strict model API와 불일치

- 증거: 현재 `logicModel.ts:698–718`의 `startParams`는 synthetic에서 `protocol`을 항상, UART에서는 top-level `baudrate`도 보낸다. `logicPanel.ts:462`에서 해당 값을 그대로 start action에 전달한다. 반면 `model.rs:463–476` START_KEYS에는 두 키가 없고 `:510–514`는 unknown parameter를 거부한다. UART baud/프로토콜은 이미 `decoders` spec에 있다.
- 영향: 기본 synthetic UART 및 SPI 시작 요청이 StartConfig 경계에서 E_USAGE가 된다. gateway가 아직 없으므로 실제 daemon RPC end-to-end 실패를 실행 확인한 것은 아니지만 **현재 두 소유 모듈의 API는 직접 호환되지 않는다**.
- 수정: 하나의 계약으로 맞춘다. 현재 model을 기준으로 UI top-level protocol/baudrate를 제거하고 decoders만 사용하거나, 명시적으로 승인한 API normalization을 추가한다. 두 정보를 받아 서로 다를 때의 우선순위를 암묵적으로 만들지 않는다.

### B3 — P2: demo pattern의 rate 제한이 현재 UI 설정 경로와 불일치

- 증거: `model.rs:572–579`는 pattern이 있으면 rate 200,000만 허용한다. `logicModel.ts:640–641`은 rate=1,000,000/pattern=graycode 기본값이며 `:701–713`은 demo에서 둘 다 보낸다. `logicPanel.ts:54,414–419`는 12/24 MHz 포함 rate 선택을 허용하고 source 변경 시 200 kHz로 제한하는 경로가 현재 없다. backend 범위는 `model.rs:20–22,581–584`의 1 kHz…10 MHz다.
- 영향: 사용자가 기본 설정에서 source만 demo로 바꾸면 backend의 새 제약에 걸린다. 12/24 MHz 선택도 model에서 거부된다. backend 오류를 숨기는 문제라고 단정하지 않지만, 정상 선택으로 제시된 설정과 허용 계약이 다르다.
- 수정: demo pattern 선택 시 200 kHz를 명시/강제하거나 검증 안내를 제공하고, 공통 samplerate 선택을 backend 범위와 맞춘다. 추후 engine argv 변경으로 임의 rate+pattern을 지원하면 해당 제약을 함께 제거한다.

### 현재 model에서 확인한 정상 방향 / 실행 미검증 경계

- API: source 명시, unknown key/잘못된 JSON type 거부, duration/max_samples/text/frame period 상한, 1개 decoder 제한이 있다. driver는 fx2lafw exact allowlist, 16ch 드라이버 명시 거부, conn은 bus.address 문자열만 허용한다(`:516–560,586–643`). shell command 생성 대신 argv 원소로 분리하며 실제 장치 scan을 리뷰 중 실행하지 않았다.
- argv: acquisition은 항상 전체 D0…D7, decoder binary 입력은 고정8ch/정수 samplerate이며 decoder 채널은 숫자로 변환한다. `-l 2`, sample-number flag, word-only `-A` filter가 이전 실제 엔진 프로브와 일치한다(`:695–734`). demo pattern 경로의 `-g Logic -c pattern=…`는 backend 작성자의 확인 주석/코드만 읽었으며 이번 정적 리뷰에서 재실행하지 않았다.
- EdgeAcc: sample index는 마스킹 전 byte마다 1 증가, 표시 mask는 원래 bit 위치 유지, 첫 4096개 전이를 남기고 이후 전이를 dropped로 계수하면서 실제 final state를 계속 갱신한다. take 이후 sample offset은 이어진다(`:783–841`). 이 prefix-cap 정책은 UI가 마지막 보관 edge 이후를 unknown으로 표현할 수 있다. 공백 chunk, chunk 경계 initial의 effective state, publish 실패 후 sequence gap의 최종 의미는 worker/caller와 함께 다시 확인해야 한다.
- Synthetic: UART는 fractional sample/baud 비율을 bit 경계 누적 계산으로 처리하며 8N1/LSB-only 제한을 upfront 검사한다(`:939–949,1016–1034`). SPI는 설정 pin/CPOL/CPHA/bitorder/CS를 waveform 생성에 반영하고 8-bit word로 제한한다(`:951–963,1036–1065`). **현재 Rust Synth가 만든 bytes를 실제 sigrok decoder로 돌린 증거는 아직 없다**. 이전 Python fixture의 성공을 새 Rust 구현 성공으로 대체하지 않는다. backend 소유자가 생성한 fixture로 UART 예상 바이트, SPI 4 modes 및 CS polarity/선택 line을 실제 엔진에 대조하면 이 경계가 해소된다.
- Annotation: 숫자 sample span, `uart-1` ID 및 class null 보존은 계약과 맞는다(`:869–903`). 다만 parser는 b<a도 parsed row로 수용한다. 외부 프로세스 이상 출력의 추가 방어로 역순 span을 `_raw`/오류로 처리하는 것이 적절하며, 이 점은 UI R2와 같은 invalid sample 경계에 포함한다.
- Engine resolve는 파일 존재만 확인하므로 executable 여부/버전 실패의 명시적 보고는 worker 구현 확인 항목이다(`:1103–1122`). worker 미생성 상태에서 lifecycle 실패를 추정하지 않았다.

미완료 검토: worker의 spawn 실패 cleanup, stdin 유지, raw/decoder 동시 drain, bounded tee/backpressure, STOP/deadline/process-group reap, source EOF/exit 분류, failed state/손실 이벤트, sample/annotation batching 및 snapshot canonical seq. Cargo 실행 권한은 backend Opus에만 있으며 Sol은 실행하지 않았다.

## Worker / gateway 독립 리뷰 재개 — ROOT P1 수정 반영 후

범위는 **Sol이 작성하지 않은** `worker.rs`와 `gateway.rs`의 현재 소스다. Sol 작성 CLI/스킬은 독립 리뷰가 아니며 이 판정 대상에서 제외했다. 제품 수정/Cargo/추가 real-engine 실행 없음. 아래는 정적 실행 경로와 line 증거이며 backend가 진행 중인 실제 Rust engine 검사를 중복하지 않았다.

### W1 — P1: 최종 decoder writer의 실패를 성공 STOP으로 마감할 수 있음

- 증거: `worker.rs:725–732` writer의 `write_all` 실패는 `broken=true`만 남긴다. `:994–1002`는 writer가 다 끝나기 **전에** `tee.failed`를 확인하고, writer join 뒤 `:1008–1010`은 decoder exit status/killed만 확인한다. 마지막 단계에서 broken 또는 decoder_input_bytes를 다시 검사하지 않는다.
- 조건: STOP 시 raw queue가 남았고 decoder가 stdin을 조기에 닫고 **exit 0**. stop-drain 또는 queue EOF 처리 중 writer가 EPIPE를 받으면 active loop의 `:885` 검사로 돌아가지 않는다. `tee.failed`는 feed 호출이 더 없으면 설정되지 않으므로 raw samples보다 적은 decoder input을 처리했는데도 `:1021`이 stopped를 선택할 수 있다.
- 수정: writer join 뒤 broken/잔여 queue/`decoder_input_bytes != accepted raw samples`를 검사해 failed/error와 관측 범위를 기록한다. writer가 partial write 후 실패한 경우도 완전 전달로 계산하지 않는다. 기존 ROOT R3의 **sticky queue overflow** 개선과 별개인 비동기 writer 최종 오류 경계다.

### G1 — P1: ASCII wait가 손실을 건너뛰어 거짓 연속 문자열을 증명

- 증거: `gateway.rs:253–254`는 dropped annotation을 loss row로 추가한다. Wait `:383–387`은 scan의 complete/gap 결과를 버리고 annotation 이외 row를 건너뛴다. `:397–404`의 byte window는 이런 손실에서 reset되지 않는다. loss row는 해당 batch의 정상 rows 뒤에 붙으므로 나중에 loss row에서 reset만 해도 이미 잘못 match한 결과를 취소할 수 없다.
- 조건: 앞 batch에서 `4C`, 다음 batch의 retained rows `4F 4F 4D`와 `dropped:1` → 실제 빠진 byte가 사이에 있는데 `--ascii LOOM`이 matched=true가 될 수 있다. journal gap을 사이에 두는 경우에도 같은 문제다.
- 수정: flatten 전 event의 dropped/관측 gap을 검사한다. 손실 위치가 불명확한 batch에서 연속 ASCII 증거를 만들지 않고 window를 끊거나 incomplete로 반환한다. journal coverage gap과 단순 scan-budget continuation은 구분한다. 개별 text hit와 연속 byte predicate의 증명 수준도 구분한다.

### G2 — P1: reducer annotation snapshot은 200행만 cap, byte cap 없음

- 증거: `gateway.rs:457–461`에는 edge_bytes만 있고 annotation byte counter가 없다. `:500–511`은 annotation 200행까지만 보관하며 `:524–528`에서 전부 한 state object로 내보낸다. `worker.rs:40,1080–1098`에서 허용하는 약4KiB text는 JSON escaping 최악 약24KiB/row이므로 200행은 약4.8MiB가 된다. 실제 row가 producer의 작은 batch 여러 개에 나뉘어 와도 reducer에서 다시 합쳐지므로 per-event byte cap으로 해결되지 않는다.
- 전송 경로: hub `crates/loom-gateway/src/hub/stream.rs:116–138`은 state items를 첫 part에 통째로 넣으며 **items 자체를 byte 분할하지 않는다**. server `src/server.rs:139`의 WS message cap은1MiB다(수신 제한 설정임을 구분; 브라우저 수신 cap의 직접 증거로 확대하지 않음). 어쨌든 snapshot의 의도된 약60KiB part bound를 수MiB 단일 frame이 위반하며 late join 비용이 급증한다.
- 수정: serialized annotation row bytes를 **합계64KiB 이하**로 추가 제한하고 초과 시 오래된 row부터 퇴출한다. 단일 row/배열 구분자 및 state/frame overhead도 고려한다. eviction/truncation 수치를 snapshot에 남긴다. ROOT가 별도 제시한 우려를 실제 행 경로로 확인한 동일 결함이며 중복 별도 이슈가 아니다.

### G3 — P2: read 결과는 rows 합계만64KiB, 실제 결과 크기는 상한 초과 가능

- 증거: `gateway.rs:286–303`은 row JSON 길이+1만 합산해 MAX_RESULT_BYTES까지 채운다. `:336–338`은 이후 rows wrapper와 complete/rows_left_out/next를 붙이며, Envelope metadata도 추가된다.
- 영향: 상한에 근접한 유효 row 조합에서는 실제 직렬화 결과가64KiB를 넘는다. annotation batch의48KiB cap이나 edges 요약64개 제한은 여러 row 합계의 이 문제를 없애지 않는다.
- 수정: 최종 result/Envelope 크기에 고정 headroom을 두거나 실제 직렬화된 전체 결과를 기준으로 oldest rows를 추가 제거하고 rows_left_out/complete를 다시 계산한다. ROOT의 worker per-event byte 수정과는 다른 RPC 응답 경계다.

### G4 — P2: follow=true의 최소 wait 및 wait timeout 최소값 불일치

- 증거: `gateway.rs:69–74`는0을 허용한다. `:320–325`는 `(Some(0),true)`를 일반 Some(w)로 수용한다. `:374`의 wait timeout도0을 수용한다. 따라서 frozen contract의 follow wait_ms=1..30000 및 wait timeout_ms=1..600000과 다르다. CLI가 거부하더라도 직접 RPC는 통과한다.
- 수정: follow=true && wait_ms==0 및 timeout_ms==0을 E_USAGE로 거부한다. non-follow wait_ms=0은 **현재 `:321`에서 정상 수용하도록 수정됨**을 확인했다. 기존 wire 통합 수정을 다시 미해결이라고 주장하지 않는다.

### 수정 반영 확인 / 추가 차단 결함으로 세지 않은 사항

- ROOT의 기존 worker P1 대부분은 현 소스에서 개선을 확인했다: annotation reader→batcher `sync_channel(1024)` + 최대 line 길이 + try_send/drop 계수, serialized row 기준 약47KiB batch, raw2048 edges/48KiB payload, tee의4MiB inflight byte 상한과 sticky failed 상태, 모든 child의 process_group(0) 및 음수PGID signal, setup thread spawn 실패의 abort 경로.
- STOP: acquisition newline → deadline group INT/KILL → raw rx drop 후 reader join, decoder sender drop → bounded reap → writer/stdout/stderr/batcher join의 순서를 확인했다. 과거 ROOT 지적의 positive PID kill/무조건 reader join 문제를 그대로 반복하지 않는다. descendant가 group 밖으로 스스로 이탈하는 일반 공격 시나리오까지 제품 요구로 확장하지 않았다.
- `Tee.failed` 이후 decoder input을 재개하지 않아 queue-overflow 후 sample-axis hole은 막는다. raw가 이후에도 기록되고 failed로 끝나는 점과 **W1의 join 이후 writer 오류 누락**을 구분한다.
- snapshot capture state에는 원본 event seq가 없지만 현재 UI는 snapshot high-water를 상태 stale fence로 사용한다. 최신 priority state를 이전 snapshot이 덮는다고 단정하지 않았다. annotation/edge snapshot byte/row 경계 중 확인된 문제는 G2다.
- wait text는 현재 `eq_ignore_ascii_case`(`gateway.rs:395`)이며 CONTRACT/CLI의 substring 설명과 다르다. ROOT는 substring 또는 equality 중 하나로 명시적으로 통일할 필요가 있다(부차적 wire 정합성; 별도 P1로 부풀리지 않음).

ROOT 반환 핵심: **W1 최종 feed 실패, G1 ASCII 손실 횡단, G2 snapshot byte cap** 우선. G3/G4는 bounded RPC 경계 수정. 테스트 부재/미구현 지적은 추가하지 않았고 엔진 실측은 backend 소유자가 담당한다.

## ROOT 최종 통합 판정

Sol의 독립 지적 R1–R6, B1–B3, W1/G1–G4는 소유 Opus가 반영했다. CLI는 Sol 본인 구현이므로 독립 리뷰라고 부르지 않는다. 변경·검사 근거는 UI-STATUS.md/BACKEND-STATUS.md, 실제 동시 수신 근거는 STATUS.md와 suite-*-compare.json에 있다. 이 문서는 발견 시점의 리뷰 기록을 보존하며 미해결 차단 목록이 아니다.
