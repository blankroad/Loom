# logic backend — 상태 (Opus backend)

2026-11-14. 범위: `crates/loom-logic/{Cargo.toml, src/model.rs, src/worker.rs, src/gateway.rs, src/lib.rs, tests/engine.rs}`, `crates/loom-gateway/{Cargo.toml, src/domains.rs}`(feature `logic`: 기본 활성, inproc + `loomd worker logic`). CLI·skills는 Sol 소유로 손대지 않았다. Git index는 건드리지 않았다. 빌드는 `/tmp/loom-logic-build` 단일 캐시(`CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0`)만 사용했고, 전체 workspace·release 빌드는 하지 않았다. 루트 `Cargo.toml`은 변경하지 않았다(tokio는 기존 workspace dep). `Cargo.lock`은 cargo가 생성한 변경만 있다.

## 구현

- **model**: 엄격한 start 검증(unknown key, 타입·범위, allowlist; clamp 없음). 8ch/1 B 고정, acquisition `-C D0..D7`. 사용자 channels는 표시 mask일 뿐이다. 디코더는 uart(rx|tx 중 정확히 1)와 spi(4 mode, bitorder, cs, annotate)이고 `-A` data class filter를 쓴다. sigrok-device는 `fx2lafw`만 통과하며 미검증이다. saleae-logic16/-pro는 `E_NOT_IMPLEMENTED`, 그 밖의 driver나 `:` 옵션은 `E_USAGE`. demo `pattern`은 libsigrok 제약 때문에 200 kHz 고정이다(`-g Logic`와 samplerate는 같이 쓸 수 없다. 0.7.2에서 실측). 합성 파형은 run-length segment로 만들어 sample별 메모리를 쓰지 않는다. clock_hz는 연산 전에 범위를 검사한다(B1 반영).
- **worker**: 캡처는 1개씩 실행한다. 전용 thread가 다음을 맡는다: supervisor, acquisition reader(64×64 KiB 제한), decoder stdin writer(4 MiB 제한, 초과 시 sticky failed), decoder line reader(≤4 KiB 줄, 1024행 큐, overflow 계수), annotation batcher(≤256행/≤47 KiB/50 ms/2000 행/s), stderr ring(64 KiB, tail 4 KiB).
  - STOP 순서: acquisition stdin에 newline → 2 s → group SIGINT → 1 s → group SIGKILL → reap. 이어서 decoder stdin EOF → drain ≤2 s → group SIGKILL → reap.
  - 정리: thread 생성 실패는 명시적인 failed와 정리로 처리한다. worker Drop은 stop과 join을 수행한다. latch가 걸리면 `stop_latch`로 멈추고, 이후 agent의 start는 `E_STOPPED`로 거부한다. engine이 없으면 `E_NOT_FOUND`이고 fallback은 없다.
- **gateway**: engine 경로는 `loom.toml [logic] sigrok_cli`(절대경로)나 PATH와 brew 경로 검색으로만 정한다. RPC 파라미터로는 지정할 수 없다. status는 compiled 목록과 implemented 목록을 구분해 보여 준다. read/wait는 journal 기반이고 제한이 있다(4 MiB scan, ≤1000행/64 KiB). reducer snapshot은 captures 4개, annotations 200개, edges ≤64 KiB이다. `logic.stop` roles는 READ이다.

## 검증 (실행 결과)

- `cargo test -p loom-logic`: unit 15(model 7, worker 2, Sol cli 6)와 `tests/engine.rs` 9가 모두 통과했다. engine 테스트는 2회 반복 실행해 모두 통과했다(각 약 2.6 s).
  - 실제 sigrok-cli 0.7.2: synthetic UART "LOOM 0001"이 **STOP 전**에 decode됐다(state running). edges sample축에 빈 곳이 없고, samples == decoder_input_bytes, class null, decoder `uart-1`, per-bit annotation 없음.
  - synthetic SPI: cpol/cpha 4 mode × msb/lsb 8조합 모두 기대 bytes가 decode됐다.
  - sigrok-demo graycode: 실제 acquisition이다. key STOP rc 0, <3 s, dropped_edges>0이고 `edges_dropped` quality가 붙는다.
  - demo + spi tee + max_samples: 300000 = edges 합계 = decoder 입력이며, demo bytes가 실제로 decode됐다.
  - fake engine: exit 3 + stderr → failed이며 stderr_tail에 내용이 남는다. stdin EOF를 무시하고 자식이 pipe를 쥔 decoder → group kill, <6 s, 잔여 프로세스 0.
  - engine 없음 / 잘못된 경로 → `E_NOT_FOUND` + failed state. Busy, latch, 재시작, Drop 정리 확인.
  - payload 경계: 11자리 sample 최악 edges, 4 KiB 제어문자 annotation 행 모두 <48 KiB.
