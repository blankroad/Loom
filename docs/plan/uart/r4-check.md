# UART R4 최종 점검 — Sol

대상: [plan.md 통합본 v1](plan.md), 마감 재확인 1,230줄, SHA-256 앞12자리 `c09f70d87527`. 점검 중 다른 작성자의 갱신을 확인해 재검토했다. 기존 literal/regex 예시, E1의 M1 범위, T12 의존성, M2 txt export 의존성은 그 갱신에서 해소됐다. 아래 판정은 이 재확인본 기준이며 이후 변경은 별도 재점검 대상이다. 기준: [핵심 원칙](../02-core-principle.md), [공통 브리프](../00-brief.md), [UART 브리프](brief.md), [R2 리뷰](r2-sol-review.md), [R3 사실 확인](r3-facts-sol.md). **점검만 했으며 plan.md·기존 산출물·스파이크는 수정하지 않았다.**

**판정: 필수 수정 후 통과 권고.** 15개 절과 브리프 항목은 모두 존재한다. 주요 사실 정정·실시간 계측·신원 매칭은 잘 반영됐다. 남은 문제는 기능 누락보다 **regex 재개 경계, 입력권 예외, TX deadline, 터미널 격리, 승인 상태 전이**다. 아래의 통과는 계획 요건 판정이며 구현·성능 시험 통과가 아니다.

## 1. 핵심 원칙 6항목

| 항목 | 판정 | 근거 절 | 부분이면 반영할 수정 문안 |
|---|---|---|---|
| 맨 앞 핵심 루프 4요소 표 | **통과** | §0: 직접 접근·읽기·쓰기·라이브 표시/개입 표 | — |
| 구체적인 send→응답→화면 지연→사용자 개입 | **통과** | §2.0 E1의 명령·두 모드 화면·지연 예산·Ctrl-C·agent warning 충분. 마감 갱신에서 secret/run/내장 profile이 M1에 명시되고 prompt 옵션도 정정됨 | 예시가 실행된 것으로 읽지 않도록 현재 제안/실측 예정 표기 유지. |
| 실시간 목표와 고부하 시 정직한 화면 | **부분** | §3.11: stdout 완료 계측, p95, lag/idle, 3개 손실 카운터, 정상 부하 skip0, Ctrl-C gate 반영 | “모든 driver write는 유한 timeout·취소 경로를 가지며, bridge/parser 포화는 명시 gap/degraded로 처리한다. 상태줄 격리가 검증되지 않은 모드는 별도 trace/status 영역을 쓴다.” 필수4·5. |
| M1=시뮬레이션 위 네 요소 걷는 뼈대 | **부분** | §11 M1·§12 T01–T13에 네 요소와 수용 기준 있음 | T12 의존성은 갱신에서 수정됨. 남은 §11(f)의 “사용자 승인 강제”와 chat-relay의 관계를 필수6처럼 정리하고 입력/TX/표시 예외를 함께 시험한다. |
| 동일 스트림·late attach·숨은 연결 금지 | **통과** | §0·§3.9/3.12: 채널 결합, seq H 이음매, 공개 direct 경로 없음, POSIX 한계·no_viewer 경고 명시 | 같은 OS 계정의 임의 외부 open을 기술적으로 막는 보장과 설계상 우회 경로 금지는 이미 구분돼 있음. 재연결 matcher 경계는 필수7로 보강. |
| 무관한 기능 후순위/비범위 | **통과** | §1·§11 M4: 웹/viz/XMODEM/probe/msgpack/에코 추론 후순위 | — |

## 2. 공통 브리프 §4의 15개 절

모두 존재한다. 코드블록 안의 SKILL 소제목은 계획서 절 수에 세지 않았다.

