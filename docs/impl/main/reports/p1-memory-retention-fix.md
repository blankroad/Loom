# P1 실제 도메인 RSS 관문: 실패와 수정

첫 측정은 1800.02초, 오류 0, 워커 재시작 0이었으나 daemon의 워밍업 제외 앞·뒤 5분 RSS 중앙값 증가가 **68.015625 MiB**로 실패했다. 워커 5개는 각각 -1.28~-0.36 MiB로 통과했다. 원본 전체 표본은 `bench/results/p1-real-sim-30m-attempt1.json`에 보존했다.

메인 수정:

- per-channel 이벤트를 축출해도 남던 전역 `ring_order`를 `(stream seq → channel)` 순서 인덱스로 교체했다. 보관 중인 이벤트와 1:1로 관리하고, 전역 예산 초과 때 실제 최고령 이벤트부터 제거한다. 종전 압축은 뜨거운 채널의 오래된 항목을 제거하지 않아 누적을 막지 못했다.
- 링 예산에 JSON 문자열 외의 PublishedRecord/Value 트리, 문자열·metrics·quality, 순서 인덱스 소유 비용을 보수적으로 과금한다. 4/32 MiB 상수는 유지한다. 이는 allocator RSS의 정확한 계측값이 아니라 논리 보관 예산이다. 라이브 보관 깊이는 이전 JSON 전용 예산보다 줄 수 있으며 원본 저널과 축출 구간의 coverage 처리는 유지한다.
- `Notable`의 65,536건 상한을 유지하면서 반복되는 client/kind/channel 문자열을 최대 4,096키 캐시에서 공유한다. `others_since_last`의 카운트와 커서 의미는 그대로다.
- core.status에 daemon self RSS, hub cache의 실제 보관 레코드·순서 인덱스 수·JSON 바이트·과금 바이트를 표시한다. worker RSS는 self health 보고와 나이/가용성을 함께 사용한다.

1,000개 × 32 KiB 이벤트를 뜨거운 채널로 보내고 오래된 별도 채널을 보존하는 실제 hub 회귀를 추가했다. 순서 인덱스와 현재 보관 레코드 수가 같고 예산이 지켜지는지 확인한다. 로그: `/private/tmp/loom-p1-ring-heap-budget-regression.log`, `/private/tmp/loom-p1-heap-budget-gateway.log`, `/private/tmp/loom-p1-heap-budget-clippy.log`.

재측정은 동일 UART/CAN/UDS/Ethernet/PSU sim 부하와 두 WS viewer를 사용한다. 30분·최초120초 제외·앞뒤5분 중앙값·coverage 90% 이상·증가 10 MiB 이하 기준과 부하율은 변경하지 않는다. 새 실행의 source/binary 식별자와 최종 판정은 수용 보고에 추가한다. 브라우저 렌더는 이 메모리 시험에 포함하지 않는다.

독립 조사: `p1-daemon-memory-audit.md`. 조사 보고의 초기 배율 가설과 측정 binary/현재 소스 차이는 해당 문서의 메인 정정 절을 함께 읽는다. 메인 구현·자체 리뷰이며 메모리 수정에 대한 독립 코드 리뷰는 아직 없다.

수정 트리의 gateway 전체 회귀(UDS STOP·실제 TC-1 포함), daemon 9/hub 6, clippy all-targets -D warnings 및 format 검사를 수행했다. 최초 메타데이터 수정 단계의 gateway+CAN 통합도 통과했다. 2회차 시작 전 짧은 actual-domain probe는 RSS 및 cache telemetry 수집 경로만 확인하며 30분 합격으로 세지 않는다. 코드 독립 리뷰는 tasks/p1-memory-retention-review.md로 별도 배정했다.
