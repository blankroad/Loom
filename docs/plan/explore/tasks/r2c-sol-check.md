docs/plan/02-core-principle.md 준수 (최우선, 브리프보다 우선)

# R2c 사실 검증 — explore-sol: 저가 스코프/LA 조사(R2b) 검증

배경: 사용자 새 입력 `docs/plan/explore/user-notes.md` (알리 등 저가 오실로스코프/디지털 채널, PC 제어 필수). explore-opus가 `docs/plan/explore/r2b-opus-scope-la.md`에 재평가를 썼다. 리드가 지금 plan.md(R3)를 쓰고 있으며, 이 결과를 반영한다.

## 할 일 — r2b의 사실 주장을 검증하고 보완 (짧게, 실제 근거로)
1. sigrok: 정식 릴리스 버전(libsigrok 0.5.2 / sigrok-cli 0.7.2), macOS nightly가 x86 전용인지, Homebrew sigrok-cli arm64 bottle, `demo` 드라이버 채널/아날로그 옵션, sigrok-cli 연속 스트리밍 출력(-O 형식, `--continuous`, stdout 파이프로 실시간 디코드 주석 `-A`가 캡처 종료 전에 흘러나오는지 — 소스/문서 근거).
2. 드라이버 상태: fx2lafw 클론, hantek-6xxx(6022BE/BL 스코프 모드 지원 여부), kingst-la2016(정식 0.5.2 포함 여부 — 소스 트리 태그로 확인), dreamsourcelab-dslogic(U2Basic 미지원 여부), Rigol DHO800/Siglent SDS HD 지원 현황.
3. SCPI 스코프: Rigol DHO800 프로그래밍 가이드의 파형 조회 명령과 LAN(VXI-11/소켓 5555 등) 지원, Siglent SDS800X HD LAN 프로토콜·파형 명령, Hantek DSO2D15 USBTMC·LAN 유무와 비표준 파형 명령, pyvisa-py로 macOS에서 USBTMC/TCPIP 경로 가능 여부.
4. Digilent AD3: WaveForms/dwf 런타임의 Apple Silicon 네이티브 여부(최신 릴리스 노트), dwfpy 버전·라이선스, AD3 디지털 패턴 출력/AWG/전원 사양, sigrok 지원 상태.
5. 가격: 공식 표시가 재확인(DHO804, SDS804X HD, AD3). 알리 가격은 확인 가능하면 범위만, 아니면 미확인 유지.
6. 가능하면 `spikes/explore/.venv`에서 짧게: dwfpy import(런타임 없으면 실패 메시지만 기록), pyvisa-sim으로 스코프 `:WAV:DATA?` 흉내 YAML 가능 여부. 전역 설치 금지(sigrok-cli를 brew로 설치하지 말 것).
7. r2b의 결론(5위 = 관측 계측 스코프/LA, `loom scope`는 loom.instr 확장, `loom la`는 sigrok-cli subprocess, XCP는 조건부 예비, 저가 세트 A/B/C)에 대한 동의/이견과 근거.

## 출력
- `docs/plan/explore/r2c-sol-scope-la-check.md` (한국어, 150~300줄): 1) 틀린 사실(근거·확신도) 2) 확인된 사실 3) 보완 4) 결론 이견. 원자료는 `spikes/explore/r2c-sol/`.
- 파일 마지막 줄 `<!-- END R2c -->`, 화면에 "R2c DONE: <경로>" 한 줄.
