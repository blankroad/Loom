# v1.1 반영 검증 — uart-opus → plan.md v3

- 작성: uart-opus, 2026-11-10. docs/plan/02-core-principle.md 준수. plan.md는 수정하지 않았다.
- 기준: `../00-master-plan.md` v1.1(D1–D14, **A1–A16 우선**), `master-review.md`(c-1~c-13).
- 표기: 줄 = plan.md 줄 번호. 심각도 **H** = 구현자가 옛 규칙대로 만들 위험 또는 수용 기준 판정 불가, **M** = 절 사이 모순, **L** = 표기 정리.
- 요약: 교체된 절(§3.1–3.2, §3.9–3.10, §4.1, §10, §12, §15, §16.1)은 대체로 정확하다. 남은 문제는 다음 다섯 가지다.
  1. 치환되지 않은 옛 값: read 상한 200줄, 개입 신호 `warnings`/`user_input_since_last_call`, 종료 코드 5, `partial`.
  2. 바뀐 위험 매핑(`sf erase`·`rm -rf` → R4)이 예시에 반영되지 않았다.
  3. 기본 client `agent`가 예시에 남아 있다.
  4. M1 수용 기준 (f)가 쓰는 "bench 채널"을 만들 수단이 M2에 있다.
  5. 교체되지 않은 결정 기록(DR-4/5/17/35/37)이 §16.1과 충돌하는데 "→ DR-M" 표시가 없다.

---

## 1. 옛 표현 잔재 (치환 누락)

