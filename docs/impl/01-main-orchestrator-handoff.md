# 메인 오케스트레이터 인계 (Claude Opus 5.5 → Codex Sol 6.1, 2026-11-11 19:30)

당신은 이제 Loom 프로젝트의 **메인 오케스트레이터**다. 사용자 지시로 역할을 넘겨받았다. 사용자와는 **한국어로, 짧고 쉽게** 말한다(결과 먼저, 전문 용어·자체 약어 최소화).

## 0. 반드시 먼저 읽기
1. `docs/plan/02-core-principle.md` — 최우선 원칙: **에이전트가 장비에 직접 접근해 읽고 쓰고, 사용자는 같은 데이터를 같은 순간 UI에서 보고 개입한다.** 모든 관문 판정의 기준.
2. `docs/plan/00-master-plan.md` v2.2 — 결정 우선순위 **F > E > C > B > A > D**. 특히 §2.3 C(Rust 코어·UI 1급·통합 테스트), §2.4 E(계약 판정), §2.5 F1(기본 정책 = 모두 자동)·F2(이더넷 1차 포함), §3.0 단계.
3. `docs/impl/00-impl-playbook.md` — 팀 구현 규칙(worktree, 소유 경계, 품질 기준, 커밋 꼬리말, 경로·권한 주의 §6).
4. 각 팀 `docs/impl/<comp>/STATUS.md`와 `docs/plan/<comp>/plan.md`(필요할 때).

## 1. 프로젝트 한 줄
Loom = 에이전트 스킬 + Rust 데몬(`loomd`)·CLI(`loom`) + 웹 UI. UART/COM, CAN(PCAN-USB FD, Vector VN5640 CAN FD), UDS(DoIP over PC NIC + 미디어 컨버터, CAN-TP), 이더넷 패킷(tshark/Wireshark식), Saleae LA, DLT 등을 에이전트가 직접 읽고 쓰고, 사용자는 웹 UI에서 실시간으로 본다. TC·자연어 기반 혼합 통합 테스트를 에이전트가 스킬로 실행(증거 기반 판정).

## 2. 사용자 (중요한 선호)
- 자동차 진단·임베디드 개발자. Windows·Linux 실HW, macOS는 개발. 사내 기존 UI 도구를 밖에서 고칠 수 없어 **Rust 전환 + 에이전트에게 맡기는 폭 확대**가 목적.
- **Python은 대안이 없을 때만**(CANoe COM 정도). 코어는 Rust — Python 코어가 느리고 메모리로 죽었던 경험.
- 안전 기본값 **모두 자동**(R0–R4). 위험은 테스트하는 사람이 `loom.toml`로 설정. 화면 표시·감사·STOP은 유지.
- 장비: Vector VN5640(CAN FD만, XL 이더넷 안 씀) + CANoe, PEAK PCAN-USB FD, USB-시리얼, Saleae LA. DoIP는 PC 랜카드 + 미디어 컨버터.
- UI 시험·에이전트 화면 확인은 Playwright. 브라우저 Chrome. 24 h soak 머신은 HP ZBook Fury 16 / Xeon 32 GiB(자원 걱정 없음, 누수만 본다).
- 원격으로 일할 때가 많다. 결정이 필요하면 추천안을 먼저 제시.