| 절 | 존재·충분성 | 보완 문안/조치 |
|---|---|---|
| 1 범위/비범위 | **통과** | M1–M3, 후순위, 비범위 명확. |
| 2 시나리오 | **통과** | agent **A1–A5 5개**, user **U1–U6 6개**로 수량·구체성 충족. E1 범위/옵션 수정 반영. A5의 resume 보장 문구만 §4.5와 함께 제한(필수1). |
| 3 아키텍처 | **보완 필요** | “viewer 연결 수명과 채널의 미완결 입력 상태는 분리한다. dirty 입력은 detach로 지워지지 않는다.” write deadline·재연결 wait 예외도 명시(필수3·4·7). |
| 4 CLI/API | **보완 필요** | literal 예시는 갱신에서 일치함. `--confirm`→`--approval`, run의 regex prompt 내부 의미, resume_from 상한, BREAK host 관측/물리 파형을 보완. `--after-request`·`--cursor-token`을 해당 명령 synopsis에도 추가. |
| 5 사용자 뷰어 UX | **보완 필요** | DECSTBM/CUP clamp만으로 상태줄 보호를 완료했다고 쓰지 않는다. 안전 fallback·제어 시퀀스 범위·parser 길이 상한 추가(필수5). |
| 6 데이터/기록 | **통과** | raw+인덱스, seq/ro/fo, wall/mono, rotation·durability·redaction·viz 참조 충분. secret 원본 예외 및 quota도 명시됨. |
| 7 라이브러리 | **통과** | R1–R3 근거, 버전·라이선스·플랫폼/유지보수 판단 및 lock 때 LICENSE 검증 있음. regex CPU 시간의 확인 범위만 POSIX로 한정(§4). |
| 8 HW/OS | **통과** | 후보/HW gate, stop1.5·dsrdtr·flock·신호 한계, Windows sim/콘솔/COM 단계 구분. “권한 없음”은 “장치 접근 권한/driver 정책 확인”으로 완화 권장. |
| 9 시뮬레이션/테스트 | **보완 필요** | “E1 M1 버전, dirty detach, CTS stall, ED/CUD/VPA·긴 미종결 escape, PENDING ID 재제출, reconnect 경계”를 회귀 사례로 추가. regex의 긴 span이 timeout 경계를 넘는 재개 시험 추가. |
| 10 안전 | **보완 필요** | “agent의 approval ID 재제출만으로 PENDING→APPROVED 전이하지 않는다.” 채팅 중계 주장과 runtime 승인 구분(필수6). |
| 11 마일스톤 | **보완 필요** | E1 기능과 txt export의 단계 불일치는 갱신에서 해소. §11(f) 승인 gate의 의미만 §10.2와 통일(필수6). Windows 단계는 일치. |
| 12 작업 분해 | **통과** | **T00–T26에 담당 O/S·의존·병렬 열 모두 있음.** 마감 갱신에서 T12에 T09/T10/T13, T11에 조립 의존성이 추가됨. 서두 “T11 합류”→“T12 판정”은 편집 정정 권장. 한 담당의 병렬은 독립 가능한 단위를 뜻한다고 명시하면 더 좋음. |
| 13 리스크 | **통과** | 신원·glitch·고부하·secret·승인 신뢰 한계·regex fallback 등 충분. 필수 수정 후 관련 완화 문장도 동기화. |
| 14 질문 | **통과** | Q1–Q13 모두 기본값 있음. Q5 채팅 승인·Q8 전체 화면 지원은 필수5·6 결정과 맞춰 기본값 수정. |
| 15 공통 계약/타팀 | **통과** | 메인 결정/IPC·role·기록·안전·viz 의존성과 시점 명확. CAN viz M3 수용/UART 생산 M4 차이는 공급 시점만 명시하면 됨. |

## 3. UART 브리프 “꼭 다룰 것” 누락 점검

| 필수 묶음 | 판정·근거 |
|---|---|
| 탐색·VID/PID/설명·serial 파라미터·재연결·다중 포트 | **누락 없음** — §1/3.3/3.8/4.2/8. 8포트 지원은 M3 실측 후 선언. |
| 데몬 단독 소유·동시 CLI/TUI·입력 중재/출처 | **누락 없음, 예외 보완 필요** — §0/3.5–3.7/5.4. dirty detach·복수 viewer 문자 혼합은 필수3. |
| text/hex/file·EOL·tail/since·expect·독립 커서·ANSI/binary·출력 상한 | **누락 없음** — §3.3/3.6/4.3–4.5. literal/regex 예시는 불일치. 커서 기본 `agent` 공유는 별도 LOOM_CLIENT 필수 안내를 유지하고 자동 식별 보강 권장. |
| herdr attach COM3·timestamp/hex·agent TX·검색·저장·Ctrl-C/탈출키·웹 검토 | **누락 없음** — §2 U6/5/11. raw 키·trace·검색/저장 단계도 명시. 상태줄 보호 보완 필요. |
| BREAK·DTR/RTS·reset/boot·X/YMODEM 범위 | **누락 없음** — §1/4.7/8. BREAK 반환값이 선로 실측처럼 읽히는 표현은 수정. |
| raw+timestamp·줄 로그·공통 레코드·숫자 viz | **누락 없음** — §6. `_xteam/viz-requirements.md ## uart` 존재와 V-UART-0…5를 읽어 확인. |
| PTY/socat·Windows 가상 포트·가짜 부팅/login/prompt·무HW CI | **누락 없음** — §8/9. Windows sim이 물리 COM/콘솔 검증이 아니라는 제한도 적절. |
| 라이브러리 조사·유지보수·Windows | **누락 없음** — §7/8, R1–R3 근거 연결. 새 조사나 스파이크는 이번에 수행하지 않음. |