| # | 줄 | 현재 문구 | 고칠 문구 | 심각도 |
|---|---|---|---|---|
| 1 | 25 | "에이전트는 다음 호출 응답의 `warnings`·`deferred_ms`·종료 코드 5로 개입 사실을 안다." | "에이전트는 다음 호출 결과의 `others_since_last`(사람용 `NOTE:` 줄)·`deferred_ms`·종료 코드 6으로 개입 사실을 안다." | H |
| 2 | 1092 (M1 ⑤) | "에이전트 응답의 개입 `warnings`(`user_input_since_last_call`)" | "모든 결과의 `others_since_last`(D5, CORE CLI 프레임워크) — UART는 user 입력 스트림 이벤트를 원천으로 제공" | H |
| 3 | 1012 (§9.3 E2E) | "→ 에이전트 다음 호출 warnings 확인" | "→ 에이전트 다음 호출의 `others_since_last{count, origins:[user]}` 확인" | M |
| 4 | 311 | "기본 `--max-lines 200`, `--max-bytes 16384`" | "기본 `--max-lines 100`, `--max-bytes 16384`, 하드 1 000행 / 64 KiB (A7)" | H |
| 5 | 331 (상한 표) | "read 출력 \| 200줄 / 16 KiB / 줄 512자 / JSON 64 KiB" | "100줄(기본) · 하드 1 000줄 / 16 KiB(기본) · 하드 64 KiB / 줄 512자" | M |
| 6 | 579 (§4.4 문법) | `[--max-lines 200]` | `[--max-lines 100]` (상한 1000) | M |
| 7 | 614 (§4.6 문법) | `run … [--max-lines 200]` | `[--max-lines 100]` | M |
| 8 | 306 | "(이미 드라이버가 받은 바이트는 취소 불가 → `partial` + `written_bytes` 공개)" | "→ `outcome_unknown` + `written_bytes` 공개" | M |
| 9 | 295 | "쓰기 도중 끊긴(partial/interrupted) send는 취소되고" | "쓰기 도중 끊긴 send(`outcome_unknown`, `written_bytes>0`)는 취소되고" | L |
| 10 | 272 | 청크 = `{seq, rx_offset?, len, mono_ns, wall_ns, dir, origin, client, generation}` | `{source_seq, rx_offset?, len, mono_ns, wall_ns, dir, origin, client_id, generation}` — 외부 `seq`(stream_seq)는 gateway가 부여 | M |
| 11 | 353 (§3.7-9) | "모든 TX 청크에 서버가 부여한 `origin`·`client`." | "`origin`·`client_id`(연결 자격으로 결정, D2)." | L |
| 12 | 299 | "(`ts_kind="host_read_complete"`)" | "(`ts.source="host_read_complete"`, `ts.quality` 동반 — D3)" | L |
| 13 | 854 (manifest) | `"clock":{"clock_id":"…","ts_kind":"host_read_complete"}` | `"clock":{"clock_id":"…","ts_source":"host_read_complete"}` | L |
| 14 | 134 (§2.0 ③) | "뷰어가 0x03을 `uart.input`(role=user)으로 보냄" | "`uart.input`(viewer 자격 → origin=user)으로 보냄" — role enum은 `agent\|viewer` | M |
| 15 | 314 | "CLI 출력 생략(truncated)과 RX 손실(gap)은 서로 다른 필드다." | "`meta.truncated`(출력 생략)와 손실 3층(`display.gap`/`record.gap`/`source.gap`)은 서로 다른 필드다." | L |
| 16 | 323 (상한 표) | "…기록 큐로 우선 전달, 화면 gap/degraded" | "…, 해당 구간 `display.gap` + 배지 `LIVE-DEGRADED`" | L |
| 17 | 326 | "구독자 큐 … \| 해당 구독 gap" | "해당 구독에만 `display.gap{cause:"slow_client"}`" | L |
| 18 | 743 | "gap/degraded로 표시(기록은 별개)" | "`display.gap` + `LIVE-DEGRADED`로 표시(기록은 별개)" | L |
| 19 | 452 / 606 / 211 | "`E_SCAN_LIMIT`(exit 8)/`display.gap`을 명시 반환", "`E_SCAN_LIMIT`/gap 반환" | 에이전트 결과에는 `E_SCAN_LIMIT` 또는 `E_CURSOR_GAP`(exit 8) + `meta.display_gap`이 아니라 `error.detail.resume_from`. `display.gap`은 **뷰어 드롭 이벤트**라 expect 결과의 이름으로 쓰지 않는다 | M |
| 20 | 273 | "화면·기록의 client 표시는 `client_name#pid`(예 `agent#4121`, …)" | 기본 client가 더 이상 `agent`가 아니다(D5). 예: `sh-7c2e#4121`(LOOM_CLIENT 없음, 셸 세션 기반 id), `claude-7f3a#4121` | M |
| 21 | 541 (status) | "cursors: agent @ seq 1903221 (12 lines behind)" | "cursors: sh-7c2e @ seq 1903221 (12 lines behind)" | L |
| 22 | 542 | "pending: expect … by agent#4121" | "by sh-7c2e#4121" | L |
| 23 | 591 | "(cursor agent → seq 41022)" | "(cursor sh-7c2e → seq 41022)" | L |
| 24 | 599 (§4.5 문법) | `[--from cursor\|now\|SPEC]` | `[--from @last\|SPEC]` (`cursor`는 옛 문법) | M |
| 25 | 278, 484 | UART 확장에 "`now` \| `start`" 포함 | A7은 "공통 문법과 겹치지 않는 접두어" 확장으로 `off:N`, `send:last`만 승인했다. `now`/`start`는 **CORE 확장 요청 목록(§15 남은 요청)** 으로 옮기거나, `seq:` 계열로 대체(`start` = `seq:0`, `now` = 현재 high-water)한다고 적는다 | M |
| 26 | 16, 34 | "UART 데몬(`loomd-uart`)이 …", "UART 데몬 1개가 모든 포트를 소유" | "`loomd`의 uart worker가 …"(D1, §3.2와 일치) | M |
| 27 | 37, 556, 905 | "확인 팝업", "승인 팝업" | "승인 검토 화면(공통 승인 패널, `Ctrl-] A`, M2)" — §5.6은 팝업이 아니라 키 비가로채기 모달 | L |
| 28 | 922 (§7 IPC) | "websockets … **M1 채택 후보** … 메인 공통 결정 우선" | "IPC 라이브러리는 CORE 소유(D2·§3.2). UART는 선택하지 않음 — 참고용" | L |
| 29 | 1277 (자체 점검) | "§12 T01–T13, 합류 T12" | "§12 T03–T13(T01·T02는 CORE로 이관), 합류 T12" | L |
| 30 | 534 (§4.2 status 문법) | `loom uart status [CH] [--json]` | `loom uart status [CH] [--request ID \| --idempotency-key K] [--json]` — 305·474가 `status --request`를 안내함 | L |

