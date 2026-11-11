# UART v2.0 점검 — Sol

2026-11-10. `02-core-principle.md`·마스터 §2.3 C1–C8·`04-v2-tasks.md`·`v2-sol.md`와 통합 `plan.md` 대조. **점검만 수행**, plan/구현/타팀 파일 수정·추가 실행시험 없음. 검토 도중 리드의 §9.6·스킬 REF 정합 수정이 보여 최신 내용을 다시 확인했다. 위치는 절·행 이름을 기준으로 한다.

## 1. C1·C2·C3·C8 판정

| 계약 | 판정 | 근거·남은 문제 |
|---|---|---|
| C1 | 구조 준수, 시험 표 잔재 1곳 | §3.2 Rust loomd/단일 CLI/포트별 worker·Rust Registry, §7.6 UART Python 0, §12 crate 작업 반영. **§9.3 계약 fixture의 pytest는 모순**(F3). 금지 도구 예시·제거표·명시된 Python 실험 이력은 Python 의존 유지가 아님 |
| C2 | 부분 준수, 상한 통합 보완 필요 | §3.13 byte/count·drop/spill/gap·48/64 MiB·크래시 격리, §9.5 혼합 24 h/RSS, §11 M3·T28 반영. DR-V21 예산 근거, DR-V22 정본 충돌, 브라우저 송신 대기 상한이 남음(F1·F2·F4) |
| C3 | 대부분 준수, 요청과 현행 계약 분리 필요 | §3.2 serde→Schema→TS·D10 폐기, §3.5 sent/partial·재송신 금지, §4.1 봉투/종료 코드·§8 B1/B6 유지. **§9.6.1의 exit 3 확장은 미확정 요청**(F5). **§9.6.4의 “정확 digest 대신” 바인딩은 §10.2/D6·CORE O7과 불일치**(F6) |
| C8 | 준수 | §4.8 짧은 본문·references 2종·help 정본·동기화 시험·loom-test 분리, T13/T20. 최신 스킬의 `data.evidence[].ref` 복사 규칙도 §9.6과 일치. pending exit 3 확정 때 재개 규칙만 함께 고정(F5) |

## 2. 사실·수치 교차 확인

- **§3.5·§4.5·§7·§8·§8.1·§9.1·§11·§12의 주요 Rust 전환은 반영됨**: POSIX nonblocking deadline/Windows COMMTIMEOUTS, 별도 write_timeout 부재, flush 성공 조건 제외, TIOCEXCL+flock, M/S·1.5 native 확장, DTR 후설정·오류 공개, macOS PTY baud 예외가 서로 맞는다. §9.1의 Python 수치는 이력이라고 명시되어 §9.4 Rust 수치와 혼동되지 않는다.
- **§9.4는 `spikes/uart/rust/results.txt`와 일치**: 4 MiB 1,008.116 ms/3.968 MiB/s·FNV `529aaa6348e22325`, read 202.015 ms, regex 64 KiB 117.754/113.843 µs. 물리 UART·최대 지연 보증으로 과장하지 않음. §9.5 payload 용량도 재계산 일치: 8×3M 8N1 24 h **193.119 GiB**, 921600은 **59.326 GiB**.
- **regex 선형성의 핵심 서술은 적절**: §4.5·§7.4·DR-V15는 단일 find의 O(m×n), §7.4는 find_iter/captures_iter의 O(m×n²)·근사 메모리 제한·hard cancel 부재를 구분한다. 검토 중 제거된 §9.6의 “선형 시간 [확인 예정]”은 최신본의 문제로 재등재하지 않는다. 새 count/증거 검증에도 같은 제한을 명시하면 좋다(R2).

