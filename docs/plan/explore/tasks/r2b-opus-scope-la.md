docs/plan/02-core-principle.md 준수 (최우선, 브리프보다 우선)

# R2b 추가 조사 — explore-opus: 저가 오실로스코프 / 로직 애널라이저

## 배경
사용자 새 입력: `docs/plan/explore/user-notes.md` 를 먼저 읽을 것. 알리 등에서 살 수 있는 **저렴한 오실로스코프(디지털/로직 채널 포함)** 또는 로직 애널라이저를 에이전트가 PC에서 제어하는 것에 관심이 있다.
R1에서 두 초안 모두 오실로스코프(SCPI 파형)와 로직 애널라이저(sigrok/Saleae)를 "실시간 표시 △ / 쓰기 △"로 상위 추천에서 뺐다. 사용자 관심이 생겼으므로 **실제 제품 기준으로 다시 평가**한다.

## 할 일 (실제 확인 위주 — 웹 문서, sigrok wiki, 벤더 프로그래밍 매뉴얼, PyPI)
1. 후보 제품 평가표 (메인이 본 후보 + 당신이 추가할 것 몇 개):
   Hantek 6022BL / 6022BE, fx2(Cypress FX2LP) 8ch LA 클론, Kingst LA1010/LA2016 등, Rigol DHO804 / DHO914S(+디지털 프로브), Hantek DSO2D15 / DSO2C10, Siglent SDS800X HD 류(참고), Digilent Analog Discovery 3, 필요하면 Sipeed SLogic / DreamSourceLab DSLogic, Owon VDS 류.
   열: 가격대(공식/알리 표시가, 미확인 표시) / 아날로그 ch·대역 / 디지털 ch / PC 연결(USB·LAN) / 제어 경로(SCPI·sigrok 드라이버 상태·벤더 SDK) / **macOS 동작 근거** / Windows / 라이브(연속 스트리밍) vs 캡처 후 전송 / 파형 전송 시간 대략 / 쓰기(트리거·타임베이스·신호발생기 출력·디지털 패턴 출력 여부) / 프로토콜 디코드(UART/CAN/SPI/I2C — sigrok 디코더, 장비 내장) / 핵심 루프 4요소 판정.
   - sigrok 지원 상태는 sigrok wiki 'Supported hardware' 및 장치 페이지에서 확인 (supported / in progress / planned). Kingst는 sigrok 공식 지원 여부를 반드시 확인(지원 안 될 수 있음).
   - PC 연결 안 되는 휴대용(FNIRSI 류)은 제외.
2. 핵심 루프 재판정: 이 장비군에서 "쓰기"가 무엇인지 정직하게 정의 — (a) 장비 설정(트리거 arm, 타임베이스)만인지, (b) DUT에 자극을 주는 출력(AWG/신호발생기, 디지털 패턴 출력 — AD3는 있음)이 있는지, (c) 다른 Loom 도메인(UART/CAN 송신, PSU)의 쓰기와 결합해 "에이전트가 자극 → 스코프/LA가 관측 → 사용자가 같은 화면에서 봄"이 성립하는지. 실시간 표시도 "연속 스트리밍(fx2 LA, 6022는 USB 스트리밍)" vs "단발 캡처 반복(SCPI 스코프 ~수 Hz 화면 갱신)"으로 구분해 목표치를 제안.
3. **저가 조합 추천** 2~3세트 (예: 최저가 / 가성비 / macOS 확실) — 각 세트: 구성 장비, 대략 총액(미확인 명시), 되는 것/안 되는 것, 백엔드 경로.
4. 이 도메인을 상위 추천에 넣는다면의 간이 계획 (r1-common.md §4 a~j 형식): CLI 초안(`loom scope ...`, `loom la ...` 또는 통합 이름 제안), 사용자 뷰(파형/디지털 타이밍/디코드 표, loom.viz 요구), sigrok-cli subprocess vs libsigrok 바인딩 vs pyvisa 선택, 시뮬레이션 백엔드(sigrok `demo` 드라이버 — 존재 확인, pyvisa-sim), M1 걷는 뼈대, 안전(디지털 패턴 출력·AWG 출력은 DUT 쓰기이므로 등급), 리스크(GPL: libsigrok GPL-3.0 → subprocess 경계).
5. 최종 판단: 기존 추천 5개(PSU+IO, DLT, Shell, Debug, XCP 조건부) 중 어디에 들어가는지/무엇을 밀어내는지, 또는 `loom.instr`(SCPI) 확장 + 별도 `loom la`(sigrok)로 나누는지 결론과 근거.

## 규칙
- 출력: `docs/plan/explore/r2b-opus-scope-la.md` (한국어, 결론 먼저, 300~500줄 정도). 확인한 사실은 URL 출처, 확인 못 한 건 "미확인".
- 쓰기 허용: `docs/plan/explore/`, `spikes/explore/`(필요 시 `spikes/explore/.venv`에서 sigrok/pyvisa 관련 짧은 확인). 전역 설치(brew 등) 금지 — sigrok-cli가 시스템에 없으면 설치하지 말고 "미확인"으로.
- 완료 시 파일 마지막 줄 `<!-- END R2b -->`, 화면에 "R2b DONE: <경로>" 한 줄.
