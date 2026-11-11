docs/plan/02-core-principle.md 준수 (최우선, 브리프보다 우선)

# R1 지시 — uart-sol

1. docs/plan/uart/tasks/_common-r1.md 를 먼저 읽고 따른다.
2. 출력 파일: docs/plan/uart/r1-sol.md

## 당신의 관점 (무게를 둘 것)
- 라이브러리 조사: pyserial, pyserial-asyncio(및 pyserial-asyncio-fast 등 대안), serial.tools.list_ports, Textual / prompt_toolkit / urwid, pexpect / pexpect의 fdpexpect, websockets/aiohttp/FastAPI 등 IPC 후보. **실제 PyPI/GitHub/문서를 확인**해 최신 버전, 최근 릴리스 날짜, 라이선스, Windows 지원, 유지보수 상태를 표로. 확인 못 한 것은 [추정]으로 표시.
- 프로토콜/드라이버: Windows COM 배타 점유, macOS /dev/cu.* vs /dev/tty.*, Linux /dev/serial/by-id, USB 재열거 시 포트 이름 변화와 동일 장치 식별(시리얼 번호, location), break/DTR/RTS의 플랫폼별 동작, 흐름제어(RTS/CTS, XON/XOFF).
- 실현 가능성·타이밍/성능: 높은 baud(예: 921600, 3M)에서 읽기 루프/IPC 팬아웃 처리량, 타임스탬프 정밀도, 버퍼 상한·링버퍼 크기, 다중 포트.
- 테스트·시뮬레이션: macOS/Linux pty 페어(os.openpty, socat), Windows(com0com 등) 대안, 가짜 디바이스 스크립트 설계, CI에서 HW 없이 돌리는 방법.
- 필요하면 spikes/uart/ 에서 짧은 검증 (예: spikes/uart/.venv 에 pyserial 설치 후 pty 페어로 pyserial 열기/쓰기/읽기, break 송신 가능 여부). 스파이크 결과는 계획서에 요약하고 spikes/uart/README.md에 실행법을 남긴다.
- 다른 절(UX, 스킬 등)도 빠짐없이 쓰되, 위 영역을 특히 깊게.
