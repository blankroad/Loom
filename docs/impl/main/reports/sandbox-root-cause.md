# Codex/Opus 브라우저 실행환경 차이 — 원인 조사 및 지원되는 해결안

담당: Claude Code Opus 5.5 foreground (workspace w9:p1). 기준 HEAD `b1e1595`. 읽기 전용으로 조사했다. 코드/설정/권한/skill/commit 변경, 브라우저 재실행, 같은 probe 반복은 하지 않았다.
작성 시각: 2026-11-12 05:36–05:45 KST. upstream 조회 시각: 2026-11-11T20:36Z.
상태: **검토 완료 — 원인·해결 경로 정리, workspace w9 종료 확인**

## 0. 근거 자료 (직접 연 것만)
| 자료 | 위치 / 상태 |
|---|---|
| 원본 실패 로그 | `/private/tmp/loom-playwright-launch.log` (2026-11-11 23:02 KST). 4행 launch argv에 `--no-sandbox` 포함. 8행은 `FATAL:base/apple/mach_port_rendezvous_mac.cc:159] Check failed: kr == KERN_SUCCESS. bootstrap_check_in org.chromium.Chromium.MachPortRendezvousServer.23990: Permission denied (1100)`, 19행은 `process did exit: exitCode=null, signal=SIGTRAP` |
| 동일 오류 upstream | https://github.com/openai/codex/issues/21292 — **OPEN**, 생성 2026-05-06, 갱신 2026-10-23. 같은 1100 오류가 재현된다. 작성자는 "escalated permissions에서는 같은 스크립트 성공"이라고 보고했다. 2026-10-10 댓글은 macOS 27.0.1, Codex Desktop 26.1007/CLI 0.162.0-alpha에서 재현했다. 2026-09-29 댓글은 `require_escalated` 재시도에서도 동일 거부가 났다고 보고했으며, 보고자도 실제 sandbox 밖 실행이었는지 확인하지 못했다 |
| Ego 서비스 구별 | https://github.com/citrolabs/ego-lite/issues/188 — **CLOSED**, 생성 2026-07-30, 갱신 2026-10-22. (본문의 종료 이력은 2026-08-15이며 조회 JSON에는 closedAt 필드가 없었다.) maintainer(liuwd8, 2026-08-10) 설명: CLI는 먼저 app main process에 연결한다. server service가 없으면 `ego lite.app`을 **launch**하고 singleton IPC로 service를 시작한다(`--startup-ego-browser-service`). 실패는 sandbox 안에서 app 실행이 막혀서 생긴다고 설명했다 |
| Codex Seatbelt 원본 | `openai/codex` main `392ced7d3a9160fd35d40c606359869215f50522` (2026-11-11T19:18Z), `codex-rs/sandboxing/src/` |

Codex 소스 직접 인용(위 SHA 고정):
- `seatbelt_base_policy.sbpl` L8 `(deny default)`, L11–13 `(allow process-exec)(allow process-fork)(allow signal (target same-sandbox))` (주석: "child processes inherit the policy of their parent"). mach 관련 허용은 L90 `com.apple.system.opendirectoryd.libinfo`와 L103 `com.apple.PowerManagement.control`의 **mach-lookup 2개뿐이다**.
- `seatbelt_network_policy.sbpl` L1 "when network access is enabled, these policies are added after those in seatbelt_base_policy.sbpl". L13– 는 DNS/TLS/configd 등 **mach-lookup만** 추가한다.
- `seatbelt.rs` L984–985 `dynamic_network_policy_for_network(...)`, L866 이하 writable_roots → `file-write*` 규칙. L1011 `(deny mach-lookup (xpc-service-name-prefix ""))`.
- 4개 `.sbpl`과 `seatbelt.rs` 전체에서 `mach-register` 출현 0회(grep). `(deny default)` 아래에서는 bootstrap check-in, 곧 전역 Mach 서비스 이름 등록이 허용되지 않는다.

로컬 사본·hash: `/private/tmp/claude-501/-Users-ctmctm-workspace-Loom/d99812ab-bebc-4f1c-bca4-c280d82e9b32/scratchpad/ev/` (sbpl 3개, seatbelt.rs, issue JSON, 댓글, 로그 사본, SHA). 세션 종료 후 사라질 수 있다. 위 URL+SHA로 재현할 수 있다.

