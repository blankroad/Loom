# 05. worker 프로토콜 `loom-worker/1`

타입 정본: `loom_proto::worker`. 구현: `loom-worker-sdk`(Rust), `adapters/python/loom_worker_sdk`(P4, 같은 와이어).

## 5.1 전송
- 이름 있는 로컬 소켓: Unix domain socket(Linux·macOS, 디렉터리 0700) / Windows named pipe(DACL = 현재 사용자, 원격 pipe 거부).
- **물리 연결 2개**: `ctl`(요청·응답·쓰기 상태·latch·입력·하트비트·우선 이벤트·PDU), `data`(대량 RX 배치). 큰 data 배치가 STOP·PDU를 막지 않는다.
- ctl 안에서도 `Latch`와 우선 입력(Ctrl-C 등)은 worker SDK IPC 스레드가 큐에 넣지 않고 즉시 처리한다(원자 플래그·우선 mailbox).

## 5.2 프레임과 직렬화
- 프레임: `u32 LE len | u8 msg_type | u8 flags | u16 reserved | payload`. **모든 프레임 ≤ 64 KiB.** 길이는 할당 전에 검사한다. 잘린 프레임·상한 초과·모르는 msg_type은 연결 오류.
- payload = **MessagePack map**(필드 이름 포함, `rmp_serde::to_vec_named`), 바이트는 bin, u64는 정수. 큰 데이터는 chunk(`id, offset, total, sha256`, 재조립 ≤ 4 MiB/대상) 또는 `raw_ref`.
- 튜플(array) 인코딩은 고정 레이아웃 golden·형식 hash 협상 후 M2 이후 옵션.

## 5.3 메시지
| 방향 | lane | 메시지 |
|---|---|---|
| W→G | ctl | `Hello{proto{major,minor}, kind, instance, producer_id, producer_epoch, pid, build, schema_hash, codecs, domains, capabilities, journal_dir, resume_nonce?}` |
| G→W | ctl | `Welcome{accepted_minor, stream_epoch, config, budgets, latch}` / `Reject{reason}` |
| G→W | ctl | `Call{call_id, method, args, origin, client_id, actor_id, request_id, deadline_mono_ns}` · `Write{…, plan_digest}` · `JobStart/JobUpdate/JobRenew/JobStop` · `InputLease`·`Input` · `Latch{on, scope, reason, latch_seq}` |
| G→W | data | `Credit{bytes}` |
| W→G | ctl | `Reply{call_id, result, covers?}` · `PriorityEvents[…]` · `Health{…}`(1 s) · `LatchAck{latch_seq}` · `Segment{sealed…}` |
| W→G | data | `Events{first_source_seq, count, records[]}` |
- 버전: major 불일치 → `Reject`(그 worker만 비활성). minor = 교집합.
- **`loom-worker/1.1`**: `JobStart/JobUpdate/JobRenew/JobStop/Input`은 선택 정수 `call_id`를 가진다. 협상 minor ≥ 1이고 필드가 있으면 `Reply.call_id`로 그 값을 돌려주고, 없거나 minor 0이면 소켓 요청 순번으로 짝짓는다. 응답 대상 요청은 명시 ID 유무와 관계없이 순번을 소비한다.
- **publish-before-reply**: worker는 쓰기 결과 이벤트를 ctl `PriorityEvents`로 먼저 보내고 `Reply.covers`에 그 마지막 `SourceRef`를 싣는다. gateway는 허브 발행 워터마크 ≥ covers일 때만 성공 응답한다.

## 5.4 흐름 제어 (C2, can-core W2·W3)
- data lane 바이트 크레딧, 창 = 유입률 × 100 ms(기본 1 MiB, worker별 설정).
- 크레딧이 없으면 worker는 **장치 수신을 막지 않고** 화면 미러(4 MiB/8192건)에 쌓다가 넘치면 화면 전달만 버리고 `display.gap{producer_range, cause:"ipc_backpressure"}`를 ctl로 보고한다.
- **원본 기록은 worker가 직접** 저널에 쓴다(`06-journal.md`). 크레딧·gateway 상태와 무관.
- 도메인 hot path 스레드(RX·FC·주기 스케줄러)는 SDK와 non-blocking `Emitter`로만 만난다. block은 SDK IPC 스레드에만 허용.

## 5.5 감독·재기동
| 상태 | 전이 | 동작 |
|---|---|---|
| starting → ready | Hello/Welcome ≤ 5 s | `core.worker_state` |
| ready → suspect → stalled | Health 3 s 누락 → 5 s | 신규 쓰기 거절 exit 7 `E_WORKER_DOWN` |
| stalled → killed | 10 s 무수신 또는 hard 메모리 초과 | 강제 종료 |
| 종료 감지 | exit / ctl EOF | 진행 중 쓰기 `outcome_unknown`, tail 복구 후 `record.gap` |
| 재기동 | 250 ms → 500 ms → 1 → 2 → 4 … ≤ 30 s, ±20 % | 채널 재open = `WorkerSpec.reopen_after_crash`: `None`(UART — DTR 리셋 위험) / `RxOnly`(CAN — `autostart` 채널만, 장치 identity 재검증 후 RX) |
| quarantined | 60 s 안 5회 크래시 | 재기동 중단, viewer `core.worker.restart` |
- job·주기 TX·ISO-TP 송신은 **어떤 경우에도 자동 재개하지 않는다**.
- **정상 종료**: gateway는 새 요청을 거절하고 모든 worker에 `Latch{on:true, reason:"shutdown"}`(ack ≤ 200 ms) 뒤 두 연결을 닫는다. 이 latch 뒤 EOF를 받은 worker는 orphan 대기 없이 저널 seal → 장치 close → 즉시 종료(gateway는 ≤ 2 s 뒤 kill). shutdown latch는 사용자 STOP이 아니므로 영속하지 않는다.
- 그 밖의 끊김(gateway 크래시): worker는 즉시 TX latch, 저널 계속, orphan 30 s 후 seal·종료(M1). 다음 worker가 장치를 다시 열기까지의 공백은 `source.gap{cause:"gateway_restart"}`. **M2**부터 재부착(discovery·단일 인스턴스 lock·`resume_nonce`).

## 5.6 메모리 예산 (C2)
| 프로세스 | soft / hard |
|---|---|
| gateway | 256 / 384 MiB |
| CAN worker(채널 2) | 96 / 192 MiB |
| UART worker(포트당) | 48 / 64 MiB |
| seed-key helper | 32 / 64 MiB |
| export helper | 128 / 256 MiB |
| Python COM(P4) | 192 / 384 MiB |
- worker admission = worker hard 합 ≤ 1 GiB(gateway 제외, 설정 가능). 도메인이 `WorkerSpec.budgets`로 더 작게 선언할 수 있다.
- hard 수단: Windows Job `ProcessMemoryLimit`(**commit 한계**), Linux 위임 cgroup v2 `memory.max`(없으면 `RLIMIT_DATA` 백스톱), macOS 없음(watchdog). RSS는 1 s 샘플로 따로 감시.
- soft 10 s 지속 → `core.worker_state{mem:"pressure"}` + 화면 미러·구독 축소, 60 s 지속 또는 hard → latch → seal → 재기동.
- 모든 큐는 건수·바이트 상한과 넘칠 때 정책을 코드에 명시한다(plan §9.2 Q1–Q29). 표에 없는 큐는 merge하지 않는다.
