# STATUS-OPUS — 2026-11-14 완료 (idle)

- 산출물: `OPUS.md` (~1,550단어), `OPUS.html` (자급형, 외부 의존 없음, DEMO 표시, 스크립트 `node --check` 통과; 브라우저 실행 확인은 하지 않음).
- 추천: 통합 보기 = 사용 중 채널 선택기 + 기존 dock split/resize + 이름 붙은 View Set 저장(미연결 멤버는 고스트, STOP 시 패널 동결·기록 유지, seq 커서 동기). 에이전트 작업 = daemon 소유 Task 레지스트리(`task.*` RPC/이벤트 + `core.activity.task_id` 자동 귀속), 선언 단계와 서버 관측 근거 병기, STOP→`blocked`(취소 아님) / 작업 취소→latch 불변.
- 참고: GitHub Actions workflow run logs, GitHub Copilot agent sessions, Grafana variables (링크·적용점은 OPUS.md §2).
- ROOT 결정 필요: 기존 통합 작업 공간 대체 여부, 닫힌 UART/CAN 기록 서버 조회 가능 여부, task 시작 주체(CLI 명시 추천).
- 지킨 범위: 제품 코드·Git·Cargo·장치 미접촉, 다른 모델 산출물 미열람, 하위 에이전트 없음.

## ROOT 검토 반영 (2026-11-14, 추가 리서치 없음)
- 승인: 쓰기=무조건 승인 예시 제거. 기본 R0–R4 자동, `waiting_user`는 명시 질문 또는 사용자 설정 approval 정책일 때만. 단계는 도구 결과/명시 조건으로 완료 표시 (MD §1-B·§4②·§5·§6, HTML T-0413/T-0414).
- 취소: task-scoped cooperative `task.cancel` → `task.cancel_ack` 기본, 공유 runner는 프로세스 중단 없음, `dedicated` worker일 때만 ACK 지연 후 "worker 중단" 노출 (MD §5·§6·§8, HTML 취소 동작).
- `stale` 상태 삭제 → lease/status 기반 freshness 부가 칩(활성/대기 정상/lease 만료), 상태 불변. HTML에 "lease 끊김 시뮬" 버튼.
- 사실/제안 분리: 레이아웃 저장 현재 구현 = `LocalLayoutStore`(localStorage). `core.ui.layout.*`는 요청만 접수된 미구현 후속 제안으로 표기 (MD §0·§6·§7·§10).
- 검증: HTML 스크립트 `node --check` 통과, 브라우저 수동 클릭 확인은 미실시.

## Bench Light 반영 (2026-11-14)
- OPUS.html CSS palette만 수정: `prefers-color-scheme: dark` 재정의 제거 → shell/sidebar/카드/표 항상 light. 패널 내 raw 장치 출력(`.rows`)만 터미널 dark(`--term-*`). 정보 밀도·분할/resize·동작 변경 없음. `node --check` 통과.

## 문구 정리 (2026-11-14)
- OPUS.html 화면 문구에서 내부 risk 등급(R0–R4) 노출 제거: 단계명 "ECU Reset 11 01", 근거 "정책에 따라 실행", 하단 설명 "필요한 입력이나 정책상 승인이 있으면 알림". 보고서 계약의 등급 표기는 유지. 동작/레이아웃 변경 없음, `node --check` 통과.
