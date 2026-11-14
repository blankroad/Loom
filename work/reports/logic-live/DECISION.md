# 선택: sigrok 수집·디코딩 엔진을 Loom에 연결

2026-11-14, Sol·Opus 공식 소스 병렬 조사 및 로컬 실제 엔진 프로브 기준.

| 후보 | 디지털 로직 수집 | 외부 실시간 수신 | 판단 |
|---|---|---|---|
| sigrok + sigrok-cli | 드라이버별 지원, demo 지원 | 연속 binary stdout / 디코더 annotation stdout. 설치된 0.7.2에서 종료 이전 수신 실측 | **첫 구현 엔진** |
| PulseView | sigrok 기반 GUI | 별도 안정적 원격 stream API 확인 안 됨 | 파형 UX 참고. GUI 자동화로 수집하지 않음 |
| Saleae Logic 2 + HLA | Saleae 지원 모델 | decode callback 전달 확장 후보. raw 외부 stream 계약 확인 안 됨 | 장비 모델 확인 후 별도 어댑터 |
| Serial Studio | UART 등 계측·텔레메트리 | TCP/gRPC 실시간 데이터 경로 | 디지털 핀 로직 수집 엔진의 대체재로 선정하지 않음 |

프로세스 하나가 장치를 소유하고 raw bytes를 Loom 파형 처리와 별도 sigrok 디코더에 동시에 공급한다. sample index와 sample rate로 파형·디코딩 시간을 맞춘다. Loom 공통 이벤트 journal이 UI와 에이전트의 동일 증거 원본이다. synthetic은 명시 선택이며 실제 장비로 표시하지 않는다. raw sample 전체 저장을 주장하지 않고 bounded edge 요약/손실을 표시한다.

실측: UART annotation 첫 수신 약111ms, SPI 약112ms(입력 EOF 약1500ms), libsigrok demo 실제 acquisition raw 약132ms 및 tee decoder 약132ms. 이 수치는 짧은 로컬 probe 결과이며 장비 지연 보장이 아니다. 실제 USB 장비를 스캔하거나 펌웨어를 올리지 않았다.

Saleae 최신 정정: 2.4.48에서 캡처 중 Automation/MCP 분석기 추가가 반환되지 않던 문제가 수정되었다. 따라서 과거 문서로 “추가 불가”를 단정하지 않는다. 분석기 부착 수정은 외부 raw stream API의 증거와 별개다. Pro8은 libsigrok Pro16 드라이버 소스만 보고 지원된다고 할 수 없다.

근거: [sigrok CLI](https://sigrok.org/wiki/Sigrok-cli), [annotation flush source](https://raw.githubusercontent.com/sigrokproject/sigrok-cli/master/decode.c), [input source](https://raw.githubusercontent.com/sigrokproject/sigrok-cli/master/input.c), [Pro driver source](https://raw.githubusercontent.com/sigrokproject/libsigrok/master/src/hardware/saleae-logic-pro/api.c), [PulseView](https://sigrok.org/wiki/PulseView), [Saleae2.4.48](https://discuss.saleae.com/t/logic-2-4-48/3820), [SerialStudio](https://github.com/Serial-Studio/Serial-Studio). 자세한 조사 [Opus](RESEARCH-OPUS.md), [Sol](RESEARCH-SOL.md), 구현계약 [CONTRACT](CONTRACT.md).
