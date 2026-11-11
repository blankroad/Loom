# can-sol M2a(앞당김) — `crates/loom-can-periodic` (주기 송신 스케줄러, K9 코어)

먼저 `docs/impl/can/tasks/p0-common.md`(공통 규칙 — **대화형 질문 금지**, 결정은 보고서에). 이제 모든 crate는 **workspace 멤버**: worktree 루트(`/Users/ctmctm/workspace/Loom/.worktrees/can`)에서 `cargo fmt --all --check`, `cargo clippy -p loom-can-periodic --all-targets -- -D warnings`, `cargo test -p loom-can-periodic`. Cargo.toml은 다른 CAN crate처럼 `version.workspace = true` 등 + `[lints] workspace = true`, 시험 파일 맨 위 `#![allow(clippy::unwrap_used, clippy::expect_used)]`. 경로는 `/Users/ctmctm/workspace/Loom/...`만. 커밋하지 않는다.

**범위:** 새 crate `crates/loom-can-periodic/`와 보고서 `docs/impl/can/reports/m2-periodic.md`만. `crates/loom-can/`은 can-opus가 작업 중이라 건드리지 않는다(나중에 리드가 worker에 연결).

## 목표 (plan §10.5, §9.4, D5·D30·D31, A1·A3·A9)
CAN worker의 TX 중재 전용 스레드가 쓸 **sans-IO 주기 송신 스케줄러**. 스레드·시계를 갖지 않는다: 호출자가 `now_mono_ns`를 넣고, 스케줄러는 지금 보낼 프레임과 다음 깨울 시각을 돌려준다. `loom-can-isotp`과 같은 구동 방식(출력 큐 상한, `poll_at`, `on_timer`).

## 요구
1. 의존: `loom-can-frame`(경로)만. 신호 인코딩은 상위(gateway DBC codec)가 하고 여기엔 **바이트 단위 patch**만(`(offset, mask, bytes)` 또는 전체 payload 교체).
2. **잡**: `start(JobSpec{channel, frame: CanFrame, period_ns, offset_ns, origin(Agent/User/Script), owner, lease_ns(Option), max_duration_ns, count(Option — 유한 횟수)}) -> JobId`. 상한: 잡 128개(plan §9.4), patch 대기 256개. 넘치면 accepted 전에 `ResourceExhausted`.
3. **스케줄링**: 단조 절대 deadline min-heap. 놓친 주기는 **skip + 카운트**(지연분 burst catch-up 금지 — plan D5). `poll(now)` → `Due{job, frame, scheduled_ns, lateness_ns, revision}` 목록(상한 있는 출력 큐). 호출자가 `on_submitted(job, actual_ns)` / 실패(`on_send_failed`)를 알려 주면 통계(lateness p50/p99/max 근사 — 고정 크기 히스토그램, missed, sent)를 갱신.
4. **patch + revision CAS**: `patch(job, expected_rev, Patch) -> new_rev` — rev가 다르면 `RevisionConflict{current}`(UI의 "에이전트가 방금 rev 5로 바꿈" 경로). 반영은 **다음 주기**에 원자적으로.
5. **lease / 수명 (A9)**: `renew(job, now)`는 누적 `max_duration_ns`(기본 1 h) 안에서만 연장, 넘으면 `LeaseCapExceeded`. lease 만료·`count` 소진·`max_duration` 도달 → 잡 정지 이벤트(`Stopped{reason: LeaseExpired|Completed|MaxDuration|Stopped|Latched|Takeover|BusOff}`).
6. **STOP latch (A3)**: `latch(on)` — on이면 **Agent·Script 잡 전부 정지**(Latched), User 잡도 정지(A3: "모든 잡" latch — plan §10.6: 사용자 IG 잡 포함), 새 start는 Agent·Script 거절, User 1회 송신은 이 crate 밖. 해제해도 자동 재개 없음.
7. **중재 (plan §10.2 "중재")**: 같은 (channel, ext, id)에 두 번째 잡 → `Conflict{owner}`; `takeover(job, new_owner, origin)`만 허용(기존 소유자에 Stopped{Takeover} 이벤트).
8. **버스 상태**: `pause_channel(ch, reason)`(ERROR_PASSIVE/BUS_OFF — agent 잡 자동 일시정지 + 이벤트), `resume_channel`은 명시만.
9. **C2**: 모든 컬렉션 상한, 출력 큐 넘치면 상태 변경 전에 오류, 카운터 포화.

## 시험 (가상 시각, 결정적)
- 10 ms 잡 1 s → 정확히 100프레임, scheduled 간격 일정, offset 반영.
- 늦은 poll(예: 35 ms 늦음) → catch-up burst 없이 skip 3 + 카운트, 다음 deadline은 격자 유지.
- patch CAS(성공·충돌·다음 주기 반영), count 유한 잡 완료, lease 만료·renew·상한, latch(Agent/Script/User 모두 정지, 새 Agent start 거절, 해제 후 재개 없음), 중재 충돌·takeover, pause/resume, 상한(129번째 잡, 출력 큐), 128잡 × 서로 다른 주기 혼합 1분 가상 시각 무결성(순서·개수).
- **실시간 스모크(선택, ≤ 10 s, `#[ignore]`)**: 전용 스레드 + `std::thread::sleep`로 10 ms 잡 500회 → lateness p50/p99/max를 보고서에 [실측, macOS]로(이전 `spikes/can/rust/RESULTS.md` p99 4.53 ms와 비교). 고해상도 대기(짧은 spin 마무리) 옵션이 있으면 둘 다 재고 CPU 비용도 적는다.

끝나면 보고서(공개 API, 시험 목록·결과, 실측, 결정·요청)를 쓰고 멈춘다.
