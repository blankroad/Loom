# UART 서비스 벤치마크 리뷰

상태: **READY_FOR_REVIEW** · 범위: UART 서비스 작업 공간만 · 열람일: **2026-11-12**

## 판정 범위와 근거

- 기준: `dbca34d` 위 현재 공유 작업 트리. 서비스 분리 단계의 화면과 관련 코드를 읽었다. 이 보고서는 구현 변경 요청이 아니다.
- 실제 화면: [두 UART alias와 다른 서비스가 동시에 연결된 시뮬레이터 화면](../evidence/ui-workbench/service-split-final/uart-all-connected.png), [390px 화면](../evidence/ui-workbench/service-split-final/uart-390.png). 둘 다 실제 daemon 브라우저 증거이며 물리 UART 시험은 아니다.
- `uart-all-connected.png`에는 UART 서비스 탭 `mcu`, `aux` 둘만 보이고, CAN/Ethernet/UDS/PSU 연결이 공존한다. `mcu` 터미널은 `open 115200 gen1 [SIM]`, 입력권, HOLD, 키 팔레트, REC 상태와 수신 부트 로그를 보인다.
- 390px에서는 탐색기와 터미널이 세로 배치되고 UART 탭 하나가 보인다. 작은 폭의 로그 가독성은 낮아진다. 스크린샷만으로 키 송신, 복사, 검색, 기록 조작의 성공을 주장하지 않는다.
- 기존 [종합 초안](service-tool-benchmarks.md)의 UART 사실을 재검토했다. 그 초안에서 CoolTerm 세부 기능은 2차 출처였으나, 아래에서는 공식 도움말로 직접 확인했다.

## 대표 도구와 채택 근거