---

## 2. 교체된 절과 나머지 절 사이의 모순

| # | 줄 | 현재 문구 | 고칠 문구 | 심각도 |
|---|---|---|---|---|
| 31 | 1092 (M1 수용 (f)) vs 1040, 1118–1120 | (f) "**bench 채널**에서 R2 문자열은 confirm 없이 무송신" — 그런데 §10.1은 "`sim://`·pty = `sim`"(R2 `auto`)이고, env·r2_channel을 바꾸는 `[uart]` 정책 섹션은 **M2**(T18) | M1에 **채널 env 지정 최소 수단**을 넣는다: `open --env bench`(강화 방향이라 즉시 적용 가능, D6 "강화는 즉시") 또는 `loom.toml [uart.channel.X] env` 읽기만. T10 범위에 추가. 아니면 (f)를 "sim에서는 R2 auto, bench 동작은 단위 시험으로" 고친다 | **H** |
| 32 | 220 (U4), 807–810 (§5.6 화면), 573 (dry-run) | `sf erase …`를 **R3**로, `rm -rf /data` dry-run을 `"risk":"R3"`로 표기 | §10.1(1036)은 `sf erase`·`rm -rf`를 **R4**(매번 뷰어 승인)로 바꿨다. U4 "(R4)", 검토 화면 헤더 `· R4`, 버튼 `[Y] approve (R4: every time, scope once)`, dry-run `"risk":"R4"`. R3 예시가 필요하면 `saveenv`를 쓴다 | **H** |
| 33 | 172 (A1 주석) | "# R2: 세션 첫 1회 사용자 확인" | "# R2: bench는 확인 토큰(`loom uart confirm <tok>`) 또는 뷰어 y, sim은 auto" — 세션 단위 R2 승인은 v1.1에서 없어짐(A8) | M |
| 34 | 817 vs 1034, 1058 | §5.6 "R2는 `y`, R3는 대문자 `Y`" ↔ §10.1·§10.2 "뷰어 `Y`"(R2) | 한쪽으로 통일. 추천: R2 = 소문자 `y`, R3/R4 = 대문자 `Y`(오입력 방지 단계 차등). §10.1·§10.2 문구를 "뷰어 `y`"로 | M |
| 35 | 817, 1058 vs M1 ⑥ (1092) | R2 `r2_channel = viewer`면 "토큰 없이 뷰어만" — 뷰어 확인 UI(공통 패널 `Ctrl-] A`)는 **M2** | M1 ⑥에 "M1에서 `r2_channel=viewer` 채널의 R2는 `E_NO_APPROVER`(exit 5)"를 명시 | M |
| 36 | 351 (§3.7-7), 1092 (f2) vs 783 | 두 번째 뷰어 키 보류 + "`^] o to take over`", M1 회귀 (f2) "두 뷰어 동시 타이핑 → 섞이지 않음" ↔ §5.5 `Ctrl-] o` = **M2** | M1에서는 "다른 뷰어 키는 보류·폐기 + `writer viewer active — wait or ^] y there`" 안내만, 탈취(`o`)는 M2라고 §3.7-7에 표기. 또는 `Ctrl-] o`의 뷰어 간 탈취만 M1로 올린다 | M |
| 37 | 19 (§0) vs 1092 ④, 1113 (T11) | §0 M1 키 "`Ctrl-] q/h/v/y/?`" | "`Ctrl-] q/h/v/y/c/x/?`" (STOP `x`·input clear `c`는 M1) | L |
| 38 | 103, 141, 758 (상태줄 목업) | `… │ lag 14ms │ ^]? │` — 배지 없음 | M1 ④·§3.11-5가 요구하는 공통 배지 포함: `… │ LIVE lag 14ms │ …` (폭주 시 `LAG 1.4s`, 드롭 시 `LIVE-DEGRADED`) | L |
| 39 | 23, 272, 863 vs 892 | "에이전트 송신·사용자 송신·장치 수신은 **세션당 하나의 seq 순서**", "`seq` = 채널 전체 이벤트 순서", chunks.jsonl `"seq":"1042"` ↔ §6.4 "`seq` = gateway `stream_seq`, 원본 순번은 `source_seq`"(예시 seq 88123 / source_seq 1042) | 외부 `seq`는 **gateway 전역 stream_seq**(채널·세션 단위가 아님). 채널 내부 순서는 `source_seq`. §0-2는 "채널 안의 순서는 하나(`source_seq`), 외부 좌표는 stream_seq"로, §3.3·§6.2는 chunks.jsonl 키를 `ss`(source_seq)로 바꾸고 `seq` 설명을 수정 | M |
| 40 | 299 | "루프에서 순서대로 (1) seq 부여 (2) RingBuffer …" | "(1) worker가 `source_seq` 부여 … (5) gateway 전달 → 허브가 `stream_seq` 부여 후 팬아웃" — 팬아웃은 이제 gateway 허브 일(D2·§3.1) | M |
| 41 | 275 vs 313, 582 | §3.3 "`--page` … 서버 커서는 **ack 전까지 전진하지 않음**" ↔ §3.6·§4.4 "`--page`: 반환분만큼 커서 **전진**" | 하나로: 추천은 "`--page`는 반환분만큼 즉시 전진, 응답 유실 시 직전 `--cursor-token`으로 같은 범위 재조회(서버 커서와 무관)". ack 모델을 쓸 거면 §3.6·§4.4·T09를 함께 고친다 | M |
| 42 | 609 (§4.5 JSON) | `{"ok":true,"cmd":"expect","channel":"dut","generation":2,"data":{…},"range":{…}}` | 공통 봉투: `{"schema_version":1,"ok":true,"cmd":"uart.expect","data":{"channel":"dut","generation":2,…},"meta":{"range":{…},…},"warnings":[],"others_since_last":{…}}` | M |
| 43 | 573 (§4.3 dry-run JSON) | `{"ok":true,"cmd":"send","data":{…}}` | `"schema_version":1`, `"cmd":"uart.send"`, `meta`·`others_since_last` 포함(+ #32의 risk 수정) | L |
| 44 | 1011 vs 1093, 1117 | §9.3 "통합(CLI 골든) … 종료 코드·`--json` 스키마 골든 \| M1" ↔ M2 "`--json` 스키마 고정(골든)", T15 = P2/M2 | §9.3을 "M1: 봉투·종료 코드 스모크, M2: 동사별 스키마 골든 고정"으로 나눈다 | L |
| 45 | 1091 (M0) | "저장소 골격(`loom.core`, `loom.uart`, uv, 빈 3-OS CI)" | 저장소·`loom.core`·CI 골격은 CORE P0(마스터 §3.2). UART M0 = "`loom/uart/`·`loom/console/` 패키지 자리 + P0 부품 착수" | L |
| 46 | 674 vs 670, 701 | v0 설명: "attach 안내, **종료 코드 규칙**"을 v0에 둔다 ↔ "종료 코드·승인·`others_since_last`·STOP·뷰어 띄우기는 공통 `loom` 스킬" | v0 설명에서 "종료 코드 규칙"을 "UART 하위 코드 규칙(`dirty_line`, `E_NO_APPROVER` 대응)"으로 바꾼다. **공통 `loom` 스킬의 M1 존재**를 의존성으로 추가(#47) | M |
| 47 | 1092 (M1 (g)), 1115 (T13), §15 의존성 표 | (g) "Claude가 SKILL.md만 보고 …" — 종료 코드·승인 규칙은 공통 `loom` 스킬에 있으므로 M1에 공통 스킬이 필요한데 소유자·시점이 없음 | §15 타팀 의존성에 "CORE(또는 메인): 공통 `loom` 스킬 v0 — P1"을 추가하고, (g)를 "`loom` + `loom-uart` SKILL만 보고"로 고친다 | M |

---

## 3. SKILL.md 초안(§4.8, v1)과 새 규칙

| # | 줄 | 현재 문구 | 고칠 문구 | 심각도 |
|---|---|---|---|---|
| 48 | 706 | "After a user intervention warning, re-read state before continuing." | "When a result shows `others_since_last` with `user` (or a `NOTE:` line), re-read the port before continuing." — 개입 신호의 이름이 바뀌었다(D5) | M |
| 49 | 703 | "R2 (…) → exit 5 `E_CONFIRM_REQUIRED`: say what you will do, then `loom uart confirm <token>`." | 뒤에 한 줄 추가: "If the channel is viewer-only (no token; error says `r2_channel=viewer`), ask the user to confirm in the viewer and wait with `--wait-approval`." — vehicle·Codex 권장 경로(A8)에서 에이전트가 할 일이 없음 | M |
| 50 | 703 | "R3/R4 (flash write/erase) → ask the user to approve in the viewer (`Ctrl-] A`)…" | "R3 (flash write, saveenv) / R4 (erase, mkfs, `rm -rf`) → …" — §10.1 매핑에 맞춰 예시 분리. "`default_blocked` (efuse/OTP) → exit 4 `E_DENIED`: tell the user; it needs `loom.toml` unlock" 추가 | M |
| 51 | 674 (v0 설명) | v0에 R3/R4 안내 없음 | v0(M1)에는 "R3/R4 → exit 5 `E_NO_APPROVER`: tell the user this needs the approval panel (not available yet); do not retry" 한 줄 | M |
| 52 | 684 | "If a response warns `no_viewer_attached`, ask the user to run …" | §4.8 역할 분리(670)상 공통 `loom` 스킬 규칙이다. 중복을 남길 거면 "(see `loom` skill)"을 붙이고 UART 특화 내용(`loom uart attach <ch>`)만 남긴다 | L |
| 53 | 693 | "`expect … --timeout 60` — exit 3 on timeout" | 그대로 정확(A6: 3 = 조건 대기 timeout). 단 "pass `error.detail.resume_from` to `--from` to continue"를 덧붙인다(DR-33) | L |
| 54 | 698–699 | `--secret-env`, `LOOM_CLIENT` 안내 | 정확. 추가 권장: "`LOOM_CLIENT` unset → a per-shell id is used; set it only to share/split cursors deliberately" (기본 `agent` 시절 문구에서 바뀐 의미) | L |

---

## 4. 결정 기록(§16)과 §16.1의 충돌 — "→ DR-M" 표시 누락

§16.1은 "위 표와 충돌하면 이 표가 우선"이라고 했지만, 아래 행은 내용이 바뀌었는데도 취소선이나 대체 표시가 없다. 구현자가 §16만 읽고 옛 값을 쓸 수 있다.

| # | 줄 | 현재 문구 | 고칠 문구 | 심각도 |
|---|---|---|---|---|
| 55 | 1213 (DR-4) | "에이전트는 `--defer-timeout`까지 대기 후 **exit 5**(Opus)" | "exit 6(`E_USER_ACTIVE`)" + 행 머리에 `(v1.1: 코드 → DR-M4)` | M |
| 56 | 1214 (DR-5) | "written/partial/delivery 공개" | `~~(v1.1 → DR-M6)~~` 표시, 또는 "`sent`/`failed`/`outcome_unknown` + `written_bytes` 공개"로 | M |
| 57 | 1244 (DR-35) | "stall 시 partial/failed … written = OS 수용(선로·장치 확인 아님)" | "stall 시 `failed`/`outcome_unknown`(+`written_bytes`) … **`sent`** = 드라이버 수용(A14)" + `(→ DR-M6)` | M |
| 58 | 1226 (DR-17) | "최소 안전(R2/R3/R4 + 승인 객체, **사용자 CLI 승인**) … 뷰어 승인 화면 … M2 이후" | `~~(v1.1 → DR-M15)~~` 표시. M1 안전 = R2 확인 토큰·sim auto, R3/R4 `E_NO_APPROVER`, 사용자 CLI 승인 없음 | **H** (M1 범위 판단에 직접 영향) |
| 59 | 1246 (DR-37) | "Rules는 종료 코드 하위 코드별 분기" | `(v1.1: 공통 분기는 loom 스킬 → DR-M10)` — loom-uart에는 UART 하위 코드만 | L |
| 60 | 1220 (DR-11) | 취소 표시는 있으나 본문에 "사용자 터미널 `approve`", "`[s]` 세션 R2"가 남음 | 취소선이 있으니 그대로 둬도 되지만, 검색으로 옛 규칙이 걸리지 않게 결정 칸을 `(폐기 — DR-M7 참조)` 한 줄로 줄이는 것을 권장 | L |

---

## 5. 확인 결과 문제없음 (참고)

- 종료 코드 표(§4.1, 486–498)와 본문 사용처(164·165·295·296·348–363·378·424·452·471·527·559–560·584·604·1034·1052–1053·1058·1062–1071): **모두 신 번호와 일치**. 남은 옛 번호는 #1·#55뿐이다.
- `Ctrl-] x` = STOP, `Ctrl-] g` = hex, `Ctrl-] c` = input clear: §5.5·§5.3·§3.7·§10.3·M1·T11 모두 일치. 옛 hex `x` 잔재 없음.
- `loom uart daemon` → `loom daemon`(424·634), `uart.json`·`attach_token` 잔재 없음(382·423).
- 사용자 CLI `approve/deny/hold/unhold/input clear` 잔재 없음(629·349·398–399). 남은 것은 DR-11의 취소된 행뿐(#60).
- 신뢰 모드 `observe/interactive/strict`: §10.1(1042)의 "구 … 대체" 설명과 취소된 DR-10만 남았다(정상).
- `SafetyGate`: §16.1 "이전" 칸에만 있다(정상).
- 심링크 배포: §4.8(669) 복사 기본 + symlink 선택, DR-29 취소 표시(정상).
- 커서 공통 문법 `@last`·`-30s`: 278·484·622·692에 반영. 옛 `--since 30s`·`cursor`(기본값) 표기는 #24를 빼면 없다.
- 쓰기 상태: §3.5(303–305)·§3.7 표·§3.11 표·§9.3(1014)는 신 단계로 교체됐다. 남은 것은 #8·#9·#56·#57이다.
- 손실 이벤트 3층: §3.9(414·418)·§3.11-4/5·§6.1(839)·§6.4(889)는 정확하다.
- §12 작업 의존성(T08–T22): R4에서 지적한 누락이 모두 반영됐다(T11←T06·T09·T10, T12←T09·T10·T13, T19←T14·T17·T18, T20←T16·T17).

---

## 6. 리드 반영 우선순위

1. **H**: #31(M1 bench 채널 수단), #32(R4 재매핑 예시), #1·#2(개입 신호·종료 코드), #4(read 기본 100행), #58(DR-17 → DR-M15).
2. **M**: #3·#5–8·#10·#14·#19·#20·#24–26·#33–36·#39–42·#46–51·#55–57.
3. **L**: 나머지 표기 정리.