- `cargo clippy -p loom-logic --all-targets`: 내 파일은 clean이다. Sol `cli.rs:295`에 collapsible_if warning 1개가 남아 있다.
- `cargo build -p loom-gateway -p loom-cli`과 `cargo test -p loom-cli` 통과.
- **daemon E2E**(격리된 LOOM_HOME에서 foreground 실행): `loom logic status` → `start --source synthetic --decode uart:rx=D0:baudrate=115200` → `wait --ascii "LOOM 0001"`이 STOP 전에 매칭됐다 → `read --kind annotation --tail 5` → `stop`. demo graycode 3 s는 duration으로 멈췄고 종료 후 남은 sigrok-cli 프로세스는 없었다. loomd log에 error나 panic은 없다.

## 남은 것 / 정직한 한계

- 실장비(fx2lafw 포함)는 검증하지 않았다. USB scan이나 firmware 작업은 하지 않았다. 16ch 장치는 거부한다.
- 고속 demo(≥1 MS/s에서 매 sample이 토글)에서는 edges가 대량으로 dropped된다. 계수와 `final` 재동기화로 드러나도록 했지만 파형이 완전하지는 않다. 고밀도 디코드 annotation도 2000/s를 넘으면 dropped된다.
- gateway read/wait handler는 daemon E2E로만 검증했다. 전용 Rust 단위 테스트는 없다.
- `complete`는 tail이나 64 KiB 상한으로 잘린 행이 있으면 false이다(journal 손실과 같은 플래그를 쓴다. `rows_left_out`으로 구분한다).
- `sigrok-device` 실행 경로(conn 포함)는 argv 테스트만 있다.

## Sol 최종 리뷰(W1/G1–G4) 반영

- W1: writer join 뒤 `broken` 여부와 `decoder_input_bytes == tee.sent`를 검사한다. 하나라도 어긋나면 failed/error이다. 테스트: decoder가 1000 B만 읽고 exit 0 → failed, 그리고 input < samples.
- G1: reader queue drop과 rate drop을 다음 행의 `gap_before`로 붙인다(EOF 시 marker 사용). gateway `Matcher`는 gap, loss 행, journal gap에서 window를 reset한다. 테스트: 손실 없이 이어지면 매치, 사이에 구멍이 있으면 매치 안 함, scan 사이 reset.
- G2: reducer에 annotation 16 KiB, edges 24 KiB byte cap과 `evicted` 수를 넣었다. 테스트: 최악 행 300개 이상을 넣어도 snapshot <64 KiB.
- G3: rows 예산을 56 KiB로 해 headroom을 뒀다. 테스트로 확인.
- G4: follow wait_ms=0, timeout_ms=0은 E_USAGE.
- 결과: `cargo test -p loom-logic` unit 15 + engine 10 + gateway 3 = 28개 통과. clippy는 내 파일 clean. `cargo build -p loom-gateway -p loom-cli` 기본 binary를 `/tmp/loom-logic-build/debug/{loomd,loom}`에 다시 빌드했다.

## 최종 (위임: cli.rs clippy collapsible_if 1곳만 수정, CLI 동작 변경 없음)

- `cargo clippy -p loom-logic --all-targets`: warning 0 / error 0.
- `cargo test -p loom-logic`: unit 15 + engine 10(real sigrok-cli 0.7.2 + fake engine) + gateway 3 = **28 passed, 0 failed**.
- `cargo build -p loom-gateway -p loom-cli` (dev, `/tmp/loom-logic-build`), 2026-11-14 15:26:
  - `loomd` SHA256 `1a10654b5ff80fe5794d958d328fb7fa42cbd09321a1e91c06baeb9f15b19db1`
  - `loom`  SHA256 `af4ef798f8476310ae7345d25858b2bac4df9000381791e0c3ab6d697cc33726`
- 상태: idle. CLI/skills 소유권은 ROOT에 있다. Git index는 건드리지 않았다.

## UI P2 fix (ROOT 위임: ui/src/panels/logic/{waveform.ts, logic.css, logic.test.ts}만)

