import fs from 'node:fs/promises';
import {createHash} from 'node:crypto';
const r=JSON.parse(await fs.readFile('results/benchmark.json','utf8'));
const f=(n,d=3)=>Number.isFinite(n)?n.toFixed(d):'—';
const mib=n=>f(n/1048576,2);
const table=r.runs.map(x=>`| ${x.format} | ${x.renderer}${x.fixed?' Fixed':''} | ${x.received.toLocaleString('en-US')} | ${f(x.serverStats?.generated/(x.serverStats?.elapsed_ms/1000),1)} | ${f(x.generated_to_next_raf_ms.p50)} / ${f(x.generated_to_next_raf_ms.p95)} / ${f(x.generated_to_next_raf_ms.p99)} | ${f(x.decode_ms.p95)} | ${f(x.ingest_ms.p95)} | ${f(x.draw_ms.p95)} | ${f(x.raf_interval_ms.p95)} |`).join('\n');
const wire=r.runs.map(x=>`| ${x.format}/${x.renderer}${x.fixed?'/Fixed':''} | ${f(x.serverStats.bytes/x.serverStats.generated,2)} | ${x.serverStats.maxMessage.toLocaleString('en-US')} | ${x.serverStats.dropped} | ${x.gaps} | ${x.measurementDropped} | ${x.domNodes} | ${mib(x.arrayBufferBytes)} |`).join('\n');
const terminals=(r.terminal||[]).map(t=>`| ${t.webgl?'WebGL':'DOM'} | ${t.bytes.toLocaleString('en-US')} | ${f(t.parse_ms)} | ${f(t.throughput_MiB_s)} | ${f(t.completion_to_2raf_ms)} | ${t.error||'없음'} |`).join('\n');
const samples=r.soak?.samples||[],first=samples[0],last=samples.at(-1),complete=r.soak?.seconds===1800&&samples.length===60&&!!r.finished_at;
const delta=first&&last?last.heap.usedSize-first.heap.usedSize:NaN;
const startSteady=samples.find(s=>s.seconds>=300),steadyDelta=startSteady&&last?last.heap.usedSize-startSteady.heap.usedSize:NaN;
const smallSamples=samples.filter((_,i)=>i===0||i===samples.length-1||(i+1)%10===0).map(s=>`| ${s.seconds} | ${s.metrics.received.toLocaleString('en-US')} | ${mib(s.heap.usedSize)} | ${mib(s.heap.backingStorageSize||0)} | ${s.metrics.domNodes} | ${s.metrics.gaps} |`).join('\n');
const gap=r.gapTest?.metrics;
const hashes={};for(const file of ['client.js','server.mjs','run.mjs','build.mjs','package-lock.json'])hashes[file]=createHash('sha256').update(await fs.readFile(file)).digest('hex');
await fs.writeFile('results/source-hashes.json',JSON.stringify(hashes,null,2));
const text=`# UI Sol 스파이크 결과 — S1 CAN Trace / S2 xterm

측정일: 2026-11-10 KST. 계획 근거용 짧은 스파이크이며 제품 구현·성능 합격 선언이 아니다.
상태: 비교 6조합 완료, S2 완료, 30분 soak **${complete?'완료':'진행 중 ('+(last?.seconds||0)+' s 표본까지)'}**.
원시 정본: [benchmark.json](results/benchmark.json), stdout: [run.log](results/run.log).
소스/lock SHA-256: [source-hashes.json](results/source-hashes.json).

## 환경

- CPU: ${r.environment.cpus[0]}, ${r.environment.cpus.length} logical CPU. RAM ${mib(r.environment.ram_bytes)} MiB.
- OS: ${r.environment.os} ${r.environment.release}, ${r.environment.arch} (Darwin kernel 값이며 OS 마케팅 버전으로 추정하지 않음).
- Node ${r.environment.node}; Playwright 1.65.0; Chrome for Testing ${r.environment.browser}.
- full Chromium의 headless 모드(channel=chromium), viewport 1280×900, DPR 기본1, canvas 1050×616.
- flags: enable-precise-memory-info, disable-background-timer-throttling, disable-renderer-backgrounding.
- 동일 머신 loopback 127.0.0.1; WS permessage-deflate off. 실제 CAN·Rust daemon·장치 driver 없음.
- 다른 작업이 있는 개발 머신이므로 전용 성능 runner나 OS 간 비교 수치가 아니다.
- WebGL addon 생성 성공은 기록했지만 실제 GPU vendor·hardware 가속 여부는 미확인이다.
- 최초 headless-shell은 rAF 표본0·screenshot timeout으로 무효 처리했다. 본 표에는 해당 결과를 포함하지 않았다.

## S1 방법·측정 정의

1. 생성률 목표10,000 frames/s, 20 ms tick, 메시지당 최대200행·64 KiB. 타이머 지연은 유한 catch-up.
2. row는 seq(u32), id(u32), generated_ms(f64), 8-byte data(lo/hi) 5필드. origin=device/RX/channel은 시나리오 상수다.
3. JSON은 같은 row 배열, MessagePack은 같은 객체, column은 16-byte header+24 bytes/row의 LE 열 블록.
4. full D3 envelope·uint64 seq·CAN FD64B·string dictionary는 포함되지 않는다. 바이너리 제품 schema가 아니다.
5. chronological ring2,000,000행, 48,000,000 byte(45.78 MiB), seed history를 먼저 채워 고정 할당한다.
6. Fixed는 id0..2047의 최신 slot2,048개(49,152 byte), 동일 ID overwrite 경로다.
7. 최신28행만 그리는 pooled DOM / 자체 Canvas2D 비교. 실제 TanStack·Glide·React 성능 시험이 아니다.
8. 각 조합3 s warmup 뒤60 s 통계, 총 수신 수에는 warmup도 포함한다.
9. 생성→다음rAF 지연은 배치 생성 시각에서 draw 후 다음 rAF까지. 모든 row의 개별 표시 지연이 아니다.
10. DOM은 layout read를 강제한다. Canvas draw 반환·다음rAF는 compositor presentation을 보장하지 않는다.
11. 따라서 표의 지연은 렌더 proxy다. 실제 화면 완료 지연·물리 장치→화면 지연은 이번에 미확인이다.
12. 시계는 같은 호스트 epoch 기반 performance.timeOrigin+performance.now, HTTP8회 최소RTT midpoint offset 사용.
13. 이는 Rust daemon mono↔browser perf의 실제 동기 시험이 아니다. wall jump·sleep·비대칭 오차는 미검증이다.
14. decode는 JSON.parse/MP decode/DataView 생성, ingest는 필드 접근·ring 쓰기·seq 검사. 둘을 같이 읽어야 한다.
15. column의 열 숫자 접근은 ingest에 포함된다. DataView 생성만 빠르다고 전체 decode가 그만큼 빠르다는 뜻은 아니다.
16. rAF interval은 스케줄 간격, draw time은 JS/layout/draw 제출 비용이다. GPU frame time으로 부르지 않는다.
17. 지연·처리 시간 배열은 최대100k 표본, pending descriptor512개로 제한. rolling 보관 정책이다.
18. 화면 밖 history의 과거 스크롤·필터·접근성·정렬·사용자 입력 지연은 S1 미측정이다.
19. flush 때 행을 생성하므로 독립 장치 이벤트의 배치 축적 대기는 포함하지 않는다.
20. 배치 descriptor 행의 viewport 포함 여부를 검증하지 않았다. Fixed도 전체ID 중28개만 보이므로 이 수치는 개별 frame/ID의 시각적 가시성 보장이 아니다.

| format | renderer | 수신 프레임(warmup 포함) | 생성률 frames/s | 생성→다음rAF p50/p95/p99 ms | decode p95 ms/message | ingest p95 ms/message | draw p95 ms | rAF interval p95 ms |
|---|---|---:|---:|---|---:|---:|---:|---:|
${table}

각 p95는 별도 분포의 분위수다. decode p95와 ingest p95의 합을 정확한 combined p95라고 부르지 않는다.
메시지당 행수는 최대200이며 일부 tick에서 작은 후속 batch가 생긴다. 고정200행 decoder microbench가 아니다.
기본 render 경로에서는 intermediate 행이 화면을 지나지 않을 수 있지만 ring에는 모든 수신 행을 쓴다.
현재 viewport 그림과 raw 수신·보관을 혼동하지 않는다.

| 경로 | 전송 byte/생성행 | 최대 메시지 byte | server 생략행 | client seq gap | 측정 descriptor drop | DOM nodes | ring MiB |
|---|---:|---:|---:|---:|---:|---:|---:|
${wire}

정상6조합은 seq 연속·마지막 row id/payload 일치·measurement drop0을 assert했다.
모든 행의 D3 canonical roundtrip 또는 전체 저널 byte 일치 시험을 했다는 의미는 아니다.
모든 조합에서 crossOriginIsolated=${r.runs.every(x=>x.crossOriginIsolated)}를 관측했다(COOP same-origin, COEP require-corp).
SAB 자체나 Worker decode를 측정하지는 않았다.

## 고부하 정직성·사용자 버튼 확인

- viewer consume을500 ms 중단하는 주입으로 display-only drop ${gap?.gaps??'—'}행 발생.
- range: ${JSON.stringify(gap?.gapRanges||[])}.
- visible status: \`${r.gapTest?.badge||'—'}\`.
- first/last/count 산술과 누계가 같은지 assert. 원본 수집·기록 정합성은 서버 저널이 없어 미검증이다.
- Playwright의 실제 STOP 버튼 click 후 acknowledgement DOM 확인: ${r.stopTest?.clicked?'성공':'미완료'}.
- 이 STOP은 simulation UI event handler만 실행한다. daemon latch·device submit 중단·우선 큐 지연은 미측정이다.
- 메인 스레드 stall·서버 큐 포화·TCP HoL에서 control 우선 처리 보장은 제품 fixture로 별도 검증해야 한다.

## S2 xterm 출력

- @xterm/xterm6.0.0 / addon-webgl0.19.0, cols120·rows30·scrollback1000, CSS 로컬 포함.
- 동일42,000-char ASCII chunk100개를 write callback으로 차례대로 공급(4,200,000 bytes).
- callback은 parser 처리 완료다. 모든 중간 문자를 사용자가 봤다는 뜻이 아니다.
- S1 synthetic CAN10k/s 수신을 같은 페이지에서 계속 실행한 상태다. 완전히 유휴 terminal-only 시험은 아니다.

| renderer | bytes | write callback까지 ms | MiB/s | callback→2rAF ms | 오류 |
|---|---:|---:|---:|---:|---|
${terminals}

한 차례 burst 실측이다. 입력→장치·sanitizer 비용·실UART·자연GC peak·GPU 성능 비교로 일반화하지 않는다.
이번 결과만으로 WebGL이 DOM보다 항상 빠르다고 판단하지 않는다.

## 30분 heap soak

- column/canvas 한 조합·10k/s·2m ring, 매30 s CDP HeapProfiler.collectGarbage 후 Runtime.getHeapUsage.
- 표본 수 ${samples.length}/60, 마지막 ${(last?.seconds||0)} s, 누적수신 ${(last?.metrics.received||0).toLocaleString('en-US')}프레임.
- 첫30 s post-GC heap ${first?mib(first.heap.usedSize):'—'} MiB → 마지막 ${last?mib(last.heap.usedSize):'—'} MiB, signed delta ${mib(delta)} MiB.
- 300 s warmup 이후 마지막까지 signed delta ${mib(steadyDelta)} MiB.
- typed ring 자체48,000,000 byte 고정. 측정 histogram 배열은100k까지 warmup 중 성장할 수 있다.
- 전체 표본은 raw JSON에 보존한다. 아래는5분 간격과 처음/마지막 요약이다.

| elapsed s | 수신 frames | post-GC JS heap MiB | CDP backing storage MiB | DOM nodes | seq gap |
|---:|---:|---:|---:|---:|---:|
${smallSamples}

이 표는 전체 renderer RSS·GPU·네이티브 메모리를 포함하지 않는다. 24 h·3OS·멀티패널 C2 gate는 미실행이다.
GC를 강제로 호출했으므로 자연GC peak/멈춤 시간도 이 수치로 판단하지 않는다.

## 재현

repository root에서:

\`\`\`sh
cd spikes/ui/sol
npm ci --cache .npm-cache --no-audit --no-fund
PLAYWRIGHT_BROWSERS_PATH="$PWD/.browsers" ./node_modules/.bin/playwright install chromium
node build.mjs
PLAYWRIGHT_BROWSERS_PATH="$PWD/.browsers" BENCH_SECONDS=60 SOAK_SECONDS=1800 node run.mjs
node summarize.mjs
node bundle-sizes.mjs
\`\`\`

- OS sandbox가 Chromium Mach-port를 막으면 sandbox 밖 실행 권한이 필요하다. 실제 첫 launch가 이 이유로 실패했다.
- 빠른 smoke는 BENCH_SECONDS=2 SOAK_SECONDS=0. 결과 JSON을 덮어쓰므로 보존할 결과를 먼저 복사한다.
- 모든 npm 의존성은 이 디렉터리 node_modules, browser는 .browsers에만 설치했다. 전역 설치 없음.
- screenshot [canvas.png](results/canvas.png)를 직접 확인해 header·LIVE·seq·28행 viewport 렌더를 검증했다.
- 버전/라이선스: [npm-evidence.json](npm-evidence.json), [compat-evidence.json](compat-evidence.json), [crates-evidence.json](crates-evidence.json).
- 최소 counter bundle 크기는 [bundle-sizes.json](results/bundle-sizes.json). 앱 전체 크기·성능 비교가 아니다.

## 계획에 반영한 결론

1. 단순 visible-row 업데이트에서는 DOM/canvas 둘 다 낮은 proxy 지연을 보였다. canvas 전면 채택 근거는 부족하다.
2. M1 JSON·DOM 가상화·external ring, M2 full D3/FD schema로 binary opt-in을 다시 측정한다.
3. MessagePack은 작아질 수 있지만 JSON보다 항상 빠른 decode를 가정하지 않는다.
4. binary의 필드 접근·ring 반영까지 포함해 비교하고 schema evolution·u64·origin 보존을 먼저 합의한다.
5. xterm은 WebGL/DOM 지원 조합을 사용하며 Canvas addon은6.0.0과 조합하지 않는다.
6. browser data cap·credit·gap·clock uncertainty·실제 paint 계측을 제품 수용 시험에 별도로 남긴다.

## 추가 타입 생성 probe

[Schema probe](schema-probe/README.md)에서 Rust→draft2020-12→TS·Ajv validation·최대u64 string을 실제 검증했다.
직접 ts-rs 생성에서는 optional field가 required로 나와 실제 직렬화 fixture가 tsc에서 거부되는 차이를 보존했다.
이는 전체 Loom schema 호환성 검증이 아닌 작은 DTO 비교다.
`;
await fs.writeFile('RESULTS.md',text);
console.log({runs:r.runs.length,soak_complete:complete,soak_seconds:last?.seconds,heap_delta_MiB:delta/1048576});
