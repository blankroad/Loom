# IPC worker kind / event domain 분리

P1 실제 mixed sim probe에서 can.frame의 domain=can_sim 발견. UI CAN 패널은 domain=can만구독하므로 기존 WS 전체구독시험은통과해도 실제화면이누락될수있었다.

SDK Worker/SharedWorker에 DOMAIN 기본KIND를추가, serial/concurrent driver와WorkerHello.domains/Emitter가이값사용. CanSimWorker DOMAIN=can, KIND=can_sim(감독/기동키)유지. 기존 backend의source기록/저널도정상도메인으로발행.

실제gateway기본feature IPC can_eth_smoke3(ECU응답domain=can, backendkind유출없음assert추가), uds_tc1_smoke3(originDeviceRXassert포함), preview_policy_smoke2 PASS: /private/tmp/loom-p1-domain-routing.log. SDK/CANclippyalltargets-DwarningsPASS /private/tmp/loom-can-domain-clippy.log. 실제probe2에서 can.frame/can.tx/can.channel_state 모두domaincan, errors0. 브라우저렌더자체증거는별도필수.
메인구현/자체리뷰, 독립리뷰없음. push없음.
