# Sol CLI / skill 구현 반환

2026-11-14. ROOT의 소유권 이전에 따라 실제 구현했다. **독립 리뷰가 아니며 ROOT 통합 리뷰 대상.** backend model/worker/gateway, UI, workspace Cargo.toml/Cargo.lock, Git index는 수정하지 않았다. 추가 에이전트 없음.

## 작성 완료

- `crates/loom-logic/src/cli.rs`: public `LogicCli` (`Debug, Default`, `DomainCli`) 및 `parse_rate`/`parse_duration`. 기존 echo `CliOf` fragment 패턴. `status/start/stop/read/wait`; help/JSON/transport/common warnings는 기존 CLI framework 사용.
- start: source 필수, fallback 없음. 사용자 rate/duration을 정수 Hz/ms로 변환하고 **현재 `model::StartConfig::from_params` → `to_params()`**로 검증·정규화. UART/SPI decoder spec과 synthetic text/frame period/SPI clock 지원. legacy top-level protocol/baudrate 없음. acquisition packing을 CLI가 재구현하지 않음.
- read: tail 1..1000(default100), canonical Cursor `--since`, capture/kind filter. follow는 단일 long-poll, `--wait` 기본10s/최대30s이며 follow 없이 wait 사용 거부. 일반 지속 follow는 `loom events` 재사용.
- wait: text/ascii 정확히 하나, 1..256 bytes/ASCII 검증, capture/decoder/since 필터, timeout 기본5s/최대600s, transport margin2s. bounded 출력 64KiB는 gateway 계약으로 유지.
- `crates/loom-cli/Cargo.toml`: optional `loom-logic`, cli-only dependency 및 default feature 등록.
- `crates/loom-cli/src/lib.rs`: `CliOf::<loom_logic::cli::LogicCli>::new()` 등록, domain Duration timeout과 root u64 timeout 충돌 방지의 기존 회귀 테스트에 logic 케이스 추가.
- `skills/loom-logic/SKILL.md`: common loom 참조, source provenance, raw/decode/sample 축, STOP 전 UI/agent 같은 journal 증거, bounded read/follow/wait, fx2lafw는 physically unverified 8-bit candidate / 16ch refused 경계.
- `crates/loom-cli/src/skills.rs`: copy/symlink 모두의 skill bundle 등록. 기존 FILES 기반 배포 테스트가 새 skill도 포함한다.
- `CONTRACT.md` 말미: read/wait params/result/cursor/timeout 형식 명시. backend Opus는 이 문단을 기준으로 RPC를 구현하고 CLI 소유 파일은 수정하지 않는다.

## read/wait 인계 핵심

`logic.read`: `{capture_id?,kind,tail,since?,follow,wait_ms}` → `{rows:[flattened row],complete,rows_left_out,next:<resume Cursor>,…}` (ROOT 최종 wire 채택). follow=false이면 wait_ms=0, true이면 1..30000. next는 사용자가 다음 `--since`에 그대로 전달한다. gateway는 최대1000 rows/64KiB, truncated/gap metadata를 보존한다. human renderer는 rows와 complete/rows_left_out/next를 출력하며 원본 PublishedRecord는 기존 loom events로 조회한다.

`logic.wait`: `{capture_id?,text?|ascii?,decoder?,since?,timeout_ms}`. text/ascii 정확히 하나. decoder는 base ID 또는 실제 `uart-1` ID. timeout 1..600000ms. gateway의 표준 envelope를 그대로 렌더한다. `stop`: `{capture_id?}`.

## 검증 상태

- **PASS**: 소유 cli.rs standalone rustfmt, Python TOML parse/cli-only feature isolation, 모든 include_str skill 경로 존재 검사.
- **PASS**: skill-creator `quick_validate.py skills/loom-logic` → `Skill is valid!`.
- **작성 완료·실행 대기**: cli.rs unit tests 5개(명시 source/driver·channel validation, synthetic/model wire roundtrip, SPI/limits/stop/status, read cursor/long-poll bounds, wait predicates/timeout/quantity overflow) 및 root timeout regression logic 케이스.
- **Cargo 미실행**: backend Opus만 단일 cache build 권한. Cargo.lock도 직접 수정하지 않았다. 컴파일/테스트 통과를 주장하지 않는다. 전체 리뷰는 요청대로 중단 상태이며 worker 생성으로 리뷰를 재개하지 않았다.

backend/ROOT 실행 요청: 지정 cache/env에서 `cargo test -p loom-logic --no-default-features --features cli` 및 `cargo test -p loom-cli --lib`의 관련 parsing/skills 테스트. 실제 RPC/stdio/device를 이 테스트가 열지 않는다. CLI parser가 model validation의 unsupported 오류를 발견하면 기존 DomainCli의 UsageError 규약에 따라 로컬 usage 오류로 반환한다; raw JSON/RPC의 model ErrorCode와는 별도다.

참고: ROOT thread에 wire/소유권 전달 도구를 한 번 시도했으나 `already has an active writer`로 실패했다. 전달 완료로 간주하지 않는다. 이 파일과 CONTRACT가 소유권·wire 인계 기록이며 ROOT가 backend Opus에 전달해야 한다.

## ROOT rows wire 반영 완료 — CLI 소스 안정

최종 rows wire에 맞춰 cli.rs human renderer/command help 및 skill 설명을 수정했다. complete=false/rows_left_out/next, decoder uart-1/class null의 렌더링 회귀 테스트를 추가했다(총 6개 CLI tests + root timeout 회귀 케이스). rustfmt와 skill validator 재통과. Cargo는 실행하지 않았다. **현재 cli.rs 소스 안정, backend Opus가 합쳐 컴파일/테스트 가능. Sol은 후속 Cargo 실행 대기.** gateway/model/worker 수정 없음.
