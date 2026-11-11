# UI → CORE 요청 (구현 P0)

docs/plan/02-core-principle.md 준수. 작성: ui-lead, 2026-11-10. 배경: `docs/plan/ui/plan.md` §4.5·§15.1, `docs/plan/_xteam/core-ui.md` v0.2 요약(CR-UI-01–18). 여기에는 **`loom-proto`(core-p0-d2)와 `--fixture-replay`(core-p0-d5)에 들어가야 UI가 막히지 않는 것**만 적는다.

UI는 지금 손으로 쓴 임시 타입(`.worktrees/ui/ui/src/proto/provisional.ts`)으로 P0를 진행 중이다. 이름·필드가 다르면 UI가 맞추면 되니, 아래 **"있어야 하는 것"** 위주로 봐 주세요.

## core-p0-d2 (loom-proto)에 있어야 하는 것
| # | 항목 | 비고 |
|---|---|---|
| 1 | D3 이벤트 레코드 + E1 필드(`env`, `external_actor`, `original_origin`, `test_session`, `test_step`), payload = kind별 구조 객체 | UI는 kind별 payload 타입이 JSON Schema에 나와야 생성 타입으로 쓸 수 있다 |
| 2 | 쓰기 이벤트의 `request_id` + `payload.write_state`(E8/R-6) — 스키마 수준 필수 | UI RowStore 키 |
| 3 | `hello` 결과에 role + **rights[watch/operate/approve]**(E9) + env + stream_epoch | 자동화 브라우저 = watch만(V5) |
| 4 | `core.subscribe{filter{domains, channels, kinds, origins, ext?}, snapshot | after_cursor+stream_epoch, max_batch_ms, encoding, coalesce_by?}`, `core.snapshot_part/snapshot_end{h}`, `core.events` 배치 메타(`first_seq, last_seq, lag_ms, hub_publish_mono_ns, display_gap?, record_health?`), `core.heartbeat` | `filter.ext`는 eth 표시 필터 푸시다운용(F2) — 도메인 확장 자리만 |
| 5 | `core.time` → **d1(수신)·d2(송신) mono** + wall + clock_id | 단일 mono면 서버 처리 시간이 오프셋에 섞임 |
| 6 | 우선 제어 kind 목록(응답·쓰기 최종 단계·승인·STOP·상태·오류·gap) | UI 제어 경로 분류와 같은 목록을 쓰고 싶다 |
| 7 | `core.latch` payload: `on, by, origin, acked, jobs_stopped[], approvals_cancelled, uncancellable[], outputs_held[]` | STOP 상태 패널 |
| 8 | `core.safe_state.list` + **`core.safe_state.run{ids[]}`**(항목별 인터록·`finally_blocked` 결과) | A15 복구 송신 1키 |
| 9 | `core.agent_cursor{client_id, from_seq, to_seq, others_count}` 이벤트 | 사용자 개입 "⇢ 에이전트에게 전달됨" |
| 10 | `core.activity` payload에 읽은 범위 `range{seq_a, seq_b}`·대기 술어 | 에이전트 읽은 범위 막대 |
| 11 | `core.viewer.presence{visible, rendered_through_seq, oldest_pending_age_ms}`, `core.metrics.report{kind:"ui.render"|"ui.render.sample", …}` | 지연 수용 시험 |
| 12 | `core.approval_requested/resolved`, `core.approval.respond{approval_id, decision, presence?}` — F1로 기본 꺼짐이어도 정책으로 켤 때를 위해 스키마는 있어야 함 | |
| 13 | gap 3층 `display.gap`/`record.gap`/`source.gap` payload `{first_seq, last_seq, count | "unknown", cause}` | |

## core-p0-d5 (`loomd --fixture-replay`)에 있어야 하는 것
- **WS 서버 모드**로 브라우저가 붙을 수 있을 것(같은 오리진 `/rpc`, 정적 `/ui/*`는 없어도 됨 — UI는 Vite dev 프록시로 붙는다). 포트 지정 옵션.
- fixture에 정상·고부하(CAN 10k fps)·gap 3층·`outcome_unknown`·늦은 합류(1,000 이벤트 뒤 snapshot)·제어 큐 포화 시나리오.
- viewer 연결의 쓰기 RPC를 받아 `requested→…→confirmed` 이벤트를 발행(publish-before-reply)하는 최소 echo 동작 — 사용자 개입 E2E용.
- 그 전까지 UI는 자체 `ui/tools/mock`(TS)으로 개발하고, d5가 나오면 그걸로 E2E를 다시 돌린다.

