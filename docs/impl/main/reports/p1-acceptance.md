# P1 통합 수용 — 2026-11-12

현재 **BLOCKED — 실제 브라우저 권한**. 실제 브라우저가 필수 축이며 앱 접근 권한 차단을 완료/통과로 세지 않는다. 이 문서는 과거 STATUS의 미통합 표기를 대체하는 최신 통합 현황이다.

| 수용 축 | 현재 결과 | 증거 |
|---|---|---|
| UART 실제 IPC 읽기·쓰기·사용자 개입·STOP·저널 | PASS (웹 화면 제외) | [UART smoke](uart-p1-smoke.md), gateway `r1` |
| CAN 실제 IPC 송신·ECU 응답·동일 viewer·사용자 개입·STOP | PASS (웹 화면/지연 제외) | [CAN/ETH smoke](can-eth-smoke.md), [generation/overflow](can-p1-regressions.md) |
| UDS 실제 domain·loopback DoIP·NRC/timeout·진행 중 STOP | PASS (웹 화면 제외) | [UDS domain](uds-domain-foreground.md), [STOP](uds-stop-p1.md), [CLI](uds-cli-p1.md) |
| Ethernet 실제 sim 목록·상세·송신·동일 viewer·사용자 개입·STOP | PASS (웹 화면 제외) | [CAN/ETH smoke](can-eth-smoke.md) |
| 실제 powered TC-1·원본 저널 판정·재부팅 후 상태 | PASS (Inproc 배치, 웹 화면 제외) | [TC-1](tc1-live-p1.md), [원본 evidence](uart-uds-evidence-p1.md), [TEST runtime](test-runtime-p1.md) |
| 실제 payload UI 정합 | 단위검사 PASS (실브라우저 차단) | [UDS 화면](p1-uds-ui-contract.md), [ETH 화면](p1-eth-ui-contract.md) |
| TEST 조건·CAN 술어 scope | PASS | [이벤트별 schema](p1-anchor-contract.md), [CAN 독립 리뷰](p1-can-predicate-scope.md) |
| 기본 CLI 7개 domain·제품 skills sync | PASS | [기본 CLI](cli-default-domains-p1.md), [독립 CLI 리뷰](cli-integration-review-p1.md) |
| 30분 실제 혼합 sim RSS | 2회차 PASS: 1800.032초, daemon +1.703MiB, worker 5개 PASS | [실패와 수정](p1-memory-retention-fix.md), [독립 메모리 리뷰](p1-memory-retention-review.md) |
| 실제 브라우저 5개 시나리오·CAN 화면 지연 | **BLOCKED — 앱 접근/프로세스 실행 권한** | [브라우저 증거·재개 명령](browser-smoke.md) |

## 제품 경계와 정직한 판정

- TC-1은 실제 PSU sim readback에 UART PTY 부팅·CAN bus 노드·DoIP ECU를 연동한다. 임의 이벤트 publish, echo/replay 또는 복제한 판정을 쓰지 않는다. 장치 저널의 UART/CAN/DTC 근거로 dirty DTC 실패 → clear → 재확인 통과를 판정한다.
- TC-1의 Inproc 배치는 지원되는 `[workers] mode = "inproc"`와 같은 실제 백엔드를 사용한다. 기본 IPC 배치에는 producer flush/shared-clock 프로토콜이 아직 없어 TEST가 `fence_unflushed`로 **uncertain**을 반환한다. IPC TEST 통과를 주장하지 않는다. 개별 domain의 IPC 통신·안전 스모크와 구별한다.
- 마스터 F1 기본 Auto에서는 부여된 operate 권한을 유지한다. confirm/approve 정책의 자동화 watch 상한과 재연결 전 failclosed 경계는 별도 유지한다.
- CAN live-cache는 메모리 예산에 parsed payload까지 과금하므로 JSON 전용 예산 때보다 짧다. 표준 부하에서 약 1,400 frame/7초 수준이며 원본 저널은 유지한다. 누락된 backlog는 coverage에서 표시한다.
- 실 CAN/NIC/COM 하드웨어·Windows/Linux 드라이버 수용은 P2 경계다.

## 브라우저 차단과 준비된 결과

Ego Lite 실행 상태는 확인했다. 앱 제어 자동 승인 검토가 `Computer Use was not approved to use ego lite`로 거부했고 구체 사유는 반환하지 않았다. Ego CLI는 XPC 연결 실패, 허용된 Playwright fallback은 `MachPortRendezvousServer Permission denied (1100)`로 페이지 생성 전에 종료했다. 같은 권한에서 반복하거나 다른 Herdr 담당을 실행 통로로 이용하지 않는다.

실제 UART trace → terminal 입력, CAN device RX/20회 DOM+2RAF 지연/사용자 send, UDS 요청 행/사용자 요청, Ethernet 목록·상세·사용자 send, 실제 TC-1 live/DONE 화면의 5개 필수 시나리오는 코드로 준비했다. 공식 bootstrap cookie·실제 viewer WS·실제 TC-1 passed·child 정상 종료의 사전 검증은 통과했다. 이 결과는 `browser_rendering=false`, `browser_input=false`이고 브라우저 합격의 대체가 아니다.

사용자/관리자 측 Computer use의 Ego Lite 접근 허용 변화가 있어야 실제 앱 검증을 재개할 수 있다. Playwright는 별도로 현재 세션의 프로세스 실행 정책에서 Chromium 시작을 허용해야 한다. 접근 변경 응답은 대기 중이다.

## 실행 운영과 보존

`feature-team-coding`에 따라 Claude Opus 담당을 Herdr의 대화형 포어그라운드 패인에서 기능별 새 문맥으로 실행했다. 내장 background subagent는 사용하지 않았다. 메인만 Git index/커밋을 조작하며 사용자 데모의 STOP, 기존 미커밋·워크트리·세션 로그를 보존했다.

요청한 foreground 스킬 수정은 `~/.cache/feature-team-coding-foreground/feature-team-coding/` 및 `foreground.patch`로 작성·검증했다. 설치 위치 `~/.codex/skills`는 현재 세션 쓰기 허용 밖이어서 설치됐다고 보고하지 않는다.

## 최종 회귀

- UI `pnpm check`: schema 33종, typecheck, lint, 385 tests PASS (`/private/tmp/loom-p1-final-ui-v2.log`).
- UI `pnpm build` PASS (`/private/tmp/loom-p1-final-ui-build-v2.log`).
- Rust workspace all-targets clippy `-D warnings`, fmt PASS (`/private/tmp/loom-p1-final-{clippy,fmt}-v2.log`).
- Rust workspace test: **1116 PASS / 0 FAIL / 4 ignored, exit0** (`/private/tmp/loom-p1-final-workspace-v2.log`).

메모리 2회차의 측정 대상은 고정된 `37feca4` binary다. 그 뒤의 oversized 단일 레코드 guard(표준 부하에서는 분기 미실행), TEST 조건 schema/CAN 판정 scope(부하에서 술어 미호출), ETH 단위 메타데이터·UI·문서 보완을 그 binary에서 실행했다고 주장하지 않는다.

- 최신 UI를 포함한 daemon/CLI 빌드 PASS (`/private/tmp/loom-p1-final-native-build-v2.log`).

- UI 자산 별도 검사 11 PASS (`/private/tmp/loom-p1-final-assets-v2.log`).
- 정식 RSS exit0, errors0, STOP/release29회, coverage96.85%, cache 순서 인덱스 불일치0. [최종 수치](p1-memory-retention-fix.md#정식-2회차-최종-결과--pass).