**DR-V21 예산 [확인: 산술]**: 구독 큐는 이전 8×2=16 MiB에서 **16×2=32 MiB**로 16 MiB 증가한다. 저널 8채널×8=64 MiB와 합치면 **96 MiB**, gateway UART 128 MiB 중 **32 MiB**가 남는다. 따라서 16구독 자체가 128 MiB를 초과하는 것은 아니다. 다만 남은 32 MiB에 제어·인코딩·IPC 수신/WS 송신·snapshot/scan이 들어가는 **세부 예산이 없으므로 “128 MiB 안”은 아직 검증된 결론이 아니다**. worker의 48/64 MiB 예산은 구독을 gateway가 소유하므로 이번 변경으로 직접 증가하지 않는다.

**DR-V22 예산**: 2 MiB는 패널 수신·그리기 **대기열**이지 패널 전체 메모리가 아니다. §5.2는 아직 4 MiB, §3.13은 2 MiB다. 또한 §3.13의 16 MiB/32,768항목과 DR-V22의 trace 20,000행을 어떻게 함께 적용하는지 명시해야 한다. 행 수 제한·가상 스크롤만으로 byte/RSS 상한을 증명할 수 없다. 브라우저 메모리는 gateway 128 MiB와 별도이며 UI 팀 예산 확정이 남는다.

## 3. 리드 수정 목록

### 필수

**F1 — DR-V22의 2 MiB를 본문에 적용.** 위치: `plan.md` §5.2 “지연”(현재 4 MiB), §3.11 #9(일시정지 4 MiB), §3.13 “텍스트·UI”, §5.10 가상 스크롤.

대체 문안: “패널의 수신·정제·xterm 미처리 write를 합친 그리기 대기는 **2 MiB**로 제한한다. 포화 시 오래된 표시 데이터 생략 + 로컬 `display.gap{cause:render_backlog}`, parser 재동기화. 일시정지 해제도 이 큐를 늘리지 않고 유한 재질의하거나 gap으로 건너뛴다. 트레이스 보존은 **payload 16 MiB 또는 20,000행 중 먼저 닿는 상한**이며 원시 기록은 서버 저널에서 조회한다. xterm 10,000줄/최대 50,000줄은 별도 화면 보존 옵션이고 전체 JS/WASM/셀 저장소 예산은 UI 팀과 M1 전에 수치로 확정·시험한다.”

**F2 — DR-V21 정본 표·gateway 예산 동기화.** 위치: §3.13 live 구독 행(아직 전체 8개)·메모리 admission, DR-V21, §9.5 churn/T28.

대체 문안: “UART 데이터 구독 **채널당 4·전체 16**, 각 인코딩 후 2 MiB·512항목. 저널 큐 64 MiB + 구독 큐 32 MiB = 96 MiB. 남은 32 MiB는 제어·인코딩·IPC/WS·snapshot/scan의 합산 계정이며 CORE와 하위 상한을 P0에 배정한다. 임시 사본·참조가 붙잡는 버퍼도 charge하고 전체 128 MiB permit을 못 얻으면 신규 작업을 거절/보류한다. 별도 무계수 큐를 두지 않는다. soak는 **8채널×2구독=16개**, 한 채널 4개 및 17번째 거절도 검증한다.” 현재 수치만으로 초과라고 판정하지 말고, 예산 배정·실측 전 ‘128 MiB 안’을 미검증으로 표시한다.

**F3 — Python 시험 의존·옛 M1 계측 제거.** 위치: §9.3 “계약 fixture” 도구 열, “지연·폭주” 행.

대체 문안: 도구는 “**CORE Rust fixture + cargo test**”. 지연 행은 “**M1: host_read_complete→웹 xterm write cb 뒤 첫 rAF, 시계 맞춤 오차 포함(§3.11). TUI stdout write 완료 계측은 M2 보조 시험**”으로 바꾼다. 현재 M1 stdout 계측은 §11 웹 관문과 모순이다.

**F4 — 사용자 입력의 앞단 대기도 bounded로.** 위치: §5.3 입력 송신기/ack, §3.13 TX/InputArbiter.

