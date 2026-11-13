# Ethernet workbench proposals

UART 다음에 Bench Light에 임베딩할 Ethernet 모듈의 독립 목업 두 안이다. 실제 NIC나 OS 소켓에 연결하지 않는 로컬 시뮬레이터다. 기존 제품 코드와 UART 목업은 변경하지 않았다.

저장소 루트에서 기존 서버를 사용하거나 다음 명령으로 실행한다.

```sh
python3 -m http.server 4180 --bind 127.0.0.1 --directory work/reports
```

비교: http://127.0.0.1:4180/ethernet-workbench/

| 안 | 작업 흐름 | 경로 |
|---|---|---|
| Opus 5.5 | 상단 채널 바, 밀도 높은 캡처 화면, 우측 TCP/UDP 도구, 스트림 방향·HEX 보기, CSV/PCAP | `opus/index.html` |
| Sol | 좌측 채널 목록, 캡처와 Socket Test 탭 분리, 패킷 상세·바이트 보기, CSV | `sol/index.html` |

두 안 모두 Wireshark와 Packet Sender의 공식 문서·화면을 벤치마킹했다. Opus는 Hercules도 참고했다. 각 `SURVEY.md`에 관찰·추론과 채택 범위를 구분했다.

검증은 Ego Lite에서 실제 클릭·키보드·분할선 드래그·다운로드로 수행했다. 다중 채널 독립 제어, 필터, 원문 바이트, TCP/UDP 송수신과 템플릿, 1440/768/390 화면을 확인했다. 발견한 결함은 수정 후 해당 동작을 재검증했다. 최신 판정은 `qa/SOL-SUMMARY.md`, `qa/OPUS-SUMMARY.md`; 원본 실패·재검사 기록과 화면은 `qa/`에 보존했다. 소스 로컬 검사는 브라우저 검증과 구별한다.

목업용 제한 필터 문법과 시뮬레이터 피어를 사용한다. Follow는 보존된 패킷의 방향별 페이로드 보기이며 전체 TCP 재조립 엔진이 아니다. 기존 Ethernet 실제 구현·검증 계획은 `work/plans/simulator-ui/eth/PLAN.md`에 별도로 유지된다.
