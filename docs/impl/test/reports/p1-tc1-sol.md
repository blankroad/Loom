# TC-1 실제 loomd·sim·저널 E2E 준비 — sol (2026-11-11)

## 현재 결과 보존
- **실제 TC-1 E2E 미실행(`not_run`). 실행 성공/PASS 주장을 하지 않는다.** CORE R1·sim 도메인 main 미합류는 예상 의존성; 반복 실행하지 않음.
- 작업 시작 main `c4f70a3`, 빌드/검사는 test/main `071f298` 기준. preflight 중 main ref는 `8fe47f3`으로 이동했으나 재병합·재실행하지 않음. core/main 직접 병합·타팀 변경·커밋 없음.
- 코드 위치: `.worktrees/test/crates/loom-test/e2e/{tc1.mjs,tc1.test.mjs,profile.json,PowerMode.dbc}`, `examples/tc1_manifest.rs`.
- Rust manifest exporter는 기존 format parser를 재사용해 TC 원문 해시·plan digest·5단계·finally를 내보낸다. 별도 YAML 파서/가짜 명령표 없음.
- 하네스는 Node 표준 subprocess/WebSocket만 사용. 실제 loom CLI start/step/plan.show/end와 같은 daemon의 domain/test RPC를 호출하도록 준비.
- 결과 REF/DTO는 `data.evidence[]`에서 그대로 복사. 검증 결과가 verified가 아니면 PASS 제안을 보내지 않고 차단한다.
- recovery: S1 done→S2/S3 검사→S4 증거 있는 fail→S5 clear/재조회/검사→finally→end. clean/STOP 변형도 분리.
- 저널 관문: 정상 종료→같은 격리 LOOM_HOME으로 재시작→새 epoch 확인→옛 epoch 범위 `core.events.query`를 페이지 조회(`source:journal` 필수)→실시간 행과 source identity/내용 대조.
- 후검사: 2초 OFF/ON, UART 30초·CAN 5초 anchor, 장치 witness·digest·완결성, S4 원래 fail 보존, finally 기록, STOP 뒤 자동 cleanup 부재.
- 고정 client_id, 임시 sim 전용 LOOM_HOME, 명령/전체 시간·출력·페이지·행/메모리 상한, no shell, 취소/unknown 결과 재송신 없음. 토큰을 transcript에 쓰지 않음.
- artifact: manifest·binary SHA256·main/test HEAD·help·RPC/CLI transcript·live/journal events·result.json. 준비/차단은 `tc_result:null`.

## 재현 명령 (test worktree 루트; 명령당 10분 이내)
```sh
export CARGO_HOME="$PWD/.cache/cargo" CARGO_TARGET_DIR="$PWD/target"
cargo build -p loom-test --example tc1_manifest -p loom-gateway --bin loomd --offline
node --test crates/loom-test/e2e/tc1.test.mjs
node crates/loom-test/e2e/tc1.mjs --mode prepare --out crates/loom-test/e2e/artifacts/current-main-preflight
# CORE R1·TEST·sim 연결이 main에 통합된 뒤만: 실제 바이너리 경로 지정, 매회 새 빈 artifact 디렉터리
node crates/loom-test/e2e/tc1.mjs --mode run --loomd /ABS/loomd --loom /ABS/loom --variant recovery --out crates/loom-test/e2e/artifacts/live-recovery
# clean / stop은 별도 격리 실행으로 --variant와 --out을 바꾼다. 기존 디렉터리는 덮어쓰지 않는다.
```
- prepare 차단 exit 8은 실제 TC 실패/성공이 아니라 의존성 미충족이다. run은 실제 실행·재시작 저널 대조까지 마쳐야만 결과를 확정한다.

## 지금 확인한 것 / 미검증
- exporter + test worktree loomd 빌드 성공; Node 문법·synthetic 하네스/거절 시험 10개 통과(실제 TC-1 아님). 담당 crate fmt·manifest example clippy(-D warnings) 통과; 전체 workspace 재검사는 미실행.
- predicate 연결은 `071f298`: TestModule 기본 PredicateEngine, from_registries로 실제 도메인 EvidenceVerifier/PointerSpec 주입. 기존 wire.rs 시험은 가짜 저널·UART/UDS 검증기라 실제 E2E가 아님.
- prepare 1회 결과: exit 8, status:blocked/live_e2e:not_run/tc_result:null. loom 바이너리 없음·loomd --env 실 런타임 없음. artifact는 TEST e2e/artifacts/current-main-preflight/에 보존.
- profile.json·PowerMode DBC는 sim setup 후보이며 실행 검증 안 됨. 기본 CAN Light_Status를 TC-1 PowerMode=2로 오인하지 않도록 별도 후보 노드 설정 포함.
- PSU와 UART bootshell/CAN/UDS sim의 전원·부팅 연동은 미확인. 독립 sim 기동이나 agent TX를 DUT 냉부팅 근거로 주장하지 않는다.

## 필요한 API·통합 요청 (R1 이후 대조; 지금 타팀 수정 안 함)
- CORE: loomd 실제 런타임·loom TEST CLI 등록/run_local, Registry CommandTable 실제 classify/schema, SessionHooks fence·기록 health·STOP·finally dispatcher·client/step 태그 연결.
- CORE: core.hello/status/help/events.query/shutdown/stop, 도메인 command schema 노출, U64 epoch/seq, 옛 epoch durable journal 재조회·source:journal·진행하는 next_cursor.
- 도메인: psu.connect/out, uart.open/expect, can.open/db.load/sig.wait_for, uds.connect/dtc.list/dtc.clear + 실제 UART/UDS EvidenceVerifier와 결과 evidence[] 필요.
- 계획/RPC 차이: 원본 psu.out{name} ↔ 실제 ch; uart.expect{port,pattern,timeout} ↔ channel/patterns/timeout_ms; can.sig.wait-for{cond,timeout} ↔ can.sig.wait_for{expr,within_ms}.
- 데이터 차이: plan PSU anchor `/payload/state_out` ↔ 실제 psu.write `/payload/state`; 계획 can.signal 및 `/payload/name/value` ↔ 실제 CAN frame/DBC evidence 모델. 원본 fixture는 바꾸지 않았으며, R1 실제 schema와 맞춘 별도 plan 검증 필요.
- TEST 후속 확인: 현재 PredicateEngine 자극 검사는 문자열 REF만 읽어 machine Evidence DTO가 건너뛰어질 수 있음. 자극 REF의 epoch/channel/시도 창 검증도 실제 연결 전 강화 필요(현재 가짜 문자열 시험으로 보증 불가).
- 그 밖에 sim DUT 전원 연동·완전한 UDS 19 02 요청/응답·mask09·도메인 술어·pin/보존·정상 finally/STOP·재시작 저널·UI 개입은 미검증.