## 4. 사실·API 오류와 근거 범위

| 항목 | 판정·근거 | 최소 수정 |
|---|---|---|
| **resume_from = 스캔 끝 − 최대 패턴 길이** | **새 갱신의 확정 설계 오류, 높음.** §4.5의 regex는 pattern 문자열 길이로 match span 상한을 정할 수 없다. 예: `BEGIN.*END`는 짧은 pattern이지만 긴 줄 전체를 가로지를 수 있어 짧게 overlap하면 BEGIN을 잃는다. “구간 손실 없이 이어짐”은 보장되지 않음. | literal은 인코딩된 최대 literal 길이 기준 overlap, regex는 지원 window의 시작부터 재스캔/부분 상태 보존. 지원 범위 밖은 E_SCAN_LIMIT/gap. 필수1. |
| literal/regex 예시 | **마감 갱신에서 해소.** E1/A1/SKILL은 `--expect-prompt`, A5는 두 literal, A2 grep/flood는 `--regex`로 수정됨. | 이미 고친 예를 다시 고칠 필요 없음. §4.6 run의 내부 동작도 regex prompt임을 명시하면 됨. |
| §0의 `--confirm` | **확정 명세 불일치, 높음.** 최종 CLI는 `--approval`. | §0 표를 `--approval`로 교체. |
| §4.7·DR-25 BREAK “250ms 양자화/실측 지속시간” | **과도한 물리 보장, 높음.** R1은 API 소요시간만 측정했고, source의 `int(duration/0.25)`는 tcsendbreak 인자 변환이다. host on/sleep/off 경과는 USB 핀의 실제 BREAK 길이가 아님. | “`host_assert_elapsed_ms`는 host 호출 구간. `wire_duration_ms`는 로직 분석기로 확인할 때만 채운다. 물리 파형은 HW gate.” 250ms 물리 양자화 단정 삭제. |
| §3.5 `[확인: Sol 스파이크 설계]` | **근거 등급 정정, 높음.** R1 probe는 binary/PTY/bulk 검증이며 `read(1)→in_waiting drain→bounded bridge` 데몬을 구현·측정하지 않음. | `[제안: r1-sol.md 설계, M1 검증 예정]`. |
| regex timeout은 CPU 시간 | **macOS/POSIX 범위에서 R3와 일치.** R3는 Windows/3.11 미측정을 명시했는데 §4.5/7 문장은 전 OS 확정처럼 읽힘. | “확인한 POSIX 환경에서는 process CPU clock 기준. 타 OS에서도 wall deadline을 별도 적용하고 실제 timeout/GIL은 CI 검증.” |
| flock·`$`·ring 보존시간·stop1.5·dsrdtr | **정정 반영, 높음.** R2 F1–F4와 일치. | 유지. |
| 패키지 버전/라이선스·Textual suspend | **기존 근거와 충돌 없음.** R2 metadata 15종+R3 regex, aiohttp AND MIT, urwid exact SPDX, Textual Unix/Windows, pyte artifact 검증 유보 반영. | 공급망/전체 OS 검증 완료로 확대하지 말 것. |

## 5. R2 P0 항목 해소 여부

