# can-sol M1 수용 준비 — can_sim worker 부하·메모리 시험 (M1 ④⑧, C2)

공통 규칙 `docs/impl/can/tasks/p0-common.md`(질문 금지, 커밋 안 함, 검사 worktree 루트). **범위:** 새 파일 `crates/loom-can/tests/load.rs`(+ 필요하면 `crates/loom-can/tests/common/`은 **건드리지 말고** 이 파일 안에 helper), 보고서 `docs/impl/can/reports/m1-load.md`. `src/*`는 수정하지 않는다 — 버그나 상한 문제를 찾으면 보고서에 재현법과 함께 적는다(can-opus가 지금 `gateway.rs`·`cli.rs`·`tests/gateway.rs`를 고치는 중).

정본: CAN plan §11 M1 수용 기준 ④(20k fps 60 s: 멈춤 없음, 생략·손실 카운터가 실제와 일치, 기록 = 생성 수)·⑦(worker kill/재시작)·⑧(단기 soak RSS 추세), §9.4(C2 상한 표), 마스터 §3.0 P1 관문(30분 연속 RSS 증가 ≤ 10 MiB — 개발 중에는 10분 이내로).

1. **고부하(기본 시험, ≤ 10 s, release 권장이면 `#[ignore]` + 실행법)**: `CanSimWorker`(ManualClock, PumpMode::Manual) + FakeWorker. sim 노드 또는 agent burst로 **가상 시각 60 s 동안 20k fps**(Classic·FD64 혼합) 생성 → 매 가상 ms pump·drain. 확인: worker가 막히지 않음(pump 1회 시간 상한 측정), `can.frame` 발행 수 + `source.gap` 수 = 생성 수(정확히), observer 큐가 상한을 넘지 않음, `request_id` 태그 맵·burst 큐가 상한 유지. drain을 일부러 느리게(예: 10 ms마다)도 해서 손실이 **정확히 세어지는지**.
2. **메모리 추세(`#[ignore]`, ≤ 10분, 실측)**: 실시계 pump 스레드 모드로 5k fps를 10분(또는 짧게 여러 번) 돌리며 1 s마다 프로세스 RSS 표본(macOS: `mach`/`getrusage` 등 표준 경로, 외부 도구 없이 — 안 되면 방법과 한계 기록) → 처음 1분 이후 기울기·증가량을 보고서에 [실측]으로. 목표: 10분 증가 ≤ 10 MiB.
3. **재기동**: worker를 drop 후 새로 만들고 같은 채널을 다시 열 때 generation 증가, 주기 잡·ISO-TP 엔드포인트 자동 재개 없음.
4. 보고서: 측정 환경(macOS·debug/release), 수치(fps, pump 시간 p99/max, 손실 정확도, RSS 표·기울기), 찾은 문제와 재현법.
끝나면 멈춘다.
