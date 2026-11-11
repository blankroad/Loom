# v2 R1 — explore-sol: 사실 확인 + Rust 실현성 (TEST + EXPLORE)

배경: 마스터 v2.0(§2.3 C1–C8)에서 코어와 장치 경로는 **Rust**로 바뀌었다. Python은 대안이 없을 때만 쓴다(C1). EXPLORE 팀은 새로 **TEST 기획**(TC·자연어 기반 통합 테스트, C7)을 맡았고, 탐색 도메인 계획도 Rust로 이식한다. 너는 사실 확인과 짧은 Rust 스파이크를 맡는다. 계획서 본문은 리드와 explore-opus가 쓴다.

먼저 읽을 것:
- `docs/plan/00-master-plan.md` §2.3
- `docs/plan/test/brief.md`
- `docs/plan/explore/plan.md` §5(도메인별 라이브러리)와 `docs/plan/explore/v12-saleae-check.md`

## 산출물
`docs/plan/test/r1-sol.md`, 300줄 이하. 항목마다 지킬 것:
- 버전·최근 릴리스일·라이선스·플랫폼(Windows x64 / Linux x64 / macOS)을 적는다.
- **출처 URL**을 붙인다.
- 확인 못 한 것은 "미확인"으로 적는다.
- 끝에 권고 요약 표를 둔다.

## A. TEST 관련
1. **TC 가져오기**
   - Rust 엑셀 읽기 crate(`calamine` 등)와 CSV.
   - Gherkin Rust crate 현황.
   - 자동차 업계에서 흔한 TC 관리 도구의 내보내기 형식: Polarion, codeBeamer, TestRail, Xray, ASAM ATX/XIL 등. 무엇을 현실적으로 가져올 수 있는지만 짧게 정리한다.
2. **보고서**
   - JUnit XML의 사실상 표준 스키마 변형(Ant/Jenkins/Surefire)과 CI 도구 호환.
   - Rust crate(`quick-junit` 등).
   - HTML 템플릿 crate(`minijinja`·`askama`).
3. **증거 술어(판정 조건) 표현**
   - 후보: CEL(`cel-interpreter` 등 Rust crate), JSONPath(`serde_json_path`), jq 계열(`jaq`), 자체 단순 비교 DSL.
   - 각각 성숙도·라이선스·평가 비용·에러 메시지 품질을 비교한다.
   - **짧은 스파이크**를 한다.
     - 위치: `spikes/explore/v2-evidence/`
     - 샘플 이벤트 JSONL을 만든다. 50줄 정도로, UART 텍스트·CAN 신호·UDS 응답 이벤트를 섞는다.
     - 상위 후보 1–2개로 "seq 범위 안에서 술어를 만족하는 이벤트가 있는가 / 신호가 [a,b] 안에 hold 시간만큼 머무는가"를 평가한다.
     - 빌드·실행 시간과 결과를 기록한다.
4. **결정적 재실행**: 에이전트 실행 기록을 시나리오로 고정할 때 쓸 만한 선례를 짧게 정리한다(예: 녹화·재생 테스트 도구, Robot Framework의 키워드 고정). 링크 2–3개.

## B. EXPLORE 도메인 Rust 실현성 (C1)
1. **Saleae**
   - `logic2-automation` 저장소의 `.proto` 파일 위치·라이선스.
   - `tonic`/`prost` 현재 버전.
   - **짧은 스파이크**를 한다.
     - 위치: `spikes/explore/v2-saleae-rs/`
     - proto로 Rust 클라이언트를 생성하고 빌드한다.
     - 장치나 앱 없이 `localhost:10430` 연결 실패를 정상 처리하는 것까지 확인한다.
   - Python 어댑터가 필요한지 결론을 낸다.
2. **DLT**: `dlt-core`(esrlabs) 등 Rust crate. 버전·라이선스, 파싱·필터·제어 메시지(set-log-level 0x01, get-log-info 0x03) 지원 여부, 자체 codec과 비교.
3. **SSH**: `russh` / `ssh2`(libssh2) / `openssh` crate. 비교 항목:
   - PTY와 같은 연결의 여러 exec 채널
   - Windows 지원
   - 서버 기능(시뮬레이터용)
   - 라이선스
4. **Debug 프로브**: `probe-rs` 버전·라이선스. 확인 항목:
   - attach 시 halt 없이 붙는가, 보호 해제·erase를 자동으로 하지 않는가
   - RTT 지원
   - J-Link·ST-Link·CMSIS-DAP 지원
   - pyOCD를 대체할 수 있는가
   
   TRACE32 원격 API(RCL)를 Rust에서 쓸 방법이 있는지(C 라이브러리·프로토콜)도 본다.
5. **SCPI/VISA(PSU·스코프, 구매 시)**: raw TCP 소켓 SCPI 직접 구현과 `visa-rs`(NI-VISA 의존) 등을 비교한다.
6. **릴레이·IO**: `hidapi` crate, `tokio-modbus`.
7. **LA sigrok 2순위**: sigrok-cli 하위 프로세스는 언어와 무관하게 유지 가능함을 확인만 한다.

## 규칙
- Rust 스파이크에서 지킬 것:
  - **`CARGO_HOME`을 스파이크 폴더 안**(예: `spikes/explore/.cargo-home`)으로 지정한다. `~/.cargo` 설정과 레지스트리를 건드리지 않는다.
  - `CARGO_TARGET_DIR`도 스파이크 폴더 안에 둔다.
  - `cargo install`이나 전역 설치를 하지 않는다.
- 쓰기는 `docs/plan/test/`와 `spikes/explore/`만 한다.
- 웹 확인은 공식 문서·crates.io·GitHub를 우선한다.
- 시간은 90분 이내로 한다.

끝나면 대기한다.
