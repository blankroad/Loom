# P1 보고 — `loom-can` gateway 조각(G1) + `loom can` CLI(K3) + 스킬 v0

작성: can-opus, 2026-11-11. 과제 `docs/impl/can/tasks/p1-gateway-cli.md`. 범위: `crates/loom-can/src/gateway.rs`, `src/cli.rs`, `tests/gateway.rs`, `tests/cli.rs`, `skills/loom-can/SKILL.md`, 이 보고서. `lib.rs`·`schema.rs`·`Cargo.toml`·`worker.rs`는 건드리지 않았다. 커밋 안 함.

## 결과 (워크스페이스 루트에서)
- `cargo fmt --all --check` ✓ · `cargo clippy -p loom-can --all-targets -- -D warnings` ✓ · `cargo test -p loom-can` ✓ — **gateway 13 + cli 6 통과**(전체 41: lib 5, worker 17 포함), gateway 시험 0.17 s.
- `unsafe` 없음, Python 없음, 새 의존성 없음(tokio 없이 동작 — 아래 "대기").
- 스킬 `skills/loom-can/SKILL.md` 34줄(≤ 60).

## gateway.rs 내용
| 항목 | 구현 |
|---|---|
| 명령 | `can.send`(write `Request`, `RoleSet::WRITE`), `can.trace.query`, `can.last`, `can.expect`, `can.status`(R0, `RoleSet::READ`). 모두 `default_action: None`(F1 자동) |
| `can.send` | 채널 결정 → `SendParams` 파싱 → `resolve_frame()` 실패는 `E_USAGE`(worker 호출 없음). `dry_run`은 `{dry_run, channel, frame, bytes, count, risk, reason, policy:"auto"}`만 반환. 실제 송신은 `ctx.worker("can_sim").write(rid, params, deadline)`. `expect`가 있으면 **이 요청의 tx 에코 seq 다음부터** 대기(이전 응답 프레임을 근거로 쓰지 않음) → `write_state: effective_verified`/`effect_unverified` + evidence, 그리고 `can.tx` 행 publish(`env: sim`, `purpose: agent_send`) |
| `can.trace.query` | params: `channel, since(기본 @last), until, ids(0x300·768·0x7E0-0x7EF), origins, dir, max(기본 100, 1..=1000), peek`. `can.frame`+`can.tx`를 읽고, 행 수·**직렬화 64 KiB** 둘 다로 자른다. 스캔은 최대 8회(각 4 MiB, `resume_from`으로 이어감) — 예산을 다 쓰면 `truncated: true` + 이어갈 cursor. 결과 `{rows, next_cursor, truncated, range{from,to}, complete, gaps, peek}`. gap이 있으면 `complete: false` |
| `can.last` | 최근 `window_ms`(기본 10 000, 최대 60 000)를 앞으로 스캔해 (id, ext)별 마지막 프레임 + 개수. id 2 048개 초과분은 `ids_not_tracked`로 센다. 결과도 1 000행/64 KiB 상한 |
| `can.expect` | `{channel?, id, ext?, data?, mask?, origin?(기본 device), dir?, after?(기본 now), within_ms 1..=60000}`. 10 ms 저널 폴링, 일치하면 즉시 반환: `{matched, frame, evidence:[can.frame, seq 범위 n..n, source_refs], refs:["can:ch0@e1:seq:n"]}`. 시간 초과 → `E_TIMEOUT`(exit 3), detail `{frames_scanned, evidence:[can.wait_timeout, 실제 본 첫..마지막 seq], refs}`. **본 프레임이 없으면 evidence를 비워 둔다**(seq를 지어내지 않음). 대기 시작 시 `core.activity{action:"wait", waiting:{predicate:"can.frame_match", args, within_ms}}` 한 번 |
| 이벤트 종류 | `can.frame`(schemars `FramePayload`, 포인터 `/payload/id` U32, `/ext` Bool, `/fd` Bool, `/dlc` U8, `/len` U8, `/bytes/data` Hex), `can.tx`(`TxPayload`, `/payload/write_state` Enum 8개, `/frame/id`, `/count`), `can.channel_state`(`/payload/state`, `/payload/backend`), `can.stats`(`/payload/fps` F64) |
| 위험·hooks | `classify`: count > 10 → R3 "finite burst > 10 frames", env sim → R1 "sim channel", 그 밖 R2(bench = vehicle, B4: env로 등급을 바꾸지 않음 — 시험으로 고정). `presend_check`: generation `Some(0)`, 채널 state ≠ open, generation 불일치 → `EpochChanged`. 모르는 채널은 통과(worker가 `E_NOT_OPEN`) |
| reducer | `can.frame`·`can.channel_state`. 채널별 (id, ext) 마지막 프레임·개수 + 채널 상태. 채널 8개 / id 2 048개, 넘치면 가장 오래 안 본 것 교체하고 `ids_replaced`/`channels_replaced`로 센다. snapshot 항목 `{kind:"can.fixed", channel, state, ids:[{id,ext,count,last}], ...}`, SubFilter 채널로 거른다 |
| verifier | `can.frame_match{id, ext?, data?, mask?, dir?, origin?}`, coverage `Positive`. 일치 → `Match`, 없음+완전한 창 → `Mismatch`, 없음+불완전한 창 → `Inconclusive`, 인자 오류 → `Inconclusive`("false"로 만들지 않음), 예산 초과 → `Inconclusive("eval_budget")` |
| WorkerSpec | kind `can_sim`, `Shared`, mem 192/256 MiB, **credit 2 MiB**(결정 — 아래), `Reopen::RxOnly`, `Native`, Windows/Linux/macOS |
| HelpSpec | 요약, 예시 7개(status, send, send --expect, dry-run, trace, last, expect), notes(다른 도구로 장치 열지 말 것, viewer 없으면 UI 열기 요청, evidence 인용, 출력 상한, M2 목록) |
| capability | `sim` |

