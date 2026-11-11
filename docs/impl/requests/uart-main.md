# uart → main 요청

## 1. uart-sol(Codex) 세션 재시작 (2026-11-11, 구현 P0 시작 시점)
- 증상: uart-sol 화면 하단 경로가 `~/Desktop/01_PROJECT/Loom`(옛 링크 경로). core-sol·ui-sol과 같은 문제(재부팅 후 resume된 Codex 세션의 샌드박스 writable root가 옛 경로에 묶임)로 판단해 작업을 보내지 않았다.
- 요청: uart-sol을 `/Users/ctmctm/workspace/Loom/.worktrees/uart`를 cwd로 새로 시작해 달라. 그동안 Sol 몫(T03·T04·T05·T06)은 리드가 서브에이전트로 진행한다(일정 영향 작음). 재시작되면 P1 시험 하네스(T12)를 배정할 예정.

## 2. wasm32 target 설치 (선택, 급하지 않음)
- `loom-console` 정제기는 웹 패널용 WASM 빌드 대상이다(UART plan §5.4). 이 머신에 `wasm32-unknown-unknown` target이 없다(`rustup target list --installed` = aarch64-apple-darwin, x86_64-pc-windows-gnu). 전역 툴체인 변경이라 리드가 설치하지 않았다. 필요 시 메인이 `rustup target add wasm32-unknown-unknown`을 해 주면 P1에서 빌드 확인을 CI에 넣는다. UI 팀이 TS 포팅 + 골든 코퍼스를 택하면 불필요.

> 11-11 메인 공지 확인(playbook §6): Codex 워커 무승인 모드 재시작 진행 중. 재시작되면 uart-sol에 `crates/loom-uart` 분류 규칙(T10 일부)을 배정하고, 빌드 캐시는 worktree 안 `target/`(gitignore됨)을 쓰게 한다.