| R2 항목 | 판정 | 반영된 것 / 남은 위험 | 최소 수정 |
|---|---|---|---|
| **F1 POSIX exclusive** | **해소** | §0/3.12/8/DR-16이 flock·비협조 open 가능·실 HW gate를 정확히 구분. | 현재 문구 유지. |
| **B1 미완결 사용자 입력** | **부분** | 3초 idle 버그는 해결. 그러나 §3.7-3은 **detach/crash 즉시 반환**: 이미 보낸 `ifconfig eth`가 장치에 남아 있어도 agent가 이어 쓸 수 있음. 갱신의 두 번 yield 시 Ctrl-U 송신도 모든 콘솔에서 줄 정리 증거가 되지는 않음. 같은 user lease를 공유한 두 viewer의 `re`+`boot\r`도 합쳐짐. | 채널 `input_dirty`를 연결 수명과 분리. dirty 상태는 detach/timeout으로 해제하지 말고 명시 정리/검증 후 전환. 입력 owner는 viewer_id 하나, 나머지는 watch·명시 takeover. |
| **B2 대형 TX/Ctrl-C** | **부분** | 전용 TX 스레드·chunk≤1KiB·flush 제거·partial 공개·Ctrl-C 락 우회는 반영. **write_timeout/막힌 write 취소**가 빠져 CTS stall 때 다음 chunk 경계에 도달 못할 수 있음. `written`은 장치 선로 시작이 아님. | 유한 write timeout·전체 monotonic deadline·지원 backend 취소 명시. OS accepted/실제 장치 반응 지표 구분. CTS stall 중 Ctrl-C 및 RPC/뷰어 진행 시험. |
| **B3 패스스루/상태줄/출처** | **부분** | echo 추론 제거, 확정 TX trace·OSC/질의 차단은 해소. DECSTBM/CUP clamp만으로는 **ED 전체 지우기, CUD/VPA 등 다른 이동, origin mode·저장/복원 상태**를 격리하지 못함. 토크나이저만으로 “top/vi가 장치 의도대로”라는 보장도 미검증. | 상태 정보는 별도 trace/pane 또는 검증된 화면 모델에 둔다. 지원 시퀀스 allowlist·미지원 fallback·최대 escape/부분행 길이·안전 mode reset을 명시하고 adversarial 시험을 M1에 둔다. |
| **B7 재연결 신원/세대** | **부분(신원은 해소)** | 엄격 serial+interface·승인 location·약한 신원 차단·TX/approval 무효화 반영. read/expect 연속은 기록된 리드 선택으로 수용. 다만 §3.4 `--wait-connected`의 새 세대 송신과 §3.8 “대기 send 전부 취소”가 모순. 세대 전후 RX를 붙여 가짜 패턴을 만들 수도 있음. | **송신 전 대기 job만** 명시 opt-in으로 새 세대 정책 재검증 허용, interrupted/partial send는 절대 replay 금지. matcher/decoder 부분 상태는 generation/gap 경계에서 reset하고 각 세대 내부에서만 매치. |
| **B8 승인/role** | **부분** | 서버 role·attach 자격·digest/세대/단회 승인·R3 일괄 승인 제거·동일 OS 권한 한계는 반영. 그러나 기본 interactive의 **agent ID 재제출→chat-relay APPROVED**는 여전히 사용자 승인 증거 없이 자기 승인 가능. 한계 고지로 §11(f)의 승인 강제를 증명할 수 없음. | PENDING→APPROVED는 user 자격 또는 신뢰된 host 승인 중계만. agent의 `--approval`은 승인 객체 조회·소비만 가능. 해당 중계가 없는 M1은 R3/R4 송신 거절로 gate를 충족. |

## 6. 리드가 반영할 수정 목록

**필수**