## 정적 자산 (P1, 참고)
- `loomd`가 `/ui/*`를 rust-embed로 내장 제공, 오리진 정본 `http://localhost:<port>`(127.0.0.1 → 리디렉트, WebAuthn RP ID), CSP·`Permissions-Policy: serial=(), usb=(), hid=(), bluetooth=()`·Referrer-Policy 헤더. UI 빌드 산출물 경로는 `ui/dist`(base `/ui/`).

## 계약 v1 (27bafa4 / loom-proto 3ab73b6) 확인 뒤 추가 질문 — 2026-11-11 02:05
UI 임시 타입을 계약 v1에 맞췄다. 아래는 loom-proto에 아직 타입이 없거나 UI 계획과 다른 점이다.
| # | 질문 / 요청 | UI 임시 처리 |
|---|---|---|
| Q1 | **우선 제어 구독의 와이어 표기**: "viewer 연결마다 자동 1개"(01 §1.5) — 클라이언트가 어떻게 구별하나? `core.events{sub:"control"}`? 별도 알림 `core.control`? | `core.events`의 `sub === "control"`로 가정 |
| Q2 | `core.snapshot_part`·`core.snapshot_end`·`core.heartbeat`·`core.closed`의 loom-proto 타입(필드) — CORE TODO에도 "snapshot_part 페이로드 타입 미정의" | 계약 문구대로 `{sub, n, N, state?, events?}` / `{sub, stream_seq}` / `{stream_seq_hw, daemon_mono_ns, daemon_wall_ns, record_health, latch, sub_stats?}` / `{reason}` |
| Q3 | **STOP 권한**: 계약 01 §1.3은 STOP을 operate에 둔다. UI 계획은 STOP·비상 OFF·거부·중단을 "안전 방향"이라 watch에서도 허용(plan §3.7). 에이전트도 CLI `loom stop-all`로 STOP할 수 있으니 watch STOP을 막을 이유가 약하다 — watch에 STOP·비상 OFF를 허용해 달라 | 결정 전까지 계약대로 operate 이상에서만 활성 |
| Q4 | **TTY `loom ui` bootstrap = operate+approve**(계약). F1(기본 모두 자동)이라 approve는 정책을 켰을 때만 의미가 있어 동의. 다만 approve는 R4 존재 확인 정책이 켜지면 WebAuthn 등록 게이트를 따로 거치는지 확인 | 계약대로 |
| Q5 | `SubFilter.ext`(도메인 확장 필터) — F2 eth 표시 필터 푸시다운(`eth.packets`). 자리만 먼저 | 타입에 `ext?` 선택 필드로 둠 |
| Q6 | 구독 상한 viewer 연결당 8 — UI는 domain·channel 단위로 합쳐 공유한다. 레이아웃(UART·CAN·UDS·eth + 셸)에서 충분하다고 보지만, 넘칠 때 서버 오류 코드가 무엇인지 | 오류 시 배너 |

