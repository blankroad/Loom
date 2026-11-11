# CAN P1 generation / finite tail loss

기준 main 6d41c24, 메인 직접 구현·자체 리뷰(독립 리뷰 아님), 기존 worktree 반례를 현재 실제 producer epoch 모델에 맞춰 재현.

- 재생성 후 generation 재사용: IPC producer epoch를 generation 상위16bit에 반영. epoch1 기존 generation1,2 유지, 로컬 open과 epoch 공간 소진은 wrap 대신 E_BUSY. 실제 gateway의 자기 can_sim PID만 kill하고 supervisor의 새 producer epoch를 기다려 재open generation 증가 확인.
- 유한 캡처 마지막 observer overflow: 후속 marker 없이 drain 끝에서 backend 누적 drops와 이미 보고한 loss 차이를 발행. 다음 burst에서 중복계수 방지.
- EmitError::Full은 journal수락후 display거부일 수 있어 gap count 재전송을 제거. 실패 source seq/emit_failed로 불완전성을 남김.

검사: cargo test -p loom-can 전체 PASS(/private/tmp/loom-can-p1-regression.log), 신규 generation_tail4 + worker17 PASS(/private/tmp/loom-can-generation-tail-test.log). clippy all-targets -D warnings PASS(/private/tmp/loom-can-p1-clippy.log). 실제 gateway no-default features echo,uart,can,uds,eth,psu can_eth_smoke3 PASS(재생성 포함), preview_policy_smoke2 PASS /private/tmp/loom-p1-preview-can-integration.log. TEST 담당 동시편집으로 해당 실행만 TESTfeature제외, 최종 통합에서 기본구성 검사 필요.

브라우저 렌더는 별도 관문이며 이 보고로 충족했다고 세지 않는다. root공유 index만 사용, push없음.
