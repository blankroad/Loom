# 모듈별 실시간 수신 검증 팀 — DONE

ROOT: 조율, 소유 경계 관리, 소스·증거 리뷰, Ego21 실제 UI 검증, 통합·Git·정리.

| 담당 | 역할 / 작업 공간 | 반환 |
|---|---|---|
| uds-live-skill-opus | 공통 follow 결함·UDS 스킬 / w1K | 검증 후 정상 종료·닫음 |
| uds-live-observer-opus | 독립 UDS 스킬 observer / w1M | 실제 DoIP + UI 대조 통과 후 정상 종료·닫음 |
| uart-live-check-opus | UART 실수신·비밀 송신 및 감사 로그 수정 / w1N | 검증 후 정상 종료·닫음 |
| eth-live-check-opus | Ethernet 실수신·기록 UI·시간·control 상태 수정 / w1P | 검증 후 정상 종료·닫음 |

모든 구현 담당은 native foreground Opus 5.5, 하위 에이전트 없음. 공유 worktree에서는 소유 파일을 분리했고 ROOT만 index/commit/push를 조작했다. 검증 중 확인한 공유 파일 변경은 ROOT가 범위를 명시해 소유 담당에게 배정했다.

별도 simulator LOOM_HOME과 watch 전용 fixture를 사용했다. 실사용자 로그인 자격·세션·정책을 변경하지 않았다. 관측기는 별도 클라이언트 ID로 최대30초씩 제한했고 자동 갱신하지 않았다. 직접 송신 패킷을 장치 동작 증거로 간주하지 않았다.

기능별 원시 로그·준비 시각·요청 ID·epoch/seq·바이트·화면은 각 모듈 보고서에 있다. 최종 판정과 다음 범위: [STATUS.md](STATUS.md).
