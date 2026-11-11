# M2a 보고 — 주기 송신 연결: gateway·CLI·DBC 쪽 (can-opus)

작성: can-opus, 2026-11-11. 과제 `docs/impl/can/tasks/m2-periodic-wire.md`의 can-opus 몫. 커밋 안 함.
범위: `src/gateway.rs`, `src/cli.rs`, `src/signals.rs`, `tests/gateway.rs`·`tests/cli.rs`·`tests/signals.rs`·`tests/common/mod.rs`(가짜 worker에 응답 스크립트 추가), `skills/loom-can/SKILL.md`. **`worker.rs`·`periodic.rs`는 손대지 않았다**(can-sol 작업 중).

## 결과
- can-sol 작업 중인 파일 때문에 worktree 그대로는 clippy가 막혀(그쪽 `periodic.rs`의 unused import) **스크래치 복사본**(같은 내 파일 + 커밋된 `worker.rs`, can-sol 파일 제외)에서 검사: `cargo fmt --all --check` ✓, `cargo clippy -p loom-can --all-targets -- -D warnings` ✓, `cargo test -p loom-can` ✓ **78개**(lib 7, cli 13(+2), gateway 17(+3), isotp 8, signals 16(+5), worker 17).
- 실제 worktree(can-sol 진행 중 코드 포함)에서도 빌드·시험 통과(clippy만 그쪽 import 1건).
- 포맷은 내 파일만 `rustfmt`로 했다. 단, 이번 작업 초반에 `cargo fmt -p loom-can`을 몇 번 돌렸는데 그때 can-sol이 `worker.rs`를 이미 고치고 있었다면 그 파일에 **공백 정리만** 생겼을 수 있다(내용 변경은 없음).

## 구현
- **RPC 등록**: `can.periodic.start|update|stop|renew|takeover` = write **`WriteClass::Job`**(A1, 클래스가 있어 그대로 사용), roles WRITE. `can.periodic.list` = R0. `can.sig.set` = write `Job`. 모든 write에 risk row(RegistryProbe 경고 0).
- **classify**: 메서드를 먼저 본다(주기 `count`는 burst가 아님). periodic·sig.set → sim R1 "sim channel", 실HW R3 "periodic"(F1: 표시만), `stop` → R0. `can.send`는 기존 규칙 그대로(시험으로 고정).
- **`can.job_state`** kind 등록, 포인터 `/payload/job`(String), `/payload/state`(Enum started|patched|renewed|paused|stopped).
- **`can.periodic.start`**: raw(`frame`: cansend 텍스트 또는 `can.frame` payload 객체, `ext`) 또는 DBC(`msg`, `signals{Sig: 숫자|라벨}`, `force_range`). DBC는 `EncodeBase::Initial`(DBC 시작값) + 범위 검사, 기본 주기 = `GenMsgCycleTime`(없으면 `period_ms` 필수). 검사(주기 1..=60000, offset ≤ 60000, count ≥ 1, lease/max_duration 1..=24 h, owner ≤ 128 B, remote 프레임 거절) 후 worker write op `{method:"can.periodic.start", channel, frame: FramePayload, period_ms, offset_ms?, count?, lease_ms?, max_duration_ms?, owner?}`. 결과에 `request_id, channel, frame, period_ms, period_from_dbc, msg, db_sha256, risk, reason`을 붙인다.
- **update/stop/renew/takeover**: 파라미터 검사(update patch는 `{data}` 또는 같은 길이 `{mask, bytes}`, stop은 `job` xor `all_mine:true`, 모르는 키 거절) 후 `{method, …}` 그대로 worker write.
- **`can.sig.set{channel?, signal, value, job?, force_range?}`**: `periodic.list`에서 그 메시지(id+ext)의 잡을 찾고(없으면 `E_NOT_FOUND` + hint `loom can periodic start --msg M --channel ch first`, 지정한 job이 다른 메시지면 `E_USAGE`), 잡의 최신 바이트를 기준(`EncodeBase::Payload`)으로 그 신호만 인코드 → `can.periodic.update{expected_rev}`. **revision 충돌(`detail.current_rev`)이면 다시 읽어 1회 재시도**, 두 번째도 충돌이면 worker 오류 그대로. 결과 `{job, revision, previous_revision, data, changed:[{byte, old, new}], base, risk, reason, signal, value, unit, …}`.
- **CLI**: `loom can periodic start <FRAME>|--msg M [--set Sig=V …] [--period MS] [--offset MS] [--count N] [--lease S] [--max-duration S] [--owner] [--force-range]`, `periodic update JOB --rev N (--data HEX | --mask HEX --bytes HEX)`, `periodic stop JOB|--mine`, `renew JOB`, `takeover JOB`, `list [-c]`, `loom can sig set Msg.Sig=V [--job J] [--force-range]`(숫자는 물리값, 그 밖은 라벨). 출력 한 줄씩, 예: `started j1 on ch0: 100#E02E… every 10 ms (DBC cycle time) (rev 0)  risk=R1 (sim channel)`, `Gear = "D" → j1 rev 4  (bytes 3:04→06)  risk=R1`.
- **스킬**: "Periodic / IG" 절(job id 기억, `sig set`, 끝에 `periodic stop --mine`, STOP 뒤 자동 재개 없음, 사용자 IG 변경은 `others_since_last`/`NOTE:`). M2b 목록에서 periodic·sig set 제거. 47줄.

## can-sol과 맞춘 점 (확인함, 읽기만)
- write op에 `method` 필드로 구분 — can-sol `periodic.rs`도 `w.op["method"]`로 분기한다. ✓
- revision 충돌: can-sol은 `E_LOCKED` + `detail{current_rev, reason:"revision_conflict", code:"E_CONFLICT"}`. 내 재시도는 코드가 아니라 `detail.current_rev`로 판단하므로 그대로 맞는다. ✓
- `list` 항목: can-sol은 `data`가 없고 `frame`(FramePayload)·`revision`·`effective_revision`을 준다. sig.set 기준 바이트 우선순위: `data` → `frame.bytes.data` → 저널의 그 잡 마지막 프레임(`job_id`) + 경고. **`effective_revision < revision`(패치가 다음 tick을 기다리는 중)이면 `E_BUSY`(재시도 가능)로 거절** — 옛 바이트로 인코드해 대기 중 변경을 덮어쓰는 일을 막는다.

## 결정·요청
1. **CORE: `E_CONFLICT` 오류 코드 추가 요청**(계약에 있으나 proto에 없음). 지금은 worker가 `E_LOCKED`(exit 6) + `detail.code`로 낸다. 생기면 sig.set은 그대로 동작(detail 기준).
2. **can-sol: `list` 항목에 최신 revision의 바이트(`data`, 대기 중 패치 포함) 추가 제안** — 있으면 패치 대기 중에도 sig.set이 바로 된다(지금은 `E_BUSY` 후 다음 주기에 재시도).
3. sig.set 결과의 `revision`은 worker update 결과의 `revision`을 그대로 쓴다.
4. DBC 메시지로 시작한 잡은 gateway가 메시지 이름을 기억하지 않는다(id+ext로 찾음). 같은 id 잡은 채널당 하나(E_LOCKED)라 모호하지 않다.
5. `can.periodic.start`에 `dry_run`은 넣지 않았다(과제 범위 밖). 필요하면 `can.send`처럼 risk·bytes만 돌려주게 추가 가능.
6. 실HW가 생기면 `classify_periodic`의 R3는 env로 등급을 올리는 것이 아니라 "주기 송신 = R3" 규칙이다(B4와 충돌 없음: bench/vehicle 같음).
