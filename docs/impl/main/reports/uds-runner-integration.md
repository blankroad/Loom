docs/plan/02-core-principle.md 준수
# UDS runner / DoIP 인수

- 기능 / 상태 / 담당: prepared 안전 수정 통합 / READY_TO_INTEGRATE / uds_tc1_smoke 직접 구현·자체 리뷰
- 작업 위치·기준: main `/Users/ctmctm/workspace/Loom`, `1a2e817`.
- 소유: `crates/loom-uds-runner`, `crates/loom-uds-doip`, 본 보고서. domain/load/gateway/index는 비범위.
- 이식 원천: `b23171a`. 단일 commit patch는 main에 priority/abort/read_retry 선행 코드가 없어 4파일에서 apply check 실패. 두 crate의 main→b23171a 최종 tree diff를 생성하고 `git apply --check` 성공 후 적용. 기존 branch merge/cherry-pick/commit 없음.
- 추가 선행 범위: runner abort.rs/priority_abort 시험, domain-api dependency, transport·DoIP entity 시험 등 기존 source branch의 완결된 두 crate 코드를 포함. 타 domain/load 미커밋은 제외.
- 검사 중: 두 crate fmt check PASS. 최초 `cargo test -p loom-uds-runner -p loom-uds-doip --locked --offline`는 lockfile에 runner→loom-domain-api dependency 누락으로 실행 전 실패. root에게 Cargo.lock 한 줄 소유 변경 요청; 의존성 변화 전 반복 안 함.
- 검사 로그: `/private/tmp/loom-uds-runner-integration-test.log`. patch: `/private/tmp/loom-uds-runner-integration.patch`.
- 자체 리뷰: submit poll 전 unresolved 예약, 확정 성공/미송신만 해제; transport arrival timestamp로 deadline 판단; admission count/bytes 및 caller capacity 축소 확인. 시험·clippy 완료 후 상태 갱신 예정.
- 커밋·푸시 없음; 한도·기한·사용량 미제공. 인수 담당 메인.

## 최종 검사·반환

- root가 Cargo.lock의 runner dependency에 loom-domain-api 한 줄 추가 후 재개.
- `CARGO_TARGET_DIR=~/.cache/loom-main-target cargo test -p loom-uds-runner -p loom-uds-doip --locked --offline`: **100 PASS**, 실패/무시 없음. DoIP 46, runner 54(실 TCP DoIP E2E 4, drop/timestamp/admission 신규 10, priority/abort 10, 기존 30). doc-tests 0.
- `cargo clippy -p loom-uds-runner -p loom-uds-doip --all-targets --locked --offline -- -D warnings`: PASS. 로그 `/private/tmp/loom-uds-runner-integration-clippy.log`.
- `cargo fmt -p loom-uds-runner -p loom-uds-doip -- --check`, 소유 범위 `git diff --check`: PASS.
- 검증 source: b23171a 두 crate 최종 tree와 일치. `git ls-tree b23171a`로 열거한 두 crate 전체 24파일을 `git show b23171a:<path>` 바이트와 직접 비교하여 모두 일치함을 확인했다(미추적 신규 파일도 포함).
- drop 중 handoff/응답 대기, 늦은 arrival stamp, queue byte/count/capacity, DoIP activation/ACK/cancel 세대 경계를 자체 리뷰했다. 독립 리뷰는 받지 않았다.
- 다음 행동: 메인이 이식 diff와 Cargo.lock 한 줄을 함께 리뷰·기능 커밋. domain/load/실 gateway UDS 연결은 이번 기능 범위 밖이며 별도 진행해야 한다. 전체 P1 통과 아님.
- 미커밋 보존, commit/push 없음. 인수 시점부터 메인이 소유한다. 추가 작업 없이 담당 역할 반환.