## 1. 왜 실패하는가 — 확정 관측과 추론 분리

### 확정 관측
1. root Codex 세션의 유효 정책은 workspace-write + `approval_policy=never`, network enabled다.
2. 그 세션에서 Playwright 기본 `chromium.launch()`는 `bootstrap_check_in … MachPortRendezvousServer.<pid>: Permission denied (1100)` 뒤 SIGTRAP으로 끝났다. **이때 argv에 Playwright 기본 `--no-sandbox`가 이미 있었다.**
3. root Ego CLI는 `hiservices-xpcservice Connection invalid` / `Failed to start`로 실패했다.
4. root CUA `getApp`은 `Computer Use was not approved to use ego lite`로 거부됐고, 상세 사유는 없었다.
5. 같은 macOS 27.0.1 / Ego 0.6.2.9 / Playwright 1.65.0 호스트에서, Opus 5.5 foreground native CLI auto mode의 일반 개별 명령 승인으로 실행하자 Ego CLI와 기본 `chromium.launch()`가 성공했다. PW 5/5 PASS, CAN p95 83.4 ms였다(`p1-browser-final.md`). 보안 설정 변경, skip-permissions, sandbox 해제는 없었다.
6. Codex upstream Seatbelt 정책은 deny-default이고 mach-register 허용이 없다. network policy는 mach-lookup만 더한다(위 인용).

