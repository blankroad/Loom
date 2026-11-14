# 두 탭 개정 — Fable 재사용안 (claude-fable-5-1)

2026-11-14 · base 143fd46 · 소유: `reuse/fable/**`만. 원본 모듈·Git index·Cargo·패키지 무변경. 실장치 접근 없음.

## 핵심 결정: 다시 그리지 않고 **원본을 그대로 띄운다**

앞선 Opus/Sol 안의 불만은 "기존 패널의 정보량이 요약 카드로 줄었다"는 점이었다. 이번 안은 UART/Ethernet/UDS 원본 `index.html`을 **같은 origin iframe**으로 삽입하고, 원본 Bench Light outer shell 중 중복되는 부분(헤더 `data-shell="header"`, 상태바 `data-shell="status"`)만 iframe 문서에 주입한 두 줄의 adapter CSS로 숨긴다. 모듈 툴바·세션·매크로·채널·필터·상세·hex·DID·history·응답은 원본 DOM 그대로 살아 있다. "원본 shell 보기" 버튼으로 adapter를 끄면 원본이 변경되지 않았음을 바로 확인할 수 있다.

세 원본의 localStorage 키 공간은 서로 다르다(`loom-uart-bench`, `loom.eth.opus.v1.*`, `loom-uds-*`). 같은 origin에 세 개를 동시에 띄워도 저장 상태가 충돌하지 않는다.

## 재사용 원본 → 유지된 조작부

| 원본 | 유지된 조작부 (검사 selector) | 숨긴 것 |
|---|---|---|
| `/uart-workbench/opus/index.html` (+app.js, styles.css, xterm vendor) | `.mtbar` 툴바, `#side` 세션·매크로, `#treeRoot` 중첩 분할 pane, `.xterm` 터미널, split-right/maximize, `#railMacros`, 키 입력 대상 세그먼트, timestamps/log recording dialog | 헤더(Loom 브랜드·svcnav·SIM 칩), 상태바 |
| `/ethernet-workbench/opus/index.html` (+app.js, pkt.js, sock.js, tools.js) | `.mtbar`, `#chbar` 채널, `#dfInput` 표시 필터(+자동완성), `#plBody` 패킷 목록, `#tree` 상세, `#hex`, 소켓 테스트, 스트림 따라가기, 내보내기 | 동일 |
| `/uds-read-did/app/index.html` (+engine/app/config/catalog/services/workflow/trace-tools/layout/project) | `#selEcu`, `#segRoute`(DoIP/CAN-TP/EthDiag), 연결 설정·주소 상세·SIM 옵션, `#toolTabs`(서비스·ECU·DID·명령·시퀀스·감시·프로젝트), `#didList`, `#req` DID/Raw, `#reqPdu`, `#hist`, `#detail`(seq/fields/hex), trace 도구, 확정값 | 동일 |
| CAN | **없음.** 기준 UI `ui/src/panels/can/canTrace.ts`는 TypeScript 번들 전용(`../../sdk`, `can.css` import). 빌드·패키지 설치 금지라 iframe 재사용 불가. slot에 그대로 "원본 없음·범위 외·실물 미확인"으로 표기하고 표로 위장하지 않음 | — |

## 추가된 것 (outer shell 1겹)

