docs/plan/02-core-principle.md 준수.

# R1 — ui-sol 지시

먼저 `docs/plan/ui/tasks/r1-common.md`를 읽고 그대로 따른다. 출력: `docs/plan/ui/r1-sol.md` (한국어, 15절 + §0 핵심 루프). 스파이크: `spikes/ui/sol/`.

## 당신의 관점 (무게를 둘 곳)

**기술 조사(실제 문서·레지스트리·소스 확인) · 실현 가능성 · 타이밍/성능 · 테스트/시뮬레이션.** 화면 설계는 ui-opus가 맡으니 와이어프레임은 최소로.

1. **프레임워크 비교**: React / Preact / SolidJS / Svelte 5 등 — 고빈도 갱신 성능, 번들 크기, 생태계(가상 스크롤·도킹 레이아웃·xterm 래퍼), 장기 유지보수. 버전·라이선스 확인.
2. **고속 트레이스 표**: 수만 행/s 유입 + 수백만 행 보관에서 가상 스크롤. 후보: DOM 가상화(TanStack Virtual 등) vs 캔버스 그리드(Glide Data Grid 등) vs 자체 캔버스. Fixed(ID별 덮어쓰기) 모드 포함.
3. **플롯**: uPlot vs WebGL 계열(webgl-plot, regl 기반 등) vs 기타 — 다중 신호·10^7 점·min/max 다운샘플·라이브 스크롤·커서. 라이선스·유지 상태.
4. **터미널**: xterm.js 현재 버전, WebGL/Canvas 렌더러 애드온, 원시 키 입력(Ctrl-C, Ctrl-] 접두키 충돌), 대량 출력 처리량, 정제 패스스루(UART 계획 §5.2)와의 관계.
5. **레이아웃 도킹**: dockview, FlexLayout, golden-layout 등.
6. **전송 형식**: JSON vs MessagePack vs 열 단위 바이너리(Arrow IPC, 자체 열 프레임) over WebSocket — 브라우저 디코드 비용, Web Worker 디코드, SharedArrayBuffer(COOP/COEP 필요 여부, 127.0.0.1에서 동작), 압축 여부. D2(JSON-RPC 2.0 over WS, 64 KiB 배치)와 공존 방법 제안.
7. **스키마 → TS 타입**: Rust serde 타입 → JSON Schema(schemars) → TS(json-schema-to-typescript 등) 경로와 ts-rs/specta/typeshare 직접 경로 비교.
8. **배포**: 정적 자산을 Rust 바이너리에 내장(rust-embed, include_dir 등), 오프라인. Tauri 2 셸의 득실(Windows WebView2·Linux WebKitGTK 렌더 성능 차이 포함).
9. **스파이크 (필수 1개, 가능하면 2개)** — `spikes/ui/sol/`:
   - S1: **10k 프레임/s CAN 트레이스 렌더링 측정.** 생성기(Node/bun WS 서버 또는 Rust tokio — 선택) → 브라우저(headless Chromium) 트레이스 표 + 링 버퍼. 측정: 생성 시각→렌더 완료 지연 p50/p95/p99, 프레임 시간, JSON vs 바이너리 디코드 비용, 30분(가능하면) 힙 증가. 후보 2개(예: DOM 가상화 vs 캔버스) 비교면 더 좋음.
   - S2(선택): 다중 신호 라이브 플롯(uPlot) 또는 xterm.js 대량 출력 처리량.
   - 결과는 `spikes/ui/sol/RESULTS.md`에 수치·환경(머신, 브라우저 버전)·재현 방법과 함께. 계획서에는 요약.
10. **테스트 전략**: HW 없는 CI에서 브라우저 E2E(Playwright 등) + 지연 측정 하네스 + 고부하 정직성 시험(gap 배지가 실제로 뜨는지) + 3-OS, 시뮬레이션 백엔드와의 결합.
11. 브라우저 쪽 지연 예산 분해와 시계 정렬 방법(데몬 mono ↔ 브라우저 performance.now)의 구체안.

로컬 설치만(`spikes/ui/sol/node_modules`, `PLAYWRIGHT_BROWSERS_PATH=spikes/ui/sol/.browsers`). 네트워크 사용 허용.
