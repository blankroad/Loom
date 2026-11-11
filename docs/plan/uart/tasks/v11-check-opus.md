docs/plan/02-core-principle.md 준수 (최우선)

# v1.1 반영 검증 — uart-opus (20분 내외, 수정 금지·점검만)
리드가 docs/plan/uart/plan.md 를 v3로 고쳤다(마스터 계약 v1.1 반영, 일부 절은 통째로 교체, 나머지는 문자열 치환). 치환 누락으로 남은 **옛 표현과 내부 모순**을 찾아라:
- 옛 종료 코드 번호(구: 4=장치, 5=사용자/락, 6=정책, 7=데몬 → 신: 4=정책, 5=확인·승인, 6=사용자·락, 7=장치, 9=데몬), `gap`/`recording` 옛 이벤트 이름, written/partial/delivery, 사용자 CLI(approve/hold/input clear), 신뢰 모드(observe/interactive/strict), SafetyGate, `Ctrl-] x`=hex, `loom uart daemon`, uart.json, 심링크 배포, `--since 30s` 같은 옛 커서 문법, 기본 client `agent`.
- 교체된 절(§3.1–3.2, §3.9–3.10, §4.1, §10, §12, §15, §16.1)과 나머지 절 사이의 모순(M 배정, 작업 ID, 키, 기본값).
- SKILL.md 초안(§4.8)이 새 규칙과 맞는지.
출력: docs/plan/uart/v11-check-opus.md — 표(줄 번호 / 현재 문구 / 고칠 문구). 완료 시 "V11 DONE" 한 줄.
