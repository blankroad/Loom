# explore(psu) → core 요청 (2026-11-11, loom-psu ca07142)

| # | 요청 | 지금 psu가 하는 임시 처리 |
|---|---|---|
| X-1 | **worker 주기 tick**: `Worker::on_tick` 또는 `run_worker`가 kind별 주기로 호출 | 5 Hz 측정 폴링을 `psu.poll` 호출로 받음 |
| X-2 | 훅에서 **다른 도메인 인터록 조회**(`WriteRequest`에 `interlocks` 뷰) | `uds.flashing`·`debug.flashing`을 볼 수 없어 주입식 조회(기본 "모두 꺼짐") |
| X-3 | `EmergencyOffSpec`에 호출 인자 명시 | 코어가 `psu.out{state:"off"}`(+채널)로 부른다고 가정 |
| X-4 | data 레인 `Emitter::rx`에 `metrics` | `psu.meas`를 priority(ctl) 레인으로 보냄 |
| X-5 | 한계 초과 전용 `E_LIMIT`(exit 4) 또는 `BlockClass::Limit` | `HardDeny`/`E_DENIED` |
| X-6 | `policy_section`으로 `[psu.<name>] vmax/imax`를 loom.toml에서 읽는 경로 | 기본값 16 V/5 A + connect로 낮추기만 |
| X-7 | eth 요청과 같음: write 클래스 `dry_run` 정책·감사 생략, 채널별 worker 지정, bounded wait API | (eth-core.md P1-4·P1-7·P1-8) |