## cli.rs 내용
- `loom can send <frame> [-c ch] [--ext] [--count 1..=100] [--gap 0..=10000] [--expect ID [--within MS]] [--dry-run]` — 프레임은 `loom_can_frame::parse_frame`으로 미리 검사(`E_USAGE` exit 2). `--within`만 있으면 오류. `--expect` 기본 100 ms, id > 0x7FF면 ext.
- `trace [--since C] [--until C] [--ids a,b-c] [--origin a,b] [--dir rx|tx] [--max N] [--peek]` — `cursor_arg` 사용, id/범위는 정규화(`0x7E0-0x7EF`).
- `last [--id X] [--window MS]`, `expect --id X [--ext] [--data HEX --mask HEX] [--origin O] [--after C] --within MS`, `status [-c ch]`.
- 대기 명령은 `RpcCall.wait = within + 2 s`.
- 사람용 출력: `812.004  tx AGT 0x300       1  01  sent` 형식. 같은 요청의 `can.tx` 상태는 에코 줄에 합치고(UI처럼 요청당 한 줄), 에코가 페이지에 없으면 마지막 상태 행만 한 줄. 끝에 요약 `-- N rows · next <cursor> · truncated (...) · INCOMPLETE`. send/expect/last도 각자 형식, 시간 초과는 `scanned: can:ch0@e1:seq:10..14` 뒤 `render_common` 오류 줄. `--json`은 cli-kit 그대로.

## 시험
- `tests/gateway.rs`(13): `RegistryProbe::assert_ok` + 등록 내용(포인터, 예산, F1, verifier, help), `HookHarness`로 classify/presend(재열기·닫힘), trace 필터·페이지(중복·누락 없음)·64 KiB 상한·usage 오류·gap→incomplete·activity(agent만, viewer 제외), last, expect 일치(이전 프레임·agent 프레임·데이터 불일치 무시)/시간 초과(창 evidence, 빈 버스면 evidence 없음)/usage, send dry-run·usage가 worker에 안 감, send+expect verified(이전 응답 seq를 인용하지 않음)/unverified, 채널 기본값·`E_AMBIGUOUS_CHANNEL`, reducer 상한, verifier 5가지 결과. 가짜 `CtxServices`/`JournalReader`/`WorkerHandle`과 작은 `block_on`을 시험 안에 둔다.
- `tests/cli.rs`(6): to_call 변환·경계값(clap 거절 vs usage 거절 구분), `run_cli_with` exit 코드(2, 그리고 클라이언트 미연결 1), trace/send/expect/last/timeout 출력 golden.

