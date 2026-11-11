docs/plan/02-core-principle.md 준수
# CAN / Ethernet 실제 IPC 스모크

기능 / 상태 / 담당: CAN gateway lifecycle 연결 + CAN/ETH sim 스모크 / READY_FOR_REVIEW / can_eth_smoke 직접 구현·자체 리뷰
작업 위치 / 기준 SHA: /Users/ctmctm/workspace/Loom / main 1a2e817
소유 파일: crates/loom-can/src/{gateway,worker}.rs, crates/loom-can/tests/gateway.rs, crates/loom-gateway/tests/can_eth_smoke.rs, 본 보고서. 공유 index 조작·커밋·푸시 없음.

## 발견과 수정

최초 실제 WebSocket RPC에서 can.status는 sim을 반환하지만 can.open은 -32601 method not found. 기존 worker CALL_OPEN/CALL_CLOSE가 gateway에 미등록되어 CAN 핵심 루프 진입이 불가능했다. /private/tmp/loom-can-eth-smoke.log.

can.open/close를 Request write class로 등록, agent/script/operate viewer에 허용. gateway risk(open R1, close R0), Sent terminal을 선언하고 worker.write lifecycle dispatch를 추가했다. gateway의 STOP·recording·audit 관문을 통과하며 worker도 agent/script STOP을 재확인한다. open은 closed channel의 과거 generation으로 차단하지 않고 worker가 실제 lifecycle 중복·상한을 검증한다. worker 기존 call entry는 내부 테스트 호환용 유지. 기존 worker의 channel/node/queue 상한 변화 없음.

## 실제 검증

실제 gateway daemon 서비스(start(Config))를 테스트 프로세스 안에서 시작하고, loomd binary를 별도 IPC worker 프로세스로 구동. TCP WebSocket agent/viewer 연결과 실제 sim 도메인을 사용. fixture/mock worker 아님. 각 테스트 별도의 임시 HOME과 랜덤 포트, 테스트 전용 operate viewer 사용. 기존 사용자 daemon/STOP 미접촉. graceful shutdown으로 worker seal 후 정리.

- CAN: open ch0 + 기본 BCM 노드, can.send 300#01 → 0x301 ECU reply를 동일 live viewer can.frame에서 확인, core.stop 뒤 agent send/open E_STOPPED, operate viewer send 및 origin=user 이벤트, viewer close 성공.
- Ethernet: sim capture start → live eth.pkt → eth.query 상세 layers, agent ARP raw TX와 attributed_origin=agent 이벤트, core.stop 뒤 agent send E_STOPPED, operate viewer send와 attributed_origin=user 이벤트.

명령: CARGO_TARGET_DIR=~/.cache/loom-main-target cargo test -p loom-gateway --test can_eth_smoke --locked --offline -- --nocapture
최종 기능 소스 실행 결과: 2 passed / 0 failed / 2.49s. /private/tmp/loom-can-eth-smoke-final.log.

초기 lifecycle 소스의 cargo test -p loom-can은 registry_probe_passes의 명령 목록 기대값만 신규 2개 누락으로 실패. 현재 목록 보강 완료. 일시적 병렬 manifest/lock 불일치를 메인이 정리한 후 최종 cargo test -p loom-can --locked --offline: 114 passed, exit 0. cargo clippy -p loom-can -p loom-gateway --all-targets --locked --offline -- -D warnings: PASS. /private/tmp/loom-can-lifecycle-final-{test,clippy}.log. 소유 Rust 4개 파일 rustfmt --check 및 git diff --check PASS. 자체 리뷰 완료; 독립 리뷰는 메인 담당.

## 한계 / 다음 행동

브라우저 렌더·화면 지연은 검증하지 않았다. WS 동일 stream 수신까지의 증거이며 P1 전체 DONE 아님. 실제 NIC/CAN 하드웨어·CLI open/demo 연결, 기존 CAN generation 재사용 및 tail-gap 이슈, TC-1·30분 RSS 관문은 비범위. 추가 하위 에이전트 없음. 한도·기한 미지정, 제공 사용량 미상.
다음 담당 메인: 독립 리뷰 및 기능별 커밋·통합 판정. 소유권은 이 반환 확인 시 메인으로 이전하며 추가 기능을 진행하지 않는다.
