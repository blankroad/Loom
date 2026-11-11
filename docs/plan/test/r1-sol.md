# v2 R1 사실 확인 — TEST + EXPLORE Rust 실현성

확인: 2026-11-10 KST. 마스터 §2.3 C1–C8, TEST brief, EXPLORE §5 및 Saleae 확인서를 읽었다. 계획 본문은 수정하지 않았다.
**결론:** 조사 대상에 Python 어댑터를 필수로 요구하는 근거는 발견되지 않았다. Saleae Rust 코드 생성·빌드와 무앱 연결 실패 처리까지 성공했다. 실제 장치 호환은 별도 관문이다.
버전·날짜·라이선스는 crates.io의 최신 **non-yanked stable** 메타데이터를 기준으로 한다(날짜 UTC). [수집 결과](../../../spikes/explore/v2-facts/crates.json), [수집 코드](../../../spikes/explore/v2-facts/collect.py).
플랫폼 표기: **W=Windows x64 / L=Linux x64 / M=macOS**. `P`=소스/API상 이식 가능한 경로(판단), `○`=공식 지원 경로, `△`=외부 런타임·드라이버 등 조건부, `×`=지원 안 함. P는 3-OS 빌드 성공을 뜻하지 않는다.
실행한 환경은 **macOS 27.0.1 arm64, rustc/cargo 1.93.1, protoc 29.6**뿐이다. Windows/Linux 빌드와 모든 실장비 시험은 **미확인**. macOS는 개발·시뮬레이션 용도라는 B1을 유지한다.

## A1. TC 가져오기