| 도구 | 공식 문서에서 확인한 실제 작업 흐름 | 외부 채택 근거와 강도 | Loom에 유효한 비교점 |
| --- | --- | --- | --- |
| [Tera Term](https://teratermproject.github.io/manual/5/en/menu/file-new.html) | 포트 선택 → [속도·데이터·패리티·정지 비트·흐름 제어](https://teratermproject.github.io/manual/5/en/menu/setup-additional-serialport.html) → 터미널. [파일 메뉴](https://teratermproject.github.io/manual/5/en/menu/file.html)에 로그 시작·중지, [제어 메뉴](https://teratermproject.github.io/manual/5/en/menu/control.html)에 Break가 있다. | [Microchip의 임베디드 대상 직렬 터미널 설정 가이드](https://ww1.microchip.com/downloads/aemdocuments/documents/fpga/ProductDocuments/UserGuides/configuring_serial_terminal_emulation_programs.pdf)가 Tera Term 설정을 구체적으로 안내한다. **실무 권장 사례**, 사용자 수 자료는 아니다. | 연결 설정을 터미널과 구분하고, 현재 포트 조건과 기록 상태를 항상 보인다. |
| [PuTTY](https://the.earth.li/~sgtatham/putty/latest/htmldoc/Chapter3.html#using-serial) | Serial 선택 시 포트·속도를 지정한다. 입력한 문자가 곧바로 직렬 포트로 나가고 수신 문자가 화면에 나타난다. 같은 [공식 매뉴얼](https://the.earth.li/~sgtatham/putty/latest/htmldoc/Chapter3.html)에 선택 복사·붙여넣기, 세션 로그, Break가 설명된다. | [Microchip 가이드](https://ww1.microchip.com/downloads/aemdocuments/documents/fpga/ProductDocuments/UserGuides/configuring_serial_terminal_emulation_programs.pdf)가 PuTTY 설정도 안내한다. **실무 권장 사례**, 순위 근거는 아니다. | 즉시 키 입력 콘솔을 주 작업 흐름으로 유지하고 복사·로그 접근을 명확히 한다. |
| [CoolTerm](https://www.freeware.the-meiers.org/CoolTermHelp/) | 공식 도움말: 여러 포트 동시 연결, 텍스트/HEX 수신 보기, Raw 즉시 키 송신과 Line 버퍼 송신, 텍스트/HEX Send String, 줄끝 설정, 캡처. | [Microchip SAMA7G54-EK 안내](https://developerhelp.microchip.com/xwiki/bin/view/software-tools/mcu-dev-boards/32-bit-kits/sama7g54-ek/console_serial_communications/)가 macOS 콘솔용 CoolTerm과 115200·8N1 설정을 명시한다. **보드별 권장 사례**다. | 장치 콘솔과 바이트 송신 도구를 같은 포트에 두되 조작 모드를 분명히 구분한다. |

- 비교 가능한 공개 사용자 수·점유율은 확인하지 못했다. 세 도구는 **대표 작업 흐름**이며 “가장 많이 쓰는 1~3위”가 아니다.
- 위 링크는 모두 도구 개발자 또는 칩 공급사의 1차 자료이며 2026-11-12에 본문을 열람했다. ADI 가이드는 종합 초안에 있으나 이번 직접 열람이 되지 않아 채택 근거로 세지 않았다.

## 현재 Loom 사실

- [`terminalPanel.ts`](../../../../ui/src/panels/uart/terminalPanel.ts)의 `TerminalView.onInput`은 바이트를 `uart.input` stream으로 보낸다. 큰 붙여넣기는 `inputLarge` 확인 카드로 분리한다. **명시적 줄 작성기나 HEX 송신기는 현재 없다.**
- [`terminalView.ts`](../../../../ui/src/sdk/terminalView.ts)는 xterm 기반 VT 화면이다. Ctrl-C를 `0x03`으로 보내고 복사는 Ctrl-Shift-C/Cmd-C 경로를 쓴다. 자동 터미널 응답을 차단하는 훅과 사용자 입력 게이트가 있다. 이 VT 해석 화면과 아래 Trace의 텍스트 행은 성격이 다르다.
- 터미널에는 입력권·dirty·HOLD, 권한 및 쓰기 잠금, 포트 상태·baud·generation, agent 송신/expect 대기, 수신 속도, 화면 생략, `REC` 건강 상태가 있다. `Break`는 키 팔레트에서 `uart.signal`로 현재 제공된다.
- [`tracePanel.ts`](../../../../ui/src/panels/uart/tracePanel.ts)는 TX/RX 출처·시간·내용을 행으로 표시하며 비인쇄 문자를 기호화한다. 20,000행 링, 수신 조각의 줄 조립, gap 후 줄 리셋이 있다. HEX 보기, 검색 UI, Trace 전용 복사·내보내기 UI는 이 파일에서 확인되지 않았다.
- 서비스 분리의 현재 기본 프리셋은 alias별 `uart.terminal` 탭이다. [`serviceSeparation.test.tsx`](../../../../ui/src/shell/serviceSeparation.test.tsx)는 두 alias가 UART 서비스 탭에 남고 다른 도메인 도착에도 섞이지 않음을 확인한다. Trace는 채널별 패널로 등록돼 있으나 기본 탭에 자동 포함되지는 않는다.
- 포트 설정을 표시하는 것과 UI에서 변경하는 것은 다르다. 현재 헤더는 baud를 보여 주지만 이 UART 패널 안에 baud/패리티/흐름 제어 변경 폼은 없다. 연결·설정의 소유자는 인터페이스 관리 흐름이다.

| 항목 | 현재 UI/코드 | 다음 UART 기능 단계의 제안 |
| --- | --- | --- |
| 입력 | VT 콘솔에서 키 즉시 송신, 큰 붙여넣기 확인 | 직접 입력 유지; 선택형 줄/HEX 작성기 추가 검토 |
| 수신 | VT 화면과 별도 텍스트 Trace | Trace의 HEX 투영·검색·복사 검토 |
| 대상 | alias별 터미널 탭, 포트 상태와 baud 표시 | Trace·송신 도구에도 alias 고정 표시 |
| 기록 | 전역 REC 건강 상태와 daemon 저널 | 시작/중지·내보내기 동작은 실제 지원 범위 확인 후 배치 |
| 제어 | Ctrl-C 바이트와 Break 신호 팔레트 | Break의 대상/결과를 더 명확히 표시 |
| 설정 | 터미널에서 현재 baud 표시 | 설정 변경은 연결 관리자와 권한 계약 확인 후 결정 |

## UART 안에서 가능한 3가지 화면 흐름

| 대안 | 배치와 작업 순서 | 장점 | 단점 |
| --- | --- | --- | --- |
| A. alias별 터미널 탭 유지 | 서비스 진입 → `mcu`/`aux` 탭 선택 → 즉시 키 입력 → 필요할 때 해당 alias Trace 열기 | 지금 구현과 시뮬레이터 화면에 일치한다. 두 alias와 타 도메인 격리가 명료하다. 좁은 화면에도 맞는다. | 수신 원문 검색·HEX 점검에 Trace를 추가로 열어야 한다. 동시 포트 비교는 탭 전환이 필요하다. |
| B. 선택 alias 터미널 + Trace 나란히 | alias 선택 → 왼쪽 VT 콘솔, 오른쪽 같은 alias의 시간순 Trace → 검색·복사·기록 확인 | 부트 콘솔 조작과 TX/RX 감사가 한눈에 연결된다. VT 화면과 바이트 기록의 역할이 분명하다. | 390px 화면에서는 둘 다 좁다. 기본 패널 수와 화면 비용이 늘며 alias를 바꿀 때 두 보기의 채널을 함께 바꿔야 한다. |
| C. alias별 콘솔 + 접을 수 있는 송신 도구 | A를 기본으로 두고 필요할 때 줄/HEX 작성기를 펼침 → 줄끝과 실제 송신 바이트 확인 → 전송 | CoolTerm의 Raw/Line/HEX 분리가 필요한 장치 명령에 적합하다. 기존 즉시 콘솔을 유지한다. | 새 입력 경로와 초점/입력권 규칙을 설계해야 한다. 이번 서비스 분리 범위를 넘는 제품 기능이다. |

**권고:** 이번 서비스 분리에서는 **A를 유지**한다. alias별 탭은 실제 시뮬레이터와 테스트로 확인됐고, 각 서비스 화면을 분리한다는 당면 목적에 맞는다. 다음 UART 기능 단계에서는 B의 Trace 접근성과 검색/복사를 먼저 검토하고, 명령 송신 요구가 확인되면 C를 접히는 보조 도구로 추가한다. C가 생겨도 VT 터미널의 직접 키 송신을 대체하지 않는다.

## 다중 인터페이스 및 조작 경계

- alias는 각각 포트 상태·generation·입력권·HOLD·수신 기록의 경계다. 탭 전환은 같은 연결을 관찰할 뿐 새 포트 열기/닫기나 기록 제어가 되어서는 안 된다.
- 같은 서비스에서 `mcu`와 `aux`를 나란히 열더라도 모든 송신, Break, Trace, 검색 결과에는 대상 alias가 보여야 한다. 다른 UART 포트에 보내는 실수를 막는 핵심 표시다.
- Tera Term과 CoolTerm의 로그 저장 UI는 참고할 만하나 Loom의 `REC ok`는 기록 **상태** 표시다. 기록 시작/중지, 파일 내보내기, 패널 일시정지는 서로 다른 동작으로 유지한다.
- Ctrl-C 같은 제어 바이트와 Break 신호는 다르다. 현재 키 팔레트가 둘을 구분한다. Break는 명시적 대상 alias와 신호 동작임을 계속 표시한다.
- CR, LF, CRLF, 없음은 장치별로 결과가 달라지는 **미래 송신 도구의 설정**이다. 현재 터미널의 Enter 키가 별도 줄 작성기를 거친다고 표시하면 안 된다. HEX 도구를 만들 때는 인코딩·줄끝을 합친 최종 바이트 미리보기가 필요하다.
- UART RX는 비동기 스트림이다. TX 행 뒤에 온 RX를 확정된 “해당 명령의 응답”으로 연결하지 않는다. agent `expect`가 있다면 패턴과 시간 제한을 함께 표시한다.

### 우선순위의 이유

- 서비스 분리의 성공 조건은 도메인 경계와 열린 alias의 안정성이다. A는 이 조건을 가장 작은 변경으로 만족하며 현재 증거로 검토할 수 있다.
- B는 현장 디버깅에서 가치가 있지만 화면 폭과 기본 패널 수를 늘린다. 먼저 사용자가 `Trace`를 쉽게 열 수 있는지 확인할 필요가 있다.
- C는 실제 송신 기능을 새로 만든다. 줄끝, HEX 파싱 실패, 큰 입력 확인, HOLD, lease가 한 번에 얽혀 별도 설계와 검증이 필요하다.

## 다음 구현 단계 수용 확인

1. UART 서비스에 두 alias가 열리면 두 UART 터미널 탭만 생성되고, CAN·Ethernet·UDS·PSU 이벤트는 이 탭 그룹에 들어오지 않는다.
2. 탭 전환, Trace 열기, 서비스 전환은 daemon 포트 open/close·입력권 이전·기록 시작/중지를 유발하지 않는다. 통합 화면에서는 기존 다중 서비스 관찰이 가능하다.
3. alias별 상태·baud·generation·입력권·HOLD·REC가 실제 선택 포트와 일치한다. 보기 전용 또는 STALLED 화면에서 즉시 키 입력과 보조 송신은 막힌다.
4. 키 입력·큰 붙여넣기·Ctrl-C·Break는 각각 기존 `uart.input`, 확인 카드, `0x03`, `uart.signal` 경로를 유지한다. 향후 줄/HEX 송신은 대상 alias와 실제 바이트를 전송 전 확인한다.
5. Trace를 확장한다면 VT 화면과 분리하고 RX/TX 출처·gap·생략을 보존한다. 검색/복사는 화면 버퍼 범위와 daemon 기록 범위를 혼동하지 않게 표시한다.
6. 390px에서는 alias 선택과 입력권·대상 상태가 보이고 터미널에 입력 초점을 줄 수 있어야 한다. 나란히 Trace는 충분한 폭에서만 제안한다.

## 미확인 범위

- 물리 UART 하드웨어와 세 외부 프로그램을 직접 실행하지 않았다. 공식 문서와 Loom 코드, 실제 daemon **시뮬레이터** 브라우저 화면을 대조했다.
- 스크린샷은 다중 alias 격리와 화면 배치를 뒷받침하지만, 실제 키 송신·검색·복사·기록 파일의 종단 간 검증 근거는 아니다.
- UART 설정 변경 권한과 연결 관리자 UX는 이 UART 서비스 리뷰에서 설계하지 않았다. 서비스 분리 완료 후 별도 기능 단계에서 결정한다.
