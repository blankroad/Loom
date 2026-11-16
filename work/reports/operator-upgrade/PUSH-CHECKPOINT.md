# Operator upgrade implementation checkpoint

사용자의 즉시 푸시 지시에 따른 현재 구현 체크포인트입니다. **전체 기능 완료·정식 릴리스 승인이 아닙니다.**

## 남은 작업
- CAN: 실제 gateway의 주기 작업 제어 분류, CLI PID 간 논리 소유권, 내보내기 선택 범위 메타데이터 보강. `can/ROOT-RT-DECISION.md`가 이전 core Job 라우팅 제안을 대체합니다.
- Video: 새 격리 빌드에서도 ROI 테두리0.004/0.008px가 거의 보이지 않는 결함. 열림 상태 문제는 새 빌드에서 재현되지 않았습니다.
- 최종 브라우저·일반 네이티브 검증, 하드웨어 캡처보드/TCC, 실제 Luna 지시 시작·감지 지연은 완료되지 않았습니다. HTML은 DRAFT입니다.
- Opus/Sonnet 사용량 한도로 남은 모델 작업이 중단됐고 자동 재시도·임의 모델 전환은 없습니다. 모든 자체 모델·장치 검증 프로세스는 정상 정리했습니다.

## 검사 결과
- 전체 UI1,308/1,308 통과. 타입·린트·스키마·fmt·diff 검사 통과.
- 새 ROOT 전용 Cargo target으로 전체 Clippy all-targets `-D warnings` 통과.
- 전체 Rust1,561통과·1실패·6ignored(exit101). 실패한 UART suite는 단독10/10, 원래 동일 바이너리로도10/10 재검사 통과. 최초 실패를 숨기거나 전체 단일 실행 통과로 표시하지 않았습니다.
- 실제 시뮬레이터 ETH 대형 저널 재개 조회7항목 및 새 Video UI/5프레임 증거8항목 통과. 물리 장치 검증과 구별합니다.
- 공유 Cargo target에서 다른 worktree의 오래된 decoder가 참조된 문제를 확인했습니다. 최종 빌드는 격리 target과 명시적 사전 빌드 바이너리로 진행해야 합니다.

## 제품 커밋
- `1cbdf64770f1` style(rust): format CLI and gateway code
- `eb4d54495ba2` feat(eth): improve packet inspection and bounded history queries
- `02314be68488` feat(logic): add durable decoder archives and navigation
- `cc53e8d2e898` feat(uds): add diagnostic databases and session automation
- `7a947fef4710` feat(psu): add SCPI history and verified output controls
- `c23ff09d215c` feat(can): expand signal inspection and capture export
- `bf23147c1a2c` feat(video): add ROI inspection and retained evidence frames
- `440aadcb1155` feat(uart): improve terminal controls and serial session recovery

기준 커밋610029e. 각 제품 파일은 검사 시점 `integration/SOURCE-CHECKPOINT-ISOLATED.json`의270개 해시와 일치합니다. 기존 사용자 변경8개 문서와 무관한 보고서·캐시·바이너리·private LOOM_HOME은 제외했습니다.
