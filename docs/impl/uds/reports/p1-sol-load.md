# P1 부하·메모리 도구 — uds-sol

**구현·5분 지연 측정 완료 / BLOCKED: RSS 실측·30분 외삽**. 샌드박스에서 `ps`가 `operation not permitted`; RSS 302회 전부 null이므로 증가량·기울기를 만들지 않았다.
코드: worktree `crates/loom-uds-load/`만 생성. 기존 3개 공유 crate·루트 Cargo.toml 수정/커밋 없음. Cargo.lock에는 새 workspace package가 자동 추가된다.

## API·사용법

- `run(LoadConfig) -> Result<LoadReport, LoadError>`; `Cli::parse_bounded`, 고정 `Histogram`, `rss::parse_rss_kib`/sampler. sim 어댑터는 이 crate가 소유하며 loom-uds에 의존하지 않는다.
- worktree 루트: `CARGO_HOME=.cargo-home CARGO_TARGET_DIR=target cargo run --release -p loom-uds-load --offline --bin uds-load -- --targets 64 --hz 20 --duration 5m --faults --reconnect-every 15s --out <path>`; out 생략 시 JSON 하나를 stdout에 출력.
- 하나의 RA 소켓을 64 LA/runner가 공유한다. polling backlog 없음, missed tick은 skipped. Drop 뒤 SID U는 보존하며 15초마다 구 연결 폐기+새 RA 뒤 generation 교체. 재전송·TTL 회복 없음.
- 결정적 혼합: 97번째 Drop, 101번째 NRC 0x21, 73번째 80 ms late, 31번째 0x78×2 후 최종, 17번째 8 ms delay(대상별 offset·첫 일치만 적용). P2 50/P2* 100/총 200 ms인 측정 전용 sim 프로필.

## C2 상한

| 항목 | 상한·초과 정책 |
|---|---|
| 대상/tasks·rate·duration | 64·200 Hz·600 s, 초과 설정은 I/O 전 거부; 요청 진행 중 1/대상, 밀린 tick은 재생 안 함 |
| latency | 2×4002 고정 u64 bins(250 µs), 초과값 overflow bucket+실제 max; p50/95/99는 상한 근사 |
| 오류/세대/sim | CORE enum 53 buckets, generation watch 1개, ECU 64·단발 fault rule 1개/요청·reply ≤64 |
| RSS | 602 samples, 초과 drop counter; ps 출력 128 B·1초 뒤 kill/reap, 실패 null+counter(0 대체 금지) |
| argv/출력 | 24 args×4096 B, 초과 거부; JSON streaming·writer buffer 8 KiB |

## 시험·측정

- 관문: `cargo fmt -p loom-uds-load --check` PASS; `cargo clippy -p loom-uds-load --all-targets --offline -- -D warnings` PASS; `cargo test -p loom-uds-load --offline` **8 passed**(단위 6·2초 mixed/reconnect smoke·잘못된 library config).
- duration 파서 10만 ASCII 입력, RSS 파서 10만 결정적 무작위 byte 입력 시험. 참고로 기존 DoIP 29·runner 44 시험도 통과했다.
- 5분 release/macOS arm64: [원본 JSON](p1-sol-load-5m.json), 300.001046 s, 설정 64×20 Hz. 381,064회 시도·121,232회 송신·2,935 skipped; 최종 118,297건(**394.322/s**), 긍정 117,088·NRC 1,209.
- 최종 응답 latency p50/p95/p99 **≤0.25/8.5/9.5 ms**, max **13.605 ms**. timeout/거부 포함 attempt max 54.006 ms. 성공/최종 NRC의 지연만으로 오류를 숨기지 않는다.
- 오류: `E_QUARANTINE` 259,832 / `E_NO_RESPONSE` 2,935 / `E_NRC` 1,209. Drop 후 U 유지로 실제 송신률이 1280/s보다 낮다. pending 7,746·late 1,655; 재연결 19/19, entity/RX/link-event overflow 0, adapter 오류 0.
- 실측 slot 합계 383,999(명목 384,000 대비 종료 경계 1 slot 차이)는 원본에 보존했다. 이후 해당 skipped 계수를 고치고 2초 smoke에서 800/800 slot을 검증했다. 처리량은 실제 시도·응답 개수 기준이며 초기 구간에 별도 debug 시험/빌드가 병행된 개발 머신 측정이다.
- RSS: baseline/final/peak/증가·30분 외삽 모두 null, 실패 302회. **30분 RSS 증가 ≤10 MiB 판정은 불가**이며 5분 외삽으로 PASS를 주장하지 않는다. 웹 UI·IPC·저널·TLS·HW·24 h도 미측정.
- 측정 binary SHA256 `17f392d9f439c2c71fd808cf28397f819e3e41b74cf5fd857435bc787d219742`(이후 종료 경계 skipped 계수·RSS fuzz·문서/예제 보강, 5분 JSON은 수정하지 않음).

## 결정·질문

- 외부 crate 추가/전역 설치 없음. `.cargo-home`/`target`을 사용하고 `--offline`; 승인·안전 기본값은 F1을 따르며 이 도구에 새 승인 흐름을 넣지 않았다.
- 임시 정의·통합 교체: 없음. 이 부하 도구는 M1 웹 렌더/사용자 개입 관문의 대체물이 아니다. 독립 검토와 실행 반례는 [p1-sol-review.md](p1-sol-review.md).
- 질문(리드 실행 요청): ps를 허용하는 환경에서 위 동일 5분 명령을 재실행해 RSS 시계열·증가량·30분 선형 외삽을 채워야 한다. 이 세션은 승인 요청이 금지되어 더 진행할 수 없다. CORE 의존 추가 요청 없음.
