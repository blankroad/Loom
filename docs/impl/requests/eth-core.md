# eth → core 요청 (2026-11-11, explore-lead)

## 1. workspace.dependencies 추가 (eth P0 crate가 쓰는 외부 crate)
eth crate 4개(`loom-eth-decode`, `loom-eth-pcapng`, `loom-eth-capture`, `loom-eth-filter`)는 지금 독립 crate로 시험한다. 루트 workspace가 `main`에 들어오면 `[lints] workspace = true`와 `.workspace = true` 의존으로 옮긴다. 그때 다음 항목을 루트 `Cargo.toml`에 추가해 달라.

| crate | 판 | 용도 | 비고 |
|---|---|---|---|
| etherparse | =0.21.0 | L2–L4 파싱, sim 프레임 합성 | decode·capture |
| pcap | =2.5.0 | 라이브 캡처(Npcap/libpcap) | capture feature `live`(기본 꺼짐), optional |
| regex | 1.13.1 | 표시 필터 `matches` | filter — UART expect와 같은 엔진(TEST TC-C7) |
| ipnet | 2.12.2 | 필터 CIDR 리터럴 | filter |

- 내부 path 의존: `loom-eth-pcapng`(capture가 사용), `loom-eth-decode`(filter가 사용).
- someip_parse는 쓰지 않는다(decode가 직접 구현).

## 2. 계약 v1 관련 (지난 협의 `_xteam/eth-core.md` 재확인)
- 저널 codec 레지스트리에 `pcap-epb/1`을 등록한다(레코드 = pcapng EPB 바이트, Loom 메타는 `opt_comment` `loom1 …` — eth 결정 X10).
- `source.gap` cause 추가: `kernel_drop`, `if_drop`, `capture_restart`, `stats_unavailable`.
- `ts.source` 추가: `pcap_host`, `pcap_adapter`, `replay`, `sim`.
- worker kind `eth`, PerChannel ≤ 4. 별도 실행 파일 `loom-eth-worker`가 필요하다(Linux는 사람이 이 파일에만 `cap_net_raw`를 준다).

## core 답변 (core-lead, 2026-11-11, 계약 v1.2 `cd68bb8`)
- 1: 루트 `[workspace.dependencies]`에 `etherparse = "=0.21.0"`, `pcap = "=2.5.0"`(eth crate에서 `optional = true` + feature `live`), `regex = "1.13.1"`, `ipnet = "2.12.2"` 추가 완료(`cd68bb8`). eth crate는 `.workspace = true`와 `[lints] workspace = true`로 옮겨 주세요. 내부 path 의존(`loom-eth-pcapng`, `loom-eth-decode`)은 각 crate에서 `path = "../…"` 그대로 두거나, 원하면 workspace에 올려 드립니다(요청 시).
- 2: 저널 codec 레지스트리에 **`pcap-epb/1`(id 3)** 등록(계약 06, `loom-journal`이 payload를 해석하지 않고 저장). `source.gap` cause `kernel_drop`·`if_drop`·`capture_restart`·`stats_unavailable` 계약 02에 기재(cause는 문자열이라 타입 변경 없음). `ts.source` `pcap_host`·`pcap_adapter`·`replay`·`sim`을 `loom_proto::event::TsSource`에 추가. worker kind `eth` PerChannel ≤ 4, **별도 실행 파일 `loom-eth-worker`** 예외 승인(계약 01) — 같은 worker 프로토콜, 감독은 `loom-worker-host`(P1 진행 중)가 실행 파일 경로를 WorkerSpec에서 받는다.

## 3. P1 요청 (2026-11-11, eth P1 착수 — core lead가 가져가도록)
| # | 요청 | 이유 |
|---|---|---|
| P1-1 | `loomd`·`loom` 바이너리에 cargo feature `eth`로 `loom-eth` 등록(gateway·worker·CLI 조각). echo와 같은 방식 | eth는 1차(F2) 도메인 |
| P1-2 | worker 실행 파일 지정: `WorkerSpec`(또는 manifest)에 "별도 실행 파일 경로" 선택지 — Linux에서 `cap_net_raw`를 **작은 `loom-eth-worker`에만** 주기 위함(eth 결정 X3). M1은 `loomd worker eth`로 동작해도 됨 | 최소 권한 |
| P1-3 | worker 쪽 저널 writer(`loom-journal`)와 도메인 codec 등록(`pcap-epb/1`). 그 전까지 eth는 `eth.pkt.bytes`(base64)가 원본을 겸하며, **발행 예산을 넘어 개별 발행이 생략된 패킷은 원본이 남지 않는다**(eth.batch로 개수만 보고) | 원본 무손실(C2·D7) |
| P1-4 | 도메인 핸들러용 bounded wait/subscribe API(예: `ctx.wait_for(filter, deadline)`) — 없으면 eth `wait_for`는 짧은 주기 저널 재조회로 구현 | tshark식 `wait-for`·`trace --follow` |
| P1-5 | 허브 구독 필터 `filter.test_step`(UI 테스트 화면 증거 스트림) | ui-test.md 4번 |
| P1-6 | raw HTTP 경로(`raw_url`)와 `raw_ref` 저장소 — eth export 다운로드·16 KiB 넘는 프레임 | ui-eth.md 8번 |

## 4. P1 구현 후 추가 요청 (2026-11-11, eth P1 커밋 021181a·b497a9e)
| # | 요청 | 이유 / 지금 eth가 하는 임시 처리 |
|---|---|---|
| P1-7 | write class 명령의 `dry_run:true`는 정책·감사·행 없이 handler로 바로 보내기(또는 `CommandSpec.preview_param`) | UI 송신 편집 미리보기(ui-eth 6). 지금은 handler가 행 없이 답하지만 감사 생략은 파이프라인 몫 |
| P1-8 | `CallCtx::worker_for(kind, channel)` — PerChannel worker 주소 지정 | 지금은 args `ch`로 worker가 고름 |
| P1-9 | 저널 꼬리 스캔(`Cursor::Last` + `max_events=N` = 최근 N개) | 없어서 `from` 없는 조회는 최근 600 s만 봄 |
| P1-10 | `source.gap`·`record.gap`에 원 채널(또는 `SubFilter.channels`가 그 채널의 core gap 포함) | 채널 필터 스캔이 손실을 못 보면 부재 증거를 잘못 확정할 수 있음 |
| P1-11 | 증거 검증 입력 `events`에 창 안의 gap·도메인 커버리지 이벤트(`eth.batch`)가 함께 온다는 것을 계약 07에 명시 | 전수 평가의 완결성 판정 |
| P1-12 | export job API(`core.job_progress`) + 출력 폴더 정책 | 지금은 동기·상한(10만 패킷/256 MiB/600 s), 에이전트가 준 경로에 데몬이 씀 |
| P1-13 | worker-sdk: data 이벤트 draft에 `dir`(tx)·`purpose`·`quality` 지정 | 자기 송신 복사본이 레코드 `dir=rx`로 나감 → 지금은 `payload.dir`에 보존 |
| P1-14 | worker-sdk: 표준 gap 발행기(`record.gap`·`display.gap`의 domain 정규화) | 지금 priority 경로로 domain=eth로 나감 |
| P1-15 | worker `Health` 어댑터·runtime 연결, capture source_seq ↔ SDK src_seq 좌표 계약 | 상태·통계 이벤트가 끼어 두 seq가 다름 |
- 이전 P1-3(worker 저널 writer + `pcap-epb/1`)이 가장 급하다: 지금은 발행 예산을 넘어 개별 발행이 생략된 패킷의 원본이 남지 않는다(`eth.batch.unrecorded`로 개수만 보임).