## 결정 (리드 확인 바람)
1. **채널을 RPC에서 선택 사항으로**: `channel` 없으면 열린 채널이 하나일 때 그것, 여럿이면 `E_AMBIGUOUS_CHANNEL`, 모르면 `ch0`. `SendParams`는 `channel`이 필수라 gateway가 채워 넣고, 등록 스키마에서 `required`의 `channel`만 뺐다(`schema.rs`는 그대로). 채널 표는 reducer가 `can.channel_state`로 갱신하고 hooks·handler가 함께 읽는다(`Arc<Mutex<..>>`).
2. **credit 2 MiB**: 과제에 값이 없어 sim worker 한 개의 미확인 바이트 상한으로 잡았다(1 000 fps × 약 400 B 행이면 약 5 s 분량). 실측 뒤 조정.
3. **대기 구현**: tokio 의존성이 없어 `gateway.rs` 안에 공유 타이머 스레드 하나(`OnceLock` + 힙 + `Condvar`, 대기 4 096개 상한, 넘치면 즉시 양보)로 `sleep` future를 만들었다. 런타임과 무관하게 동작한다. CORE `ctx.wait`가 생기면 통째로 지운다.
4. **`now`/상대 cursor 고정**: 저널이 `now`·`-5s`를 스캔마다 다시 계산한다고 보고, 대기 루프에서는 "호출 시작 + 경과"를 `Relative{ms}`로 넘기고, seq를 하나라도 보면 `seq:(마지막+1)`로 바꾼다.
5. **`next_cursor`는 `e<epoch>:seq:<다음 행>`**: `JournalQuery.from`이 포함(inclusive)이라고 가정했다. 잘린 경우 잘린 첫 행, 아니면 마지막 행 + 1.
6. **`core.activity` payload**: `ActivityPayload{client_id, method, summary, range?, waiting?}`에 `action`·`domain`·`channel`을 더해 publish(can-ui.md 형식과 섞음). viewer 역할은 publish 안 함, publish 오류는 무시.
7. **send+expect의 `can.tx` 행**은 gateway가 publish한다(검증 결과는 gateway만 알기 때문). `env`는 M1에 sim 고정.

## 요청 (CORE / 리드)
1. **`CallCtx::wait`**(can-core.md 1번 그대로) — 들어오면 폴링과 타이머 스레드를 걷어낸다.
2. **cursor/peek API**: `CallCtx`에 `@last` 읽기/전진이 없어 `peek`은 받아서 돌려주기만 한다. 누가 `@last`를 옮기는지(core가 `next_cursor`로?) 정해 주세요.
3. **meta 끌어올리기**: `data.next_cursor`·`truncated`·`range`를 core가 envelope `meta`로 옮기는지 확인. 지금은 `data`에만 있다. `range`는 proto `SeqRange{from,to}` 모양 — can-ui.md의 `{seq_a, seq_b}`와 다르니 UI와 하나로 맞춰야 한다.
4. **도메인이 `core.activity`를 publish해도 되는지**, 그리고 payload 정본(proto `ActivityPayload` vs can-ui.md `{action,domain,channel,range}`).
5. **`ScanResult`에 스캔한 high-water seq**: 일치 프레임이 하나도 없을 때 시간 초과 evidence에 "어디까지 봤는지"를 넣을 수 없어 지금은 evidence를 비운다.
6. **`JournalQuery.from`이 inclusive인지** 계약에 명시(5번 결정의 전제).
7. **worker 쪽 필드 이름 맞춤**(worker 담당과): `can.channel_state` payload의 `state`("open" 등)·`backend`("sim")·`generation`(또는 top-level `generation`), `can.stats`의 `fps`. gateway는 이 이름을 읽는다.
8. **`lib.rs`(리드 소유)**: 바꿀 것 없음. `gateway`·`cli` 모듈 공개 항목은 `Can`, `CanCli`, `CanHooks`, `FixedReducer`, `CanVerifier`, `PRED_FRAME_MATCH`, 상한 상수들.

## 남은 일 / 알려진 한계
- 실제 daemon(`loom-gateway`)이 아직 `CtxServices`를 구현하지 않아 끝단 시험은 가짜 서비스로만 했다. cli-kit 클라이언트도 아직 `E_NOT_IMPLEMENTED`(exit 1)라 `run_cli_with` 시험은 그 상태를 고정한다 — 연결되면 시험을 바꿔야 한다.
- 폴링 대기라 응답 지연이 최대 약 10 ms 늘어난다(목표 p95 < 50 ms 안).
- `can.last`는 앞으로 읽는 창 스캔이라 창 안 프레임이 8 000개(스캔 8회 × 1 000)를 넘으면 `complete: false`가 된다.
- M2: `sig`, `check`, `stats`, periodic — 스킬에 M2로 표시만 했다.