## 3. herdr 팀 구성 (세션 `loom`, 항상 `herdr --session loom ...`)
| 워크스페이스 | 리드(Claude Opus 5.5, auto 모드) | 워커 opus(Claude) | 워커 sol(Codex, `-a never`) |
|---|---|---|---|
| w2 UART | uart-lead `64a8a95b-17b9-4ca9-97bc-fbd12948ab97` | uart-opus `b86a06de-3397-4e93-8cf5-70017841ff94` | uart-sol `01a1c09e-70b8-73a1-b8ae-0cd8b306bba3` |
| w3 CAN | can-lead `7cea7c6b-b21a-44a1-abad-b9bc856db970` | can-opus `472ad4ce-69de-4a98-9a02-ab39d8f860a3` | can-sol `01a1c09e-bbb9-7d42-b4b2-83155374a3f6` |
| w4 UDS | uds-lead `1a6cf52f-186a-4b7e-9e7c-f3cbd6fc304f` | uds-opus `244ec16b-7eca-487d-a3e3-86923d982fc3` | uds-sol `01a1c09e-bbb8-7292-91be-17066827c9e2` |
| w5 EXPLORE (eth + test 구현) | explore-lead `f0629fb8-832b-444c-95bf-1cdc5d8a39f3` | explore-opus `db323c23-e8cd-44f6-935c-7b63cc2f83d4` | explore-sol `01a1c0a4-fa52-7211-a337-781dc8ac0559` |
| w6 CORE | core-lead `0a0d1b29-ebc5-458b-9a18-551ecf62dde8` | core-opus `7f119d4f-a0e9-4c64-ad57-b265869e6620` | core-sol `01a1c28b-2339-7ea0-8ec7-ecc429c78a9c` |
| w7 UI | ui-lead `5859124a-8325-4c01-9837-9161827b8ccd` | ui-opus `603c9fb5-f822-4090-b3be-7e1787a6e857` | ui-sol `01a1c28b-2339-7021-b653-b02e22c3e9f4` |

- w1 `Loom`의 에이전트는 메인이 띄운 것이 아니다 — 건드리지 말 것. 당신은 w8 `MAIN`에 있다.
- 지시: `herdr --session loom agent prompt <name> "짧은 문장. 자세한 건 파일 경로."` — **프롬프트에 백틱·`$()` 금지**. 긴 지시는 파일로 쓰고 경로만 보낸다.
- 상태: `herdr --session loom agent list`, 화면: `herdr --session loom pane read <pane> --source visible`.
- 재시작(대화 이어받기): Claude `herdr --session loom agent start <name> --kind claude --pane <p> -- --resume <id> --model claude-opus-5-5 --permission-mode auto` / Codex `... --kind codex --pane <p> -- resume <id> -m gpt-6.1-sol -c model_reasoning_effort=high -s workspace-write -a never -c sandbox_workspace_write.network_access=true`. Codex 종료는 `/quit` 입력 + Enter 두 번. `agent_pane_busy`면 잠시 뒤 재시도. `--dangerously-skip-permissions`는 쓰지 말 것(안전 검사에 막힘).
- 다른 에이전트의 승인·질문 창은 사용자가 허용한 범위(작업 범위 안의 무해한 것)에서만 `pane send-keys`로 응답. 선택지는 숫자만으로 안 되면 Enter. 설정을 영구 변경하는 선택지는 고르지 말 것.
- 워커는 질문 창을 띄우지 않는 규칙(playbook §6). 어기면 리드에게 알린다.

## 4. 저장소·통합 절차 (당신의 핵심 일)
- 저장소 `/Users/ctmctm/workspace/Loom`(git, `main`, 원격 없음 — push 금지). 옛 경로 `~/Desktop/01_PROJECT/Loom`은 링크 — **쓰지 말 것**.
- 팀 worktree `.worktrees/<comp>` 브랜치 `<comp>/main` (core, ui, can, uart, uds, eth, test). 팀 STATUS는 루트 `docs/impl/<comp>/STATUS.md`(커밋은 메인이 정리).
- **병합 절차**: STATUS 첫 줄이 `MERGE READY: <hash>`이면
  1. `git checkout -- Cargo.lock`(테스트가 바꿨을 수 있음) → `git add -A docs && git commit`(상태 스냅샷)
  2. `git merge --no-ff <comp>/main` (메시지 꼬리말 아래 §7)
  3. `Cargo.lock`만 충돌하면 `git checkout --ours Cargo.lock && cargo metadata --format-version 1 >/dev/null && git add Cargo.lock && git commit --no-edit`. 다른 파일 충돌이면 해당 팀 리드에게 "main을 병합해 충돌을 풀고 다시 MERGE READY"를 요청.
  4. 검증: `CARGO_TARGET_DIR=~/.cache/loom-main-target cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`. UI 변경이면 `ui/`에서 팀이 쓰는 시험 명령(tsc·eslint·vitest·Playwright)도.
  5. 실패하면 병합을 되돌리지 말고 원인 팀에 수정 요청(빨간 main 방치 금지 — 빨리 고치거나 `git revert -m 1`).
  6. 통과하면 STATUS 첫 줄 앞에 `MERGED into main (<hash>, <시각>) — 이전: `를 붙이고, 모든 리드에게 "main 갱신, 병합해서 계속" 알림.
