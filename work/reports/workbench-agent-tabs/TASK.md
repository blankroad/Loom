# 통합 보기 / 에이전트 작업 두 탭 — 제안 전용

2026-11-14 사용자 추가 요청. 각 모델 전용 Herdr workspace의 native foreground로 독립 제안한다. ROOT가 기존 logic 기능의 최종 QA를 병행 중이다. 제품 코드·Git index·Cargo·장치/사용자세션 조작 금지. 보고서/목업만 각자 소유경로에 쓴다. 추가 하위 에이전트 금지. 기준 main d382628 + 현재 logic 미커밋 작업(보존).

요구:
1. **통합 보기** 탭: 사용자가 서비스·세션·인터페이스를 선택해 UART+CAN, CAN+UDS, 세 개 이상 등 원하는 조합으로 함께 관찰. split/resize, 기존 서비스 전용뷰 관계, 사용 중인 서비스만 노출, 복수UART/ECU/NIC 구분, 선택 저장·재접속·STOP 후 기록이 중요.
2. **에이전트 작업** 탭: 사용자가 요청해 실행하는 작업만 task 단위로 목록·상세. 무엇을 하는지/어느 단계인지/실제상태/진행근거/사용자介入·취소·결과가 보임. 원시LLM토큰/내부추론 전시가 아니라 명시된계획·툴실행·산출물/로그 연결. 백그라운드 작업은 탭전환과독립적으로지속. 하드웨어STOP와task취소를구분.

현재 실제 native 구조 최소 읽기: ui/src/shell/{ShellApp.tsx,ServiceNav.tsx,routes.ts,dock/*}, ui/src/store/{activity.ts?,channels.ts,shellControl.ts}; 정확파일명은rg --files로찾기. 현Bench Light work/mockups의최신공용shell도 필요한경우참고하되 backend연결된native UI와staticmock 차이를명시. 기존 core.activity는RPC활동요약이며 task정식모델과동일하다고가정하지말것.

각자 결과:
- OPUS.md / SOL.md: 추천안 하나 + 짧은대안, 비교기준, 실제정보구조/화면ASCII또는Mermaid, 사용흐름 예시2개, task단계상태표, 필요한최소backend/UI계약, 재사용vs추가구현, 검증·구현순서. 공식제품/문서2~3개 웹에서참고해링크와구체적적용점기재. 권한/세션없는사내자료추측금지.
- OPUS.html / SOL.html: 두탭을전환하고서비스선택 조합 및 작업목록→상세를직접확인하는작은자급형HTML(외부의존없음). 전체제품구현/공통컴포넌트프레임워크금지. demo 데이터는명확히표시. 벤치라이트+정보량높은유저중심설계. 에이전트탭을LLM채팅창으로대체하지말것.

소유: Opus는OPUS.*; Sol은SOL.*. 다른모델산출물읽지않고독립안, ROOT가나중비교통합. 분량~1500단어이내, 목업~한파일, 조사/작성목표5~7분; 불필요한장문코드탐색/패키지설치없음. 제공시간/토큰한도없는사용자요구를임의완료로축소하지말고이범위의구체안까지완성. 완료시STATUS-OPUS/SOL.md 짧게반환 후idle. 종료·workspace닫기는ROOT.