1. **새 resume_from 규칙을 한정하고 남은 옵션 이름을 동기화**한다. “literal은 최대 인코딩 byte 길이−1을 overlap. regex는 window 시작부터 재검사하거나 검증된 partial state를 보존하며, 유한 window 밖의 무손실 재개는 약속하지 않고 E_SCAN_LIMIT/gap을 반환.” §2 A5의 “구간 손실 없이”도 같은 조건으로 제한한다. §0 `--confirm`은 `--approval`, §4.6 run의 내부 동작은 `--expect-prompt` 또는 사용자 prompt의 regex 평가로 명시한다. **E1/A1/SKILL·A2·A5·flood의 기존 플래그 오류는 마감 갱신에서 이미 수정됐다.**
2. **M1 수용 기준에 남은 반례를 연결**한다. E1 필요 기능의 M1 승격, T12의 T09/T13 의존성, txt export의 M2 이동은 이미 반영됐다. 이제 T12가 아래 dirty detach/CTS stall/상태줄 공격/미승인 ID 재제출/regex 재개 경계를 함께 검사하도록 §9·§11을 연결한다. 범위가 넓어진 M1을 줄이고 싶으면 인증 없는 fixture·literal만으로 네 요소를 먼저 통합하는 M1a를 권장하되, 확정된 기능을 다시 M2 누락으로 지적하지 않는다.
3. **입력권 예외를 닫는다.** §3.7/DR-4/DR-26에 “`input_dirty`는 채널 상태이며 viewer 종료로 지워지지 않는다. owner 상실 시 agent TX는 보류하고 정리/명시 회복을 요구한다. Ctrl-U는 검증된 profile에서만 줄 취소로 간주한다. 여러 viewer 중 writer는 하나”를 넣는다. detach/crash·복수 viewer·Ctrl-U 미지원 콘솔 반례를 추가한다.
4. **TX deadline과 불완전 송신을 명시**한다. “모든 write는 남은 wall budget에 맞춘 유한 write_timeout을 쓴다. stall 시 지원 cancel 경로/실패 결과로 빠져나오며 partial은 자동 재전송하지 않는다. driver 수용 바이트와 물리 선로 송신 시작을 구분한다.” CTS low·driver stall에서 viewer/RPC·Ctrl-C gate를 시험한다.
5. **상태줄 격리를 미검증 완료로 쓰지 않는다.** §5.1/5.2·DR-3/Q8에 “지원 terminal subset만 보장. 토크나이저의 clamp만으로 전체 VT 격리를 보장하지 않으며 충돌 시 별도 trace/status pane 또는 검증된 화면 모델로 전환”을 넣는다. ED/CUD/VPA/alternate·원점 모드·긴 미종결 escape 시험을 M1 T07에 포함한다. bridge/parser 포화 시 discard 범위와 gap/degraded 처리를 명시한다.
6. **PENDING ID 재제출만으로 승인을 만들지 않는다.** §10.2·§11(f)·Q5/DR-11을 “agent는 이미 APPROVED인 객체만 소비 가능. 실제 사용자 승인 전이는 user 자격/신뢰된 host 중계만”으로 맞춘다. 같은 ID를 agent가 즉시 재제출했을 때 **무송신**을 검증한다. M1에 승인 채널이 없으면 R3/R4 거절만 지원해도 일반 쓰기 핵심 루프는 성립한다.
7. **재연결 wait/매처 경계를 한 문장으로 통일**한다. “write 이전의 명시 `--wait-connected` job만 새 세대 정책·신원·승인을 재검증해 실행 가능; partial/interrupted TX는 취소. expect job은 유지하되 이전 세대/손실 구간의 미완결 decode·match window는 이어붙이지 않는다.”
8. **근거와 물리 측정 표현을 정정**한다. BREAK 결과는 host assert 경과와 wire waveform을 분리하고 DR-25의 물리 양자화 단정을 제거. reader bridge는 `[제안]`으로, regex CPU clock 확인은 POSIX로 한정한다. 반영 후 말미 core 자체 점검의 전부 ✅를 다시 판정한다.

**권장**

9. bridge byte/event·TX 큐·parser 부분행/escape·worker queue의 수치 상한과 전체 채널 상한을 한 표로 모은다. 이미 bounded인 구조를 구현자가 임의로 풀지 않게 한다.
10. 자동 client 이름 또는 세션별 안정된 agent 식별을 검토한다. 현재 `LOOM_CLIENT` 미지정 시 공통 `agent` cursor는 순차 실행에서도 데이터를 소비하므로 revision 충돌만으로 모두 탐지되지 않는다. page 응답 유실 복구는 **요청 때 사용한 시작 token**을 보존하는 예를 추가한다.
11. `--no-record`가 자동 기록 공통 계약의 예외임을 §15에 요청하고, intentional disabled를 failed와 별도 상태로 정의한다. “기록 없음인데 agent TX 허용 여부”를 명시한다. macOS/Linux 권한 표와 CAN viz M3/UART 생산 M4 시점도 간단히 보강한다.
