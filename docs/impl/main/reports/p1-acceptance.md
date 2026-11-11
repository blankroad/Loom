# P1 시뮬레이터 통합 수용 — PASS

2026-11-12. 실제 daemon/sim → 같은 live stream의 브라우저 화면 → 사용자 개입/STOP → 원본 저널 판정 및 30분 RSS 관문을 확인했다. 브라우저는 Herdr foreground Claude Opus 5.5의 정상 승인 경로에서 Ego Lite를 우선 사용했고, Ego의 RAF 정지로 지연 측정이 막힌 구간은 사용자가 허용한 Playwright fallback으로 검증했다. 권한/보안 설정 변경·sandbox 해제·skip-permissions는 사용하지 않았다.

## 수용 결과

| 수용 축 | 결과 | 증거 |
|---|---|---|
| UART 실제 IPC 읽기·쓰기·동일 viewer·사용자 입력·STOP·저널 | PASS, 실제 Ego 및 Playwright 화면 포함 | [UART smoke](uart-p1-smoke.md), [최종 브라우저](p1-browser-final.md) |
| CAN 실제 IPC 송신·ECU 응답·동일 viewer·사용자 송신·STOP | PASS, 실제 Ego 및 Playwright 화면 포함 | [CAN/ETH smoke](can-eth-smoke.md), [최종 브라우저](p1-browser-final.md) |
| CAN 표시 지연 | **PASS: p95 83.401 ms < 150 ms**, 20샘플, Playwright | [원본 시간 표본](../../../../bench/results/p1-browser-can-latency.json) |
| UDS 실제 domain·loopback DoIP·NRC/timeout·진행 중 STOP·사용자 요청 | PASS, 같은 request_id의 실제 화면 확인 | [UDS domain](uds-domain-foreground.md), [STOP](uds-stop-p1.md), [브라우저](p1-opus-browser.md) |
| Ethernet 실제 sim 목록·상세 tree/hex·편집 송신·사용자 행·STOP | PASS, SDK 수정 후 Ego 재검증 및 Playwright | [ETH 상세 수정](p1-eth-show.md), [최종 브라우저](p1-browser-final.md) |
| 실제 powered TC-1·원본 저널 판정·재부팅 후 상태·LIVE/DONE 화면 | PASS, **Inproc 배치** | [TC-1](tc1-live-p1.md), [원본 evidence](uart-uds-evidence-p1.md), [최종 화면](../evidence/p1-browser/playwright/actual-tc1-journal-verdict.png) |
| 30분 실제 UART/CAN/UDS/ETH/PSU sim RSS | **PASS: 1800.032초, daemon +1.703 MiB, worker 5개 PASS** | [실패·수정·최종 수치](p1-memory-retention-fix.md) |
| 기본 CLI 7개 domain·제품 skills sync | PASS | [기본 CLI](cli-default-domains-p1.md), [독립 CLI 리뷰](cli-integration-review-p1.md) |
| TEST 조건 schema·CAN 술어 scope·라이브 보관 경계 | PASS | [이벤트별 schema](p1-anchor-contract.md), [CAN 독립 리뷰](p1-can-predicate-scope.md), [메모리 독립 리뷰](p1-memory-retention-review.md) |

## 실제 브라우저 검사

`real-smoke.config.ts`: **5 passed / skipped 0 / unexpected 0 / flaky 0, exit0, 18.5초**. UART·CAN·UDS·ETH는 브라우저 사용자 입력/송신과 STOP 뒤 agent `E_STOPPED`까지 확인했다. TC-1은 실제 powered DUT·journal 판정과 화면 DONE/passed를 확인했다. mock/replay/jsdom/WS 사전 검증으로 대체하지 않았다.

- [Playwright 원본 결과](../../../../bench/results/p1-browser-playwright.json), [CAN 20개 표본](../../../../bench/results/p1-browser-can-latency.json).
- [최종 실행 구성·SHA·5개 결과](p1-browser-final.md), [Ego 최초 실행·실패 원본](p1-opus-browser.md).
- 저장소 캡처: `../evidence/p1-browser/playwright/` 9장, `ego-final/` ETH 3장. 이전 Ego 기능/실패 캡처 및 `can2-samples.json`도 같은 증거 디렉터리에 보존했다.

CAN의 83.401ms는 **agent 명령 시작 → 매칭 장치 RX DOM → 2회 RAF**의 보수적 end-to-end 상한이다. 장치 응답·테스트 전송 오버헤드를 포함한다. 물리 디스플레이의 정확한 paint timestamp라고 주장하지 않는다. 150ms 기준은 완화하지 않았다.

## 실제 화면에서 발견하고 고친 것

