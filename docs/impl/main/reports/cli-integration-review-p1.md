# CLI 기본연결 / skill sync 독립리뷰 (P1)

- 리뷰어: Herdr foreground Claude Opus, 새 컨텍스트, 하위 agent 없음
- 기준: main 43588e5 + 미커밋 root CLI 변경 (`crates/loom-cli/{Cargo.toml,src/lib.rs,src/skills.rs,tests/cli.rs}`, CAN/ETH/PSU `cli`/`runtime` feature 분리, ETH `idempotency_key` id)
- 리뷰어는 source/manifest/lock/index를 수정하지 않았다. 이 보고서 파일만 썼다. 수정은 전부 main이 했다.
- 재현은 모두 scratchpad로 복사한 바이너리와 일회용 `LOOM_HOME`(`sim://bootshell`, CAN sim)으로 했고, 띄운 daemon은 모두 종료했다.
- 증거 주의: main은 `/private/tmp/...final-test.log`를 동시 redirect 충돌로 폐기했다. 전체 CLI 도메인 검사는 `/private/tmp/loom-cli-delivery-margin-regression.log`로 다시 돌고 있다. 리뷰어는 이 로그를 보지 않았다. 아래 "재리뷰" 결과는 리뷰어가 직접 실행한 반례에 한정된다.

## 상태 요약

| 항목 | 원본 리뷰 (00:37 빌드) | 재리뷰 1 (00:42 빌드) | 재리뷰 2 (00:45/46 빌드) |
|---|---|---|---|
| MAJOR-1 도메인 timeout → transport 결합 | 발견, 재현됨 | 해결 확인 | 해결 유지 |
| MAJOR-2 도메인 `call.wait`에 reply margin 없음 | (미발견) | 발견, 재현됨. 신규 회귀 테스트 UART 케이스 실패 예상 | 해결 확인 |
| MINOR-1 `context()` 문자열 parse 복제 | 발견 | 해소 (복제 제거) | 해소 |
| MINOR-2 `--symlink` 기본 source = 빌드 repo 경로 | 발견 | 변경 없음, minor 유지 | 변경 없음, minor 유지 |
| MINOR-3 `MAX_WAIT` clamp가 최대 deadline에서 margin을 없앰 | (미발견) | (미발견) | 발견 (코드 근거, 1시간짜리라 미실행) |

현재 판정: blocker 0, major 0 (열린 항목 기준), minor 2 (MINOR-2, MINOR-3).

---

## 원본 리뷰 (00:37 빌드, 소스 최종수정 00:34)

결론: blocker 0 / major 1 / minor 2.

### MAJOR-1 도메인 연산 timeout이 transport(connect/hello) timeout으로 새어 정상 daemon을 "안 떠 있음"으로 오판하고 두 번째 loomd를 띄우려 함

당시 코드 위치: `crates/loom-cli/src/lib.rs` `context()`(도메인이 shadow한 `--timeout`을 `u64` 초 / `Duration` / `u32` ms / `String` 순서로 해석해 `ctx.timeout`에 넣음), `:699` `connect(.., ctx.timeout, Start::Auto)`, `:719` `core_envelope(.., ctx.timeout)`, `client.rs:108-128,196-198,298,349`.

원인: 도메인의 연산 deadline(CAN `u32` ms `1..=60000`, ETH/CAN-wait `Duration`, TEST 문자열, UART `u64` `0..`)이 `Conn::open` read timeout과 `core.hello` 대기에 쓰였다. hello가 실패하면 `try_existing`이 None을 반환하고, 살아있는 daemon이 있는데도 `spawn_daemon`을 호출한다. 새로 뜬 loomd는 거부되고, CLI는 `START_WAIT` 동안 기다린다.

당시 재현 (healthy daemon):

```
loom --json can isotp recv ch0:7e8 --timeout 1
  → 10.03s 후 E_DAEMON "the Loom daemon did not come up"
loom --json uart expect dut ok --timeout 0
  → 10.03s 후 동일 E_DAEMON
loomd.log: "loomd: cannot start: another loomd is running for this LOOM_HOME" ×2
```

당시 단위테스트(`eth trace --timeout 25ms` → `ctx.timeout == 25ms`)가 이 결합을 계약으로 고정하고 있었다.

권고: transport와 연산 timeout을 분리한다(transport에는 루트 초만, 도메인 deadline은 `RpcCall.wait`).

### MINOR-1 `context()` 문자열 분기가 loom-test `duration_ms`를 복제

다른 문법의 도메인이 들어오면 조용히 30s로 떨어질 수 있었다. MAJOR-1을 분리 방식으로 고치면 필요 없다.

### MINOR-2 `skills sync --symlink` 기본 `--source`가 컴파일 시 `CARGO_MANIFEST_DIR`

패키지된 바이너리에서는 경로가 없어 `skill source unavailable`로 안전하게 실패한다. help 문구가 이를 밝힌다.

### 원본에서 문제없음으로 확인한 항목 (재검사하지 않음)

- feature 분리: `loom-cli`가 can/eth/psu/uds/test를 `default-features=false, features=["cli"]`로 사용한다. eth/psu `gateway`/`worker`/`sim`은 `runtime` cfg 뒤에 있다. ETH `live = ["runtime", ...]`.
- ETH `idempotency_key` id가 루트 global id와 같아 clap 중복 id 충돌이 없다.
- skills: 내장 파일 11개(7 SKILL.md + 4 references)가 `skills/` 트리와 일치한다. copy 모드는 쓰기 전에 전체를 계획하고, conflict면 아무것도 쓰지 않는다. `--force`는 copy만 교체하고 symlink 목적지/조상은 거부한다. 쓰기는 `create_new` 임시파일 + rename이다. project를 `canonicalize`해 macOS `/var` 경로를 처리한다. `skills` 분기는 daemon에 연결하지 않는다(autostart 없음).
- 기본 도메인이 실제 daemon에 도달하고 stop gate가 걸리는 것은 기존 로그와 `tests/cli.rs` `default_domain_commands_reach_real_daemon_and_stop_gate`로 확인했다. 단, 이 근거는 main의 이전 로그이고, final-test 로그 폐기와 무관한지는 리뷰어가 확인하지 않았다.