- **상단 탭** `통합 보기 | 에이전트 작업`, 공통 칩 "원본 SIM · 엔진 미연결", **하드웨어 STOP**(작업 취소와 분리, 엔진 미연결이라 시뮬 표기), 검사 버튼.
- **frame host**: iframe은 처음 한 번 생성되어 `#frameHost`에 고정된다. 레이아웃은 slot 자리표시자이며 iframe은 rAF로 좌표만 따라간다. **재부모화를 하지 않으므로 탭 전환·분할·크기 조절·최대화·숨김에서 reload가 일어나지 않는다**(iframe을 DOM에서 옮기면 반드시 reload되기 때문에 이 구조를 택했다). 숨김 = `visibility:hidden`, 상태 유지. 명시적 ⏏만 unload.
- **통합 보기 사이드**: 서비스 체크(표시/숨김), 프리셋(가로/세로/UART|UDS·ETH), slot ◂ ▸ ▾ 이동, ⤢ 최대화, 구분선 드래그. slot 최소 600×320, 부족하면 영역 스크롤 — scale 축소 없음. 세션/포트/ECU/채널 선택은 원본 툴바에 그대로 둔다(중복 UI를 만들지 않음).
- **에이전트 작업 사이드**(얇음): 작업 목록(진행/대기/실패/완료/취소), 현재 단계 체크리스트, 대기 조건, 오류, "근거" 링크 → 해당 원본 패널로 포커스(숨겨져 있었으면 표시, reload 없음). 오른쪽은 통합 보기와 **같은 frame**. 채팅형 카드·risk 등급·내부 용어 없음, 승인 강요 없음.
- **관측 strip**(두 탭 공통): source별 구독 상태 / 마지막 수신 / lag·drop·error. 마지막 수신은 frame 안 원본 SIM의 DOM 카운터(패킷 건수, history 건수, xterm 행) 변화로 측정해 작업 단계와 **자료 신선도를 분리**한다. lag/drop/error는 "엔진 미연결, 값 없음"으로 정직 표기. 작업 데이터는 목업임을 매 카드에 적는다.
- **검사 패널**(`?`): 각 frame의 원본 제어부 selector 존재 여부, outer shell 숨김 여부, 로드 횟수(1회여야 함), window 마커 보존, 탭 전환 전후 상태 지문(필터 입력값·DID 입력값·UART 컨텍스트) 일치 여부를 표시한다.

## 정직성 표기

Outer shell은 Bench Light 밝은 테마가 기본이며 터미널(xterm)은 원본이 어둡게 유지한다. 관측 strip의 "마지막 화면 변화"는 frame DOM 텍스트 변화를 1초 폴링한 시각으로, 엔진 텔레메트리가 아니다. UART sim / UDS external DoIP / ETH sim은 이전 동시대조 확인, CAN/PSU는 실물 미확인, video 미구현. 이 목업 안 frame은 모두 각 원본의 로컬 SIM이며 실제 Loom 엔진 연결을 주장하지 않는다.

## 한계와 다음 단계

- 기본 배치는 UART 위 / UDS 아래 세로 쌓기다(1297px에서 UART 폭이 900px을 넘어 원본의 overlay 서랍 분기를 피함). 좁은 slot에서도 adapter가 UART 세션 서랍을 inline으로 유지해 툴바/작업면을 덮지 않는다. UDS는 1180/860 breakpoint로 DID 열이 접히며 최대화로 복원.
- iframe 안 키보드 단축키는 포커스가 frame 안에 있을 때만 원본에 전달된다(outer가 가로채지 않음).
- 제품 적용 시 adapter는 iframe 대신 원본 모듈을 "shell-less mount 모드"로 띄우는 한 줄 옵션으로 바꾸면 되고, CAN은 제품 패널을 같은 slot 규약으로 노출하는 것이 다음 작업이다.

## 검증 방법 (ROOT Ego)

`http://127.0.0.1:64785/workbench-agent-tabs/reuse/fable/index.html` → UART 세션에 타이핑·ETH 필터 입력·UDS DID 입력 → 에이전트 탭 ↔ 통합 탭 전환, 분할 이동, 최대화 → `검사`: 로드 1회·마커 보존·지문 일치·제어부 ✓ 확인. "원본 shell 보기"로 원본 헤더 복귀 확인.

---

## 부록 (개정 2, 2026-11-14) — 핵심 패널만

사용자 수정("유아트면 유아트 패널만, 캔이면 트레이스창만, 조작은 에이전트가")에 따라 위 안의 "원본을 그대로 띄운다"는 **"원본 frame은 그대로, 보이는 것은 핵심 관측면만"**으로 바뀌었다. 같은 iframe/adapter 구조를 유지하되 adapter CSS가 `core` 모드에서 카탈로그·네비·연결 폼·매크로·전역 툴바를 숨기고 grid를 핵심면만으로 재정의한다. 사이드바 서비스 카탈로그는 제거하고, 식별은 서비스가 아니라 **세션/채널 단위 칩**(UART·mcu, UART·aux, UDS·BCM; 원본 DOM에서 읽음)으로 한다. 관측 카드 strip은 한 줄 **에이전트 운영 strip**(대상·작업·단계·상태·근거·최근)으로 바뀌고 대상 패널/세션 헤더에 배지가 붙는다. 서비스 점프는 같은 frame의 "전체 서비스" 토글(reload 없음)이다. 상세·검사 항목·범위는 `STATUS.md` 참조. CAN은 여전히 독립 원본이 없어 범위 외로 표기한다.
