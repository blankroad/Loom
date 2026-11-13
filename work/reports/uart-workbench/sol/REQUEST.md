# UART 사용자 작업대: 벤치마킹 + 동작하는 독립 목업 1안
사용자 최신 요청: 전체 서비스 동시작업 중단, UART 하나부터. MobaXterm처럼 자유로운 창 분할/크기조절과 매크로를 자유롭게 넣어 사용. 사람의 일상 사용성이 중심, 제품 agent는 뒤로. Sol과 Opus가 독립적으로 다른 도구를 벤치마킹하고 목업 제작. 각 모델 **완성도 있는1안씩**, 총2안. 메인은 오케스트레이션만. feature-team-coding 적용, 기존5모듈 구현/검증계획 work/plans/simulator-ui/ 유지.

범위/권한: 이번은 실제로 조작 가능한 HTML 목업과 조사 근거/검증까지. 제품 ui/src/backend/기존목업/계획/STATUS/패키지/설정/깃 수정 금지. CAN/ETH/UDS/PSU 화면 만들지 말 것. native foreground Herdr, 내부 subagent/background process/모델대체 금지. 서버/브라우저는 별도 소유자만, 임의 중복 생성 금지. main a7d1be6 + 무관 dirty docs8개 보존. 인수 후 main이 정상종료/owned workspace close. 모델ID/세션/출처 기록. CLI권한 문제를 전역설정/우회로 바꾸지 말고 좁은 문제/대안 보고.

목업 품질 기준: UART 전문가가 쓰는 실제 작업대. MobaXterm을 주 기준으로 관련 터미널2~3개(예: Windows Terminal/Terminator/WezTerm/Tabby/SecureCRT 등) 공식 문서/실제 이미지에서 조사. 목록 나열/색 복제 금지. 관찰/추론과 채택·기각 이유를 짧게 기록, exactURL과 가능하면 공식 screenshot을 직접 확인. 중요한 요소는 세션 탐색/세션별 설정·연결상태/터미널 탭과 멀티pane/매크로 실행 흐름. 최신/특정제품 사실은 공식 web조회로 확인. 다른 모델 creative파일은 자기안 동결전 읽지 않음.

같은 시나리오: UART mcu(1152008N1), aux(96008N1), console/debug 세션들을 저장 목록으로, 최초 mcu+aux 2개 실제 pane. 충분한 원문 부트/텔레메트리 로그/상태/오류; compact 연결·baud·TXRX·대상 표시. 좌측 세션/매크로는 사용자가 접고 펼침. 큰 KPI/마케팅카드/Agent채팅/항상열린승인영역 금지. 프로토타입/실장비없음 표시는 얌전하게1곳. model별 outline명과layoutdifferent하지만필수사용성이 핵심.

**실제 동작 필수**
1. 활성 pane 선택 후 가로/세로 **중첩 분할** 자유롭게, 최소3pane이상; 경계선을 **실제 pointer drag**로 크기조절, 최소크기, focus/maximize/restore/close, 마지막pane닫기처리. 가능하면 탭을pane사이 native drag/drop 이동(범위초과시 명시한키보드move대안+드래그실제지원), session alias대상 혼동 금지. 미리정한2열/4분할 버튼만이면 미달.
2. 저장된 UART세션 열기(현재pane/tab 또는새분할), 세션별 설정 보기/변경(로컬sim), 세션 독립, 직접 입력/송신과localecho·연결끊김/재연결 상태. TEXT/HEX 전환(최소 유용한 raw보기)·검색/로그일시정지·clear(화면만) 중 유용한 조합. Terminal영역 충분히 넓고읽기쉽게, desktop1440핵심,768반응형,390기본조작도달가능(세로스크롤허용).
3. **사용자 매크로 CRUD**: 생성/수정/복제/삭제, 이름/명령·여러단계(최소 send+delay 또는 wait)/실행대상·간단변수(권장)·단축키(권장). JSON import/export 또는localStorage로 사용자설정지속. 실행 전 현재대상확인, step진행/결과, **중지하면 후속step송신없음**; 런타임도 로컬모의. 코드eval/실shell/실장비접속없음. 기본 Login/Read status macro2개; 사용자가 직접추가해실행가능. 매크로가agent프롬프트박스처럼보이면미달.
4. controls는 의미있는기능만. browserQA는 스크린샷만아님: 새세션→중첩분할→경계drag기하변화→탭이동→직접입력→매크로생성/수정/실행/중지/재로드보존을 실제조작. 사용자데이터를지우는전체localStorageclear금지, 자기prefix키만쓴다.

구현방식: 코드native HTML/CSS/JS, 외부 CDN/폰트/대형dependency설치없이 로컬프리뷰 가능. 필요시 프로젝트기존로컬자산사용,사용권/출처유지. 서버 main의 http://127.0.0.1:4180/ (work/reports root). fallback 필요하면 main에게 좁게알릴것. 브라우저 Opus가 독점실행; Sol은DOM/state자체검사후freeze, Opus가2안공동EgoQA. source동결후 cosmetic변경하지말것; 실제QA결함만좁게수정/영향검사. html로 풍부한 benchmark설명을만들기보다 **목업 자체**우선.

반환 파일(자기폴더만): index.html + styles.css/app.js(선택), SURVEY.md(주장근거와한안구조/차별점,간결), HANDOFF.md(실제선택자/상호작용/knownlimits/실행명령/모델증거/소유파일/상태). 소유QA가아니면브라우저검증완료주장금지. 체크포인트3분내, 초기목업목표10분. 불필요한기능·광범위조사·수십개새테스트금지. interaction위험있는split/macro state만적절히자체검사. 토큰숫자를비용으로환산금지.

당신은Sol 담당. 모델 gpt-6.1-sol native Codex foreground. 소유 work/reports/uart-workbench/sol/만. 자기안 완성후 root index.html 비교진입페이지 추가소유(짧고중립적인2안링크/설계차이, 제품shell밖에큰보고서chrome추가금지). 이때만Opus HANDOFF/설계읽기허용. 브라우저기동금지. exactcurrent selectors와drag splitter bounding/macro workflow를HANDOFF에남겨Opus검증혼선줄일것. 자체freeze후main에반환.
