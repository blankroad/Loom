# ROOT 통합 리뷰 — 수정 요청 기록 / 최종 수용

2026-11-14, worker.rs 작성 직후 정적 검토. 실행 검증 결과가 아니며 관련 수정 후 실제 엔진/오류 테스트 필요.

1. **P1 annotation reader→batcher queue 무제한**: Capture::run `mpsc::channel::<Annotation>()`, read_lines tx.send, batch_annotations RateGuard가 소비자에만 있다. 빠른/이상 decoder 출력이 rate limit 전에 메모리를 무한 점유할 수 있다. byte/row bounded queue + try_send 드롭계수 또는 읽기단 rate limit 필요.
2. **P1 annotation batch byte 상한 부재**: 4096byte줄 ×256rows는 JSON escaping 이후 1MiB를 넘을 수 있다. IPC/WS 및 journal64KiB bounded read와 충돌. rows 수와 함께 serialized payload byte 상한(예 48KiB) 유지; 개별행 최악escaping도 포함.
3. **P1 STOP drain 중 tee 실패를 무시**: acquisition drain에서 `keep` immutable, pump 오류를 주석만 달고 계속한다. queue 초과때 보내지 않은 chunk 뒤에 queue가 줄면 나중 chunk를 다시 보내 sample축 구멍이 생길 수 있고 최종user_stop성공으로 남는다. 최초실패시 failed/error로전환하고 이후 decoderfeed중단(또는 gap없는 엄격종료), 실패숨기지말것.
4. **P1 process group 만들지만 group kill하지 않음**: spawn process_group(0), sigint는 positivePID, kill_wait child.kill만. decoder/helper descendants가 pipe를 잡고 있으면 reader/stderr join이영구대기 가능. 외부kill(1) fixedargv로 음수PGID대상 signal하고 부모wait/reap, probe process도동일관리가필요. 정상CLI호환 영향확인. 실제descendant-holding-pipe fixture로STOP/reap기한검증.

추가: thread spawn `.ok()` 실패는기능을조용히제거하지않고 capturefailed/cleanup으로보내는것이적절. 최소아래경로들은실패를감지해야함: decoderstdoutreader, stdinwriter, acquisitionreader, annotationbatcher.

5. **P1 raw edge event도 64KiB bounded read에 맞출 것**: 4096 edges × 장시간 sample decimal 길이(최대11자리) + mask/JSON overhead는 64KiB를 넘을 수 있다. JSON byte cap으로 prefix를 보관하고 나머지는 dropped로계수하거나 이번 MAX_EDGES_PER_EVENT를2048로낮춰 유효설정 최악sample에서도payload<48KiB보장. 계약4096은상한이므로작게구현해도됨. 경계값 test를 사용하고 cap은 annotation만이아니라 event-envelope까지포함.

## 최종 판정

위 항목은 backend/model/worker 수정과29개 Rust 검사로 확인했다. 실제 UI 검증에서 찾은 zoom·축·대비·snapshot 최신 선택도 수정하고591개 UI 검사 및 최종 native 동시 대조를 통과했다. 해결 근거와 범위는 BACKEND-STATUS.md 및 STATUS.md를 따른다. 실물 장비 검증은 별도이며 완료로 주장하지 않는다.
