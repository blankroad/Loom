# logic 도메인 — 최종 연동 계약

2026-11-14. 실제 구현은 crates/loom-logic, native UI logic.analyzer, CLI loom logic. 초기 제안/소유권 기록은 모델별 보고서이며 이 문서가 최종 wire 계약이다.

## 수집/디코딩

- sources: sigrok-demo(실제 libsigrok demo acquisition), synthetic(명시적 Loom 파형 생성 + 실제 sigrok decoder), sigrok-device(fx2lafw만 허용하는 실물 미검증 후보). 자동 fallback 없음. saleae-logic16/pro는 16채널 packing 미지원으로 거부. compiled driver와 implemented support는 별도 표시.
- acquisition은 항상 D0..D7, 1 byte/sample, bit i=Di. channels는 표시 mask만 선택하고 acquisition packing을 바꾸지 않는다. 원시 바이트의 한 수집 소유자가 파형 처리와 별도 sigrok-cli decoder stdin에 같은 데이터를 공급한다.
- UART 또는 SPI 디코더 최대1개. libsigrok word annotation 필터를 사용한다. raw sample clock과 host receipt timestamp를 구분한다. 실제 장비 스캔/펌웨어 작업은 이번 검증에 포함되지 않았다.

## RPC와 CLI

- logic.status / loom logic status: engine path/version/error, compiled 목록, implemented 목록(device_verified=false), 최근 capture와 running을 반환. USB scan 없음.
- logic.start / loom logic start --source S: request R1, agent/script/operate 허용. strict params source, driver?, conn?(USB bus.address), samplerate(1000..10000000), channels?(D0..D7[]), decoders?(0..1 spec[]), duration_ms(100..3600000, 기본60000), max_samples?(1..1e11), pattern?, text?(최대256B), frame_period_ms?(1..10000), clock_hz?. demo pattern을 지정하면200000Hz. 합성 UART는8N1 LSB, 합성SPI는8-bit 및4mode/bitorder지원. 결과 capture_id/state/config/argv/engine/request_id. 동시에1캡처; Busy/STOP/설정오류를 명시.
- logic.stop / loom logic stop [ID]: request R0, viewer watch 허용. capture_id?를 받아 최종 capture 상태 또는 stopped:false 반환. STOP latch는별도core안전기능이며agent start를거부하고 실행캡처를멈춤.
- logic.read / loom logic read: capture_id?, kind(annotation|edges|state|all), tail(1..1000), since?(정식Cursor), follow(bool), wait_ms. false에서는wait_ms0만, true에서는1..30000(기본10000). 응답 rows/next/complete/rows_left_out. annotation batch는행으로평탄화, raw는최대64전이요약과생략수를표시. 원본PublishedRecord는loom events로조회. rows결과예산56KiB로64KiB envelope여유보존.
- logic.wait / loom logic wait: text 또는 ascii 정확히하나, decoder?(base uart 또는uart-1), capture_id?, since?, timeout_ms(1..600000). text는대소문자무시정확일치. ascii는hex byte annotations를재구성하며 gap_before/drop/journal손실에서window를끊는다. 개별text매치와연속byte증거를구분.
- CLI는 --rate 1M, --duration 10s, --decode uart:rx=D0:baudrate=100000, --channels D0,D1 형태를wire로정규화한다. read --follow --wait 10s는한번의bounded long-poll이며next로재개. 지속관측은기존loom events --domain logic --since now --follow --timeout 40 재사용.

## 공통 이벤트: domain=logic, channel=capture_id

- logic.capture_state(priority): capture_id, state(starting/running/stopping/stopped/failed), source/driver/samplerate/channels/decoders, config/argv, limits, requested_by, reason/error/stderr_tail, exit{acquisition,decoder}, counters. UI는onControl과snapshot/backlog모두수신하고seq를비교한다.
- logic.edges(data): capture_id, seq(캡처내chunk번호), samplerate, sample_start inclusive/sample_end exclusive, initial/final/mask/active 숫자bitmask, edges:[[absoluteSample,stateAfter]], dropped. 전이 최대1024개/통상100ms. 범위밖전이는실제표시손실이며정상파형으로보간하지않는다. host ts는샘플시각이아니다.
- logic.annotations(data): capture_id, samplerate, annotations:[{decoder,class,start_sample,end_sample,t_start_ns,t_end_ns,text,gap_before?}], dropped. decoder는uart-1/spi-1등실제ID, class=null. 파싱불가줄은class=_raw 및null sample로별도표시. 배치최대256행/약47KiB/50ms, rate2000행/s와burst4000. 큐/속도손실은카운트및gap marker로명시.
- canonical record epoch/seq/source_seq는daemon이부여한다. UI 행식별은capture_id와decoder/sample span/text를같이대조해원본record에연결.

## Snapshot과 자원 수명

Snapshot은 captures(오래된순최대4개), annotations(최대200행/16KiB), edges(24KiB), evicted 메타를포함. UI는동일snapshot high-water를가진capture의순서에서도최신을정확히선택한다. raw producer1024 cap은허용최악sample길이에서도24KiB안에맞는다. late join은전체기록복구가아니라bounded최근표시다.

Decoder stdin queue4MiB, acquisition64×64KiB, annotation queue1024행, 줄4KiB, stderr ring64KiB/tail4KiB. tee실패는sticky failed로후속sample을구멍너머연결하지않고, writer join뒤최종전달bytes/오류도검사한다. STOP은acquisition newline→2s→groupINT→1s→groupKILL/reap, decoder EOF→최대2s drain→groupKILL/reap. descendant pipe/초기화실패/Drop/latch도정리한다.

원시전체파일export, 아날로그, trigger,16채널,다중동시캡처는이번slice범위가아니다. 표시손실과decoder 입력손실을같은것으로보고하지않는다. 실제고밀도demo에서는표시손실이커질수있으며이를UI에그대로표시한다.
