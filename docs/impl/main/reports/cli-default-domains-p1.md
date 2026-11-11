# P1 기본 CLI 및 Loom 스킬 배포

main이 CAN/ETH/PSU/TEST의 CLI/runtime 기능 경계를 분리하고 UDS CLI 담당 반환을 통합했다. 기본 loom은 echo,uart,can,uds,eth,psu,test 7개 명령 조각을 제공한다. 장치 접근은 daemon만 수행하며 독립 cargo metadata I1 검사를 통과했다. CAN open/close도 실제 gateway lifecycle RPC에 연결했다. CAN 조건식/ISO-TP hex 파서는 pure 모듈로 옮기고 기존 runtime 모듈의 public 경로를 re-export해 호환한다.

`loom skills sync --target claude|codex|both --project DIR [--dry-run]`은 번들 7개 스킬과 공통 참조 4개를 로컬 프로젝트에 복사한다. 기존 변경은 보존하고 충돌을 알린다. 명시적인 --force 복사는 원자적으로 교체하며 추가 사용자 파일은 삭제하지 않는다. --symlink는 원본 패키지가 있을 때만 선택할 수 있다. 에이전트 설정/정책을 변경하거나 daemon을 기동하지 않는다. 실제 CLI의 격리 프로젝트 복사도 확인했다(/private/tmp/loom-skill-sync-lve21dp8/project).

독립 리뷰: reports/cli-integration-review-p1.md. 두 major는 수정 후 재현 검사로 해결했다. ETH 공통 idempotency-key 옵션 id 충돌을 해결했고, 도메인 연산 deadline을 연결/인증 timeout과 분리했다. UART/ETH/PSU/TEST는 기기 timeout을 그대로 두고 RPC 응답 전달 여유 2초를 확보한다. 열린 UART sim의 1초 NEVERMATCH가 E_TIMEOUT으로 반환되고 짧은 CAN/0초 UART 조회가 정상 daemon을 다시 기동하지 않는 것을 확인했다. UDS 쓰기 smoke는 먼저 extended session으로 전환한다(기본 session의 NRC 거부는 정상 장치 동작).

검증: /private/tmp/loom-cli-delivery-margin-regression.log 154 PASS, /private/tmp/loom-p1-verified-workspace.log 1106 PASS/4 ignored, /private/tmp/loom-p1-final-clippy.log workspace all-targets -D warnings PASS. 독립 CLI I1 boundary PASS. 실제 CLI→daemon CAN/UDS/ETH/PSU 송수신 및 STOP 포함. UDS CLI 스키마 정합과 조합별 검사는 reports/uds-cli-p1.md. UI/브라우저 및 정식 RSS는 별도 수용 축이며 이 결과로 합격 처리하지 않는다.
