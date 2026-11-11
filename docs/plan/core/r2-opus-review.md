docs/plan/02-core-principle.md 준수

# CORE R2 교차 리뷰 — Opus → Sol

작성: core-opus, 2026-11-10(재부팅 후 재개) · 대상: `core/r1-sol.md`, `spikes/core/README.md`·`library-evidence.md`·`evidence/`, `_xteam/core-ui.md`의 "core 의견 — R1 Sol" 절 · 비교: `core/r1-opus.md` · 입력 확인: `_xteam/{can,uart,uds}-core.md`, `core-test.md`, `core-ui.md`, `test-domains.md`, `ui-test.md`, `ui-panel-template.md`, `vn5640-eth.md`, 마스터 §2.3(C4.1 포함).
표기: **[확인]** 원문·crates.io 메타데이터·스파이크 실측 · **[추정]** 산술·설계 판단. 초안·xteam 파일은 수정하지 않았다.
추가 스파이크(10분 이내): `spikes/core/opus-cli-rtt/` — CLI 생성+WS 연결+hello+RPC 1회+종료 **p95 3.30 ms**(macOS M1, 300회) [확인]. 재부팅 전 실행, RESULT.md는 재개 후 기록.
**전제 변경 C4.1**: DoIP = PC 랜카드 + 미디어 컨버터(OS TCP/IP). VN5640은 CAN FD 전용. 두 초안의 "VN5640 이더넷/smoltcp" 관련 문장은 모두 무효로 보고 판단했다.

---

## 1. 요약 판정

