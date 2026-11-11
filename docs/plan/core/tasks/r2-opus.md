docs/plan/02-core-principle.md 준수

# CORE R2 — core-opus

`docs/plan/core/tasks/r2-common.md`를 읽고 따른다.
- 리뷰 대상: `docs/plan/core/r1-sol.md` (+ `spikes/core/` 스파이크 결과와 README, `_xteam/core-ui.md`의 core 의견 R1 Sol 절)
- 비교용 자기 초안: `docs/plan/core/r1-opus.md`
- 출력: `docs/plan/core/r2-opus-review.md`
- 특히 봐야 할 것: Sol 초안의 아키텍처·안전 파이프라인·UX·스키마 계약이 02 원칙(사용자 화면이 에이전트와 같은 스트림, 개입 경로)과 마스터 D/A/B/C 결정을 지키는지, 스파이크 수치가 결론을 실제로 뒷받침하는지(측정 조건·macOS 한계).

## 재개 메모 (재부팅 후, 2026-11-10 21:xx)
- 이전 R2 작업은 파일 시스템 정지로 중단됐다. 리뷰 파일이 없으면 처음부터, 메모가 남아 있으면 이어서 쓴다.
- 프로젝트 경로가 `/Users/ctmctm/workspace/Loom`으로 바뀌었다(옛 경로는 링크). `docs/plan/05-resume-after-reboot.md`를 먼저 읽을 것.
- 새 결정 C4.1: DoIP = PC 랜카드 + 미디어 컨버터(OS TCP/IP). VN5640은 CAN FD 전용, XL 이더넷·smoltcp는 범위 밖. 리뷰에서 이 경로를 전제로 판단한다.
- core-sol의 리뷰(`docs/plan/core/r2-sol-review.md`)는 이미 나왔다. 읽어도 되지만 내 리뷰는 독립적으로 쓰고, 동의·반박이 있으면 별도 절 "Sol 리뷰에 대한 의견"으로 짧게 덧붙인다.
- 스파이크를 돌리면 10분 이내로. 파일 읽기가 30초 넘게 멈추면 기다리지 말고 리뷰에 그 사실을 적고 끝낸다.