---

## 재리뷰 1 (main의 MAJOR-1 수정, lib.rs 00:42:21 / 바이너리 00:42:23)

main 수정 내용: `context()`는 `try_get_one::<u64>("timeout")`(루트/UART 초)만 transport에 쓰고, 0이나 비-u64 도메인 값이면 `DEFAULT_TIMEOUT`을 쓴다. 문자열 parse를 제거했다. 회귀 테스트 `tiny_domain_deadline_does_not_restart_a_healthy_daemon`을 추가했다(CAN 1ms, UART 0; 동일 PID, 3초 이내, autostart 없음, E_DAEMON 아님).

결과: MAJOR-1 해결, MINOR-1 해소. `status`의 `data.pid`는 실제 daemon pid를 반환하므로 PID assert는 의미가 있다. 새 major 1건이 나왔다.

### MAJOR-2 도메인 `call.wait`에 reply margin이 없어, 0 또는 짧은 deadline이면 RPC 대기가 deadline과 같아짐 → `E_DAEMON outcome_unknown`

당시 코드: UART `expect`/`run`의 `wait = Some(Duration::from_secs(t))`, `lib.rs:698` `call.wait.unwrap_or(ctx.timeout)`, `client.rs:148-152`(대기가 0이면 응답을 읽기 전에 실패). ETH wait-for/trace follow, PSU wait-for, TEST도 margin이 없었다. CAN/UDS/PSU watch에는 margin이 있었다.

당시 재현 (00:42 바이너리 복사본):

```
uart expect dut ok --timeout 0 (채널 없음)        → rc=9 E_DAEMON "no answer to uart.expect within 0 ms", outcome_unknown
uart expect dut NEVERMATCH --timeout 0 (sim 열림) → rc=9 같은 E_DAEMON
uart expect dut NEVERMATCH --timeout 1 ×5        → 5/5 E_DAEMON "within 1000 ms" (기대 E_TIMEOUT exit 3)
psu wait-for bench --timeout 1s --volt-gt 1000 ×3 → 3/3 E_TIMEOUT (통과하지만 같은 경합)
```

영향: 회귀 테스트의 UART 케이스는 `assert_ne!(.., "E_DAEMON")`에서 실패했을 것이다. 실사용에서는 UART timeout이 "결과 불명, 재전송 금지"로 보고된다.

---

## 재리뷰 2 (main의 MAJOR-2 수정, 바이너리 loom 00:45:54 / loomd 00:46:04)

main 수정 내용(diff 확인): 기기 params의 timeout은 그대로 두고, 각 fragment의 `RpcCall.wait`에만 reply margin 2초를 더했다. CAN/UDS의 기존 margin은 유지했다.

- UART `send` `t + d + 2`, `expect`/`run` `t + 2` (`REPLY_MARGIN_S`, `crates/loom-uart/src/cli.rs:19,273,314,325`)
- ETH `trace --follow` `wait + 2s`, `wait-for` `timeout + 2s`
- PSU `wait-for` `timeout + 2s` (watch는 기존 +10s)
- TEST `timeout_ms`/`wait_approval_ms` + 2s

리뷰어 재현 (00:45/46 바이너리 복사본, healthy daemon):

```
uart expect dut ok --timeout 0 (채널 없음)        → E_NOT_OPEN (E_DAEMON 아님)
uart expect dut NEVERMATCH --timeout 0 (sim 열림) → E_TIMEOUT "no match before the timeout"
uart expect dut NEVERMATCH --timeout 1 ×5        → 5/5 rc=3, E_TIMEOUT
can isotp recv ch0:7e8 --timeout 1               → 0.02s, E_NOT_OPEN, daemon_autostarted 경고 없음
status pid: 시작 전/후 동일 (99598)
```

결과: MAJOR-2 해결. main이 추가하겠다고 한 "열린 sim NEVERMATCH 1s → E_TIMEOUT" 회귀 케이스는 위 결과와 일치한다. 리뷰어는 그 테스트 코드는 아직 보지 않았다.

### MINOR-3 (신규, 코드 근거) 최대 deadline에서 `MAX_WAIT` clamp가 margin을 없앰

`crates/loom-cli/src/lib.rs:698` `call.wait.unwrap_or(ctx.timeout).min(MAX_WAIT)`, `MAX_WAIT = 3600s` (`loom-cli-kit/src/lib.rs:29`). 도메인 deadline 상한도 3600s다(UART `MAX_WAIT_S = 3600`, ETH/PSU `≤ 3600s`, TEST `MAX_WAIT_MS`). 따라서 `--timeout 3600`(또는 3599s 이상)이면 `deadline + 2s`가 3600s로 잘리고, 클라이언트 대기가 deadline과 같거나 margin보다 작아진다. 이때 MAJOR-2와 같은 경합으로 `E_DAEMON outcome_unknown`이 날 수 있다. 1시간짜리라 실행하지 않았다.

권고: 도메인 deadline 상한을 `MAX_WAIT - margin`으로 낮추거나, `lib.rs:698`의 clamp를 `MAX_WAIT + margin`으로 둔다. 실사용 빈도가 낮아 minor로 둔다.