- **강점 ①** 근거의 정직성: IPC·WS·저널을 실제로 재고, 측정 조건(합성 tuple, paced 3 s, Rust 수신만)과 실패한 첫 실행(정상 구독자 400건 누락)까지 보존했다. Apple `sync_data` = `F_FULLFSYNC` 소스 확인도 정확하다.
- **강점 ②** 메모리·큐: 전체 연결·구독 admission(8연결/32구독), worker 합계 1 GiB admission, Windows Job = commit 한계·RSS 별도 측정 등 **합산 가능한 상한 체계**는 내 초안보다 낫다.
- **강점 ③** 의미 정밀화: publish ≠ render(`published_through_seq`·`rendered_through_seq`), stream_seq가 없는 화면 손실은 source 범위로 표시, snapshot 중 live 대기 1 MiB/2 s 상한, request별 causal fence, STOP mailbox, 비상 OFF와 일반 복구의 구분.
- **문제 ①** P0 계약 표면이 이름 목록뿐이다. Rust 시그니처·이벤트/봉투/오류 enum·CLI 명령 트리·사람용/JSON 출력·오류 예시·공통 `loom` 스킬(C8·D13)이 없어 도메인 팀이 첫 주에 바로 쓸 수 없다.
- **문제 ②** 사용자 개입 가시성과 안전 의미에 구멍이 있다: `others_since_last`를 `(LOOM_CLIENT, channel)` 단위로 정해 다른 채널의 사용자 개입·전역 STOP이 빠질 수 있고, worker 재기동 후 "RX·상태 복구"를 일률 적용하면 UART 포트 재open(DTR 리셋)과 충돌한다. STOP 해제 권한도 정하지 않았다.
- **문제 ③** 타팀 요청 미답·자기 근거와 어긋난 값: 즉시 `check` 불일치 exit 3(UART #15·UDS C-11·TEST)을 보류했고, TEST TC-C1–C9·`test-domains`의 증거 REF·`ui-test` RPC에 답이 없다. 구독 큐 512 KiB/8배치는 자기 스파이크의 첫 실행 실패(8배치 큐로 정상 구독자 손실)와 모순된다.

---

## 2. 틀린 사실

| # | 위치 | Sol 주장 | 실제 | 근거 |
|---|---|---|---|---|
| E1 | §7 표(L228)·`library-evidence.md` | zstd 최신 0.13.4 | 최고 안정판 **0.14.3**. 0.13.4는 2026-11-01에 게시된 구 계열 백포트 | [확인] `evidence/crate-zstd.json`: `max_stable_version=0.14.3`, `newest_version=0.13.4` |
| E2 | §3.4 표(L120)·§7(L221) | fastwebsockets 0.8.1 | 최고 안정판 **0.10.0**(0.8.1은 2026-01-01 백포트) | [확인] `crate-fastwebsockets.json` |
| E3 | §7(L228) | process-wrap 8.2.2 | 최고 안정판 **10.0.1**(8.2.2·9.1.1·10.0.1이 같은 날 게시) | [확인] `crate-process-wrap.json` |
| E4 | `collect_evidence.py` 선택 기준 | "최신" = `newest_version` | `newest_version`은 "가장 최근 게시"라 백포트를 최신으로 잡는다. 40개 중 5개 불일치(위 3개 + specta·typify의 사전판 — 이 둘은 본문에 정식판을 써서 영향 없음) | [확인] 전수 대조. Sol 리뷰는 zstd만 정정했고 E2·E3은 누락 |
| E5 | §9.2 구독 데이터 큐 "512 KiB/8 batch" | 정상 구독자 지터 흡수 가능한 초기값 | 자기 첫 실행 `runs/20261109T230951Z/ws-column-4-2.json`에서 **8배치 큐로 정상 구독자 3개가 각 400건 손실**. 근거가 반대 방향 | [확인] README §2 |
| E6 | §4(L178) "이번 라운드 CLI 기동 측정은 미실행", 목표 warm p95 ≤ 50 ms | 기동이 주요 비용이라는 전제 | CLI 고정 비용은 **p95 3.30 ms**(macOS). 50 ms 예산의 대부분은 gateway 쪽, 특히 §10의 **요청마다 audit intent fsync**(Apple F_FULLFSYNC 1회 3.9–5.9 ms, Sol 저널 실측)다 | [확인] `opus-cli-rtt/RESULT.md`, Sol README §3 |
| E7 | §3.3(L110) "재연결은 RX·상태만 복구" (worker 재기동 일반 규칙) | 모든 도메인에 RX 자동 복구 | UART는 worker·데몬 재시작 후 **포트를 자동으로 열지 않는다**(DTR/RTS 토글로 보드 리셋 — UART plan §3.2·§3.4, D8). CAN만 RX 재개(can-core W9). 도메인 선언이어야 한다 | [확인] uart/plan.md §3.4 |
| E8 | §4(L164) `others_since_last` = `(LOOM_CLIENT, channel)` 커서 이후 | D5 충족 | D5는 "모든 결과에" 사용자 개입을 알리는 장치. 채널별 커서면 에이전트가 UART를 쓰는 동안 사용자가 CAN에서 보낸 프레임·전역 STOP·정책 변경이 집계되지 않는다(02 원칙 ④ "개입이 에이전트에 보임" 약화) | [확인] 마스터 D5, CAN §3.7 I6 |
| E9 | §8(L259) VN5640 DoIP "실측 대기" | 미정 | C4.1로 확정: OS NIC 위 DoIP, XL 이더넷·smoltcp 범위 밖. UDS DoIP 전송은 gateway 모듈의 OS 소켓 | [확인] 마스터 C4.1, `_xteam/vn5640-eth.md` |
| E10 | §9.1(L279) "1채널 10k fps FD를 첫 기준, 4채널 40k fps 제품 부하" | 100% 부하 대표값 | 채널당 최대 프레임률은 **최소 프레임**이 정한다. Classic 1 Mbit/s DLC0 ≈ 50 bit/프레임 → ≈ 20k fps/채널. CAN E2(100% 부하 + UDS) 시나리오 기준이면 4채널 ≈ 80k. 100k 합성 stress가 이를 덮으므로 큐 크기 산정 기준만 고치면 됨 | [추정 산술] Sol 리뷰 F11도 DLC0 ≈ 21.3k로 동일 |

---

## 3. 빠진 것 (또는 잘못 답한 것)

### 3.1 brief 범위 10항목
| 항목 | 판정 |
|---|---|
| 1 프로세스 구조 | 충족. 판정선(DLL·블로킹·CPU 20%·예산·deadline)과 UDS→worker 이동 fallback 좋음. **worker hang 처리**(heartbeat 5 s unresponsive 이후 kill·재기동 시점)가 없다 |
| 2 worker 프로토콜 | 충족(2 lane·map/bin·credit·nonce 재부착). 메시지 집합(Call/Write/Job/Input/Latch/Reply/Health)과 **publish-before-reply를 두 lane에서 어떻게 순서 보장하는지**(예: Reply의 covers 워터마크) 미기술 |
| 3 IPC·허브 | 충족(가장 강한 절). 단 role 표·hello 응답 예시 없음 |
| 4 저널 | 충족. D7 유지 판단 타당. 디스크 산술(아래 4절 저널 행)을 quota 정책과 연결하지 않음 |
| 5 CLI | **부족**: 명령 트리 7행, 사람용·`--json`·오류 출력 예시 없음, 자동 기동 체감(stderr 안내·`daemon_autostarted` 경고·Windows 부모 Job breakaway·에이전트 샌드박스 상속) 없음, `confirm`·`approvals`·`mark`·`record`·`status --request`·`doctor`·`ui open`·`skills sync` 누락 |
| 6 정책·승인·감사·STOP | 대부분 충족. **STOP 해제 주체 미정**(agent가 해제 가능하면 A3 무력화), latch 지속(데몬 재시작 후 유지) 미기술, 위협 모델은 문장뿐 표 없음, 승인 객체·확인 토큰 구조 없음 |
| 7 스키마 | 충족(도구 pin·draft 2020-12 검증·golden). 이벤트 레코드 Rust 타입 초안이 없어 P0 계약으로는 부족 |
| 8 러너·테스트 세션 | 방향 충족. TEST v0 요청(TC-C1–C9) 이후 내용 미반영(Sol의 core-test 절은 TEST가 파일을 다시 쓰며 사라짐 — `r1-sol.md` §15.3만 남음) |
| 9 C2 | 충족·우수. 단 구독 큐 값(E5) |
| 10 빌드·배포 | 충족. workspace의 **도메인 의존 허용 crate(안정 API 표면)·CODEOWNERS** 규칙이 없다 |
| (C8) 스킬 | **누락**: 공통 `loom` SKILL.md 골격, `loom skills sync`(D13), `others_since_last`·STOP·exit 5 대응 규칙 |

### 3.2 02 원칙 체크리스트
| 항목 | 판정 |
|---|---|
| 맨 앞 핵심 루프 표 | 충족 |
| 종단 간 시나리오 | 충족(UART). 사용자 개입 홉(Ctrl-C → sent, STOP → ack, 승인 → 재개)의 **지연 목표가 시나리오 안에 없음**(큐 표에만 흩어짐) |
| 실시간성·고부하 | 충족. 홉 합 ≠ 종단 분위수라고 명시한 점 좋음 |
| M1 걷는 뼈대 | 충족(웹 3패널 포함) |
| 화면이 에이전트 세션과 분리되지 않음 | 설계 충족. 단 E8(`others_since_last` 채널 단위)로 "개입이 에이전트에 보임"이 약해짐 |
| 비핵심 후순위 | 충족 |

### 3.3 타팀 요청
| 요청 | Sol 답 | 문제 |
|---|---|---|
| can-core W1–W10, §2–§6 | 전부 수용 | W9 "RX만 재개"를 전 도메인으로 일반화(E7) |
| uart #1–#11 | 수용 | #2 UI 바이너리는 M2 opt-in(의도적 차이 — 표시 필요), #5 TTY 뷰어 존재 여부 미정(브라우저 전용인지 명시 필요) |
| uart #12–#13 | `core.time`·`core.metrics.report`·presence | 수용됨(uart-core v0.2가 이 이름을 채택) — Sol 명명 승 |
| uart #14 viewer 입력 우선 경로 | "key chunk 최저지연 경로" 한 줄 | gateway 수신 루프에서 어떻게 우회하는지(별도 태스크·ctl FIFO 안 우선) 미기술 |
| uart #15·uds C-11·TEST exit 3 | 메인 결정 보류, 확정 전 exit 0 + `matched:false` | 세 팀이 같은 안을 요구. 보류 시 `loom-test` 스킬 분기가 이중화. CORE가 CR로 제안했어야 함 |
| uart #16·uds C-7 사전 승인 패턴 | 수용 | 패턴 판정 훅 이름·원자 소비 미정 |
| uds C-5 viewer dry-run 미리보기 | "agent dry-run도 기존 계약 유지" | UDS 요청은 **viewer 미리보기 감사 제외·레이트 제한**. 답이 질문과 어긋남 |
| uds C-12 재연결 공통 키 | `link_only`·새 epoch·재송신 0 | 정책 키·이벤트 이름 공통화(UART 재열거·CAN bus-off)는 미제시 |
| TEST TC-C1–C9 (`core-test.md`) | 답 없음 | 자동 태깅 `test_session/test_step`, 오류 코드 `E_EVIDENCE_MISSING/E_OUT_OF_WINDOW`, hold/measure, 평가 예산 200k/200 ms, 세션 1(≤4), pass 판정 차단(A12) 모두 미반영. 동시 세션 16·step 32/run은 TEST 안과 충돌 |
| test-domains TC-C5 증거 REF | 답 없음 | `<domain>:<channel>@seq:a..b#<json-pointer>` 문법·`data.evidence[]` DTO·JSON Pointer 정본 경로. **REF에 stream_epoch가 없다** — 데몬 재시작이 끼면 모호(세션 바인딩 또는 `@e<epoch>` 필요) — 두 초안 모두 미해결 |
| ui-test | 답 없음 | `test.pause/resume/skip/retry/check/answer/ask`, `test.plan.show`, `approval.grant{plan_hash,bundle}`(→ `core.approval.respond`로 이름 정렬 필요), `records.range` |
| core-ui U1–U8 | 상세 수용 | 우수. U7 fixture replay stub gateway를 P0 후반에 둔 것은 내 초안에 없던 것 |

---

## 4. 결정 비교

| 쟁점 | Opus 안 | Sol 안 | 추천 | 이유 |
|---|---|---|---|---|
| 프로세스 구조·판정 기준 | W1–W5 판정표, UDS gateway + `ModuleRuntime::Dedicated`, UART 전체 1 worker | DLL/블로킹·CPU 20%·예산·deadline 판정선, UDS 전용 runtime thread, 지터 p99 > 2 ms면 worker 이동 | **Opus 판정표 + Sol 수치선·이동 fallback** | 표는 도메인 팀이 스스로 판정 가능, 수치선은 측정 가능. C4.1로 "Vector worker 안 TCP/IP" 행 삭제 — UDS DoIP는 gateway의 OS 소켓 |
| worker 전송·레인 | 이름 있는 로컬 소켓 2개(ctl/data), interprocess 또는 tokio | interprocess 2 물리 연결 | **동일 — 채택** | 큰 data 프레임 뒤 STOP HoL 제거. ctl 안에서도 긴 Call 뒤 Latch가 기다리지 않게 Latch는 별도 mailbox(Sol)로 |
| 직렬화·프레임 | msgpack, ctl map / data array(tuple), 프레임 256 KiB·1 MiB, credit 4 MiB | msgpack map + 명시 tag, 프레임 64 KiB, credit 512 KiB | **map/bin v1, 프레임 64 KiB, credit = 최대 유입률 × 100 ms(기본 1 MiB, worker별 설정)** | Python 공통·진화 우선(map 351 B/건도 1.65 M ev/s 인코딩 [확인 opus-repr]). 64 KiB면 할당 전 길이 검사 용이. 512 KiB는 20k fps × 351 B에서 ≈ 73 ms분이라 gateway 순간 정지에 화면 드롭이 잦음 [추정] |
| 기록 위치(can W3) | worker가 직접 | worker가 직접 | **채택** | 합의 |
| 저널 형식 | msgpack 세그먼트 LJ1 + SQLite 인덱스(D7 변경 CR 누락) | JSONL+raw(D7) + SQLite catalog, 측정 후 binary | **M1 = D7(JSONL+raw), binary는 M2 CR + 실측** | D7 준수·디버깅 쉬움. 단 580 B/건이면 5k fps에서 2.9 MB/s → **20 GiB quota를 약 2 h에 채움**(압축 전) [추정 산술] — 보존 기본값(30일)과 양립 불가하므로 M2 전 binary+zstd 실측·D7 CR을 일정에 박을 것 |
| fsync·감사 | 데이터 1 s sync, 감사 = 요청마다 `write()` 확인 + group fsync ≤ 100 ms | 데이터 flush ≤ 10 ms·sync ≤ 100 ms, **요청형 audit intent durable(fsync) 후 송신** | **데이터 sync ≤ 100 ms(Sol), 감사는 R2+ intent durable / R1은 write 확인 + group ≤ 100 ms** | 전원 손실 시 "보낸 기록이 없는 R2+ 쓰기"는 막을 가치가 있음. R1(UART 일반 입력·sim 송신)까지 강한 fsync면 Apple 4–6 ms/건, Windows 미지 → UART "send→sent p95 < 20 ms"를 위협. Windows 실측으로 확정 |
| UI 고속 전송 | JSON 기본, 열 프레임은 측정 후(C28) | JSON 기본 + 협상 `loom.columns.v1`(seq delta·validity bitmap·dictionary) | **Sol 상세 채택** | 실측 방향 근거(57.5 KB → 14.4 KB/배치) + 제품화 조건(불연속 seq·nullable)까지 명세. 고정 64 B 실험 코드는 재사용 금지(Sol 명시) |
| JSON-RPC | 자체 얇은 라우터 | 제한 subset 자체 dispatcher + 공식 오류 코드 시험, batch 거부 | **Sol 채택** | 같은 결론, Sol 쪽이 규격 준수 시험이 구체적 |
| 스키마 생성 | `loom-proto`(타입) + `loom-schema`(내보내기), `U64`/`Bytes` 이중 표현 | `loom-schema`(타입), json-schema-to-typescript 16.0.0·datamodel-code-generator 0.91.0 pin, draft 2020-12 검증 | **Opus crate 명명 + Sol 도구 pin·golden + `U64/Bytes` newtype** | `loom-schema`가 타입인지 생성기인지 혼동 제거. newtype의 hex 디코더는 홀수 길이·비ASCII에서 panic 가능 → fallible 디코더 + invalid corpus(Sol 리뷰 F13 수용) |
| 메모리 예산 | gateway 256/512, ring 64 MiB, 64연결×32구독(합산 안 됨), CAN 128/256, UART 64/128 | gateway 256/384, ring 32 MiB, 8연결/전체 32구독, CAN 96/192, UART 48/96, worker 합계 1 GiB admission | **Sol 체계 + 큐 값 재산정** | 전체 admission이 있어야 합산 가능. 단 구독 큐는 E5 근거로 512 KiB → **시간 기반(구독 유입률 × 500 ms, 하한 1 MiB)**, 전체 구독 큐 합 64 MiB cap |
| soak 관문 | 1 h 워밍업 후 기울기 ≤ 1 MiB/h, (최대−최소) ≤ 32/16 MiB, 핸들 ±5, TC 세션 10분 반복 | 30분 워밍업, (끝 30분 중앙값 − 첫 30분 중앙값) ≤ max(16 MiB, 5%), 기울기 ≤ 0.5 MiB/h, 파일·스레드 baseline+4, 3 Mbaud 상시·100k burst | **Sol 수치 + Opus 부하 패턴(TC 세션·장애 주입) + PID/producer_epoch별 구간 판정** | 중앙값 차는 할당자 plateau에 강함. worker 재기동 구간을 한 직선에 섞지 않기 |
| crate 구조·P0 범위 | stub 6개 + echo 견본 + 동작 FakeGateway(실제 허브·정책 sim) | schema/core-api/worker-sdk stub + fixture, P0 후반 fixture replay gateway | **Opus trait 표면 + Sol 단계화**: Day 1–2 타입·trait compile-only, Day 3–5 fixture FakeWorker·echo·스키마 생성·UI용 replay gateway | 도메인은 첫날 시그니처가 필요(Sol 초안의 공백), FakeGateway가 제품 허브·정책을 선행 구현하게 만들면 안 됨(Sol 리뷰 지적 수용) |
| CLI 기동 목표 | 동기 tungstenite, Linux/macOS p95 ≤ 30 ms·Windows ≤ 60 ms(쓰기 전체 왕복) | 작은 clap CLI, warm p95 ≤ 50 ms, cold ≤ 500 ms | **동기 CLI, 쓰기 전체 왕복 30/60 ms 유지 + "CLI 고정 비용 ≤ 10 ms(Win ≤ 25 ms)" 하위 게이트 추가** | 실측 3.3 ms라 고정 비용은 작음. 나머지 예산이 감사 fsync·worker 홉에 쓰이는지 분해해 보여 주는 게이트가 필요 |
| 테스트 세션 증거 검증 | 코어: 범위·coverage·단계 창·예산 / 도메인 `EvidenceVerifier` / PASS에 device 증거 ≥ 1 / gap 겹치면 uncertain | source_refs·completeness·domain `verify_evidence`·user assertion 분리·pin | **Opus 흐름(O1–O9) + Sol source_ref 정본 + plan expect 결속** | 인용 술어를 등록된 `step.expect` id/hash에 묶지 않으면 약한 술어로 PASS 가능(Sol 리뷰 반례 수용). "device 증거 ≥ 1" 일괄 규칙은 정당한 부재 PASS를 막으므로 **witness 유형별 규칙**(긍정 관측·부재 coverage·측정·user assertion)으로 교체하되 "agent 자신의 tx만으로는 불가"는 유지 |
| `others_since_last` 범위 | client 전역 + 도메인/kind 집계 | client × channel | **client 전역 + `by_channel` 내역** | E8 |
| worker 재기동 후 채널 | 도메인 선언 `reopen_after_crash: None/RxOnly` | RX·상태 일률 복구 | **Opus** | E7 |
| viewer 자격 | 웹 viewer + `viewer-tty`(R3/R4·완화·해제 불가) | 웹 viewer만(TTY는 증명 아님), 강한 경계는 후속 | **M1 = 웹 viewer만(Sol)**, TUI가 남는 도메인이 생기면 그때 `viewer-tty` 검토 | uart-core #5가 브라우저로 전환. 내 안은 표·본문 권한 불일치도 있음(6절) |
| 즉시 check 불일치 | CR-C12: exit 3 확장(`E_NO_MATCH` 등) | 보류, exit 0 + `matched:false` | **exit 3 확장 CR을 메인에 올림** | 세 팀 요구 동일, 장치 오류 7·NRC 10과 구분 유지 |

---

## 5. 합의 가능한 부분 (그대로 plan.md에)

- 사용자에게 `loomd` 하나, 벤더 DLL·블로킹·타이밍 경로는 worker, UDS 세션 관리자는 gateway(전용 런타임 + 지터 관문), seed-key는 별도 helper worker(32비트 포함, 2 s 상한). C4.1: DoIP는 OS NIC.
- worker 2 물리 lane, MessagePack(bin bytes), 바이트 credit, credit 없을 때 장치 RX는 막지 않고 화면 미러만 드롭, 기록은 worker가 직접.
- 순번 두 층, 허브 단일 sequencer의 원자적 snapshot + `after_seq`, 구독 필터로 빠진 seq는 손실 아님, 손실 3층(`display/record/source.gap`) 분리.
- 우선 제어 큐: 정상 시 드롭 금지, 넘치면 연결 해제 + 재snapshot(무한 큐 금지).
- 쓰기 파이프라인은 코어 단일 게이트 + 도메인 동기 훅 3개(≤ 1 ms·무 I/O) + worker 최종 TX 가드. 잡형은 start/update/renew/stop만 게이트. 대화형 입력은 lease 1회 판정 + 줄/1 s 감사 묶음.
- STOP은 agent·script·job latch, 사용자 입력 허용, 출력 유지, 사용자 STOP 후 finally 자동 실행 0, safe_state 복구는 인터록·deny 통과.
- 기록·감사 불능 시 agent·script R1+ 차단, 사용자 입력 허용, stderr 폴백.
- JSON 제어 기본 + 고속 데이터만 협상 binary, 대용량은 같은 오리진 HTTP GET, 웹 자산 임베드.
- Rust serde → schemars → JSON Schema → TS/Python, 생성물 커밋 + CI drift, u64 = JSON 10진 문자열.
- 자연어 해석은 에이전트·스킬, 코어는 세션·마커·증거 재검증·보고 데이터·선언형 runner, 임의 eval·mutation 자동 재시도 없음.
- 3 OS sim CI와 Windows/Linux 실HW·24 h soak은 별개 관문, macOS 수치로 대체하지 않음.

---

## 6. 자기 초안(r1-opus.md) 수정 사항 — 인정하는 것

| # | 내 초안의 문제 | R3에 반영할 방향 |
|---|---|---|
| S1 | 구독 큐 64연결×32구독×3 MiB를 합산하지 않음(예산 설명과 불일치) | Sol의 전체 admission 체계로 교체(4절) |
| S2 | stream 인덱스를 "세그먼트에서 재구축 가능"이라 함 — worker 원본에는 stream_seq가 없어 교차 producer 순서를 복원할 수 없음 | gateway **정본 mapping 로그**(durable) + 범위 밖은 source ref 조회/uncertain |
| S3 | 발행 대기 50 ms 초과 시 `publish_lag` 경고로 응답 — publish-before-reply 위반 | 허브 입력은 같은 프로세스 enqueue이므로 **우선 이벤트에 예약 용량을 두고 enqueue 완료 후 응답**(타임아웃 경로 제거). 렌더 ACK와는 분리(Sol) |
| S4 | 저널 큐 넘침(record.gap 발생)을 degraded로 두고 agent TX 허용 | 실제 원본 손실 = A12 기록 불능으로 agent·script R1+ 차단, 단순 backlog 경고와 구분 |
| S5 | once 승인을 backend 후 consume — 동시 2요청 race | presend 전 **원자 reserve**, 제출 시 확정, 미제출 rollback, outcome_unknown은 재사용 불가. idempotency도 pending 예약 |
| S6 | STOP 시 latch.json fsync 후 worker 전달 — 디스크 정지 시 STOP 지연 | 메모리 latch → worker 전달 → 지속화는 독립. 지속화 실패는 경고, 재시작 시 불명이면 latched로 시작 |
| S7 | 정본을 msgpack 세그먼트로 바꾸며 D7 변경 CR을 빠뜨림 | M1 D7 유지, binary는 M2 CR(4절) |
| S8 | `crc32c` 또는 `crc32fast` — 다른 다항식(CRC32C vs CRC32/IEEE) | 하나로 고정 + 헤더에 알고리즘 id |
| S9 | Windows Job 메모리 상한을 RSS hard cap처럼 씀 | commit 한계로 표기, working set/PrivateUsage 별도 감시(Sol) |
| S10 | opus-repr 스파이크의 "JSON 1.05 M ev/s → 10배 여유"를 허브·소켓 경로로 확대 해석 | 8 B payload 인코딩 루프일 뿐 — 표현을 "인코딩 비용은 병목 아님"으로 축소. hex 디코더 panic 수정 전 복사 금지 |
| S11 | `presend_check`가 trait에선 동기, §10.3 의사코드에선 `.await` | 동기로 통일 |
| S12 | 보존 GC가 세션 단위 — 24 h 열린 데몬 세션 안 sealed 세그먼트를 못 지움 | 활성 파일만 제외, 증거 세그먼트만 pin, rolling retention |
| S13 | PASS에 device 증거 ≥ 1 일괄 필수 — 정당한 부재 PASS를 막음 | witness 유형별 규칙(4절) + plan expect hash 결속 |
| S14 | UI용 P0 fixture replay stub gateway(core-ui U7) 누락 | P0 후반 산출물에 추가 |
| S15 | `viewer-tty` 표(§3.4)는 STOP 가능·해제 불명, §10.6은 해제 불가 — 권한 불일치. uart-core #5는 브라우저로 전환 | M1은 웹 viewer만, STOP 해제·R3/R4·완화·worker 재시작 = 웹 viewer 전용으로 표 하나에 통일 |
| S16 | `core.clock.ping`/`core.ui.latency_report` 이름 | uart-core v0.2가 채택한 `core.time`·`core.metrics.report`로 |
| S17 | §3.1 "VN5640 이더넷 위 DoIP → Vector worker 안 smoltcp" 행 | C4.1로 삭제. UDS DoIP = gateway 모듈의 OS TCP 소켓 |
| S18 | §0.2 홉 p95 합 60 ms를 종단 보장처럼 표기 | "예산 합"으로 명시, 종단 분위수는 하네스로(Sol 표현 채택) |

---

## 7. 리드에게 남은 사실 확인 질문

1. **Windows 감사 fsync 비용**: 같은 감사 레코드로 `FlushFileBuffers` 1회 p50/p99(SSD, Defender 켠 상태). R2+ intent durable / R1 group 정책의 가부가 여기서 갈린다(5–10분 스파이크).
2. **JSONL 정본 디스크 산술 확인**: 실제 full-schema 레코드 크기 × 정상 프로파일(5k fps) × zstd 실압축률(규칙적 합성 데이터 말고 실제 CAN 로그 샘플). 20 GiB / 30일 기본값을 유지할지 메인에 올릴 근거.
3. **증거 REF의 epoch**: `test-domains`의 `<domain>:<channel>@seq:a..b`에 stream_epoch를 넣을지(`@e3:seq:…`), 세션 바인딩으로 풀지 TEST와 한 줄 합의.
4. **exit 3 확장**: 메인이 CR(C-11/#15)을 받을지 — 세 팀의 스킬 분기가 이 결정에 걸려 있다.
5. **M1 재부착(can-core W6)**: Sol은 M1, 나는 M2. "M1 = gateway 사망 시 worker latch + orphan 후 종료"로 갈지 결정 필요(기록 단절 허용 여부).

---

## 8. Sol 리뷰(r2-sol-review.md)에 대한 의견

- **동의**: F1(구독 예산 합산), F3(stream mapping 복원 불가), F4(publish_lag는 순서 위반), F5(record.gap = A12), F6(승인 원자 예약), F7(STOP을 디스크 뒤에 두지 않기), F8(D7 CR 누락), F9(CRC 다항식), F10(Job = commit), F13(hex 디코더 panic), F14(동기/비동기 불일치), F15(세션 단위 GC) — 모두 6절에 반영했다. O5 반례(정당한 부재 PASS)와 plan expect 결속도 수용한다.
- **보충**: Sol 리뷰는 자기 crate 버전 오류를 zstd만 정정했는데, 같은 원인(`newest_version` 사용)으로 **fastwebsockets 0.8.1(→0.10.0)·process-wrap 8.2.2(→10.0.1)**도 틀렸다(2절 E2–E4). 수집 스크립트를 `max_stable_version` 기준으로 고치면 함께 해결된다.
- **보충**: F4의 해법으로 "관측 미완료 결과를 따로 반환"보다, 허브 enqueue가 같은 프로세스라는 점을 이용해 **우선 이벤트 예약 용량 + enqueue 후 응답**으로 타임아웃 경로 자체를 없애는 쪽을 권한다(S3).
- **부분 반대**: O5의 "device 증거 규칙"을 버리지는 말 것. 부재 PASS는 coverage witness로 허용하되, **에이전트 자신의 tx 이벤트만으로 DUT 동작을 증명하는 PASS는 계속 거절**해야 환각 방지 목적이 산다.
- **부분 반대**: Sol 리뷰는 CLI 목표를 "어느 쪽도 미측정"이라 했으나 이제 고정 비용 3.3 ms 실측이 있다(macOS). 목표 논쟁보다 **감사 fsync·worker 홉 분해 게이트**가 필요하다(4절 CLI 행).
- **동의**: P0 첫 주 범위 축소(타입·trait compile-only → fixture → replay gateway)와 FakeGateway가 제품 허브·정책을 선행 구현하지 않게 하는 것.
