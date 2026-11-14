# Logic analyzer 실시간 연동 — DONE

2026-11-14. Sol·Opus가 별도 foreground Herdr 작업 공간에서 공식 웹·드라이버 소스를 조사했다. sigrok 수집·디코딩 프로세스를 Loom 공통 journal에 연결하고 native 파형/디코딩 패널, CLI, loom-logic 스킬을 구현했다. [선정 근거](DECISION.md), [최종 계약](CONTRACT.md), [Opus 조사](RESEARCH-OPUS.md), [Sol 조사](RESEARCH-SOL.md).

## 구현

- daemon이 수집 프로세스를 단독 소유한다. 원시 전이와 실제 libsigrok UART/SPI 해석이 같은 sample 축으로 STOP 전에 배포된다. 합성 입력은 명시 선택이며 실제 장비로 표시하지 않는다.
- native UI는 채널별 파형, zoom/pan/최신 따라가기, 디코드 표, source·손실·상태, 관측 캡처 정지와 기록 보존을 제공한다. 활성 서비스만 메뉴에 나온다. 실제 브라우저에서 발견한 초기 zoom, 축 정밀도, 선택 행 대비, 새로고침 시 최신 캡처 선택 문제를 수정했다.
- CLI는 status/start/stop/read/wait를 제공한다. 실시간 관측은 기존 events follow를 재사용한다. 새 스킬을 Codex·Claude 프로젝트 스킬에 기존과 같은 상대 symlink로 배포하고 내용 일치를 확인했다([배포 기록](SKILL-SYNC.json)). 기존 copy sync는 symlink 보호로 거부되어 기존 링크를 변경하지 않고 새 스킬 링크만 추가했다.

## 검증

최종 기능 소스에서 Rust **29개**(unit15 + engine10 + gateway4), UI **53 files / 591개** 검사가 통과했다. 실제 sigrok-cli0.7.2로 Rust 합성 UART·SPI8조합, demo acquisition/tee, EOF 오류, descendant pipe 정리, Busy/latch/Drop을 검증했다. clippy·typecheck·eslint와 새 Rust crate의 scoped fmt는 clean이다. 기존 전체 workspace의 포맷 차이는 이번 범위 밖으로 유지했다. 마지막 형식 정리 후 기본 기능 gateway/CLI를 다시 빌드했다.

| 최종 native 실행 대조 | UI↔agent 동일 행 | 증거 |
|---|---:|---|
| UART synthetic + 실제 PD |11|[정확 대조](suite-uart-compare.json), [live DOM](suite-uart-ui-live.json)|
| SPI synthetic + 실제 PD |10|[정확 대조](suite-spi-compare.json), [live DOM](suite-spi-ui-live.json)|
| 실제 sigrok demo + SPI PD |10|[정확 대조](suite-demo-compare.json), [live DOM](suite-demo-ui-live.json)|

세 run 모두 UI state=running, document.visibilityState=visible, watch 권한에서 대조했다. 31행의 decoder/sample span/text가 canonical epoch/seq/source_seq에 연결됐고, **UI 캡처 시각과 observer 수신 시각이 모두 실제 stopping 이벤트 이전**임을 검사했다. 별도 observer는 최대40s로 종료됐다. [원본 observer 압축](final-suite.ndjson.gz).

각 run은 실제 UI의 watch 정지 버튼으로 종료했다. demo 새로고침에서 최신 캡처·STOPPED·정지 버튼 비활성·기록 유지를 확인했다([재열기](suite-demo-reopened-ui.json), [파형](suite-demo-fit-ui.png)).

수신 지연은 수십ms부터 약1s까지 관측됐으며250ms 상한을 보장한다고 주장하지 않는다. 고밀도 demo에서는 표시와 annotation 상한에 따른 생략을 확인했고 카운터와 unknown 구간에 노출했다. 전체 샘플의 무손실 저장 기능은 아니다.

검증은 자체 LOOM_HOME/SIM에서 수행했다. 실물 USB scan·firmware 접근은 하지 않았다. fx2lafw도 실물 미검증 후보이며 Saleae16채널은 명시 거부한다. 검증 실행 파일은67aa971d/bbd99f5e이며, 기능 불변 형식 정리 후 보존한 최종 파일은 [FINAL-BINARIES.json](FINAL-BINARIES.json)에 있다.

## 정리와 인계

모델 반환: [backend](BACKEND-STATUS.md), [CLI](CLI-STATUS.md), [UI](UI-STATUS.md), [독립 리뷰](REVIEW-SOL.md), [ROOT 리뷰](ROOT-REVIEW.md). 리뷰 차단 문제는 수정·검증했다.

작업용 daemon과 worker를 정상 종료했고 종료 ACK 누락은0개였다. foreground 에이전트 정상 종료 후 전용 작업 공간을 닫고 MAIN w8을 보존했다. Ego23과 작업용 HTTP server를 종료했다. 임시 빌드 캐시 약1.9GiB·테스트 홈·중복 UI 빌드를 정리하고 기본 기능 loom/loomd와 설치한 sigrok 의존성을 유지했다([정리 기록](CLEANUP.json)). 기존 다른 미커밋 변경은 보존했다.

작업 중 help 조회1회에 LOOM_HOME 지정이 빠져 기본 홈에 새 daemon55482가 자동 시작됐으나, 생성 PID를 확인해 정상 종료하고 사용자에게 설명했다. 장치는 열지 않았다. 이후 모든 QA 명령은 테스트 home/noautostart를 강제했다.

별도 추가 요청인 통합 보기/에이전트 작업 두 탭은 [구성 제안](../workbench-agent-tabs/SUMMARY.md)과 DEMO 목업까지 제공했다. 해당 두 탭의 제품 코드는 아직 구현하지 않았다.