### `--no-sandbox`와 외부 Seatbelt의 차이 (사용자 지적 반영)
- `--no-sandbox`는 **Chromium 자신이** renderer 등 자식 프로세스에 거는 내부 sandbox를 끄는 플래그다. 부모 프로세스에 이미 걸린 OS 정책은 풀지 못한다.
- Codex는 명령을 `sandbox-exec`/Seatbelt 프로필 안에서 실행한다. base 주석대로 자식은 부모 정책을 상속하므로 node → chrome-headless-shell 전체가 그 밖으로 나갈 수 없다. 프로세스가 스스로 정책을 완화할 수도 없다.
- `MachPortRendezvousServer` 등록은 Chromium **browser(부모) 프로세스의 시작 단계**에서 일어난다. 자식 sandbox 여부와 무관하게 항상 필요하다. 따라서 `--no-sandbox`가 있어도 실패하고, Chromium 플래그나 채널을 바꿔도 해결되지 않는다(#21292 작성자 보고와 일치).
- 결론: 실패 지점은 Chromium 내부 sandbox가 아니다. Chromium 바깥, 명령을 감싼 macOS Seatbelt 경계다.

### 원인 판정
| 증상 | 판정 | 강도 |
|---|---|---|
| Chromium 1100 SIGTRAP | Codex 명령 실행 Seatbelt가 전역 Mach 이름 등록(bootstrap_check_in)을 허용하지 않음 | **높음**. 정책 원문에 mach-register 0회, 외부 Seatbelt 아래에서 `--no-sandbox`로도 실패, sandbox 밖 승인 실행은 성공(Opus 실측 + #21292). 단, 우리 Codex 세션의 sandbox violation log 원본은 없다 |
| Ego CLI XPC 실패 | 같은 Seatbelt의 `(deny mach-lookup (xpc-service-name-prefix ""))`, 그리고 sandbox 내 app/service launch 차단과 일치(#188 maintainer 설명) | **중간**. `hiservices-xpcservice`가 정확히 어느 규칙에 걸렸는지 로그로 확인하지 못했다. 같은 host에서 sandbox 밖 성공으로 Ego 앱 자체 결함 가능성은 낮다 |
| CUA "not approved" | **별개 계층**. Computer Use 앱 승인/자동 검토 정책이며 Seatbelt와 무관 | 확정(오류 문구). 내부 사유는 미노출 |
| Ego RAF 1 s 정지 | 실행 권한 거부와 구별되는 렌더링 환경 차이. headed 창 throttling 등 가능성 | RAF 정지와 headless 60 Hz 대비는 관측됐다. 정확한 창/OS 원인은 미확정 |

추론(미확정): 이 Codex 세션에 관리형/호스트 정책(managed requirements, 앱 측 정책)이 있는지는 확인할 수 없었다. `~/.codex/config.toml`에 명시 설정이 없고 requirements.toml 2경로가 없다는 사실이 "정책 없음"을 뜻하지는 않는다.

## 2. 즉시 적용 가능한 정상 경로와 Codex에서 계속하는 방법
**A. 즉시 경로(검증됨, 권장)**: 브라우저 실행이 필요한 작업은 Claude Code Opus 5.5 foreground auto mode에서 일반 개별 명령 승인으로 수행한다. P1에서 이미 실측 PASS한 경로다. 추가 설정은 필요 없다.

**B. Codex에서 계속하려면(사용자/관리자 조치)**
1. agent는 `approval_policy=never`를 바꿀 수 없다. 그 세션은 sandbox 밖 실행을 요청할 경로 자체가 없다. 파일 편집만으로 현재 유효 정책이 바뀐다고 가정하면 안 되며, 관리형/호스트 정책이 덮어쓸 수 있다. 현재 세션 정책은 에이전트가 변경하지 않는다.
2. 사용자/관리자가 승인 요청이 가능한 정책을 선택한다. 공식 CLI는 현재 세션에서도 `/permissions :workspace --approval-policy=on-request --reviewer=user`를 지원한다(구성이 허용할 때). 따라서 새 세션이 무조건 필요하다는 초안의 단정은 철회한다. UI/호스트가 변경을 지원하지 않으면 허용된 설정으로 새 세션을 시작하고, 관리형 requirements가 금지하면 관리자가 조정한다. Full Access 전역 적용은 전제하지 않는다. [공식 /permissions](https://learn.chatgpt.com/docs/developer-commands#update-permissions-with-permissions), [설정 검사](https://learn.chatgpt.com/docs/developer-settings#inspect-your-settings).
3. 그 세션에서 Chromium launch **한 명령만** sandbox 밖 실행(escalation)을 요청하고, 사용자가 그 명령을 개별 승인한다.
4. 성공 기준(단계별, 하나라도 실패하면 중단하고 오류 원문 기록):
   - (a) 최소 probe: `chromium.launch()` 기본값 → `page.goto('data:…')` → title 반환, exit 0, 1100 문자열 없음.
   - (b) 실제 수용: `real-smoke.config.ts` 5/5 PASS(`LOOM_REAL_SMOKE=1`), CAN p95 < 150 ms.
5. 주의: #21292의 2026-09-29 댓글은 `require_escalated` 재시도에서도 1100이 재현됐다고 보고한다. **승인 정책을 바꿔도 자동으로 해결된다고 보장하지 않는다.** (a)가 실패하면 승인된 명령이 실제로 Seatbelt 밖에서 실행됐는지부터 확인하고, 그다음 upstream 결함으로 분류한다.
6. Ego GUI 제어는 별도 계층이다. ChatGPT Settings → Computer use에서 Ego Lite 앱 접근을 승인하고, macOS 손쉬운 사용·화면 기록 권한을 확인한다(이전 보고 `browser-smoke.md` 해결 절차 절, 공식 문서 https://learn.chatgpt.com/docs/computer-use 인용 기준. 이번 세션에서 문서를 다시 열지는 않았다). 명령 승인과는 독립적으로 승인해야 한다.

사용자 permission 질문이 필요한 이유: B-2/B-3의 정책 선택과 개별 승인은 사용자나 관리자만 할 수 있다. 근거는 확정 관측 1과 Codex 정책 구조다. 경로 A로 진행하면 질문할 필요가 없다.

## 3. 제품 측 근본 개선안
**현재 config로 지원되는 것**
- 승인 정책 선택(on-request 등)과 명령별 escalation 승인. 이 방법은 해당 명령을 Seatbelt **밖**으로 보낸다. 정책 자체를 넓히지는 않는다.
- `network_access`와 `writable_roots`: 각각 network policy 추가와 `file-write*` 경로 허용일 뿐이다(seatbelt.rs L984, L866). **Mach 서비스 등록(mach-register)과 XPC lookup은 다른 sandbox operation 종류**라서 network_access가 일부 DNS용 mach-lookup을 추가하더라도 Chromium mach-register나 임의 앱 XPC 서비스를 허용하는 옵션은 아니다. 그래서 network enabled 상태에서도 실패했다.
- 워크스페이스(w9) 생성도 권한 확대가 아니다. herdr workspace는 터미널/작업 단위일 뿐이다. 그 안에서 동일한 Codex 권한 프로필·승인 정책으로 명령을 실행하면 같은 제약이 적용된다. 별도 실행기의 정책이 다를 수는 있지만 workspace 생성 자체의 효과는 아니다.

**upstream 코드 변경이 필요한 것 (openai/codex executor/sandbox)**
- 1안: `seatbelt_base_policy.sbpl`에 Chromium 이름으로 한정한 `mach-register`(필요하면 `mach-lookup`)를 허용하는 opt-in 프로필(예: browser-testing 권한). #21292 댓글에 regex 예시가 있다. 다만 이는 커뮤니티 제안이며 maintainer가 수용했다는 기록은 확인하지 못했다.
- 2안: 1100 같은 Seatbelt Mach 거부를 감지해 "sandbox 밖 실행 승인"을 명확히 안내하는 기능(#21292 기대 동작 2).
- 3안: Codex가 지원하는 브라우저 도구 경로 제공(#21292 기대 동작 3).
- Ego 측: app이 GUI 시작 시 CLI service를 미리 기동하는 옵션(#188에서 논의됐지만 미결정).
- 위 항목은 모두 upstream이 결정할 사항이다. 로컬 `.sbpl` 수정, 임의 MCP/소켓/CDP 브리지, `--no-sandbox` 계열 플래그 조작은 우회이므로 권장하지 않는다.

## 4. 권장 순서 / 담당 / 완료 기준
1. **main**: 브라우저가 필요한 기능은 경로 A(Opus foreground)로 배정한다. 완료 기준은 해당 기능의 브라우저 시험 PASS다(P1은 이미 완료).
2. **사용자/관리자**: Codex 경로가 필요할 때만 B-2 정책 선택(허용된 세션 내 변경 또는 새 세션)과 B-3 개별 승인을 진행한다. 완료 기준은 B-4(a), 이어서 (b)다.
3. **사용자**: Ego GUI 제어가 필요하면 Computer Use 앱 승인을 처리한다. 완료 기준은 실제 Loom 화면 읽기와 입력/STOP 상호작용이다.
4. **main(선택)**: #21292에 우리 재현을 추가한다(macOS 27.0.1, PW 1.65.0, `--no-sandbox` 포함 실패, sandbox 밖 승인 성공). 외부 게시이므로 사용자 확인 후 진행한다.

## 5. 한계
- 이번 세션은 sandbox violation 원본(`log show`의 `Sandbox: deny mach-register`)을 수집하지 않았다(재측정 금지). 실패 operation의 이름은 정책 원문과 오류 코드에서 추론한 것이다.
- 조사 담당은 소스 원문을 우선 사용했다. 메인은 공식 Developer commands/settings를 직접 다시 열어 세션 내 /permissions 변경 지원을 확인하고 위 재생성 필수 주장을 정정했다.
- 실행 중인 Codex 바이너리 버전과 위 main SHA가 같다고 확인하지 않았다. 다만 #21292의 2026-10-10 재현 버전과 같은 계열이다.
- 8분 한도 때문에 #21292 댓글 67행 중 앞부분과 일부만 읽었다.

## 메인 검토·작업 공간 정리

- 조사: 전용 workspace w9 / pane w9:p1 / session d99812ab-bebc-4f1c-bca4-c280d82e9b32. READY 보고 인수 후 `/exit`로 정상 종료, `herdr workspace close w9` 성공. `workspace list`에서 w9 제거와 MAIN 보존을 확인했다. 소스·보고서·로그는 삭제하지 않았다.
- 정책 원문은 upstream 고정 SHA에 대한 증거다. 현재 실행 중인 Codex와 그 SHA가 같다고 확인한 것이 아니므로 실제 실패 규칙의 완전한 입증이 아니라 원인에 대한 강한 설명으로 구별한다.
- 고정 소스: https://github.com/openai/codex/blob/392ced7d3a9160fd35d40c606359869215f50522/codex-rs/sandboxing/src/seatbelt_base_policy.sbpl#L8 및 seatbelt.rs#L1011. 메인이 보존된 원문에서 deny-default/부모 정책 상속/XPC deny 구문을 대조했다.
- 외부 이슈 게시나 실행 권한/관리 정책 변경은 수행하지 않았다. 검증된 해결 경로는 현재 Opus 정상 승인 실행이며, Codex 경로 변경은 사용자/관리자가 지원 UI에서 선택한 뒤 최소 probe로 확인해야 한다.
