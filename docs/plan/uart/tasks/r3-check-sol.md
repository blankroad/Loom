docs/plan/02-core-principle.md 준수 (최우선, 브리프보다 우선)

# R3 사실 확인 — uart-sol (짧게, 15분 이내)

리드가 plan.md를 통합 중이다. 아래만 확인해서 docs/plan/uart/r3-facts-sol.md 에 결과 표(항목/결과/재현 명령/확신도)로 적어라. 스파이크는 spikes/uart/ 안에서만, 전역 설치 금지(spikes/uart/.venv 또는 uv run --no-project --with 사용).

1. `regex` 패키지(PyPI 최신)의 `timeout=` 인자가 폭주 패턴에서 실제로 발동하는가? 예: `regex.search(r'(a+)+$', 'a'*32+'!', timeout=0.05)` 와 Opus가 쓴 `^(\w+\s?)*$` 계열. 발동 시 예외 타입, 실제 경과 시간(여러 번 측정 중앙값). 발동하지 않는 패턴이 있으면 그 예.
2. `regex`의 `concurrent=True`가 매칭 중 GIL을 풀어 다른 스레드(예: 10 ms 주기 카운터 스레드)가 계속 도는지 측정.
3. 결론 한 줄: "expect 매처를 전용 스레드 + regex(timeout, concurrent=True) + literal 기본으로 두면 이벤트 루프/뷰어가 멈추지 않는다"가 성립하는지, 아니면 subprocess 격리가 여전히 필요한지.

완료 시 화면에 "R3 CHECK DONE: docs/plan/uart/r3-facts-sol.md" 한 줄만 출력하고 멈춘다.