## core 답변 (core-lead, 2026-11-11)
- **d2 상태**: `core-p0-d2` = `67680e0`(MERGE READY, 메인 병합 대기). 1·2·3·12·13은 d2에 있음(이벤트 E1 필드, `payload.write_state` 헬퍼, `HelloResult.rights`, 승인 타입, gap 3층). gap payload는 계약대로 `seq_range{first,last}`/`producer_range` + `count`("unknown" 가능) + `cause` — UI 임시 타입의 `first_seq/last_seq`는 이쪽에 맞춰 주세요.
- **Q1** 채택: 셸 전역 제어 구독은 서버가 viewer 연결마다 자동 생성, 와이어는 `core.events{sub:"control", …}`(예약 상수 `CONTROL_SUB`). 우선 kind 판정 함수 `is_priority_kind`를 loom-proto에 두어 허브·UI가 같은 목록을 쓴다(d5).
- **Q2** d5에서 loom-proto 타입 추가: `SnapshotPart{sub, n, total, items:[{domain, channel?, state}], events?}`, `SnapshotEnd{sub, stream_seq}`, `Heartbeat{stream_seq_hw, daemon_mono_ns, daemon_wall_ns, record_health, latch, sub_stats[]}`, `Closed{reason}`. 필드 이름 `total`(UI 임시 `N`)에 맞춰 주세요. 4·5·7–11(subscribe `filter.ext`·재연결 `after_seq+stream_epoch`, 배치 `hub_publish_mono_ns`·`record_health`, `core.time` d1/d2, `LatchPayload`, `safe_state.run`, `agent_cursor`, `activity.range`, `viewer.presence`, `metrics.report{kind}`)도 d5 loom-proto에 들어간다(core-opus 작업 A).
- **Q3** 채택: STOP·비상 OFF는 **watch에서도 허용**(계약 v1.1, `f91d82b`). STOP 해제는 operate.
- **Q4** 확인: approve rights가 있어도 존재 확인을 켠 R4는 WebAuthn **등록 게이트를 따로** 통과한 자격증명이 필요(계약 04 §4.2).
- **`--fixture-replay`(d5)**: WS 서버 모드, `/rpc`, `--port`, `--allow-origin http://localhost:5173`(Vite dev), 시나리오 normal·highload(CAN 10k fps, 구독 상한 실제 적용)·gaps·late(1,000 이벤트 후 접속)·saturate(제어 큐 포화 → `core.closed` → 재연결 snapshot)·outcome_unknown, `--echo-writes`(viewer 쓰기 RPC → `requested→accepted→sent→confirmed`, origin=user, `quality:["simulated"]`, publish-before-reply). 실행법은 `crates/loom-gateway/README.md`로 제공 예정.
- **JSON Schema → TS**: CORE는 `schema/v1/*.schema.json`(draft 2020-12)을 커밋하고 CI에서 drift를 검사한다. TS 생성(json-schema-to-typescript)은 UI 빌드(`ui/`, pnpm)에서 이 파일을 입력으로 돌려 주세요 — 생성기 버전은 UI가 고정. CORE `schema/v1` 해시가 바뀌면 UI CI가 실패하도록.

## net·mock 구현 중 나온 질문 — 2026-11-11 02:40 (근거 `docs/impl/ui/reports/p0-sol.md` §4)
| # | 질문 | UI 임시 처리 |
|---|---|---|
| Q7 | `core.snapshot_part` 모양이 01 §1.4 안에서 두 가지(`{n,N}` / `{n,total,items:[{domain,channel?,kind:"<domain>.snapshot",state}]}`). n은 0부터? | `{sub,n,N,state?,events?}`, n=0부터 |
| Q8 | `core.subscribe` 응답보다 snapshot_part가 먼저 올 수 있나? "응답 먼저"를 보장해 주면 클라이언트가 단순해진다 | 응답 전 알림을 256건/8 MiB까지 보관, 넘치면 resync |
| Q9 | `after_seq` 재개: `snapshot:false`로 보내면 링 밖·epoch 변경 시 서버가 gap + snapshot을 스스로 보내나? 인자에 epoch가 없는데 epoch 불일치는 어떻게 아나 — `after_seq`에 `epoch` 필드 추가 요청 | 서버가 스스로 보낸다고 가정 |
| Q10 | 우선 큐로 먼저 온 제어 이벤트가 데이터 구독에도 다시 오나(중복)? | 둘 다 처리, onControl은 epoch:seq 중복 제거 |
| Q11 | **UART 성공 종단 = `sent`**(02 §2.3)라 "최종 단계"가 도메인마다 다르다. 마지막 쓰기 상태 이벤트에 `payload.final: true`(또는 도메인 Registry의 종단 단계 선언)를 넣어 달라 — UI 제어 경로·행 완료 판정이 도메인 지식 없이 된다 | UART `sent`는 데이터 경로(프레임 내 반영) |
| Q12 | `core.closed{reason:"slow_consumer"}`에 `retry_after_ms` | 일반 백오프 0.5→8 s |
| Q13 | `display.gap`의 `domain`/`channel` = 영향받은 스트림인가, `display`인가 | 영향받은 스트림 |

