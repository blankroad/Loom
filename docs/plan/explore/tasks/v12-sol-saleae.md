# v1.2 짧은 사실 확인 — Saleae Logic 2 자동화 (explore-sol)

배경: 사용자가 Saleae 로직 애널라이저(모델 미확인)를 보유. LA 1순위 백엔드를 Saleae Logic 2 자동화 API(`logic2-automation`)로 바꾼다. 계획에 쓸 사실만 확인한다. **공식 문서·PyPI·GitHub 출처 URL**을 항목마다 붙이고, 확인 못 하면 "미확인"이라고 쓴다. 설치·실행 스파이크는 하지 말 것(웹 확인만). 30분 이내, 결과 40줄 이내.

결과 파일: `docs/plan/explore/v12-saleae-check.md`

확인할 것:
1. `logic2-automation` 최신 버전·라이선스·지원 Python 버전, 지원 OS(Windows x64, Linux x64, macOS).
2. 연결 방식: Logic 2 앱을 `--automation`(포트 10430?)으로 띄워 연결 vs `Manager.launch()`. **headless(앱 창 없이) 실행이 가능한가** (옵션 이름).
3. 캡처 모드: Manual / Timed / DigitalTrigger. 캡처 중 데이터 스트리밍·부분 읽기가 정말 불가능한가. 최대 캡처 길이/메모리 한계(버퍼 크기 설정).
4. 내보내기: `export_raw_data_csv`, `export_raw_data_binary`(형식: 디지털 전이 기반인가), `export_data_table`(분석기 결과 표), `add_analyzer`(내장 분석기 이름: SPI/I2C/Async Serial/CAN 등과 설정 키). 저수준 분석기(HLA) 사용 가능 여부.
5. 시뮬레이션 장치: device id `F4241` 같은 시뮬레이션 장치 ID 목록, 실장비 없이 CI에서 쓸 수 있는가(앱 설치 필요 여부 → CI 러너에서 Logic 2 AppImage 실행 가능성).
6. 지원 하드웨어: Logic 8 / Logic Pro 8 / Logic Pro 16 / 구형 Logic(4/8/16 1세대)이 Logic 2(및 자동화 API)에서 지원되는가.
7. sigrok(libsigrok)이 Saleae Logic 8 / Pro 8 / Pro 16(신형)을 지원하는가. 구형 Logic(fx2 기반)·Logic16 지원 여부.
8. 동시 사용: 자동화로 캡처 중 사용자가 같은 Logic 2 앱 창에서 파형을 실시간으로 보는가(캡처 중 화면 갱신 여부). 자동화 연결은 동시에 몇 개까지인가.
9. Linux 실행 조건: udev 규칙 필요 여부, AppImage 의존성.

끝나면 대기.
