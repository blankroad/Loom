# 실시간 에이전트 / 실제 UI 대조 — DONE

UDS·UART·Ethernet을 모듈별 Herdr 작업 공간의 포그라운드 Opus 5.5로 검증했다. ROOT가 Ego Lite 실제 화면을 조작하고 원시 로그와 대조했다. 검증 중 발견한 문제는 수정 후 다시 확인했다.

| 모듈 | 대조 결과 | 핵심 증거 |
|---|---|---|
| UDS / 실제 외부 DoIP 서버 | PASS | 최종 네이티브 소스에서 VIN w4, NRC w5의 tx/rx/result 6개 이벤트(335–337, 340–342)와 UI의 요청 ID·응답 원문 일치. [원시 대조](final-native/uds-live-compare.json), [VIN 화면](final-native/uds-ui-vin.png), [NRC 화면](final-native/uds-ui-nrc.png) |
| UART / bootshell SIM | PASS | w4 마커가 TX62 / 장치 RX65와 실제 터미널에 일치. 비밀 입력 w3 이후에도 수신 지속. 송신 이벤트·저널·감사 로그에서 테스트 비밀 원문 검출 0. [보고서](uart/REPORT.md), [터미널](uart/final/ui-marker-terminal.png) |
| Ethernet / native SIM | PASS | w6 패킷841의 요청 ID·60바이트 원문 및 장치 OfferService를 양쪽에서 대조. 최종 소스에서 w2 패킷83과 장치 행11개 추가 대조. 재열기 전후 시간 25.854100 일치. 종료 후 STOPPED·정지 버튼 비활성·기록 패널 유지 확인. [보고서](eth/REPORT.md), [시간 대조](eth/raw/phase6-final-ui.json), [종료 화면](eth/raw/phase7-stopped.png) |

## 수정 내역

- CLI: 첫 조회가 비어 있을 때 `events --follow --since now`가 이후 이벤트를 계속 놓치는 문제. 고정된 epoch/seq 커서와 페이지 이어받기로 수정했다.
- UART: `--secret-env` 값이 송신 이벤트와 링에 남는 문제, 별도로 감사 로그의 요청 인수에 남는 문제를 수정했다. 실제 장치에는 원래 바이트를 보낸다.
- Ethernet: 필터·일시정지·내보내기의 숫자 커서를 `seq:N`으로 수정했다. 캡처 종료 후 패널과 기록을 유지하고, 활성 인터페이스가 0개여도 명시적으로 연 기록을 볼 수 있게 했다.
- Ethernet: 실제 캡처 시작 시각을 스냅샷에 보존하고 UI가 읽도록 수정했다. 일부 수신 패킷을 시작 시각으로 추정하던 동작을 제거했다.
- UI SDK: 패널에 범위를 제한한 읽기 전용 control 이벤트 구독을 추가했다. Ethernet 종료 상태가 데이터 스트림에 오지 않아 LIVE로 남던 표시와 버튼 상태를 수정했다.

## 검증

CLI 단위 검사 6개 통과. UART worker/schema/실제 worker E2E 회귀 검사 및 관련 clippy 통과. Gateway 감사 로그 검사 4개와 무결성 검사 통과. Ethernet gateway 검사 13개와 clippy/fmt 통과. UI 51개 파일·561개 검사, TypeScript 및 ESLint 통과. 새 회귀 검사는 기존 동작 또는 변이에서 실패하는 것을 확인했다.

CLI 실시간 follow는 200 ms 폴링이며 UI는 실제 daemon 연결이다. 초기 UDS 독립 Opus 관측 뒤, 새 기본 gateway에서도 ROOT가 별도 observer/sender 클라이언트로 외부 DoIP 서버와 추가 통합 검증했다. UDS 최종 통합과 Ethernet 시간 검증에 사용한 gateway 8e4cc56b의 네이티브 소스는 최종 c56f59ea와 같고, 마지막 변경은 UI 자산이다. UART 비밀 입력 네이티브 검증은 4ac3ee71에서 수행했으며 이후 해당 네이티브 소스는 바뀌지 않았다. 런타임별 해시는 각 보고서와 [최종 실행 파일](final-native/binaries.json)에 기록했다.

공통·UDS·UART·Ethernet 스킬에 검증 절차를 반영했다. Codex/Claude 양쪽으로 배포한 갱신 파일 14개가 원본과 일치하고, 4개 스킬의 형식 검사도 통과했다.

## 범위와 다음 작업

이번 실제 화면 검증 대상은 loomd가 제공하는 네이티브 UI다. 새 Bench Light 목업은 아직 실제 엔진과 연결되지 않았다. UART/Ethernet은 시뮬레이터를 사용했으며 실제 CAN·NIC·차량 및 현대기아 전용 호환성 검증 결과로 확대하지 않는다.

다음 우선순위는 Bench Light의 UDS 실제 연결이다. 서버·ECU 설정과 연결/해제, DID·Raw 송신, 실시간 응답·NRC, 에이전트 동시 대조, 끊김·타임아웃·재연결을 하나의 흐름으로 완성한다. 이후 같은 연결 방식을 UART·Ethernet에 적용한다.

## 정리

이번 작업용 시뮬레이터·DoIP 서버·daemon을 정상 종료했고, worker 종료 확인에서 누락은 없었다. 에이전트 정상 종료 뒤 전용 작업 공간 w1K/w1M/w1N/w1P를 닫았고 MAIN w8은 유지했다. Ego 작업 공간21과 작업용 탭을 닫았다. 새로 만든 임시 빌드 캐시·테스트 홈을 정리하고 최종 기본 기능 loom/loomd 실행 파일을 보존했다. [정리 기록](CLEANUP.json).
