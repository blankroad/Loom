# 실제 브라우저 반례: Ethernet 상세 RPC

Opus foreground/Ego 실제 화면에서 agent/device 패킷 목록은 보이지만 상세 바이트 트리가 없고 재전송/편집 송신이 비활성화됐다. UI는 mock의 `eth.query{ref,detail:true}`를 사용했지만 실제 계약은 `eth.show{ref}`다 (`loom-eth/src/gateway/handlers.rs::Show`, schema-v1.md).

UI 상세 호출과 manifest 허용 query를 `eth.show`로 고쳤다. 목록 쿼리 `eth.query`는 유지한다. mock 서버에는 canonical `eth.show`를 기존 상세 reader로 연결했다. 기존 상세/재전송/watch/비동기취소 테스트도 canonical RPC를 사용하고, 상세 선택이 목록 쿼리를 호출하지 않음을 검증한다.

전체 UI check387/schema/type/lint PASS, build PASS. 로그 `/private/tmp/loom-eth-show-ui-{check,build}.log`. 실제 재검증은 Opus 담당 진행 중(`p1-opus-browser.md`). 소유 ethPanel.ts/eth.test.ts/mock/server.ts는 메인. E2E는 Opus 소유. 메인 직접 구현·자체 리뷰.