- 대기 루프: STATUS들의 `MERGE READY`/`BLOCKED`/`P1 DONE`과 `agent list`의 `blocked`를 2분 간격으로 확인하는 셸 루프를 돌리고, 신호가 오면 처리 후 다시 루프. 세션 한 턴 안에서 가능한 한 계속 돈다.

## 5. 현재 상태 (2026-11-11 19:25, main = `c32faa1`)
- `main`에 들어간 것: CORE P0(계약 v1.x, loom-proto, domain-api, worker-sdk, cli-kit, gateway·loomd fixture-replay, testkit), UI P0+P1 선행, CAN P0+P1 1차+M2a 신호, UART P0+T08, UDS P0+P1-a, ETH P1(+PSU sim). 마지막 검증: fmt·clippy·test 녹색.
- **병합 대기(당신의 첫 일)**: UI `19c6276`(P1 M1 sim·mock 기준 셸·패널·수용 스위트, `crates/loom-ui-assets`), CAN `f9887b8`(주기 송신·IG 등), UART `f7eae0e`(위험 분류·TX 취소), TEST `071f298`(.tc.md 파서·술어 AST·검증기·세션 연결).
- 진행 중: CORE P1(타팀 요청 판정 `docs/impl/requests/core-answers-p1.md`, 도메인 등록·채널·wait·UI 자산/bootstrap), UDS P1 M1, ETH M1 E2E(CORE 관문 대기).
- **P1 관문(다음 목표)**: 사용자 시연(웹 UI) — 세 도메인(UART·CAN·UDS) + eth 각각 "에이전트 송신 → 장치 응답 → 사용자 화면 → 사용자 개입", TC-1(혼합 TC)을 에이전트가 자연어로 실행·증거 판정, 30분 연속 RSS 증가 ≤ 10 MiB. 시연은 sim 위에서, Playwright로 화면 확인. 관문 통과 후 P2(첫 실HW: PCAN-USB FD Win·Linux, VN5640, 실 COM, DoIP over NIC, Saleae).

## 6. 지난 사고와 규칙
- 08:30경 옛 Desktop 경로에서 파일 열기가 커널 수준으로 멈춰 재부팅(원인 미상). 그 뒤 `~/workspace/Loom`으로 이전. 파일 읽기가 30초 넘게 멈추면 기다리지 말고 사용자에게 보고.
- 장시간 soak·벤치마크는 개발 중 10분 이내. 대형 다운로드는 프로젝트 밖(`~/.cache`).
- 5시간 사용량 한도에 에이전트들이 걸리면 풀린 뒤 이어서 한다. 한도 직전엔 중간 커밋.
- 사용자에게 길게 보고하지 말 것. 결과 먼저, 필요한 결정만 추천안과 함께.

## 7. 커밋 꼬리말 (모든 커밋)
```
Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01Hkioe2Dum77VbLisuue7eV
```
(인계 후 당신이 만든 커밋은 사용자가 다른 꼬리말을 지시하면 그것을 따른다.)

## 8. 참고 파일
- 결정·이력: `docs/plan/00-master-plan.md`, `docs/plan/_xteam/*.md`, `docs/impl/requests/*.md`.
- 이전 메인(Claude)의 메모: `~/.claude/projects/-Users-ctmctm-workspace-Loom/memory/loom-project-plan.md`(읽기 전용 참고).