| 후보 | 최신 버전 / 릴리스일 | 라이선스 | W/L/M | 확인·용도 / 출처 |
|---|---|---|---|---|
| calamine | 0.38.0 / 2026-11-02 | MIT | P/P/P | 순수 Rust, xls/xlsx/xlsm/xlsb/ods 등 읽기. Office 설치 불필요. [메타데이터](https://crates.io/api/v1/crates/calamine) · [README](https://github.com/tafia/calamine) |
| csv | 1.4.0 / 2025-10-17 | Unlicense/MIT | P/P/P | CSV reader/writer, Serde·ByteRecord. [메타데이터](https://crates.io/api/v1/crates/csv) · [문서](https://docs.rs/csv/latest/csv/) |
| gherkin | 0.16.0 / 2026-04-23 | MIT OR Apache-2.0 | P/P/P | `.feature` 구문→AST 파서, Rust 1.88+. [메타데이터](https://crates.io/api/v1/crates/gherkin) · [저장소](https://github.com/cucumber-rs/gherkin) |
| cucumber | 0.23.0 / 2026-04-23 | MIT OR Apache-2.0 | P/P/P | Gherkin 실행 프레임워크, 파서와 별개. [메타데이터](https://crates.io/api/v1/crates/cucumber) · [문서](https://cucumber-rs.github.io/cucumber/current/) |

- **권고:** M1은 calamine+csv로 `TC ID / 전제 / 단계 번호 / 동작 / 기대 결과`를 정규화한다. 공급자별 열 이름·여러 행으로 나뉜 단계·병합 셀·원본 행 위치를 보존하는 매핑이 필요하다. 파서가 자연어 기대 결과를 실행 가능한 판정으로 바꿔 주지는 않는다.
- calamine은 읽기 라이브러리다. 수식 문자열·저장된 값을 읽는 것과 Excel 재계산은 구분한다. 대형 xls/ods는 전체 시트 로딩이 있으므로 입력 크기·행 수 상한을 둔다. CSV는 구분자·BOM·문자 인코딩을 명시한다. [calamine 설명](https://github.com/tafia/calamine) · [CSV ReaderBuilder](https://docs.rs/csv/latest/csv/struct.ReaderBuilder.html)
- Gherkin은 선택적 입력 형식으로 적합하다. cucumber의 step handler를 TC마다 Rust로 작성하게 하면 C7의 목적과 어긋난다. 공통 장치 동사/판정으로 변환하는 경로를 먼저 둔다. [Cucumber Rust book](https://cucumber-rs.github.io/cucumber/current/)

아래는 **내보낸 파일을 가져오는 가능성**만 확인했다. Polarion/codebeamer/TestRail/Xray의 제품 최신 버전·최근 릴리스일 및 서버 OS는 **미확인**; 상용 제품의 정확한 계약 조건도 미확인이다. 파일 처리기는 W/L/M=P이며, 공급자 앱 자체의 지원 OS와 구분한다.

| 원천 / 확인 문서 버전 | 형식·현실적 가져오기 범위 | 라이선스·플랫폼 / 출처 |
|---|---|---|
| Polarion / 2304·2404 기능 문서 | Excel XLSX round-trip의 Test Steps, LiveDocs의 test steps를 포함한 ReqIF 교환. M1은 TC·step·expected 열 매핑부터; ReqIF는 속성/링크 매핑이 추가로 필요 | 상용; 파일 W/L/M=P. [2304 Excel 단계](https://blogs.sw.siemens.com/polarion/polarion-alm-2304-whats-new-and-noteworthy/) · [2404 ReqIF 단계](https://blogs.sw.siemens.com/polarion/polarion-alm-2404-whats-new-and-noteworthy/) |
| codebeamer / r2.2·r3.0 문서 | Excel round-trip은 단계별 행과 Action/Expected를 제공; ReqIF archive에도 Test Case 포함. XLSX 우선, 재사용 단계·첨부는 별도 해석 | 상용; 파일 W/L/M=P. [Excel 단계](https://support.ptc.com/help/codebeamer/r2.2/en/codebeamer/user_guide/987893.html) · [ReqIF](https://support.ptc.com/help/codebeamer/r3.0/en/codebeamer/cbx_user_guide/cbx_req_if/cbx_ug_export_reqif_data.html) |
| TestRail / 지원 문서 갱신 2025-03-13 | XML/CSV/Excel. CSV·Excel은 separated steps를 별도 행으로 내보내기 가능. XML은 섹션/TC 구조 보존에 유리 | 상용; 파일 W/L/M=P. [공식 내보내기](https://support.testrail.com/hc/en-us/articles/15144643126932-Export-test-cases) |
| Xray / Server+DC 및 Cloud REST v2 문서 | Server/DC manual steps→CSV; Jira XML/Word에 step을 포함하려면 화면 필드 설정 필요. Cloud Cucumber export→feature. Cloud/DC를 같은 형식으로 가정하지 않는다 | 상용; 파일 W/L/M=P. [manual CSV](https://docs.getxray.app/space/XRAY/159711602/Steps%2B%28Manual%2BTests%29) · [Cloud feature export](https://docs.getxray.app/space/XRAYCLOUD/44577687/Exporting%2BCucumber%2BTests%2B-%2BREST%2Bv2) |
| ASAM ATX 1.0.1 / 2018-09-25 | XML schema·`.atxml`, 테스트 명세/계획/결과 등 교환. 샘플이 들어오면 전용 importer 검토; 일반 XLSX 대체로 즉시 채택할 근거 없음 | ASAM 저작물·이용 조건, OSS 라이선스 미확인; 형식 W/L/M=P. [공식 표준](https://www.asam.net/standards/detail/atx/) |
| ASAM XIL 3.0.0 / 2024-09-30 | 테스트 자동화 도구↔벤치 API와 설정/자극 자료(xml/sti/stz). **일반 TC 내보내기 표준이 아니다.** ATX와 구분 | ASAM 저작물·이용 조건, OSS 라이선스 미확인; 구현체별 OS 미확인. [공식 표준](https://www.asam.net/standards/detail/xil/) |

## A2. 보고서

JUnit XML은 하나의 보편적 정본 XSD가 아니라 생산자·소비자별 방언으로 다룬다. 이름이 같은 JUnit **실행 프레임워크**와도 구분한다.

| 계열 | 확인한 차이 / 호환 판단 | 버전·날짜·라이선스·플랫폼 / 출처 |
|---|---|---|
| Ant legacy XML | 단일 `testsuite`, 집계 `testsuites`, properties/testcase/failure/error/stdout/stderr. `junitlauncher`의 legacy-xml은 기존 junit 형식을 따른다 | 형식 공통 버전·릴리스일 없음; Ant Apache-2.0, XML 파일 W/L/M=P. [Ant XML 상수·DTD](https://ant.apache.org/manual-1.9.x/api/org/apache/tools/ant/taskdefs/optional/junit/XMLConstants.html) · [legacy formatter](https://ant.apache.org/manual/Tasks/junitlauncher.html) |
| Jenkins | JUnit 플러그인이 XML 결과를 수집한다. 플러그인 버전별 허용 확장·표시가 다르며 유일 표준 스키마로 간주하지 않는다 | 확인 제품 최신 버전·일자 미확인; JUnit plugin MIT, 서버 실행 환경별 확인 필요. [공식 플러그인](https://plugins.jenkins.io/junit) · [소스/라이선스](https://github.com/jenkinsci/junit-plugin) |
| Maven Surefire | 공식 `surefire-test-report.xsd`가 있다. 재실행/flake 관련 확장까지 모든 소비자가 읽는다고 가정하지 않는다 | 확인 문서 3.6.0 계열, 최신 버전·릴리스일 미확인; Apache-2.0, XML W/L/M=P. [공식 XSD](https://maven.apache.org/surefire/maven-surefire-plugin/xsd/surefire-test-report.xsd) |

- **공통 최소 출력 권고:** suite/case name·classname·time(초), 집계 counts, `failure`/`error`/`skipped`, 제한된 system-out. PASS는 실패 자식 없음, 기대 불일치는 failure, 실행/인프라 오류는 error. inconclusive/사용자 중단은 skipped+구체 사유로 내보내고 HTML/정본 JSON에 원래 판정을 보존한다(권고 매핑).
- GitLab은 JUnit의 **일부 요소만** 읽고 properties·일부 집계를 무시하며, XML 업로드 자체로 job을 실패시키지 않는다. Jenkins는 `junit` 수집 단계가 필요하다. GitHub Actions는 XML artifact 저장과 전용 결과 표시 action을 구분한다. 이 조사에서 세 CI에 실제 업로드한 결과는 **미확인**. [GitLab](https://docs.gitlab.com/ci/testing/unit_test_reports/) · [Jenkins](https://plugins.jenkins.io/junit) · [GitHub artifacts](https://docs.github.com/en/actions/concepts/workflows-and-actions/workflow-artifacts)

| Rust 후보 | 최신 버전 / 릴리스일 | 라이선스 | W/L/M | 비교 / 출처 |
|---|---|---|---|---|
| quick-junit | 0.8.0 / 2026-09-10 | Apache-2.0 OR MIT | P/P/P | nextest 사용; Jenkins 계열 직렬화·역직렬화, rerun/flake·추가 속성, XML 불가 문자 제거. [메타데이터](https://crates.io/api/v1/crates/quick-junit) · [소스](https://github.com/nextest-rs/quick-junit) |
| minijinja | 3.1.0 / 2026-11-07 | Apache-2.0 | P/P/P | 런타임 템플릿·Serde 입력·상세 오류, autoescape/strict undefined/fuel 설정 가능. 사용자 템플릿이 필요할 때 유리. [메타데이터](https://crates.io/api/v1/crates/minijinja) · [문서](https://docs.rs/minijinja/latest/minijinja/) |
| askama | 0.16.1 / 2026-09-04 | MIT OR Apache-2.0 | P/P/P | 빌드 시 Rust 코드 생성·타입 검사, HTML escape. 제품에 포함한 고정 보고서에 유리; 변경 시 재빌드. [메타데이터](https://crates.io/api/v1/crates/askama) · [문서](https://askama.rs/en/stable/) |

권고: quick-junit + **Askama 고정 HTML**로 시작한다. evidence URI/seq 구간·artifact hash는 HTML/정본 JSON에 보존하고 XML system-out에는 짧은 참조만 넣는다. MiniJinja는 런타임 보고서 편집 요구가 생기면 선택한다. 라이브러리 존재를 CI 방언 호환 실증으로 간주하지 않는다.

## A3. 증거 술어: 성숙도·비용·오류

아래 비용·선호는 소스 검토와 제한된 실측에 근거한 판단이다. 파싱은 1회, 이벤트/필드/표현식 크기는 제한하는 전제다. 라이브러리의 boolean 평가만으로 seq 존재·보존 범위·타임스탬프·gap 검증까지 해결되지는 않는다.

| 후보 | 최신 버전 / 릴리스일 · 라이선스 · W/L/M | 성숙도·평가 비용·오류 품질 / 출처 |
|---|---|---|
| cel-interpreter | 0.10.0 / 2025-07-23 · MIT · P/P/P | 기존 이름의 마지막 배포. 같은 프로젝트의 후속 **cel**을 비교해야 하며 오래된 버전만 보고 프로젝트가 중단됐다고 판단하면 안 된다. [메타데이터](https://crates.io/api/v1/crates/cel-interpreter) · [후속 changelog](https://github.com/cel-rust/cel-rust/blob/master/cel/CHANGELOG.md) |
| cel | 0.15.0 / 2026-10-06 · MIT · P/P/P | 활동 중인 pre-1.0, breaking API 변경 존재. C 계열 술어가 읽기 쉽다. 중첩 comprehension/변환은 비용 증가; 이 스파이크는 이벤트별 JSON 변환까지 포함. 문법 오류에 행·열·caret, 누락 필드 오류는 있으나 seq/path는 Loom이 덧붙여야 한다. [메타데이터](https://crates.io/api/v1/crates/cel) · [공식 소스](https://github.com/cel-rust/cel-rust) |
| serde_json_path | 0.7.2 / 2025-02-03 · MIT · P/P/P | RFC 9535 지향, nodelist·위치 반환. 필드 선택과 제한된 filter에 적합. 광범위 descendant/wildcard는 방문·결과 수 증가. 문법 위치 오류는 유용하나 누락/다른 타입은 filter 비일치로 조용히 제외될 수 있다. [메타데이터](https://crates.io/api/v1/crates/serde_json_path) · [API](https://docs.rs/serde_json_path/latest/serde_json_path/) |
| jaq / jaq-core | 3.1.2 / 2026-10-21 · MIT · P/P/P | jq 호환 처리·변환과 다중 결과에 강점. core+std+json을 조합; 임의 재귀는 시간·메모리·스택 무제한 가능하다고 공급자가 명시. parse/compile/runtime 오류 객체가 있으며 UI 진단 포맷은 별도 통합. 실제 오류 메시지·성능 시험은 미확인. [jaq 메타데이터](https://crates.io/api/v1/crates/jaq) · [core](https://crates.io/api/v1/crates/jaq-core) · [보안·자원 한계](https://github.com/01mf02/jaq#security) |
| jaq-std / jaq-json | 3.0.4 / 2026-10-21; 2.0.3 / 2026-08-28 · 각각 MIT · P/P/P | core와 버전 숫자가 같지 않다. 필요한 함수만 등록 가능하나 단순 증거 비교에는 기능이 과하다(판단). [std](https://crates.io/api/v1/crates/jaq-std) · [json](https://crates.io/api/v1/crates/jaq-json) |
| 자체 단순 비교 AST/DSL | 미구현·버전/릴리스/라이선스 미정(미확인) · W/L/M=P(설계) | eq/ne/lt/le/between/contains + any/all/hold만 허용하면 스칼라 비교는 이벤트 수에 선형, hold 누적 상태는 상수 크기로 제한 가능. seq/path/expected/actual 오류를 직접 통제하지만 의미 정의·유지 비용은 Loom 책임. [로컬 검증 연산자](../../../spikes/explore/v2-evidence/src/main.rs) |

### Rust 스파이크 결과

- 위치: [v2-evidence](../../../spikes/explore/v2-evidence/README.md). [JSONL 50행](../../../spikes/explore/v2-evidence/events.jsonl)은 UART 텍스트·CAN VehicleSpeed·UDS 응답과 gap/epoch/타입 오류를 포함한다. cel 0.15.0과 serde_json_path 0.7.2를 정확히 고정했다.
- 두 엔진 모두 **10개 판정 사례씩 통과**: exists 양성/음성/포함 경계, hold 양성/길이 부족/중간 범위 위반, gap, epoch 변경, 샘플 간격 초과, 잘못된 타입. 오류 진단도 별도로 수집했다. [전체 결과](../../../spikes/explore/v2-evidence/run.log)

| 동일 질의 | 결과(두 엔진 동일) |
|---|---|
| seq [10,15] 안 UDS positive response `[0x62,0xF1,…]` 존재 | PASS, witness seq=12 |
| seq [13,18]의 같은 술어 / [12,12] 경계 | FAIL / PASS(seq=12) |
| seq [1,24], speed ∈ [10,15], hold 300 ms | PASS, 증거 seq [4,13], 관측 시간 300 ms |
| 같은 구간 hold 800 ms / 범위 밖 샘플을 가로지르는 hold | FAIL / FAIL |
| seq [31,42] hold 300 ms(gap·새 epoch) | inconclusive, 공백을 이어 PASS로 만들지 않음 |
| seq=43의 문자열 신호 값 | 오류: `seq=43 path=/payload/value expected finite number` |

| 측정(최초 실행, release) | 결과 |
|---|---|
| 의존성 다운로드 포함 첫 빌드 / no-op 재빌드 | 24.828 s / 0.103 s |
| 실행 프로세스 전체 / 프로그램 내부 | 0.375 s / 96.935 ms |
| 네 술어 최초 compile 합계 | 1.853 ms |
| exists+hold 한 쌍 × 1,000회, CEL / JSONPath | 80.718 ms / 13.476 ms (평균 한 쌍 80.7 / 13.5 µs) |

- 단일 소형 fixture·단일 측정이다. CEL Context/JSON 변환·JSONPath singleton 배열 복제·결과 생성이 포함되어 **언어 일반 성능 순위로 확대하면 안 된다**. [빌드/실행 계측](../../../spikes/explore/v2-facts/spike-results.json)
- CEL 문법 오류는 `<input>:1:19`와 caret, 누락 값은 `No such key: value`; JSONPath 문법 오류는 position 20, 누락 값은 오류 없이 false였다. 따라서 JSONPath를 그대로 “유효한 증거 없음”과 “필드 잘못됨” 구분에 쓰면 안 된다. [실측 진단](../../../spikes/explore/v2-evidence/run.log)
- hold는 **연속 관측 샘플** 기준이다. 동일 source/signal/epoch, 단조 시각, 샘플 간격 ≤150 ms가 필요하며 gap에서 누적을 끊는다. 샘플 사이 물리적 연속 상태를 증명한 것은 아니다. 전체 저널 보존·record gap·시간 동기 품질 검증은 제품 구현 과제다.
- 권고: M1 판정 정본은 **형식화된 작은 비교 AST + Rust 시간/coverage 연산자**. CEL은 고급 술어 후보로 유지, JSONPath는 필드/이벤트 선택 보조, jaq는 선택적 오프라인 변환용. 식 길이·깊이·대상 이벤트·결과 수와 실행 예산을 제한한다(C2). 임의 동기 평가를 async timeout만으로 중단할 수 있다고 가정하지 않는다.

## A4. 실행 기록을 고정하는 선례

| 선례 | 확인 버전 / 릴리스일 / 라이선스 / 플랫폼 | Loom에 적용할 점 |
|---|---|---|
| Robot Framework keyword-driven test | 7.5.1 / 2026-11-02 / Apache-2.0 / W·L·M 경로, 이 조사 실행 미확인. [배포 메타데이터](https://pypi.org/pypi/robotframework/json) | 순서가 고정된 keyword+argument+assertion을 명세로 저장. TC마다 프로그램을 작성하지 않는 입력 모델의 선례이며 Python 런타임 채택 제안은 아니다. [공식 가이드](https://robotframework.org/robotframework/latest/RobotFrameworkUserGuide#test-case-syntax) |
| Playwright Codegen | 참고 Python 배포판 1.65.0 / 2026-11-09 / Apache-2.0 / W·L·M 경로, 이 조사 실행 미확인. [메타데이터](https://pypi.org/pypi/playwright/json) | 녹화한 상호작용을 편집 가능한 테스트 동작·assertion으로 생성하는 선례. Trace 열람과 재실행 가능한 테스트 생성은 구분한다. [공식 Codegen](https://playwright.dev/docs/codegen) |

권고: 에이전트의 확정된 RPC 동사·정규화 인자·변수 바인딩·기대 술어·timeout을 `loom run` 시나리오로 저장한다. 과거 seq/승인 토큰/시각을 복사하지 말고 새 세션의 단계 구간으로 다시 바인딩한다. 이는 **명령·판정 절차의 고정**이며 실제 DUT 응답의 bit-exact 재현 보장이 아니다. C3의 승인·감사·STOP 규칙은 새 실행에서도 적용한다.

## B1. Saleae: 공개 proto → Rust

| 구성 | 버전 / 날짜 | 라이선스 | W/L/M | 출처 |
|---|---|---|---|---|
| 공개 proto | 헤더 API 1.0.0; master 확인 commit `cb33973429eed40626cb86f331f4e58a0bf2e379` / 2024-01-31 | Apache-2.0 | P/P/P | 실제 경로 **`proto/saleae/grpc/saleae.proto`**. [고정 원본](https://github.com/saleae/logic2-automation/blob/cb33973429eed40626cb86f331f4e58a0bf2e379/proto/saleae/grpc/saleae.proto) · [LICENSE](https://github.com/saleae/logic2-automation/blob/cb33973429eed40626cb86f331f4e58a0bf2e379/LICENSE) |
| tonic | 0.14.9 / 2026-10-29 | MIT | P/P/P | [메타데이터](https://crates.io/api/v1/crates/tonic) |
| prost | 0.14.4 / 2026-06-07 | Apache-2.0 | P/P/P | [메타데이터](https://crates.io/api/v1/crates/prost) |
| tonic-prost / tonic-prost-build | 각각 0.14.9 / 2026-10-29 | 각각 MIT | P/P/P | 0.14 계열의 protobuf codec·생성기. [codec](https://crates.io/api/v1/crates/tonic-prost) · [생성기](https://crates.io/api/v1/crates/tonic-prost-build) |
| Logic 2 자동화 서버 | Python 배포판 1.0.11 / 2025-11-18; 앱·별도 headless 프리뷰와 버전 구분 | Python wrapper Apache-2.0; 앱은 별도 독점 배포 | ○/○/○(앱 조건) | [기존 사실 확인](../explore/v12-saleae-check.md) · [공식 gRPC 안내](https://docs.saleae.com/automation/guides/getting-started) |

- [v2-saleae-rs](../../../spikes/explore/v2-saleae-rs/README.md)에서 `tonic_prost_build`로 ManagerClient와 메시지를 생성하고 release 빌드 성공. GetAppInfo 호출 코드도 타입 검사했다. protoc는 기존 설치본 29.6을 사용했으며 전역 설치하지 않았다.
- **최초 빌드 35.875 s, no-op 빌드 0.174 s, 실행 전체 0.264 s**. `localhost:10430` 연결은 내부 1.853 ms에 Connection refused(os error 61), 오류 체인을 처리하고 **exit 0·panic 없음**. 연결 2 s/외부 3 s deadline을 두었다. [로그](../../../spikes/explore/v2-saleae-rs/run.log) · [계측](../../../spikes/explore/v2-facts/spike-results.json)
- proto SHA-256 `13ed873e9d4eb6eac222ef4e532a39f4c330da9ea45c4e998a1805650a15311e`; 원본·라이선스·커밋을 [upstream.json](../../../spikes/explore/v2-saleae-rs/upstream.json)에 보존했다. 생성물은 target 아래, Cargo.lock으로 의존성을 고정했다.
- **Python 어댑터 불필요가 현재 결론**: gRPC 호출과 앱 `--automation` 실행/감독은 Rust에서 구현 가능하다. Python Manager 편의 API의 버전 검사·capture handle 수명·에러 변환을 Rust에 옮겨야 한다. GetAppInfo→장치 열거→F4241 캡처·export의 실제 왕복, 최신 앱/네이티브 headless 프리뷰 proto 호환, W/L 빌드는 **미확인**이다.
- 이번 빌드는 1.0 계열 공개 proto의 컴파일 가능성만 입증했다. raw 스트리밍·GUI 실시간 파형·동시 클라이언트 수에 대한 기존 미확인을 해소하지 않는다. [공식 연결/버전 안내](https://docs.saleae.com/automation/guides/getting-started)

## B2. DLT

`dlt-core` **0.20.2 / 2026-03-17 / Apache-2.0 / W·L·M=P**(순수 Rust codec, 이번 빌드 미실시). `dlt`라는 다른 crate는 카메라 보정 알고리즘이므로 혼동 금지. [메타데이터](https://crates.io/api/v1/crates/dlt-core) · [공식 저장소](https://github.com/esrlabs/dlt-core) · [이름 충돌 예](https://docs.rs/dlt/latest/dlt/)

- 파싱/직렬화, verbose/non-verbose, FIBEX 선택 기능, storage header 유무 구분과 stream 기능이 있다. APID/ECUID/CTID 집합 + 심각도 필터가 있으나 APID별로 다른 레벨을 결합하는 복잡한 OR 필터는 제공 설명상 제한된다. [필터 소스](https://github.com/esrlabs/dlt-core/blob/b2a8e90f996fccaaeefae9b8ecd1d4d1f70f288b/src/filtering.rs)
- **set-log-level 0x01 / get-log-info 0x03 완성형 클라이언트는 확인되지 않음**. `service_id_lookup`에 이름이 있는 것과 요청 구성·응답 payload 해석·상관관계 처리는 다르다. 현재 제어 payload는 1바이트를 ControlType로 읽는 형태여서 AUTOSAR 서비스별 codec을 그대로 대체한다고 볼 수 없다. [service 표](https://github.com/esrlabs/dlt-core/blob/b2a8e90f996fccaaeefae9b8ecd1d4d1f70f288b/src/service_id.rs) · [parser](https://github.com/esrlabs/dlt-core/blob/b2a8e90f996fccaaeefae9b8ecd1d4d1f70f288b/src/parse.rs) · [COVESA 프로토콜](https://github.com/COVESA/dlt-daemon/blob/master/include/dlt/dlt_protocol.h)
- 권고: verbose/FIBEX 읽기 재사용 후보로 두되, TCP 프레이밍·0x01/0x03 요청/응답은 작은 자체 Rust codec과 비교한다. 자체 codec은 현재 미구현·버전/라이선스 미정, W/L/M=P(설계). COVESA dlt-daemon 3.0.1 기존 fixture로 endian·분할·조각화·정확한 제어 바이트·ACK/반영 분리를 검증한 뒤 결정한다. 이번에는 **소스 검토만** 했다. [기존 DLT 관문](../explore/plan.md#52-dlt--loom-dlt)

## B3. SSH

| 후보 | 최신 버전 / 릴리스일 / 라이선스 | W/L/M | PTY·같은 연결의 여러 exec / 서버 기능 |
|---|---|---|---|
| russh | 0.65.1 / 2026-11-03 / Apache-2.0 | ○/○/○ 경로, 이번 실행 미확인 | async client Handle에서 여러 channel, request_pty·shell·exec; **서버 있음**. crypto backend 선택 필요. [메타데이터](https://crates.io/api/v1/crates/russh) · [Channel API](https://docs.rs/russh/latest/russh/struct.Channel.html) · [저장소·서버 예제](https://github.com/warp-tech/russh) |
| ssh2(libssh2) | 0.9.7 / 2026-11-05 / MIT OR Apache-2.0 | ○/○/○ 경로, C 빌드·링크 조건 | 여러 channel_session·request_pty·exec 가능. **client 전용**, sim 서버는 별도. blocking read가 같은 Session의 다른 채널 호출도 막으므로 nonblocking 중재 필요. [메타데이터](https://crates.io/api/v1/crates/ssh2) · [Session 동기화 설명](https://github.com/rust-lang/ssh2-rs/blob/master/src/session.rs) · [Channel](https://docs.rs/ssh2/latest/ssh2/struct.Channel.html) |
| openssh | 0.11.6 / 2025-12-03 / MIT OR Apache-2.0 | ×/○/○, 외부 OpenSSH 필요 | ControlMaster로 여러 remote command 가능. 현 공개 API의 PTY 요청은 미확인; 소스에 `cfg(not(unix)) compile_error!`. **서버 없음**. [메타데이터](https://crates.io/api/v1/crates/openssh) · [공식 문서](https://docs.rs/openssh/latest/openssh/) |

권고: **russh 하나로 실 client와 loopback sim 서버**를 구성한다. PTY+2 exec의 동시 stdout/stderr/exit, 채널별 상한·flow control, host key pinning을 후속 관문으로 둔다. 라이브 API 경로 존재는 확인했지만 이번에 SSH 서버를 실행하지는 않았다. [client 연결/채널 설명](https://docs.rs/russh/latest/russh/client/index.html)

## B4. Debug 프로브 / TRACE32

`probe-rs` **0.32.0 / 2026-07-22 / MIT OR Apache-2.0 / W·L·M=△**(USB 드라이버·udev·타깃 조건). [메타데이터](https://crates.io/api/v1/crates/probe-rs) · [공식 프로브별 OS 설정](https://probe.rs/docs/getting-started/probe-setup/)

- CMSIS-DAP·ST-Link V2/V3·J-Link 지원 경로가 있다. Windows J-Link는 공식 SEGGER 드라이버 대신 WinUSB가 필요하다는 공식 안내가 있어 기존 도구 공존을 실측해야 한다. [프로브 설정](https://probe.rs/docs/getting-started/probe-setup/)
- RTT는 crate 내부 `rtt` 모듈에 attach·up-channel read·down-channel write가 있다. 대상 RTT control block과 실행 중 메모리 접근 가능성은 MCU/펌웨어별 확인 대상이다. [0.32 RTT 소스](https://github.com/probe-rs/probe-rs/tree/v0.32.0/probe-rs/src/rtt)
- 일반 `Probe::attach(target, Permissions::new())`와 reset을 거는 `attach_under_reset`은 별개다. 후자를 자동 fallback하면 안 된다. **일반 attach도 타깃별 init/unlock 시퀀스를 호출하므로 전 MCU에 무정지·무변경을 보장할 수 없다.** [Probe API 소스](https://github.com/probe-rs/probe-rs/blob/v0.32.0/probe-rs/src/probe.rs) · [Session attach 흐름](https://github.com/probe-rs/probe-rs/blob/v0.32.0/probe-rs/src/session.rs)
- `Permissions::new()`는 erase_all=false이며 `allow_erase_all()`을 부르지 않아야 한다. nRF52의 잠금 해제 erase는 권한 확인 뒤 실행된다. 다만 이 설정은 범용 “auto_unlock=false” 스위치가 아니고 모든 벤더 init의 부작용까지 막는 권한도 아니다. [권한·attach 소스](https://github.com/probe-rs/probe-rs/blob/v0.32.0/probe-rs/src/session.rs) · [nRF52 구현](https://github.com/probe-rs/probe-rs/blob/v0.32.0/probe-rs/src/vendor/nordicsemi/sequences/nrf52.rs)
- **pyOCD 대체 가능 범위:** 지원 MCU의 attach/read/write/watch/RTT에 Rust 경로가 있으므로 기본 후보로 적합하다. 전체 target pack·벤더 옵션 동등성은 미확인. MCU/프로브 허용 목록의 attach/disconnect 무 halt/reset/erase 실측을 통과해야 기존 R0 attach 계약을 확정할 수 있다. `probe-rs run`은 flash/reset을 포함하므로 attach 대신 사용하지 않는다. [공식 README](https://github.com/probe-rs/probe-rs)

TRACE32 RCL은 **C Remote API FFI**로 Rust에서 사용 가능하다는 판단이다. 공식 Rust binding은 **미확인**이며 Python을 필수로 거칠 이유는 없다.

- 확인 자료: **Remote API 설명서 Release 02.2026, 문서 revision 2026-04-03**; 독립 라이브러리 최신 버전·배포일은 미확인. 라이선스는 **Lauterbach 자체 조건**이며 MIT로 쓰면 안 된다. W/L은 C 소스와 prebuilt 라이브러리 경로, macOS C 라이브러리 빌드·지원은 **미확인**. [공식 C API 설명서 pp.7–17](https://www2.lauterbach.com/pdf/api_remote_c.pdf)
- `~~/demo/api/capi/src/t32.h`, `hremote.c`, UDP `hlinknet.c` 또는 TCP `tcpsimple2.*`/`t32nettcp.*`; Rust `extern "C"` 바인딩 + cc/link가 현실적이다. TCP는 09.2020 이후, 대상은 프로브 직접이 아니라 **실행 중인 PowerView**이며 유효 TRACE32 라이선스가 필요하다. 호출은 thread-safe가 아니므로 전용 worker/직렬화가 필요하다. [같은 공식 설명서](https://www2.lauterbach.com/pdf/api_remote_c.pdf)
- C 소스 이용·언어 이식은 허용하지만 수정본 배포·APDU 변경·서버 구현 등에 제약이 있다. 그러므로 프로토콜 자체 재구현보다 원본 C API FFI를 우선 검토하고 원문 조건을 함께 보존한다. Rust 통합 빌드·장치 왕복은 **미확인**. [공식 이용 조건 p.7](https://www2.lauterbach.com/pdf/api_remote_c.pdf)

## B5. SCPI / VISA (장비 구매 시)

| 경로 | 버전 / 최근 릴리스 / 라이선스 | W/L/M | 비교·출처 |
|---|---|---|---|
| raw TCP SCPI | Tokio 1.54.1 / 2026-10-21 / MIT; 장비 명령 방언 버전은 기종별 미확인 | P/P/P | TcpStream+deadline+상한 있는 응답 framing. VISA 설치 불필요. [Tokio 메타데이터](https://crates.io/api/v1/crates/tokio) · [TcpStream](https://docs.rs/tokio/latest/tokio/net/struct.TcpStream.html) |
| visa-rs | 0.7.1 / 2026-09-22 / MIT OR Apache-2.0 | △/△/△ | 설치된 VISA C 라이브러리에 링크. README는 W/L/M 기본 링크 경로 제공; `LIB_VISA_NAME/PATH` 지정 가능. NI-VISA는 예시이며 그것만 가능한 것은 아니다. 외부 런타임 버전·라이선스·CPU/OS 조합은 별도 확인. [메타데이터](https://crates.io/api/v1/crates/visa-rs) · [공식 requirements](https://github.com/TsuITOAR/visa-rs) |

권고: socket SCPI 지원 기종이면 **Rust raw TCP 우선**, VXI-11/HiSLIP/USBTMC/GPIB·SRQ/Device Clear가 필요하면 VISA를 검토한다. `TCPIP::…::SOCKET`과 `…::INSTR`은 같은 전송이 아니다. 5025는 흔한 기본값이지만 모든 기종의 포트로 고정하지 않는다. [Keysight 전송 비교](https://helpfiles.keysight.com/IO_Libraries_Suite/English/IOLS_Windows/Connection_Expert_New/Content/Connection_Expert/HTML/Instruments/ManageLANInstrument.htm)
텍스트 종료자와 길이 기반 binary block을 구분하고 응답 바이트 상한·한 장치당 미완료 query 1개를 둔다(설계). visa-rs의 macOS NI-VISA 23.5 async I/O가 timeout까지 block할 수 있다는 공급자 주의도 있어 worker 격리가 유효하다. 이번 SCPI/VISA 빌드·장치 실행은 **미확인**. [공식 async 설명](https://github.com/TsuITOAR/visa-rs/blob/main/ASYNC.md)

## B6. 릴레이 / IO

| 후보 | 최신 버전 / 릴리스일 / 라이선스 | W/L/M | 확인·한계 / 출처 |
|---|---|---|---|
| hidapi | 2.6.7 / 2026-08-27 / MIT(crate) | △/△/△ | HID enumeration/read/write/feature report 경로. Windows native·Linux hidraw/libusb·macOS backend가 있다. 보드별 VID/PID/report 규약·실접점 피드백은 제공하지 않는다. [메타데이터](https://crates.io/api/v1/crates/hidapi) · [backend 문서](https://docs.rs/hidapi/latest/hidapi/) |
| tokio-modbus | 0.17.0 / 2025-10-22 / MIT OR Apache-2.0 | P/P/P(TCP), △/△/△(RTU) | 순수 Rust async/sync TCP·RTU client, 선택 server 기능으로 sim 가능. RTU는 COM/tty 드라이버·권한 조건. [메타데이터](https://crates.io/api/v1/crates/tokio-modbus) · [공식 기능](https://docs.rs/tokio-modbus/0.17.0/tokio_modbus/) |

권고: HID 모델은 hidapi report 어댑터, Modbus 보드는 tokio-modbus를 쓴다. 상태 되읽기와 실제 접점 피드백을 구분한다. 두 후보 모두 이번 빌드·실기 시험은 **미확인**, Python 필요 근거 없음.

## B7. LA sigrok 2순위

sigrok-cli 공식 stable **0.7.2 / 최근 릴리스일 미확인 / GPL-3.0-or-later / W·L·M=○ 경로**(배포판/USB 드라이버 조건, macOS 공식 nightly는 x86). Rust `std::process::Command`의 argv·stdout/stderr pipe로 기존 하위 프로세스 경로를 유지할 수 있다. `-O binary`, `--continuous`, `--time`은 언어와 무관한 CLI 계약이다. [공식 CLI 문서](https://sigrok.org/wiki/Sigrok-cli) · [배포·플랫폼](https://sigrok.org/wiki/Downloads)
Python 어댑터는 불필요하다. libsigrokdecode 내부 Python 런타임 의존과 Loom의 Python worker는 구분한다. 제한된 버퍼·stderr 분리·취소 후 프로세스 회수는 Rust worker 책임이며, 장치 지원은 [기존 모델별 확인](../explore/v12-saleae-check.md)을 유지한다. 이번 설치·실행은 하지 않았다.

## 재현성과 검증 범위

- 모든 Cargo 빌드·포맷 호출의 `CARGO_HOME`은 `spikes/explore/.cargo-home`, `CARGO_TARGET_DIR`은 각 스파이크의 `target`으로 지정했다. `~/.cargo` 설정/registry를 사용·변경하지 않았고 `cargo install`·전역 설치를 하지 않았다.
- Cargo.lock·원본 proto·라이선스·첫 빌드/실행 로그를 보존했다. 포맷 적용 뒤 `cargo fmt --check`, 두 프로젝트의 `cargo build --release --locked`, 두 실행 재확인도 성공했다. [검증 기록](../../../spikes/explore/v2-facts/verification.json)
- metadata 수집/빌드 계측 보조 스크립트만 Python이며 **실제 판정·Saleae 클라이언트는 Rust**다. HW 왕복, CI 업로드, soak/RSS, 악의적 식 자원 고갈, W/L 빌드는 미확인이다. 스파이크의 성공을 이 항목들의 통과로 확대하지 않는다.

## 권고 요약

| 대상 | 권고 | 남은 결정·관문 |
|---|---|---|
| TC 가져오기 | calamine + csv, Gherkin은 선택 입력 | 실사내 TC 열 매핑·병합/재사용 단계·첨부 보존 |
| 보고서 | quick-junit + Askama 고정 HTML | Jenkins/GitLab/GitHub 결과 소비 실증·inconclusive 매핑 |
| 증거 판정 | 작은 비교 AST + Rust seq/coverage/hold; CEL 고급 후보 | 예산·타입·누락·단위·gap 의미를 계약으로 고정 |
| 기록→시나리오 | 동사·인자·기대 술어를 고정, 새 세션에 재바인딩 | 승인/STOP 재집행·DUT 응답은 비결정적 |
| Saleae | **tonic/prost Rust worker, Python 예외 불필요** | 앱 GetAppInfo/F4241→capture/export, 버전/OS 호환 |
| DLT | dlt-core 읽기 비교 + 자체 Rust 제어 codec | COVESA 0x01/0x03 실제 바이트·ACK·효과 검증 |
| SSH | russh client + sim server | PTY+다중 exec 동시성·윈도우·상한·host key |
| Debug | probe-rs 조건부 기본, TRACE32는 C API FFI | 무 halt/reset/erase gate·벤더 시퀀스·RCL 조건 |
| PSU/스코프 | raw TCP SCPI 우선, VISA 선택 | 구매 기종 방언·전송·binary 응답 실측 |
| IO / sigrok | hidapi·tokio-modbus / CLI 하위 프로세스 유지 | 보드 report·되읽기·USB 권한·장치 호환 |