- 축 tick: `tickLabel`은 tick step을 라벨 단위로 표현할 수 있을 만큼 소수 자릿수를 동적으로 정한다(예: 2.7 s 근처, 50 µs step → `2.70005 s`). s/ms/µs/ns 정보를 잃지 않는다. tick은 누적이 아니라 곱셈으로 계산한다. 라벨 폭(≈6.5 px/글자)이 tick 간격보다 넓으면 n번째 tick에만 라벨을 붙이고, grid 선은 모든 tick에 남긴다.
- 선택된 annotation 행: `.lg-panel .vt-row[aria-selected='true']`에 #d6e6ff 배경과 #10213a 글자색을 준다(logic 패널에만 적용).
- 회귀 테스트: 1 MHz에서 2.7 s 근처 300 µs span의 라벨이 서로 다르고 간격이 라벨 폭 이상인지, ns·sample-index 축, CSS scope와 색을 검사한다.
- UI: vitest 53 files / **590 passed**, `tsc -b` 통과, `eslint .` clean, `vite build` OK(chunk-size 경고는 기존에도 있던 것).
- 재빌드: `cargo clean -p loom-ui-assets` 후 `cargo build -p loom-gateway -p loom-cli` 실행. 새 dist(`index-D8vwd_Ig.js`, `index-fQR6Z3vA.css`)가 loomd에 포함된 것을 문자열 검사로 확인했다.
  - `loomd` SHA256 `c171b78a7f5d554feda313eab752a351618028887ea1ee243f35323f407ca888`
  - `loom`  SHA256 `af4ef798f8476310ae7345d25858b2bac4df9000381791e0c3ab6d697cc33726`(변경 없음)
- CLI/worker/gateway source는 변경하지 않았다. 상태: idle.

## 마지막 리뷰 2건 (ROOT 위임: ui logicModel.ts/logic.test.ts + Rust edges cap)

- UI CaptureBook: newest 판정을 `seq >= newestSeq`로 바꿨다(snapshot은 같은 H를 쓰고 오래된 것부터 온다 → 마지막이 최신). list()는 seq가 같으면 나중에 들어온 것을 앞에 둔다. live record는 seq가 고유해서 동작이 그대로이다. 회귀 테스트: 같은 H로 3개 → newestId=demo3, 순서 [demo3, uart2, uart1]. 이후 live H+1이면 그것이 최신이고, H-5는 최신이 되지 않는다.
- Rust: `MAX_EDGES_PER_EVENT` 2048 → **1024**. 경계 테스트: 최악 producer payload(11자리, 1024 edges) ≤ `SNAP_EDGE_BYTES`(24 KiB)이고 reducer가 버리지 않는다(evicted 0).
- 검사: `cargo test -p loom-logic` 15+10+4 = **29 passed**, clippy clean. UI vitest 53 files / **591 passed**, tsc 통과, eslint clean, vite build OK. ui-assets를 clean한 뒤 재빌드했고, 새 dist(`index-DVbGndTA.js`)가 loomd에 포함된 것을 확인했다.
  - `loomd` SHA256 `67aa971d85913d4780edbd120306d6c71525f58b6a143afb36a7571bac1e84cb`
  - `loom`  SHA256 `bbd99f5e2be50de320809ac09869ab9d5719992c5bc647154add7f28be815376`
- 상태: idle.

## 최종 정리 (형식만, 기능 변경 없음)

- `rustfmt --edition 2024`(workspace rustfmt.toml: max_width 110)를 `crates/loom-logic/src/*.rs`, `tests/*.rs`에만 적용했다. 범위를 좁힌 `--check`는 clean이다.
- `crates/loom-cli/src/lib.rs`: 이번에 추가한 `cases.push` logic hunk만 rustfmt 모양으로 줄바꿈했다. 기존 baseline의 미포맷 부분은 그대로 두었다. 전역 `cargo fmt --all --check` 실패는 기존 baseline에서 온 것이다.
- `git diff --check`(logic, cli, gateway, ui logic 경로)는 통과했다. 새 crate(untracked)에 trailing whitespace는 없다.
- 최종 `cargo build -p loom-gateway -p loom-cli`. ui/dist `index-DVbGndTA.js`가 포함돼 있다. 테스트는 기능 변경이 없어 다시 돌리지 않았다(ROOT 지시).
  - `loomd` SHA256 `110bd61811c4279ef057c77fa16f49d545b62227b694f3bd8184424d12ef077a`
  - `loom`  SHA256 `5299254a4306f5082f18742729f1dfa8c7d8fe1c9d4b5e90d728dbbaf53e15b8`
- commit/cleanup은 ROOT가 넘겨받는다. 상태: idle.