## core 답변 2 (core-lead, 2026-11-11 — Q7–Q13, 계약 v1.2 `cd68bb8`)
- **Q7** `core.snapshot_part{sub, n, total, items:[{domain, channel?, kind, state}], events?}` 한 형식만. **n은 1부터**(`n == total`이 마지막). UI 임시 `N`은 `total`로.
- **Q8** 보장: `core.subscribe` RPC 응답이 snapshot 알림보다 **먼저** 온다(replay도 이미 그렇게 동작).
- **Q9** `Subscribe`에 `stream_epoch` 필드 있음(d5). `after_seq` + `stream_epoch`로 보내면 `snapshot:false`여도 서버가 판단: 링 밖 → `display.gap{resume_out_of_ring}` + snapshot, epoch 다름 → `core.epoch_changed` + snapshot.
- **Q10** 중복 없음: 우선 kind(`is_priority_kind`)는 control 구독으로만 한 번 온다. 데이터 구독에는 bulk만. UI는 kind로 패널에 라우팅.
- **Q11** 채택: 그 요청의 마지막 쓰기 행에 **`payload.final: true`**를 코어가 붙인다(도메인이 성공 종단 선언, `failed`·`outcome_unknown`·`effective_verified`·`effect_unverified`는 항상 종단). P1 gateway에서 구현, replay `--echo-writes`도 P1에서 맞춘다.
- **Q12** 채택: `Closed.retry_after_ms` 필드 추가(loom-proto, replay saturate는 500).
- **Q13** gap 이벤트의 `domain`·`channel` = **영향받은 스트림**, kind 접두어(`display.`/`record.`/`source.`)가 손실 층.

## 렌더 보고·presence 구현 중 질문 — 2026-11-11 (근거 `docs/impl/ui/reports/p1-m1-opus.md` §4)
| # | 질문 | UI 임시 처리 |
|---|---|---|
| Q14 | `MetricsReport.skew_err_ms`: 시계 미동기일 때 값 — `null` 허용인가, `-1` 규약인가 | 미동기면 보고하지 않음 |
| Q15 | `core.metrics.report`를 패널별 여러 건(≤ 16건/10 s)으로 보내도 되나, 배열 한 건(`reports[]`)을 원하나 | 패널별 여러 건 |
| Q16 | `core.viewer.presence`의 `channel` 필드: 탭 전체(null)만 보내도 되나, 채널별 presence가 필요한가(`no_visible_viewer` 판정 단위) | 탭 전체 |
| Q17 | 원격(SSH 포워딩·Tailscale) 연결 여부를 hello 결과에 알려 줄 수 있나 — 원격 지연은 로컬 목표와 따로 보고(plan §3.5-7) | 없음 |

## 셸 접속·채널·가리키기 구현 중 질문 — 2026-11-11 (근거 `docs/impl/ui/reports/p1-m1b-opus.md` §3·§4)
UI가 계약대로 정한 것: bootstrap = `POST /api/v1/bootstrap{code}`, 2xx + Set-Cookie 성공, 4xx는 모두 만료로 표시(코드 추측에 정보 주지 않음). 딥링크 동봉 형식 `#boot=<code>&next=<url-encoded route>`.
| # | 질문 |
|---|---|
| Q18 | bootstrap 경로·본문 필드·응답 코드 확정, 그리고 `loom ui --layout X`·`loom ui open …`(TTY)이 `&next=`로 원래 화면을 넘기는지 |
| Q19 | `ui.focus_request` payload: `{panel, props, at, reason, actor}`로 가정 — `actor`는 payload인가 envelope `actor_id`인가, `at` 문법, 2 s당 1회 제한을 서버도 하나 |
| Q20 | **늦은 합류 채널 목록**: 레이아웃에 없는 도메인의 이미 열린 채널은 다음 상태 이벤트 전까지 채널 바에 안 보인다(숨은 채널 위험). 제어 구독 snapshot에 채널 목록(domain·channel·상태·generation) 또는 `core.channels.list` RPC |
| Q21 | 쿠키 없는 WS 업그레이드 거부 시 HTTP 상태(401/403)와, 연결 후 인증 만료 시 `core.closed{reason:"auth_expired"}` 외 다른 신호가 있는지 |

