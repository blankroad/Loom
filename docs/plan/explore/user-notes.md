# 사용자 추가 메모 (메인 오케스트레이터 전달)

- 2026-11-09: 사용자가 **알리 등에서 살 수 있는 저렴한 오실로스코프, 디지털(로직) 채널 되는 것**에 관심. 계측기/로직 애널라이저 후보를 평가할 때 반영할 것.
  - 핵심 루프 기준: 에이전트가 PC에서 제어 가능해야 함 (SCPI over USB/LAN, sigrok-cli, 벤더 Python SDK). PC 연결 안 되는 휴대용(FNIRSI 류)은 제외.
  - 메인이 본 후보 예: Hantek 6022BL(2ch 20MHz + 16ch LA, 모드 스위치 전환, sigrok/OpenHantek6022), fx2 계열 8ch LA 클론 / Kingst LA 계열(sigrok, UART/CAN/SPI/I2C 디코더), Rigol DHO804(SCPI, 디지털 없음) / DHO914S(16 디지털, 프로브 별도), Hantek DSO2D15(USB SCPI), Digilent Analog Discovery 3(알리 아님, Python SDK + macOS 지원).
  - 실제 macOS 지원, sigrok 드라이버 상태, SCPI 품질을 확인해서 "저가 조합 추천"을 plan.md에 포함할 것.