추가 문안: “브라우저 입력 송신기는 worker credit과 연동해 미확인 데이터 **4 KiB·64항목**, Ctrl-C 예약 **64 B·64항목**을 넘겨 쌓거나 WS에 미리 밀어 넣지 않는다. 포화 시 입력 보류/미수용을 표시하고 수용한 바이트는 무음 drop·재송신하지 않는다. gateway의 대기 입력 RPC도 같은 credit으로 제한한다. WS bufferedAmount/전송 중 사본을 포함한 추가 버퍼는 CORE·UI 예산에 계수하며 RX 팬아웃은 이 입력 경로를 막지 않는다.” worker 뒤쪽 큐만 제한해서는 브라우저·gateway 앞단 누적을 막지 못한다.

**F5 — exit 3 확장은 현행 계약과 분리.** 위치: §9.6.1 absent/count 표·종료 코드 요청, §4.1 exit 3, §4.8 timeout 재개 규칙, DR-V7.

대체 문안: “A6 정본은 **exit 3=대기 timeout만**이다. `E_UNEXPECTED_MATCH`/`E_COUNT_OUT_OF_RANGE`의 exit 3 사용은 CORE/메인 수용 전 미확정안이며 구현·help·스킬에 확정값으로 배포하지 않는다. 확정 전 판정 질의는 exit 0 + `data.verdict:pass|fail`, 증거 불충분은 exit 8로 분리한다. 자동 재개는 **`error.code=E_TIMEOUT`이고 resume_from이 있을 때만** 한다.” 요청 자체는 위반이 아니지만 표가 확정 계약처럼 읽히는 문제를 없앤다.

**F6 — 사전 승인에도 공통 바인딩 유지.** 위치: §9.6.4 마지막 “정확 digest 대신” 문장, §10.2 approval_binding, `_xteam/core-test.md` O7(읽기 근거).

대체 문안: “세션 사전 승인은 **CORE O7의 session_id·plan digest·env·정책/프로필 해시·stream_epoch·기한·횟수 상한을 유지**한다. UART의 장치 신원·generation·허용 rule/바이트 조건은 추가 바인딩이다. 각 실행의 실제 TX digest를 감사하고 승인된 plan에 속하는지 presend에서 검사한다. 세대/정책/내용 변경과 계획 밖 요청은 공통 무효화·개별 승인 규칙을 따른다. rule 이름만으로 공통 digest 바인딩을 대체하지 않는다.” C7의 일괄 승인 요구가 D6/A2의 공통 필드 삭제를 허용하는 것은 아니다.

### 권장

**R1 — Python 시절 설명·플랫폼 범위 정리.** 위치: §3.4 ERROR 예시, §8.1 #3, §9.3 실 HW 행, §13 고속 팬아웃 리스크.

대체 문안: 각각 “지원하지 않는 native 설정은 §7.2 capability/readback 기준 `E_UNSUPPORTED_PARAM`”, “serialport 기본 **TIOCEXCL+flock**, 실 tty의 배타 효과는 HW gate”, “Windows/Linux 실 HW, macOS 개발·시뮬레이션만(B1)”, “고속 팬아웃의 **직렬화·CPU·렌더 경합**”. `serialposix.py` 근거·macOS 실 HW 선택·GIL은 현재 구현 설명에서 빼고 필요한 이력만 남긴다.

**R2 — count/증거 검증의 regex 예산도 명시.** 위치: §9.6.1 count, §9.6.3 EvidenceVerifier, §7.4.

추가 문안: “증거 검증은 CORE의 bounded 실행기에서 streaming하고 WS/RPC 수신 루프를 막지 않는다. **64 MiB 스캔 상한은 총 I/O량이며 RAM window는 64 KiB**다. 정제/regex 옵션·원래 검색 문맥을 재현한다. 개수 열거는 전체 O(n)이라 주장하지 않고 match 수·CPU·byte 예산을 적용하며 초과 시 uncertain/eval_budget. regex 단일 호출은 hard preemption 불가다.” CORE CPU 200 ms를 UART regex의 강제 중단 보증으로 해석하지 않는다.