- [UART LF 없는 프롬프트](p1-uart-live-prompt.md): `login:`/`Password:`가 완결 줄이 아니라는 이유로 숨겨졌다. 이제 즉시 표시하고 같은 행에서 완결한다. gap을 넘어 합치지 않는다.
- [Ethernet 상세 RPC](p1-eth-show.md): mock의 목록 쿼리로 상세를 요청하던 것을 실제 `eth.show`로 연결했다.
- [SDK native RPC 응답](p1-ui-query-envelope.md): query 성공은 Envelope.data를 읽고, 실패는 partial data가 있어도 reject한다. ActionGate는 성공 응답형태를 보존하면서 실패를 성공 WriteHandle로 만들지 않는다. 권한/confirm/STOP/no-retry 경계는 유지한다.
- 실제 [UDS payload](p1-uds-ui-contract.md), [ETH U64 통계·unknown 드롭](p1-eth-ui-contract.md)를 UI가 읽도록 맞췄다.
- TC-1의 실제 sim fixture 환경 배지를 plan.env와 같은 SIM으로 명시했다.
- 테스트 기대값은 가상화된 live tail, Enter 이전 입력권, 실제 화면의 TC 제목/단계명을 사용하도록 바로잡았다. 실제 사용자 개입·STOP·장치 응답 조건은 유지했다.

## 최종 검사와 식별자

- Rust workspace **1116 PASS / 0 FAIL / 4 ignored, exit0**: `/private/tmp/loom-p1-final-workspace-v2.log`. 이후 native runtime 변경은 없고 TC fixture의 Env::Sim 보완은 실제 TC-1 1 PASS 및 clippy로 별도 확인했다.
- UI `pnpm check`: **393 tests**, schema33/typecheck/lint PASS. SDK 보고서에 회귀·반증 결과가 있다. 최종 `pnpm build` PASS: `/private/tmp/loom-sdk-envelope-build.log`.
- workspace all-targets clippy `-D warnings`/fmt PASS. 최신 동일 UI assets 포함 daemon·CLI build exit0와 UI assets **11 PASS**: `/private/tmp/loom-p1-browser-final-{native-build,assets}.log`.
- 브라우저 실행 소스: `d7638ac` + 검증된 E2E 선택자 diff. dist JS `index-BExAuFCo.js`, SHA256 `96b1d2c732ebad9b495b5a5b6a6a6ecd43bbdcda964d00af11b197e0b4af4318`. native/test binary hash와 실행 명령은 최종 브라우저 보고서에 기록했다. 그 뒤 패키징 빌드는 동일 dist를 embed했다.
- 정식 RSS 2회차: 동결 `37feca4` binary, error0, STOP/release29회, worker 재시작0, coverage96.85%, cache index 불일치0. 최초 FAIL(+68.016MiB)과 수정 후 PASS 원본을 모두 보존했다. 이후 UI/조건 술어/단일 oversized guard를 동결 binary에서 실행했다고 주장하지 않는다. [자세한 경계](p1-memory-retention-fix.md).

## 수용 범위와 남는 제한

- **TC-1 PASS는 지원되는 Inproc worker 배치**다(`[workers] mode = "inproc"`). 실제 backend·PTY·DoIP·원본 journal을 사용했다. 기본 IPC TEST는 data-lane flush/shared-clock 프로토콜 부재로 `fence_unflushed`/uncertain을 반환한다. IPC TC PASS를 주장하지 않는다. 개별 domain의 IPC 통신·안전 수용과 구별한다.
- Ego 창은 visibility=visible 상태에서도 RAF가 약1초 정지해 device→paint p95 2087ms가 나왔다. 이 **실패 원본은 유지**하며 Ego 성능 PASS라고 바꾸지 않는다. 이번 지연 관문 PASS는 정상 60Hz RAF의 Playwright headless runtime 기준이다. Codex CUA 자동 승인 거부·sandbox CLI 실패도 해당 경로의 과거 실패로 남긴다.
- 라이브 cache는 parsed payload를 포함한 논리 예산으로 보관해 JSON 전용 때보다 짧다. 표준 CAN 부하 약1,400 frame/7초 수준이며 원본 journal 및 누락 coverage 표시는 유지한다.
- 실 CAN/NIC/COM 장비·Windows/Linux 드라이버·전기적 버스 타이밍은 P2 수용 범위다.
- 사용자 기존 daemon/STOP·다른 세션·미커밋·워크트리·세션 로그를 보존했다. 이 시험의 서버·브라우저 child와 control 파일 정리 결과는 최종 브라우저 보고서에 기록했다.

## 별도 스킬 설치 상태

요청한 feature-team-coding foreground 규칙 수정은 `~/.cache/feature-team-coding-foreground/feature-team-coding/` 및 `foreground.patch`에 작성·검증했다. 설치 위치 `~/.codex/skills`는 현재 메인 세션 쓰기 허용 밖이므로 **수정본 준비 / 설치 미반영**으로 남긴다. P1 제품/브라우저 수용과 별개다.
