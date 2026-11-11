# ① UART 브리프

목표: 에이전트가 COM 포트(UART)를 읽고 쓰며, 사용자는 **같은 포트를 터미널 프로그램 수준으로** 실시간으로 보고 직접 타이핑도 할 수 있다.

## 꼭 다룰 것

- 포트 탐색/열기: 포트 목록(VID/PID, 설명), baud/data/parity/stop/flow control, 재연결(USB-시리얼 뽑았다 꽂기), 다중 포트 동시 세션.
- 데몬 하나가 포트를 소유 → 에이전트 CLI와 사용자 터미널 뷰어가 동시에 붙는 구조. 사용자 입력과 에이전트 입력이 섞일 때의 규칙(락? 우선순위? 표시?).
- 에이전트 동작: `send` (문자열/hex/파일, 줄끝 CR/LF/CRLF 선택), `read --tail/--since`, `expect <regex> --timeout` (pexpect 스타일 — 부팅 로그 기다렸다 로그인 등), 버퍼 커서(에이전트가 "내가 마지막으로 본 이후"만 읽기), 바이너리/ANSI 이스케이프 처리, 출력 크기 상한.
- 사용자 뷰어: herdr 패인에서 띄울 수 있는 TUI 터미널 (`loom uart attach COM3`). 타임스탬프 표시 토글, hex 보기, 에이전트가 보낸 바이트 강조, 스크롤백 검색, 로그 저장. 키 입력 그대로 전달 (Ctrl-C 등 처리 규칙 포함). 원하면 웹 뷰도 검토.
- 특수 동작: break 신호, DTR/RTS 토글(보드 리셋/부트모드), XMODEM/YMODEM 같은 파일 전송은 범위 판단.
- 기록: 원시 바이트 + 타임스탬프. 줄 단위 텍스트 로그. 나중에 viz로 숫자 추출 플롯 (`docs/plan/_xteam/viz-requirements.md`에 요구 작성).
- 시뮬레이션: pty/가상 포트 페어(macOS/Linux `socat`/pty, Windows com0com 등)와 가짜 디바이스 스크립트(부팅 로그, 쉘 프롬프트 흉내)로 HW 없이 테스트.
- 후보 라이브러리 예: pyserial, pyserial-asyncio, Textual/prompt_toolkit/urwid (TUI), pexpect류. 실제 유지보수 상태와 Windows 지원 확인.
