# STATUS — TI Blackhawk XDS560v2 debugger survey (w2B)

- Model/session: claude-opus-5-5 (native), session `b69cea51-12e2-4404-b913-e21c602b935f`
- Loom HEAD: `d6fc6f3` · sources accessed 2026-11-14 · research only (no implementation, no device scan, no Git edits, no Ego/browser, no subagents)
- Scope: **Blackhawk XDS560v2 only** (user correction applied)
- State: **FINAL (ROOT 리뷰 4건 + 편집 패스 반영, /exit 대기)** · SURVEY.md DONE, SURVEY.html DONE, READY.json DONE

## 예비 결론 (ROOT → 사용자 전달용)
- **macOS 호스트 직결: 미지원.** TI CCS 20.4.x 릴리스 노트가 macOS에서 XDS560v2를 지원하지 않는다고 명시하고, Blackhawk XDS560v2 제품 페이지의 호스트 OS 목록에는 Windows와 Linux만 있습니다. Intel Mac과 Apple silicon 모두 해당합니다. CCS 21.0.x 노트는 이 문제를 언급하지 않으므로 지원이 추가됐다는 근거도 없습니다.
- **Windows 10/11 x64 또는 Ubuntu 22.04/24.04 x64 + CCS 21.0.x: 가능 (조건부).** CCS 20.4.x에 Blackhawk 에뮬레이터 드라이버(v9.0.0.002)가 번들되어 있고, `dtc_conf`가 `bh560v2u`/`bh560v2e` 어댑터를 지원합니다. 조건: 대상 칩이 JTAG/cJTAG를 쓰는 지원군이어야 합니다(**MSP430과 SWD 전용 칩은 불가**). Blackhawk 문서는 CCS v10까지만 명시하므로 CCS 21 조합은 실기 게이트에서 확인해야 합니다.
- **권장 경로**: x64 Windows/Linux 브리지 PC에 CCS 21.0.1을 설치하고, CCS Scripting 헤드리스(`run.sh`, Node)로 Loom 소유의 얇은 bridge 스크립트를 돌립니다. macOS loomd의 `dbg` worker는 SSH 터널(loopback TCP) 너머로 이를 호출합니다. LAN 모델(BH-XDS-560v2)이 유리합니다. VM 경로는 TI가 공식 지원 구성이 아니라고 밝힙니다. Apple silicon VM은 공식 지원이 확인되지 않아 비권장이며(불가능하다는 증거는 아님), 네이티브 x64 호스트를 권장합니다.
- **시뮬레이터**: CCS 20/21의 공식 지원을 확인하지 못했으므로 MVP 검증 수단으로 삼지 않습니다(존재하지 않는다고 단정하지는 않음). MVP는 fake backend로 진행하며, 이는 프로브 검증이 아닙니다.
- **비침습 메모리 읽기**: 디바이스와 모드에 따라 다릅니다(C28x 실시간 모드는 가능, 그 외는 보장 불가). 따라서 제안 CLI(미구현)는 `E_WOULD_HALT`를 반환하고 자동 halt는 하지 않습니다.
- **지연 지표**: 획득 지연, LLM 지연, end-to-end(요청→실제 시작, 이벤트→올바른 보고)를 모두 기록합니다.
- **reset/load/flash**: MVP에서는 제공하지 않습니다. 향후 명시 요청 action으로 제공하며 Loom 기존 자동 기본값과 설정 정책을 따릅니다(새 필수 승인 흐름 없음).

핵심 공식 출처:
1. https://software-dl.ti.com/ccs/esd/CCSv20/CCS_20_4_1/exports/CCS_20.4.1_ReleaseNote.htm (macOS XDS560v2 미지원, BlackHawk drivers 번들)
2. https://www.blackhawk-dsp.com/products/jtag-emulators/xds560v2 (Blackhawk 모델·호스트 OS Windows/Linux)
3. https://software-dl.ti.com/ccs/esd/CCSv20/CCS_21_0_0/exports/ccstudio-ide_21.0.0_releasenote.html (CCS 21 호스트 OS)
4. https://software-dl.ti.com/ccs/esd/documents/users_guide/ccs_debug-scripting.html (헤드리스 CCS Scripting)

## 사용자 확인 필요
모델 정확명(BH-XDS-560v2 USB+LAN인지 BH-USB-560v2인지, 라벨이 "USB560/LAN560"이면 v1이라 미지원) · TI 칩/보드 P/N · x64 Win/Linux PC 보유 여부 · CCS 버전/.ccxml/ELF · halt/flash 정책 등급

## 산출물
- `SURVEY.md` (한국어 본문, 매트릭스, 아키텍처, CLI 초안, 출처 29개)
- `SURVEY.html` (아키텍처 SVG, 매트릭스, 출처 링크)
- `READY.json`
