# 스킬 보완 — 브라우저 실행 환경과 Codex 재확인

상태: 수정본·패치 작성/검증 완료, 설치본 반영 미완료. 메인 직접 수행·자체 검토. 전용 module workspace wB를 생성했다. 설치본 경로의 쓰기 제한은 그대로 유지했다.

## 추가한 규칙

- 실행기/유효 권한, 앱 승인, CLI 서비스 기동, 렌더링 환경의 실패를 구분한다.
- 정확한 오류로 공식 문서·upstream 사례를 검색해 관측과 추론을 나누고 조치 주체/해결 순서/성공 기준까지 반환한다.
- 대체 환경도 사용자 허용과 자체 정상 승인 경로가 필요하다. 권한 거부의 워커 우회·전역 권한 변경을 하지 않는다.
- 명시 재확인 요청이나 조건 변화 시 bounded 최소 probe부터, 성공해야 실제 수용을 실행한다.
- 실패/환경별 성능 결과를 보존하고 기준을 완화하지 않는다. 실제 UI/입력/STOP 증거가 필요하다.
- 본인 페이지·서버·control 정리와 모듈 workspace 종료까지 기록한다.

## 사용자 요청: 현재 Codex 경로 1회씩 재확인

권한은 기존 workspace-write + approval_policy=never 그대로, 기본 옵션만 사용했다. Opus 실행을 재사용해 결과를 대신하지 않았다.

| 최소 probe | 결과 | 시간 | 오류 |
|---|---|---:|---|
| Ego CLI, own TaskSpace/data URL | FAIL exit4, 페이지 생성 전 | 0.34초 | hiservices-xpcservice Connection invalid / Failed to start ego lite |
| 기본 chromium.launch | FAIL exit1, 페이지 생성 전 | 0.63초 | MachPortRendezvousServer Permission denied(1100), SIGTRAP |

전체 스모크는 재실행하지 않았다. 일반 옵션의 최소 기동이 실패했기 때문이다. 로그는 ~/.cache/loom-codex-browser-recheck/{ego,playwright}.{stdout,stderr}.log에 보존했다. 기존 Opus 정상 승인 경로의 P1 브라우저5/5 PASS는 별개 유효 결과로 유지한다.

## 검증

quick_validate.py PASS. 설치본의 임시 복제에 3파일 unified patch dry-run/apply PASS, 적용 결과와 후보 파일 바이트 일치. 기존 module workspace/foreground 규칙과 메타데이터를 유지했다. 설치 위치 ~/.codex/skills/feature-team-coding는 수정하지 않았다. 최신 전체 패치는 [feature-team-coding-workspace.patch](../skill-updates/feature-team-coding-workspace.patch)다.

## 후보 SHA256

- SKILL.md: `a689cd68e60d3fdaacbf6bf95d749e8be568643f338ed546f31374914732679a`
- references/herdr.md: `b239b79ec809c7a8996fa41669b2795a442abe1b146c76366aae0cdda7e7e1a7`
- references/browser-execution.md: `b9302330dec2d11a0a32aebd1bc0ce422de76d767f8f63c87a8522bbbee640cd`
- patch: `3bb45b4883cc9c71f3fd9ee5fba32991b204a04a02b862587b1a81aa6029d16c`

## 모듈 정리

wB/wB:t1/wB:p1은 스킬 검토 모듈용으로 생성해 보고서를 표시·확인한 뒤 close했다. Codex probe는 현재 메인 Codex exec_command의 원래 샌드박스에서 실행했으며 Herdr shell로 경로를 바꾸지 않았다. `workspace list`에서 wB 제거와 MAIN(w8) 1개/pane1 보존을 확인했다. 보고서·패치·실패 로그는 보존했다.