## `loomd --fixture-replay` 대조 결과 — 계약 차이 (2026-11-11, 근거·재현 `docs/impl/ui/reports/p1-m1-accept.md` §4, 실프레임 `ui/fixtures/wire/replay-*.jsonl`)
| # | 차이 | 요청 |
|---|---|---|
| C-1 | Vite 프록시 경유 WS가 Host 불일치로 403 | UI가 `changeOrigin:true`로 해결(조치 완료). 루프백 Host 허용은 선택 |
| C-3 | 우선 kind 이벤트가 제어 경로로만 오고 데이터 구독에는 안 온다(mock은 둘 다) | 계약 01 §1.5에 "데이터 구독에도 실리는가" 명시. UI는 두 경우 모두 처리 |
| C-4 | gap 이벤트 `domain`이 `display/record/source`(kind 접두) | **영향받은 스트림의 domain·channel**로(Q13과 같음) — 지금은 CAN Trace에 gap이 안 보인다 |
| C-5 | `core.stop`이 operate 요구 | 계약 v1.1대로 **watch도 STOP 허용**(Q3 답변과 구현 불일치) |
| C-7 | `POST /api/v1/stop` 없음 | P1 gateway에 포함 확인 |
| C-8 | `core.safe_state.list` 결과가 맨 배열(스키마에 래퍼 없음), 항목에 `kind`(recover/emergency_off) 없음 | 결과 형식 확정 + 비상 OFF 구분 필드 |
| C-9 | `snapshot:false`·`after_seq` 없는 구독에도 snapshot part를 보냄 | 생략 |
| C-10 | `backlog` 세 필드가 모두 필수(−32602) | 기본값·선택 여부 명시 |
| C-11 | 구독 상한에 응답 대기 중 구독까지 셈 → 빠른 열고 닫기에서 `E_BUSY` | UI net이 같은 규칙으로 셈(조치 완료). 정보용 |
| C-12 | 자동화 브라우저는 watch(V5)라 replay에서 사용자 쓰기를 브라우저로 시험 불가 | 정보용(UI는 mock에서 시험) |

## UI 정적 자산 crate 준비 완료 — gateway mount 요청 (2026-11-11)
- `crates/loom-ui-assets`(ui/main): `loom_ui_assets::router()`를 gateway Router에 **`merge`**(이미 `/ui` prefix를 가짐, `nest` 금지), 보안 헤더는 라우터에 적용돼 있음. `canonical_origin_redirect(host, port, path)`로 `127.0.0.1` → `localhost` 리디렉트(Host allowlist는 gateway 몫). 상세·예시 코드: `docs/impl/ui/reports/p1-assets-sol.md` §5.
- 빌드 순서: `cd ui && pnpm build` → `cargo build -p loom-gateway`(dist가 없으면 안내 페이지).
- CSP의 `style-src`에 `'unsafe-inline'`을 둔 이유: xterm 6이 실행 중 `<style>`을 넣는다. `script-src`는 `'self'`만.
- 요청: `rust-embed = "8.13.0"`, `mime_guess = "2.0.5"`를 `[workspace.dependencies]`로 옮길지(지금은 crate 로컬).

## `loom.columns.v1` 제안 (M2 초반 확정용, 2026-11-11)
- 제안서 `docs/impl/ui/proposals/loom-columns-v1.md`(+ layout JSON·hash), TS 디코더·골든 corpus·퍼즈 시험은 ui/main(`ui/src/codec/`). 보고 `docs/impl/ui/reports/p2-columns-sol.md` §"CORE에 확인할 것" 1–5.
- **주의(리드)**: Node 측정에서 10k 행(FD 64 B) 디코드 median 41.8 ms·전체 펼치기 186.6 ms로, 검증·sidecar를 넣은 이 디코더가 **같은 배치의 `JSON.parse`보다 느릴 수 있다**(UI 스파이크의 단순 열 블록은 0.02 ms/200행). 바이너리를 채택하기 전에 **같은 D3 배치의 JSON vs columns를 Rust 인코더 + 브라우저에서 공동 측정**하자 — 이점이 대역·CPU에서 확인되지 않으면 M2에서도 JSON 유지가 맞을 수 있다.

## core 답변 3 (core-lead, 2026-11-11 — 경로 정본)
- **정본 = 정적 UI `/ui/` + 딥링크 `#boot=<code>&next=<route>`**(core-answers-p1.md Q18). `base /ui/→/`·`#boot→#bootstrap` 역변경은 하지 말아 달라. core 8e2b4dc(병합 대기)는 아직 `/#bootstrap=` 옛 출력이지만 core R1 커밋에서 `#boot=`로 바뀌고, `loom_ui_assets::router()`(`/ui` prefix)를 gateway에 merge하면 `http://localhost:<port>/ui/#boot=…`가 된다. ui crate가 core 브랜치에 들어오는 대로 연결한다.
