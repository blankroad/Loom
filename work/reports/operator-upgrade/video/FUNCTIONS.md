# Video operator upgrade — FUNCTIONS (contracts)

Batch 1 is complete and needs no Sonnet design. Batches 2–7 get their contracts here before their leases start (PLAN §3). Sonnet implements **only** the bodies listed in the batch allowlist; tests, exported types/signatures, manifest/schema, CSS and test files are Opus-owned. A missing contract or helper → stop and return to Opus with the exact reason.

**Test resource rule (ROOT, 2026-11-16; scheduling, not a permission change).**
- Every targeted Vitest run adds `--maxWorkers=2`, e.g. `npx vitest run src/panels/video --maxWorkers=2`. This supersedes the plain command in earlier acceptance lines.
- The full UI suite is ROOT-only, at the final integrated freeze.
- Rust stays `CARGO_BUILD_JOBS=2`.
- Never weaken assertions or timeouts, and never kill existing test runs.
- Checks that already passed are not re-run just to add the flag.

Common UI rules (all batches): device/label text is untrusted → `textContent`/attributes only, never `innerHTML`; no new timers except the existing `schedule()` pull; no RPC on mount; every write goes through `ctx.act`, reads through `ctx.query`, bytes through `ctx.media` (declared prefix `/api/v1/video/`).

---------------------------------------------------------------------------------------------------

## Batch 1 — UI: V-05 truthful ended health · V-06 safe artifact viewing · V-07 reuse settings · V-08 source lists

### B1 lease

| File | Lease holder during B1 | Notes |
|---|---|---|
| `ui/src/panels/video/videoModel.ts` | **Sonnet** (bodies below) | Opus already wrote the new types/signatures (stubs throw `unimplemented: B1 …`). |
| `ui/src/panels/video/videoPanel.ts` | **Sonnet** (bodies below) | Opus already wrote the manifest change (`queries` += `video.sources`) and imports only. |
| `ui/src/panels/video/video.css` | Opus (done before lease) | Sonnet must not edit. |
| `ui/src/panels/video/videoOperator.test.ts` | Opus | Sonnet must not edit; run only. |
| existing `video.test.ts`, `videoModule.test.ts` | Opus | must stay green unchanged. |

**Body allowlist (Sonnet):** `readCapture`, `readHealth`, `VideoBook.applyState`, `VideoBook.apply`, `VideoBook.applySnapshotItem` (only if needed to pass `null`), `fmtWallNs`, `fmtAgo`, `healthLine`, `formFromCapture`, `readSources`, `sourceListText`, `artifactFileName` (videoModel.ts); module-level `ensureOption` and, inside `createVideoWorkbench`: DOM construction additions, `syncForm` (unchanged unless needed), `handle`, `pull`, the freeze/snapshot/open click listeners, new `reuseSettings`, `onSourceChange`, `loadSources`, `viewArtifact`, `leaveArtifactView`, `draw`, `dispose` (videoPanel.ts). Nothing else.

**Acceptance (from `ui/`):**
```
npx vitest run src/panels/video          # videoOperator.test.ts + existing video tests, all green
mkdir -p ../work/typecheck
npx tsc -p tsconfig.app.json --noEmit --tsBuildInfoFile ../work/typecheck/app.tsbuildinfo    # exit 0
npx tsc -p tsconfig.node.json --noEmit --tsBuildInfoFile ../work/typecheck/node.tsbuildinfo  # exit 0 (isolated cache, COMMON)
npx eslint src/panels/video              # no errors
```
State at handoff (expected red, proves tests bite): `videoOperator.test.ts` fails on `unimplemented: B1 …` / missing DOM; `tsc` reports the two literals in `readCapture`/`readHealth` missing the new fields. Existing tests green.

### B1-M  videoModel.ts (pure)

**Types (written by Opus — do not change):**
```ts
export interface CaptureView { /* existing fields … */ stateWallNs: string | null }   // ts.wall_ns of the newest applied capture_state event
export interface HealthView  { /* existing fields … */ lastRecvUtcNs: string | null; state: string | null }
export interface HealthExtras { renderFps: number; fetched: number; sameSkipped: number }
export interface Prefill { form: OpenForm; note: string | null }
export interface SourceList { replay: string[]; local: { enumerated: boolean; devices: string[]; warning: string | null } }
export const REUSE_HTTP_NOTE = 'URL은 쿼리 문자열 없이 기록됩니다 — 토큰이 필요하면 다시 입력하세요';
export const MAX_SOURCE_ITEMS = 100;
```

**M1 `readCapture(p: unknown, seq: bigint, wallNs: unknown = null): CaptureView | null`** — existing behaviour plus `stateWallNs: wide(wallNs)` (exact decimal string or null; numbers accepted only when safe, same as `wide`). Ex: `readCapture({capture_id:'v1',state:'closed',config:{}}, 3n, '1762770000000000000').stateWallNs === '1762770000000000000'`; `wallNs:'abc'` → `null`; omitted → `null`.

**M2 `VideoBook.applyState(p, seq, wallNs: unknown = null)` / `VideoBook.apply(kind, p, seq, wallNs: unknown = null)`** — `apply` forwards `wallNs` only to `applyState`; other kinds ignore it. Seq ordering unchanged: an older seq never overrides (including its `stateWallNs`). `applySnapshotItem` passes `null` (reducer snapshots carry no event ts; UI must show `—`, never invent a time).

**M3 `readHealth(p)`** — existing behaviour plus `lastRecvUtcNs: wide(o.last_recv_utc_ns)` and `state: str(o.state)`. Absent fields → `null` (old daemons). Ex: payload without `last_recv_utc_ns` → `lastRecvUtcNs === null`.

**M4 `fmtWallNs(ns: string | null | undefined): string`** — `'—'` unless `ns` is an exact decimal (`wide(ns) !== null`) whose ms value `Number(BigInt(ns) / 1_000_000n)` is a safe integer. Output = local time `HH:MM:SS.mmm` from `new Date(ms)` using `getHours/getMinutes/getSeconds/getMilliseconds`, zero-padded 2/2/2/3. Ex: `'1762770000123000000'` → `${pad2(d.getHours())}:${pad2(d.getMinutes())}:${pad2(d.getSeconds())}.123` for `d = new Date(1762770000123)`; `null`, `''`, `'12a'` → `'—'`.

**M5 `fmtAgo(ms: number): string`** — non-finite or negative → treat as 0. `< 60_000` → `` `${floor(ms/1000)}초` ``; `< 3_600_000` → `` `${floor(ms/60_000)}분` ``; else `` `${floor(ms/3_600_000)}시간 ${floor((ms % 3_600_000)/60_000)}분` ``. Ex: `0→'0초'`, `59_999→'59초'`, `60_000→'1분'`, `960_000→'16분'`, `3_660_000→'1시간 1분'`, `-5→'0초'`.

**M6 `healthLine(h: HealthView | undefined, c: CaptureView | null, x: HealthExtras, nowWallMs: number): string`** (V-05)
- `c === null` → `` `표시 ${x.renderFps} fps` ``.
- `LIVE_STATES.has(c.state)` (opening/live/stalled/stopping): if `h` undefined → `` `표시 ${x.renderFps} fps` ``; else exactly the current live line (segments joined by `' · '`): `frames ${framesTotal}`, `수신 ${measuredFps}/${requestedFps ?? '?'} fps`, `표시 ${renderFps} fps`, `마지막 수신 ${ageMs ?? '—'} ms`, `STALE`|`fresh`, `dup ${dup} / drop ${drop}`, `oversize ${oversize} · truncated ${truncated} · junk ${junkBytes}B`, `media skip ${mediaSkipped}`, `ring ${ringFrames}`, `fetched ${fetched} · same ${sameSkipped}`.
- otherwise (ended: closed/failed/unknown): **never** contains `fresh`, `STALE`, `수신 n/m fps` or `마지막 수신 n ms`. Exactly:
  `` `종료 · frames ${h?.framesTotal ?? '—'} · 마지막 frame ${c.stoppedAtFrame ?? h?.lastFrameId ?? '—'} · 마지막 수신 ${fmtWallNs(h?.lastRecvUtcNs)} · 종료 시각 ${fmtWallNs(c.stateWallNs)}${ago}` ``
  where `ago = ` `` ` (${fmtAgo(nowWallMs - endedMs)} 전)` `` when `fmtWallNs(c.stateWallNs) !== '—'` (endedMs = ms of stateWallNs), else `''`.
- Ex (regression from `ui/video-open.png`): closed/eof, stoppedAtFrame 1011, stateWallNs = now−16 min, h `{framesTotal:1011, measuredFps:3, ageMs:25, stale:false, lastRecvUtcNs:null}` → `종료 · frames 1011 · 마지막 frame 1011 · 마지막 수신 — · 종료 시각 HH:MM:SS.mmm (16분 전)`.

**M7 `formFromCapture(c: CaptureView, base: OpenForm): Prefill | null`** (V-07) — no I/O, no validation (caller shows `validateForm`).
- `c.source === 'http-mjpeg'` and `c.sourceLabel` starts with `http://` → `form = {...base, source:'http-mjpeg', url: c.sourceLabel}`, `note = REUSE_HTTP_NOTE`.
- `'replay'` and label starts with `replay:` → `name = label.slice(7)`, note `null`.
- `'avfoundation'` and label starts with `avfoundation:` → `device = label.slice(13)`, note `null`.
- anything else (unknown kind, empty/mismatched label) → `null`.
- `width/height/fps` = `c.width/c.height/c.fps` when each is a positive integer, else `base`'s. Other fields keep `base`.
- Ex: `{source:'replay', sourceLabel:'replay:clip.mp4', width:640, height:360, fps:15}` + DEFAULT_FORM → `{form:{...DEFAULT_FORM, source:'replay', name:'clip.mp4', width:640, height:360, fps:15}, note:null}`.

**M8 `readSources(r: unknown): SourceList`** (V-08) — accepts the envelope `{data:{…}}` or the bare result (same unwrapping as `capabilityText`). `replay`: string items matching `/^[A-Za-z0-9_-][A-Za-z0-9._-]{0,63}$/`, first occurrence kept, at most `MAX_SOURCE_ITEMS`. `local.enumerated = local.enumerated === true`; `devices` = `local.avfoundation_video[].name` strings, 1..=128 chars, no control chars (`/[\u0000-\u001f\u007f]/`), deduped, ≤ `MAX_SOURCE_ITEMS`, `[]` when not enumerated; `warning = string | null`. Garbage → `{replay:[], local:{enumerated:false, devices:[], warning:null}}`.

**M9 `sourceListText(s: SourceList | null, error: string | null): string`** — `error` → `` `video.sources 실패: ${error}` ``; `s === null` → `''`; else parts joined `' · '`: `replay ${n}개` (n>0) or `replay 없음 ($LOOM_HOME/video/replay)`; plus, only when `s.local.enumerated`, `` `장치 ${m}개 (실물·TCC 미검증)` ``.

**M10 `artifactFileName(artifactId: string): string | null`** — `/^snap-v[0-9a-f]{12}-\d{1,12}$/` → `${id}.jpg`; `/^rec-v[0-9a-f]{12}-\d{1,12}$/` → `${id}.mkv`; else `null` (B5 adds `evid` → `.json`).

### B1-P  videoPanel.ts (DOM; reuse `el`, `tid`, `obj`, `ctx.query/act/media`, `ctx.scheduler.invalidate`, existing `schedule()`)

Manifest (Opus, done): `queries: ['video.status', 'video.capabilities', 'video.sources']`. No new actions.

**New closure state** (Sonnet declares inside `createVideoWorkbench`):
```ts
let reuseNoteText: string | null = null;
let sources: SourceList | null = null; let sourcesErr: string | null = null;
let sourcesPending = false; let replayRequested = false;
let viewing: { artifactId: string; frameId: number; expectedSha: string; sha: string; ok: boolean } | null = null;
let pendingView: { bmp: ImageBitmap; view: NonNullable<typeof viewing> } | null = null;
let viewPending = false;
let snapsKey = '';
```

**New DOM** (exact test ids; all `button.type='button'`):
| Element | testid | Placement / text |
|---|---|---|
| `reuseBtn` button `vd-reuse` | `video-reuse` | in `status`, after `srcEl`, before `closeBtn`; text `↺ 같은 설정으로 다시 열기`; title `직전 캡처의 소스·크기·fps로 폼을 채웁니다 (열기는 직접 누름)` |
| `reuseNote` div `vd-note` | `video-reuse-note` | in `cfgBox` right after `formErr` |
| `replayList` `<datalist>` | `video-replay-list` | appended to `cfgBox`; `id = vd-replay-${n}`; `nameIn.setAttribute('list', id)` |
| `deviceList` `<datalist>` | `video-device-list` | appended to `cfgBox`; `id = vd-device-${n}`; `devIn.setAttribute('list', id)` |
| `devLoadBtn` button | `video-device-load` | in `grid` right after `devL`; text `장치 목록 불러오기`; title `video.sources --local: AVFoundation 장치 열거 — macOS 카메라 권한 요청이 뜰 수 있음 (장치는 열지 않음)` |
| `srcListEl` span `vd-note` | `video-source-list` | in `cfgBox` after `capRow` |
`n` = module-level counter incremented per panel instance (unique datalist ids).

**P1 `ensureOption(sel: HTMLSelectElement, value: string, label: string): void`** (module level) — if no `<option>` with `value` exists, append `option(value, label)`; never removes options.

**P2 `reuseSettings(): void`** — `c = current()`; return if `!c || LIVE_STATES.has(c.state)`; `r = formFromCapture(c, form)`; return if null. `form = r.form`; `reuseNoteText = r.note`; `ensureOption(sizeSel, `${w}x${h}`, `${w}×${h}`)`; `ensureOption(fpsSel, String(fps), `${fps} fps`)`; `syncForm()`; `cfgBox.open = true`; invalidate. **No RPC**, no open. Listener: `reuseBtn` click.

**P3 `onSourceChange(): void`** — listener on `srcSel` `'change'` (the existing `'input'`→`readForm` stays). If `srcSel.value === 'replay' && !replayRequested` → `replayRequested = true; void loadSources(false)`. At most one automatic call per panel instance.

**P4 `loadSources(local: boolean): Promise<void>`** — if `sourcesPending` return. `sourcesPending = true; devLoadBtn.disabled = true`. One `ctx.query('video.sources', {local})`. Success: `s = readSources(r)`; `sources = {replay: s.replay, local: s.local.enumerated ? s.local : (sources?.local ?? s.local)}`; `sourcesErr = null`; `replayList.replaceChildren(...options(sources.replay))`; if `s.local.enumerated` → `deviceList.replaceChildren(...options(sources.local.devices))` (option `.value = name`, no innerHTML). Failure: `sourcesErr = e instanceof Error ? e.message : String(e)`. Finally: `sourcesPending = false; devLoadBtn.disabled = false`; invalidate. If `disposed`, skip DOM writes. `devLoadBtn` click → `void loadSources(true)` (every click; explicit).

**P5 `viewArtifact(s: SnapshotView): Promise<void>`** (V-06) — if `viewPending || disposed` return; `viewPending = true`; AbortController + `setTimeout(abort, FETCH_TIMEOUT_MS)`.
- `r = await ctx.media(artifactUrl(s.artifactId), ac.signal)`.
- `!r.ok` → body `obj(await r.json().catch(()=>({})))`; toast `` `아티팩트 보기 실패: ${r.status} ${String(body.error ?? '')}`.trim() ``; return.
- `content-type` must start with `image/jpeg` else toast `` `아티팩트 보기 실패: 형식 ${ct || '없음'}` ``; return.
- `buf = await r.arrayBuffer()`; `sha = hex(await crypto.subtle.digest('SHA-256', buf))`; `bmp = await createImageBitmap(new Blob([buf], {type:'image/jpeg'}))`; if `disposed` → `bmp.close()`, return.
- `pendingView?.bmp.close(); pendingView = {bmp, view:{artifactId:s.artifactId, frameId:s.frameId, expectedSha:s.sha256, sha, ok: sha === s.sha256}}`.
- catch → toast `` `아티팩트 보기 실패: ${msg}` ``. finally: clearTimeout, `viewPending = false`, invalidate.
Never navigates; never uses `window.open`/`location`.

**P6 `leaveArtifactView(): void`** — `viewing = null; pendingView?.bmp.close(); pendingView = null; if (!frozen) schedule(0)`; invalidate.

**P7 modifications**
- `handle(ev)`: `book.apply(ev.kind, ev.payload, seqOf(String(ev.seq)), ev.ts?.wall_ns ?? null)`.
- `pull()`: also skip (reschedule 250 ms) while `viewing || pendingView`.
- freeze listener: if `viewing || pendingView` → `leaveArtifactView()` and return; else existing toggle.
- open listener: after validation passes set `reuseNoteText = null` (rest unchanged).
- `dispose()`: also `pendingView?.bmp.close()`.

**P8 `draw()` changes** (order: after `applySnap`/events):
1. If `pendingView`: draw its bitmap like live frames (resize canvas to bitmap, `drawImage`) when a 2D context exists; set `viewing = pendingView.view` regardless of context; close bitmap; `pendingView = null`. Live `pending` frames are drawn only when `!frozen && !viewing`.
2. Health: `healthEl.textContent = healthLine(h, c, {renderFps: rfps, fetched, sameSkipped}, Date.now())` (replaces the inline array; `data-render-fps` stays).
3. Ident/overlay when `viewing`: `frameIdEl` = `` `frame_id ${frameId} · 아티팩트 ${artifactId}` ``; `shaEl` = `` `sha256 ${sha}` ``, `data-mismatch = ok ? 'false' : 'true'`, title `` `아티팩트 파일 바이트로 계산 (스냅샷 이벤트 sha256 ${ok ? '일치' : '불일치!'})` ``; overlay = `` `아티팩트 ${artifactId} · frame ${frameId} · sha256 ${ok ? '일치' : '불일치!'}` ``; `stage.dataset.view = viewing ? 'artifact' : 'live'`. Not viewing → existing behaviour.
4. Snapshot rows (current capture, last 4): rebuild only when `snapsKey` (`${c?.id ?? ''}|${ids.join(',')}`) changes. Row = `div.vd-snap` `data-testid="video-snap-row"` `data-artifact-id=id` containing `span.vd-mono` (existing text `📷 id · frame n · sha16… · size`, `title = sha256`), `button` testid `video-snap-view` text `보기` (click → `void viewArtifact(s)`), `a` testid `video-snap-save` text `저장`, `href = artifactUrl(id)`, `download = artifactFileName(id) ?? ''`, `rel = 'noopener'`. **No anchor without `download`.**
5. Recording: finalized → `a` testid `video-record-download` text `다운로드`, `href`, `download = artifactFileName(rec.artifactId) ?? ''`, `rel='noopener'` (replaces the bare anchor).
6. Reuse: `prefill = c && !LIVE_STATES.has(st) ? formFromCapture(c, form) : null`; `reuseBtn.hidden = !prefill`; `reuseNote.textContent = reuseNoteText ?? ''`; `reuseNote.hidden = reuseNoteText === null`.
7. Sources: `devLoadBtn.hidden = form.source !== 'avfoundation'`; `srcListEl.textContent = sourceListText(sources, sourcesErr)`; `srcListEl.hidden = srcListEl.textContent === ''`.
8. Buttons: `snapBtn.disabled = existing || !!viewing`; `freezeBtn.textContent = viewing || frozen ? '▶ 실시간' : '⏸ 고정'`; `freezeBtn.disabled = !viewing && !frozen && !drawn`.

### B1 contract amendments (Opus, after review)
- A1: `videoPanel.ts` local `type ViewState = { artifactId: string; frameId: number; expectedSha: string; sha: string; ok: boolean }` inside `createVideoWorkbench`; `viewing: ViewState | null`, `pendingView: { bmp: ImageBitmap; view: ViewState } | null`. Replaces `NonNullable<typeof viewing>`, which TS control-flow narrowing resolves to `never` (Opus contract error).
- A2: `videoPanel.ts` imports `artifactFileName, formFromCapture, healthLine, readSources, sourceListText, type SnapshotView, type SourceList` from `./videoModel` (required by the contract; Opus should have pre-written them).
- A3: M8 control-character test written as a code-point loop (`charCodeAt(0) <= 0x1f || === 0x7f`), because eslint `no-control-regex` rejects the regex literal. Equivalent for the contract range.
- **Rule from B2 on:** a missing type, import or declaration is a contract gap. Stop, return the lease to Opus and do not add it yourself.

### B1 tests (Opus, `videoOperator.test.ts`) — what each proves
- M-tests: ended health has no fresh/fps (regression), live line unchanged byte-for-byte, wall-ns plumbing incl. seq ordering and snapshot items → `—`, absent new health fields → null, `fmtAgo`/`fmtWallNs` tables, prefill per source kind + invalid labels, sources filtering/caps/envelope, file names.
- P-tests (jsdom; fetch, `createImageBitmap`, canvas 2D context and WebCrypto stubbed/injected): ended capture health line; reuse button hidden while live, visible after EOF, prefill incl. non-preset size/fps, no RPC on reuse, note shown for http; replay list fetched once on selecting replay, never on mount; device list only on explicit click (`local:true`); every anchor has `download`; in-panel artifact viewer verifies sha (match + mismatch), refuses non-JPEG with toast, `▶ 실시간` leaves the viewer; existing module tests unchanged.

---------------------------------------------------------------------------------------------------

## Batch 2 — Rust engine: V-03 pin store · V-05 health truth · V-06 artifact disposition

### B2 lease
| File | Holder during B2 |
|---|---|
| `crates/loom-video/src/model.rs` | **Sonnet**: bodies M1–M11 only. The in-file `mod tests` stays unchanged. |
| `crates/loom-video/src/worker.rs` | **Sonnet**: W1–W11 |
| `crates/loom-video/src/gateway.rs` | **Sonnet**: G1–G4 |
| `crates/loom-video/src/cli.rs` | **Sonnet**: C1–C2 (in-file tests unchanged) |
| `crates/loom-video/src/media.rs` | **Sonnet**: D1 (in-file tests unchanged) |
| `tests/pin_contract.rs`, `tests/operator_e2e.rs` | Opus (spec; do not edit) |
| `tests/fixture_e2e.rs`, `tests/platform_unsupported.rs` | Opus (must stay green) |

Pre-written by Opus (do not change): pin constants/types/stub signatures in `model.rs` (`MAX_USER_PINS`, `PIN_BYTES`, `PIN_TTL_DEFAULT_MS`, `PIN_TTL_MAX_MS`, `PinOwner`, `PinRefusal`, `PinInfo`, `PinEntry`, `PinStore`, `pin_limits`, `lookup_frame`); `worker.rs` imports, `Shared.pins` field + initializer, stub signatures `now_ns`/`lookup`/`pin`/`unpin`, free fn `ttl_arg`; `gateway.rs` `forward!(Pin …)`/`forward!(Unpin …)` declarations. **A missing type/import/declaration is a contract gap: return the lease.**

**Acceptance** (worktree root):
```
CARGO_TARGET_DIR=/Users/ctmctm/workspace/Loom/target CARGO_BUILD_JOBS=2 cargo test -p loom-video
CARGO_TARGET_DIR=/Users/ctmctm/workspace/Loom/target CARGO_BUILD_JOBS=2 cargo clippy -p loom-video --tests
```
All tests pass (fixture tests print `SKIP` only when ffmpeg/python3+Pillow is missing; report it if so). Clippy reports no warnings for `loom-video`, including that the scaffold dead-code warnings are gone. Expected-red at handoff: `pin_contract.rs` and `operator_e2e.rs` panic on `unimplemented`.

### B2-M model.rs (pure)
- **M1 `PinOwner::label(self) -> String`**: `User` → `"user"`, `Evidence(n)` → `"evidence-{n}"`.
- **M2 `PinRefusal::reason(&self) -> &'static str`**: `UserLimit` → `"pin_user_limit"`, `Bytes` → `"pin_bytes"`.
- **M3 `PinStore::pin(&mut self, frame, owner, now_mono_ns, ttl_ms) -> Result<PinInfo, PinRefusal>`**
  - `ttl = ttl_ms.clamp(1, PIN_TTL_MAX_MS)`; `expires = now_mono_ns.saturating_add(ttl * 1_000_000)` (saturating mul).
  - If the frame id is already held and this owner is already present, replace its expiry with `expires` (`refreshed = true`). This always succeeds.
  - If the frame is held but this owner is not, then `owner == User && user_count() >= MAX_USER_PINS` returns `UserLimit{pinned: user_count()}`. Otherwise add the owner. No byte change.
  - If the frame is not held, check in this order: the user limit as above, then `bytes + len > PIN_BYTES` returns `Bytes{need: len, free: PIN_BYTES - bytes}`. Otherwise insert a clone (Arc bytes) and add `len` to `bytes`.
  - The result is `PinInfo{frame_id, sha256: frame.sha256, size: len, expires_in_ms: ttl, refreshed}`. The store never evicts on its own to make room.
- **M4 `unpin(frame_id, owner) -> bool`**: remove that owner. When it was present, `released_total += 1`. When no owners are left, drop the entry and subtract its bytes.
- **M5 `unpin_owner(owner) -> usize`**: the same for every entry. Returns the count and adds it to `released_total`.
- **M6 `expire(now_mono_ns) -> usize`**: remove every owner with `expiry <= now`. Drop empty entries and subtract their bytes. `expired_total += count`, and the count is returned.
- **M7 accessors** `get` (no expiry check), `user_count` (entries with a `User` owner), `bytes`, `len`, `is_empty`.
- **M8 `entry_json(id, now)`** returns `{"frame_id","sha256","size","owners":[{"owner": label, "expires_in_ms": expiry.saturating_sub(now)/1_000_000}]}`, with owners in `PinOwner` order (User first, then Evidence ascending), or `Value::Null` when the frame is not held.
  **`json(now)`** returns `{"count": len, "user_count", "bytes", "frames": [entry_json … ascending id], "expired_total", "released_total", "limits": pin_limits()}`. Neither function mutates.
- **M9 `pin_limits()`** returns `{"max_user_pins":4,"pin_bytes":16777216,"ttl_default_ms":120000,"ttl_max_ms":600000,"released_on":["unpin","ttl","next_open"],"memory_only":true}`, built from the constants.
- **M10 `lookup_frame(ring, pins, id)`** tries `ring.get(id)` first as `(f, "ring")`, then `pins.get(id)` as `(f, "pin")`.
- **M11 `capabilities()`** adds the key `"pins": pin_limits()`. Nothing else changes.

### B2-W worker.rs
Lock order is always `ring` before `pins`. Never hold `pins` while taking `ring`. Pins publish no journal event (memory only). State lives in status, health and META.
- **W1 `Shared::now_ns(&self) -> u64`** returns `self.cx.clock().mono_ns()`, the same clock as `Frame::recv_mono_ns`.
- **W2 `Shared::lookup(&self, id) -> Option<(Frame, &'static str)>`**: if the ring holds `id`, return a clone with `"ring"`. Otherwise expire the pins at `now_ns()` and return the pinned clone with `"pin"`, or `None`.
- **W3 `Shared::pin(&self, frame_id: Option<u64>, ttl_ms: Option<u64>) -> Result<Value, WorkerError>`**
  - `ttl = ttl_ms.unwrap_or(PIN_TTL_DEFAULT_MS)`.
  - Target: `Some(id)` goes through `lookup(id)`. On a miss, return `NotFound` with the message `"frame expired from the ring (not pinned)"` when `expired`, else `"frame not received"`. The detail is `{"frame_id": id, "expired": ring.first_frame_id.is_some_and(|f| id < f), "ring": ring.range(), "pins": pins.json(now)}`.
  - `None` takes the ring's latest frame, or returns `NotFound "no frame yet"` with detail `{"no_frame": true}`. `retained_by_before` is `"ring"`.
  - `pins.pin(&f, PinOwner::User, now, ttl)`. On success return `{"capture_id", "frame_id", "sha256", "size", "owner":"user", "ttl_ms": ttl, "expires_in_ms", "refreshed", "retained_by_before", "pins": pins.json(now)}`.
  - On a refusal return `ErrorCode::Limit` with the message ``format!("pin refused: {reason} (unpin a frame first; pins are never evicted automatically)")`` and the detail `{"reason", "frame_id", "pins": json}`. For `Bytes`, the detail also has `"need"` and `"free"`.
  - Allowed while STOP is latched, and after the capture ended (frames of the current Shared only).
- **W4 `Shared::unpin(&self, frame_id: Option<u64>) -> Value`**: expire first. `Some(id)` gives `released = unpin(id, User) as u64`, and `None` gives `unpin_owner(User)`. Returns `{"capture_id", "frame_id": id|null, "released", "pins": json(now)}`. Never an error.
- **W5 `ttl_arg(call) -> Result<Option<u64>, WorkerError>`**: absent or `null` gives `None`. A `u64` in `1..=PIN_TTL_MAX_MS` gives `Some`. Anything else is `Usage` with the message ``format!("ttl_ms: expected 1..={PIN_TTL_MAX_MS}")``.
- **W6 `frame_meta`**
  - `Some(id)` uses `lookup`. On a miss, keep the current messages and detail keys and add `"pinned": false`.
  - On success, keep the existing keys and add `"retained_by"` and `"pin": pins.entry_json(id, now)`.
  - `None` (latest) adds `"retained_by":"ring"` and `"pin"`.
- **W7 `snapshot`**
  - `Some(id)` uses `lookup`. On a miss, return `NotFound` with the message `"frame not in the ring and not pinned"` and the detail `{"frame_id", "ring", "pins"}`.
  - The payload adds `"retained_by"`. Quota, write and event behaviour are unchanged.
- **W8 `serve_conn`**
  - `FRAME n` uses `lookup(n)`. The head adds `"retained_by"`.
  - On a miss the error codes are unchanged (`no_frame` / `expired` / `not_received`, judged against the ring range).
  - `FRAME` (latest) is unchanged.
  - `META` adds `"pins": pins.json(now)`.
- **W9 `health(state)`**
  - Adds `"last_recv_utc_ns"`: the latest frame's `recv_utc_ns` as `U64`, or `null`.
  - Adds `"pins": {"count","user_count","bytes"}`, taken after an expire.
  - `"stale"` becomes `!matches!(state, "opening" | "live" | "stalled") || age_ms.is_none_or(|a| a > STALL_MS)`, so terminal and stopping health is always stale.
- **W10 `status()`** adds a top-level `"pins": pins.json(now)` (expire first) for both the running and the ended capture. With no capture: `"pins": null`.
- **W11 `on_call`** handles two methods:
  - `"video.pin"`: `target` → `frame_arg` → `ttl_arg` → `ready(sh.pin(id, ttl))`.
  - `"video.unpin"`: `target` → `frame_arg` → `ready(Ok(sh.unpin(id)))`.
  - No latch refusal for either. Errors from `target`/args are returned as-is.

### B2-G gateway.rs
- **G1** The `forward!` macro gets one new key arm, `"ttl_ms"`: `opt_u64(o, k, model::PIN_TTL_MAX_MS)?`. A value of `0` is `usage(format!("ttl_ms: expected 1..={}", model::PIN_TTL_MAX_MS))`. The value is forwarded when present.
- **G2** `VideoHooks::classify` gets two explicit arms:
  - `"video.pin"` → `(R0, "holds a ring frame in bounded daemon memory (no device, no disk)")`
  - `"video.unpin"` → `(R0, "releases a pinned frame from daemon memory")`
- **G3 `register`**
  - Add `spec("video.pin", true, Arc::new(Pin), obj(json!({"capture_id": s, "frame_id": i, "ttl_ms": i})), DEFAULT_WAIT)` and `spec("video.unpin", true, Arc::new(Unpin), obj(json!({"capture_id": s, "frame_id": i})), DEFAULT_WAIT)`.
  - Extend the roles override (`close`/`record.stop`) to `video.pin` and `video.unpin`, giving `RoleSet::READ`.
  - Add risk rows: `video.pin` R0 `"bounded memory pin (no device, no disk)"` and `video.unpin` R0 `"releases a memory pin"`.
- **G4 `help()`**
  - New example: `ex("loom video pin --frame 120 --ttl 2m", "keep that exact frame past ring eviction (memory only, bounded)")`.
  - New note: `"Pins: at most 4 frames per caller and 16 MiB per capture, TTL ≤ 600 s; released on unpin, TTL or the next open; nothing reaches disk unless you snapshot."`

### B2-C cli.rs
- **C1** `pin` subcommand:
  - About text: `"Keep a ring frame past eviction (memory only, bounded; released on unpin, TTL or next open)"`.
  - Args: `capture()`, `--frame <u64>`, `--ttl <duration via parse_duration>`.
  - Output: `video.pin` with `capture_id`/`frame_id`/`ttl_ms` only when given.
  - Errors: `--frame 0` → `"--frame must be ≥ 1"`. A TTL of zero or above 600 s → `"--ttl must be 1ms..=600s"`.
- **C2** `unpin` subcommand: about text `"Release your pinned frame (--frame) or all your pins"`. Args `capture()` and `--frame <u64>`, with the same `0` refusal.

### B2-D media.rs
- **D1 `artifact()`**: on the 200 reply only, add the header `("content-disposition", format!("attachment; filename=\"{artifact_id}.{}\"", kind.ext()))`. Error replies are unchanged.

### B2 tests (Opus): what they prove
- `pin_contract.rs`:
  - Refresh and shared owners.
  - User-limit and byte bounds.
  - TTL clamp and inclusive expiry; release counters.
  - JSON shapes.
  - **V-03 regression** with pure types (ring eviction vs pin).
  - pin/unpin registered as `Request`, `READ` roles, R0, with schema keys.
  - Hooks reasons.
  - CLI conversion and refusals.
  - Artifact `content-disposition` (only on success).
- `operator_e2e.rs` (fixture):
  - A pinned frame survives eviction, with exact bytes through `video.frame`, socket `FRAME n` and the snapshot file.
  - The unpinned neighbour reports `expired`.
  - unpin is idempotent.
  - TTL expiry is honoured.
  - The user limit returns `E_LIMIT pin_user_limit`.
  - Defaults are the latest frame and 120 s.
  - Usage errors for bad TTLs.
  - Live health has `last_recv_utc_ns`.
  - Health after STOP/close is `stale:true` with state `closed`.
  - pin is allowed while latched, while snapshot is refused.
  - Pins stay readable after close.
  - The next open releases them.

## Batch 3 — UI: V-03 freeze = daemon pin · V-04 ring ◀▶ while frozen

### B3 lease
| File | Holder during B3 |
|---|---|
| `ui/src/panels/video/videoModel.ts` | **Sonnet**: bodies M1–M7 (replace each `throw new Error('B3-M…')`). Nothing else in the file changes. |
| `ui/src/panels/video/videoPanel.ts` | **Sonnet**: P1–P8 inside `createVideoWorkbench` only. The import list and `registerPanel` manifest stay as Opus wrote them. |
| `video.css`, `videoPin.test.ts`, `videoOperator.test.ts`, other tests | Opus (do not edit) |

Pre-written by Opus: `PIN_TTL_MS`, `PinStatus`, `PinView`, `PIN_REASON_LABEL`, `PIN_RETRY_HINT` and the M1–M7 signatures in `videoModel.ts`; the panel import list (`fmtWallNs`, `PIN_TTL_MS`, `pinText`, `pinTick`, `readPinError`, `readPinResult`, `ringStep`, `snapshotBlock`, `type PinView`); manifest actions `pin` (`video.pin`, watch, request) and `unpin` (`video.unpin`, watch, `safetyDirection: 'reduce'`); CSS `.vd-pin[data-status]`, `.vd-ring-nav`. Local `let`/`const`/closures inside `createVideoWorkbench` are body code (yours). A new export, type, import or CSS rule is a contract gap: return the lease.

**Acceptance** (from `ui/`): `npx vitest run src/panels/video` → all pass (B1 26 + B3 13 = 39); `npx tsc -p tsconfig.app.json --noEmit --tsBuildInfoFile ../work/typecheck/app.tsbuildinfo` and the `tsconfig.node.json` / `../work/typecheck/node.tsbuildinfo` counterpart → exit 0; `npx eslint src/panels/video` → exit 0 (the 9 scaffold unused-import errors disappear once used). Expected red at handoff: 12 of 13 `videoPin.test.ts` tests (the manifest test already passes).
Known shared impact (not yours): `ui/src/shell/videoWiring.test.ts` pins the exact action map and fails until ROOT updates it (PLAN S7).

### B3-M videoModel.ts (pure)
- **M1 `readPinResult(r, captureId, frameId, nowMs)`**: `d` = `r.data` when `r` is an object whose `data` is an object, else `r`. `e = d.expires_in_ms`; when `d` is not an object or `e` is not a finite number `≥ 0` → `{captureId, frameId, status:'error', expiresAtMs:null, ttlMs:null, reason:'bad_reply', sha256:null}`. Else `{captureId, frameId: (d.frame_id is a safe integer ≥ 1 ? d.frame_id : frameId), status:'pinned', expiresAtMs: nowMs + e, ttlMs: (finite number d.ttl_ms ? it : null), reason:null, sha256: (string d.sha256 ? it : null)}`.
- **M2 `readPinError(e, captureId, frameId)`**: `code` = `e.code` when `e` is an object with a non-empty string `code`, normalized `code.replace(/^E_/, '').toLowerCase()`, else `'internal'`. `detail` = `e.detail` when an object. `code === 'limit'` → `status:'refused'`, `reason: string detail.reason ?? 'limit'`. `code === 'not_found'` → `status:'error'`, `reason:'not_found'`. Else `status:'error'`, `reason: code`. Always `expiresAtMs:null, ttlMs:null, sha256:null`.
- **M3 `pinTick(p, nowMs)`**: `p.status === 'pinned' && p.expiresAtMs !== null && nowMs >= p.expiresAtMs` → `{...p, status:'expired'}`; otherwise return `p` itself.
- **M4 `fmtRemain(ms)`**: `s = Math.ceil(max(0, ms) / 1000)` (non-finite → 0); `` `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}` ``.
- **M5 `pinText(p, nowMs)`**, with `label(r) = r === null ? '?' : PIN_REASON_LABEL[r] ?? r` and `N = p.frameId`:
  `null` → `''`; `pending` → `📌 보존 요청 중 · frame N`; `pinned` → `📌 데몬 보존 · frame N · 남은 시간 ${expiresAtMs === null ? '?' : fmtRemain(expiresAtMs − nowMs)}`; `refused` → `⚠ 보존 거부 · frame N · ${label}`; `expired` → `⌛ 보존 만료 · frame N`; `error` → `⚠ 보존 실패 · frame N · ${label}`.
- **M6 `snapshotBlock(frozen, pin, captureId)`**: `frozen === null` → `null`. `captureId === null || frozen.captureId !== captureId` → `'다른 캡처의 고정 프레임'`. `pin === null` or its `captureId`/`frameId` ≠ the frozen frame's → `` `고정 프레임 보존 안 됨 — ${PIN_RETRY_HINT}` ``. Then by status: `pinned` → `null`; `pending` → `'보존 확인 중'`; `expired` → `` `고정 프레임 보존 만료 — ${PIN_RETRY_HINT}` ``; `refused`/`error` → `` `고정 프레임 보존 안 됨 (${label(pin.reason)}) — ${PIN_RETRY_HINT}` ``.
- **M7 `ringStep(frameId, dir, lastFrameId)`**: `n = frameId + dir`; `n < 1` → `null`; `dir === 1 && (lastFrameId === null || n > lastFrameId)` → `null`; else `n`.

### B3-P videoPanel.ts (inside `createVideoWorkbench`)
- **P1 DOM** (built with the existing `el`/`tid`; `type = 'button'` on buttons):
  - `pinEl = tid(el('span', 'vd-pin'), 'video-pin')`, appended to `ident` after `frozenEl`.
  - `prevBtn = tid(el('button', 'vd-ring-nav', '◀'), 'video-ring-prev')` (title `'이전 frame (링/보존)'`) and `nextBtn = tid(el('button', 'vd-ring-nav', '▶'), 'video-ring-next')` (title `'다음 frame (링)'`). `tools` order: `freezeBtn, prevBtn, nextBtn, snapBtn, recBtn`.
- **P2 state**: `let pin: PinView | null = null;` `let navBusy = false;` `let pendingNav: { bmp: ImageBitmap } | null = null;` (closed in `dispose` like `pending`).
- **P3 `pinFrame(f: Drawn): void`**
  - `const mine: PinView = { captureId: f.captureId, frameId: f.id, status: 'pending', expiresAtMs: null, ttlMs: null, reason: null, sha256: null }`; `pin = mine`; invalidate.
  - `ctx.act('pin', { capture_id: f.captureId, frame_id: f.id, ttl_ms: PIN_TTL_MS })` — not the `act()` helper (no toast).
    - resolve(handle): if `pin === mine`, then `pin = readPinResult(handle.result, f.captureId, f.id, Date.now())`. Otherwise the reply is stale: call `unpinFrame(f.captureId, f.id)`.
    - reject(e): if `pin === mine` → `pin = readPinError(e, f.captureId, f.id)` (otherwise ignore).
    - finally: invalidate if not disposed.
- **P4 `unpinFrame(captureId: string, frameId: number): void`** — `ctx.act('unpin', { capture_id: captureId, frame_id: frameId })`, fire-and-forget. A rejection → `ctx.ui.toast(`보존 해제 실패: ${message}`)`.
- **P5 `releasePin(): void`** — if `pin?.status === 'pinned'` → `unpinFrame(pin.captureId, pin.frameId)`; then `pin = null`. Pending pins are released by P3's stale-reply path; refused/expired/error pins hold nothing.
- **P6 freeze button** (the viewing branch is unchanged)
  - frozen → `releasePin()`, `frozen = null`, `pendingNav?.bmp.close()`, `pendingNav = null`, `schedule(0)`.
  - not frozen → `frozen = drawn`; if `drawn` → `pinFrame(drawn)`.
  - Invalidate.
  - The open handler, which already does `frozen = null`, also does `pin = null` with no unpin call: the daemon releases pins on the next open.
- **P7 ring navigation `navTo(dir: -1 | 1): Promise<void>`** (`prevBtn` → `-1`, `nextBtn` → `1`)
  - Preconditions: `c = current()`; return when `!frozen || navBusy || viewing || !c || frozen.captureId !== c.id`. Then `target = ringStep(frozen.id, dir, book.health.get(c.id)?.lastFrameId ?? null)`; `null` → return.
  - Fetch: `navBusy = true`. `ctx.media(frameUrl(c.id, target), signal)` with an `AbortController` aborted after `FETCH_TIMEOUT_MS`.
  - Failures (the frozen frame and pin stay):
    - `!r.ok` → `` fetchErr = `frame ${target} 없음 (${r.status} ${String(body.error ?? '')})` `` (body via `r.json().catch(() => ({}))`); return.
    - `x-loom-frame-id` ≠ `target` → `` fetchErr = `frame ${target} 요청에 ${id} 수신` ``; return.
    - throw → `` fetchErr = `frame ${target} 가져오기 실패: ${message}` ``.
  - Success:
    - Hash the bytes and decode exactly like `pull()`. `meta` gets the same fields; `drawnAtMs` is `Date.now()`.
    - Swap the bitmap: `pendingNav?.bmp.close()`, then `pendingNav = { bmp }`.
    - `const old = pin`; `frozen = meta`; `fetchErr = null`.
    - Then `pinFrame(meta)`, and if `old?.status === 'pinned'` → `unpinFrame(old.captureId, old.frameId)`, in that order (new pin requested before the old release).
  - `finally`: clear the timeout, `navBusy = false`, invalidate.
- **P8 draw / pull additions**
  - `draw()` paints `pendingNav` (when not `viewing`) the way it paints `pending` (resize the canvas, `drawImage`, close, `null`) but does **not** touch `drawn` or `render`.
  - Then `if (pin) pin = pinTick(pin, Date.now())`.
  - `pinEl.textContent = frozen ? pinText(pin, Date.now()) : ''`; `pinEl.dataset.status = frozen && pin ? pin.status : 'none'`; `pinEl.hidden = pinEl.textContent === ''`.
  - `` frozenEl.textContent `` when frozen: `` `⏸ 고정됨 (capture ${frozen.captureId}) · 수신 ${fmtWallNs(frozen.recvUtcNs)}` `` (the ended-capture branch is unchanged).
  - `const block = viewing ? null : snapshotBlock(frozen, pin, c?.id ?? null)`; `snapBtn.disabled = <existing condition> || block !== null`; `snapBtn.title = block ?? ''`.
  - `prevBtn.hidden = nextBtn.hidden = !frozen || !!viewing`.
  - `prevBtn.disabled = navBusy || !frozen || ringStep(frozen.id, -1, last) === null`, and `nextBtn` likewise with `1`, where `last = c ? book.health.get(c.id)?.lastFrameId ?? null : null`.
  - `pull()`'s early-return branch calls `ctx.scheduler.invalidate()` when `frozen && pin?.status === 'pinned'`, so the countdown refreshes at most 4 Hz on the existing timer (no new timer).
- Unchanged: snapshot click already sends `{capture_id, frame_id: frozen.id}` for the frozen frame of the current capture; untrusted text only via `textContent`/`title`.

### B3 tests (Opus, `videoPin.test.ts`, 13): what they prove
- Pure tables for M1–M7: envelope/bare replies, expiry from the reply time, malformed replies, code normalization (`E_LIMIT`, `limit`, `E_NOT_FOUND`, gate `E_RIGHTS`), inclusive expiry with identity kept, `m:ss`, every status line, every snapshot-block reason, and ring bounds.
- Manifest: pin is a watch request; unpin is a watch reduce action.
- Panel:
  - **V-03 regression**: freeze → exactly one `video.pin {capture_id, frame_id, ttl_ms: 120000}`; countdown `2:00` → `0:59` with no user action; snapshot targets the pinned id; live → `video.unpin` of that id.
  - Lapse → `expired`, snapshot disabled with a reason, no unpin.
  - `E_LIMIT` refusal → the visual freeze stays and snapshot is blocked with the daemon reason.
  - A late reply after going live is released.
  - Ring ◀: the exact `?id=` is fetched while the live stream moves on; the sha is shown; pin 41 is requested before 42 is released; ▶ is bounded by `last_frame_id`; a 404 miss keeps frame 41 and shows `frame 40 없음`; going live hides navigation and releases 41.

## Batch 4 — Rust pure ROI core: V-01 metrics · V-02 judgment (`crates/loom-video/src/analysis.rs`)

### B4 lease
| File | Holder during B4 |
|---|---|
| `crates/loom-video/src/analysis.rs` | **Sonnet**: bodies A1–A2, S1–S3, D1, P1–P6, O1, J1–J5 only (replace every `unimplemented!`). |
| `crates/loom-video/src/lib.rs` (`pub mod analysis;`), `tests/analysis_contract.rs`, `tests/fixtures/*.jpg` | Opus (do not edit) |
| every other file | not leased |

Pre-written by Opus (do not change): all constants, types (`LumaFrame`, `Metric`, `RoiRect`, `RoiSpec`, `PxRect`, `Samples`, `Metrics`, `Baseline`, `RoiState`, `UnknownReason`, `Observation`, `Transition`, private `Candidate`, `RoiJudge` fields), `Default for RoiJudge`, imports (`serde_json::{json, Value}`, zune `ZCursor`/`ColorSpace`/`DecoderOptions`/`JpegDecoder` under `cfg(feature = "worker")`), every signature. No new helper fns, types, imports or crates. Private closures/local variables inside a body are fine. **A missing declaration is a contract gap: return the lease.**

**Acceptance** (worktree root, same env as B2): `cargo test -p loom-video` (all targets green, including `analysis_contract` 21/21) and `cargo clippy -p loom-video --tests` with 0 warnings (the scaffold's unused-import/dead-code warnings must disappear because the bodies use them). Expected red at handoff: 21/21 `analysis_contract` tests panic on `unimplemented`.

Common rules: never panic (no `unwrap`/`expect`/indexing that can go out of range; use `get`, `saturating_*`, `checked_*`); ms→ns is `ms.saturating_mul(1_000_000)`; `round2(x) = (x * 100.0).round() / 100.0`.

### B4-A names
- **A1 `Metric`** `as_str`: `luma_delta` | `mean_abs_diff` | `changed_px`. `parse` is the inverse (anything else → `None`). `max_threshold`: `LumaDelta` → 255.0, the others → 100.0.
- **A2** `RoiState::as_str`: `unknown` | `normal` | `changed`. `UnknownReason::as_str`: `not_live`, `stale`, `no_baseline`, `baseline_mismatch`, `roi_too_small`, `decode_error`, `analysis_gap`, `config_changed`.

### B4-S spec validation
- **S1 `RoiSpec::from_json(v)`** — checks in exactly this order, first failure returned. For an optional key, absent **or `null`** = default. Error strings start with the given prefix (wording after the colon is free but should say the expected range).
  1. not an object → `"roi: expected an object"`.
  2. any key outside {`name`,`x`,`y`,`w`,`h`,`metric`,`enter`,`exit`,`dwell_ms`,`cooldown_ms`,`pixel_threshold`,`evidence`,`evidence_pre`,`evidence_post`} → `"roi.<key>: unknown key"` (first unknown key in the object's iteration order).
  3. `name`: required string, 1..=`ROI_NAME_MAX` chars, each ASCII alphanumeric, `_` or `-` → else `"roi.name: …"`.
  4. `x`, `y`: required finite numbers (`as_f64`), `0 ≤ v < 1` → else `"roi.x: …"` / `"roi.y: …"`.
  5. `w`: required finite, `w > 0` and `x + w ≤ 1 + 1e-9` → else `"roi.w: …"`; `h` likewise with `y` → `"roi.h: …"`.
  6. `metric`: optional string via `Metric::parse`, default `MeanAbsDiff` → else `"roi.metric: …"`.
  7. `enter`: required finite, `0 < enter ≤ metric.max_threshold()` → else `"roi.enter: …"`.
  8. `exit`: optional finite, default `enter / 2.0`, `0 ≤ exit ≤ enter` → else `"roi.exit: …"`.
  9. `dwell_ms`, `cooldown_ms`: optional `as_u64` (floats/negatives refused), `≤ TIMING_MAX_MS`, defaults `DWELL_DEFAULT_MS` / `COOLDOWN_DEFAULT_MS`.
  10. `pixel_threshold`: optional `as_u64` in 1..=255, default `PIXEL_THRESHOLD_DEFAULT`.
  11. `evidence`: optional bool, default `true`.
  12. `evidence_pre`, `evidence_post`: optional `as_u64` in 0..=`EVIDENCE_FRAMES_MAX`, default `EVIDENCE_FRAMES_DEFAULT`.
- **S2 `to_json`** — object with exactly the 14 keys above; `x,y,w,h,enter,exit` as JSON floats (unrounded), `metric` as `as_str`, the integers as integers, `evidence` bool. `from_json(&s.to_json()) == Ok(s)`.
- **S3 `check_set(specs)`** — `len > MAX_ROIS` → `format!("roi: at most {MAX_ROIS} ROIs")`; then the first repeated name (scan order) → `format!("roi.name: duplicate {name}")`; else `Ok(())`.

### B4-D decode (worker feature)
- **D1 `decode_luma(jpeg)`**: `options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::Luma).set_max_width(MAX_DECODE_DIM).set_max_height(MAX_DECODE_DIM)`; `JpegDecoder::new_with_options(ZCursor::new(jpeg), options)`; `decode()` (error `e` → `format!("jpeg decode: {e}")`); `info()` (None → `"jpeg decode: no image info"`); `width/height` from info as `u32`; if `luma.len() != width * height` → `"jpeg decode: luma size mismatch"`. Return `LumaFrame { width, height, luma }`. Every error starts with `"jpeg decode:"`.

### B4-P geometry, sampling, metrics
- **P1 `roi_rect_px(rect, width, height)`**: with `W = width as f64`, `H = height as f64`: `x0 = round(rect.x·W)`, `x1 = round((rect.x + rect.w)·W)`, `y0 = round(rect.y·H)`, `y1 = round((rect.y + rect.h)·H)`, each clamped to `[0, W]` / `[0, H]` then converted to `u32`; `w = x1 − x0` and `h = y1 − y0` (saturating). `None` when `w < 2 || h < 2`; else `PxRect { x: x0, y: y0, w, h }`. (`f64::round` = half away from zero.)
- **P2 `sample_step(px)`**: `area = w·h` (u64); `s = max(1, ceil(sqrt(area / MAX_SAMPLES as f64)))`; while `ceil(w/s) · ceil(h/s) > MAX_SAMPLES` → `s += 1`. Return `s` (the smallest step within the bound; the sqrt estimate is a lower bound).
- **P3 `sample_roi(frame, px)`**: `step = sample_step(px)`; for `j` in `(0..px.h).step_by(step)` (outer) and `i` in `(0..px.w).step_by(step)` (inner) push `frame.luma[(px.y + j) · frame.width + (px.x + i)]` read with `get` (missing → 0; never panics). Return `Samples { width: frame.width, height: frame.height, rect: *px, step, values }`.
- **P4 `metrics(cur, base, pixel_threshold)`**: `None` unless `width`, `height`, `rect`, `step` and `values.len()` are equal and `len > 0`. With `n = len`, `thr = max(pixel_threshold, 1)`, `d_k = |cur_k − base_k|` (as i32): `mean_luma = Σcur / n`; `luma_delta = Σcur / n − Σbase / n`; `mean_abs_diff = Σd / n / 255 · 100`; `changed_px = #{d_k ≥ thr} / n · 100` (all f64, unrounded).
- **P5 `Metrics::value(metric)`**: `LumaDelta` → `luma_delta.abs()`, `MeanAbsDiff` → `mean_abs_diff`, `ChangedPx` → `changed_px`. `to_json` → `{"mean_luma","luma_delta","mean_abs_diff","changed_px"}`, each `round2`.
- **P6 `Baseline::to_json`** → `{"frame_id","sha256","width","height","rect":{"x","y","w","h"},"step","mean_luma": round2(mean of samples.values, 0.0 when empty)}`.

### B4-O observe
- **O1 `observe(spec, frame, baseline, mono_ns)`**, first match wins (`u(r)` = `(Observation::Unknown { mono_ns, reason: r }, None)`):
  1. `roi_rect_px(&spec.rect, frame.width, frame.height)` is `None` → `u(RoiTooSmall)`.
  2. `baseline` is `None` → `u(NoBaseline)`.
  3. baseline `samples.width/height` ≠ frame dims, or `samples.rect` ≠ the px rect → `u(BaselineMismatch)`.
  4. `cur = sample_roi(frame, &px)`; `metrics(&cur, &b.samples, spec.pixel_threshold)` is `None` → `u(BaselineMismatch)`.
  5. `(Observation::Value { mono_ns, value: m.value(spec.metric) }, Some(m))`.

### B4-J judgment state machine (`RoiJudge`)
- **J1 `new()`** → `state Unknown`, `reason Some(ConfigChanged)`, `seq 0`, `candidate None`, `last_mono_ns None`, `last_switch_mono_ns None`.
- **J2** `state()`, `reason()`, `seq()` return the fields.
- **J3 `step(spec, obs)`** — at most one transition per call. `emit(to, reason, mono, value)`: build `Transition { from: state, from_reason: self.reason, to, reason, seq: seq + 1, mono_ns: mono, value }`; then `state = to`, `self.reason = reason`, `seq += 1`, `candidate = None`; if `from` and `to` are both in {Normal, Changed} → `last_switch_mono_ns = Some(mono)`; return it.
  1. `Value { value }` with non-finite `value` → handled exactly as `Unknown { mono_ns, reason: DecodeError }`.
  2. `Unknown { mono_ns, reason: r }` → `candidate = None`; `last_mono_ns = Some(mono_ns)`; if `state == Unknown && self.reason == Some(r)` → `None`; else `Some(emit(Unknown, Some(r), mono_ns, None))`.
  3. `Value { mono_ns: t, value: v }`: `prev = last_mono_ns`; `last_mono_ns = Some(t)`; `gap = max(spec.dwell_ms, GAP_MIN_MS)` in ns.
     - **gap** — `prev` is `Some(p)` and `t.saturating_sub(p) > gap`: `out = if state == Unknown && self.reason == Some(AnalysisGap) { None } else { Some(emit(Unknown, Some(AnalysisGap), t, None)) }`; then seed `candidate = Some(Candidate { to: target(v), since_mono_ns: t })` using the (now Unknown) state's target rule; return `out`. No dwell completes in a gap step.
     - **target** by current state: `Unknown` → `Changed` if `v ≥ spec.enter` else `Normal`; `Normal` → `Changed` if `v ≥ spec.enter` else none; `Changed` → `Normal` if `v ≤ spec.exit` else none.
     - none → `candidate = None`, return `None`.
     - if `candidate` is `None` or its `to` differs → `candidate = Some(Candidate { to, since_mono_ns: t })`.
     - dwell: if `t.saturating_sub(since) < dwell_ns` → `None` (candidate kept).
     - cooldown: if `state ∈ {Normal, Changed}` and `last_switch_mono_ns = Some(ls)` and `t.saturating_sub(ls) < cooldown_ns` → `None` (candidate kept).
     - else `Some(emit(to, None, t, Some(v)))`.
- **J4 `to_json`** → `{"state": as_str, "reason": as_str | null, "transition_seq": seq, "candidate": null | {"to": as_str, "since_mono_ns": since}}` (integers as JSON numbers).
- **J5 `Transition::to_json`** → `{"from","from_reason","to","reason","transition_seq","mono_ns","value"}`; reasons `as_str` or null; `value` = `round2(v)` or null.

### B4 tests (Opus, `tests/analysis_contract.rs`, 21): what they prove
Wire names; spec defaults, canonical round-trip, 26 refusal cases each naming the key, order of checks; set bound/duplicates; decode of real ffmpeg JPEGs (black/white/split/colour→luma/1-component gray) and refusals (over 4096, garbage, empty, truncated); rect rounding and 2×2 minimum; smallest bounded step (property over sizes); grid order; metric values, threshold `≥`, signed delta vs magnitude, rounding, geometry guard; baseline identity JSON; observe precedence (too small > no baseline > mismatch by size or rect) and metric selection; decoded split vs black baseline per half; judge start state; dwell, hysteresis band, inclusive exit and cooldown; interrupted dwell; from-UNKNOWN target rule; **V-02 regression** (stall while normal → immediate `unknown(stale)`, reason change, fresh dwell to recover, candidate cleared); **no inferred transitions** across an analysis gap (strict `>`; gap limit grows with dwell); dwell 0; config change; non-finite → decode_error; transition JSON.

## Batch 5a — Rust pure inspection state: V-01/V-02/V-04 (`crates/loom-video/src/inspect.rs`)

### B5a lease
| File | Holder during B5a |
|---|---|
| `crates/loom-video/src/inspect.rs` | **Sonnet**: bodies C1–C6, I0–I7, E1–E5 only (replace every `unimplemented!`). |
| `lib.rs` (`pub mod inspect;`), `tests/inspect_contract.rs`, `analysis.rs` (accepted B4) and every other file | Opus / not leased |

Pre-written by Opus: constants (`ROI_HISTORY`, `MAX_PENDING_EVIDENCE`, `EVIDENCE_BYTES_MAX`, `EVIDENCE_POST_GRACE_MS`, `EVIDENCE_PIN_TTL_MS`, `METRIC_PERIOD_MS`, `EVIDENCE_SCHEMA`, `PIXEL_NOTE`), types (`RoiConfig`, `ConfigError`, `Liveness`, `FrameRef`, `Inspector` and its private fields, `EvidenceTrigger`, `EvidencePlan`), imports, every signature including the private `fragment`/`record` helpers. No new fns/types/imports/fields; local closures are fine. **A missing declaration is a contract gap: return the lease.**

**Acceptance**: `cargo test -p loom-video` (all targets; `inspect_contract` 10/10) and `cargo clippy -p loom-video --tests` with 0 warnings (scaffold unused-import/dead-code warnings disappear). Expected red at handoff: 10/10 `inspect_contract` panic on `unimplemented`.

Common: never panic; `ns = ms.saturating_mul(1_000_000)`; ns values in payloads are `loom_proto::num::U64` (serialize as decimal strings) except `transition.mono_ns` (number, from `Transition::to_json`); "config order" = `RoiConfig::specs` order.

### B5a-C config
- **C1 `ConfigError`** `message`: `RevMismatch` → `format!("config_rev mismatch: current {current}")`; `Invalid(m)` → `m`. `detail`: `RevMismatch` → `{"reason":"config_rev_mismatch","config_rev":current}`; `Invalid(m)` → `{"reason":"invalid","error":m}`.
- **C2** `json` → `{"config_rev": rev, "rois": [s.to_json() …]}`; `get` by name.
- **C3 `put(spec, expect_rev)`**: `expect_rev` is `Some(r)` with `r ≠ rev` → `Err(RevMismatch{current: rev})`. Build the candidate list (replace the same name in place, else append); `check_set` error `m` → `Err(Invalid(m))`; else store, `rev += 1`. Every successful put is an edit, even an identical one.
- **C4 `remove(name, expect_rev)`**: revision check first (as C3). Absent → `Ok(false)`, no revision change. Present → remove, `rev += 1`, `Ok(true)`.
- **C5 `replace(specs, expect_rev)`**: revision check; `check_set` → `Invalid`; store; `rev += 1`.
- **C6 `parse_list(v)`**: not an array → `Err("rois: expected an array")`; item `k` error `e` → `Err(format!("rois[{k}]: {e}"))`; then `check_set` (its message unprefixed); `Ok(specs)`.

### B5a-I inspector
- **I0 `Liveness::reason`**: `Live` → `None`, `NotLive` → `Some(NotLive)`, `Stale` → `Some(Stale)`.
- **I1 `sync(cfg, now)`**
  - `synced == Some(cfg.rev)` → `vec![]`.
  - Otherwise first set `synced = Some(cfg.rev)` and drop every judge/baseline/last/history of names no longer in `cfg` (no event). Then, in config order:
    - New name → `RoiJudge::new()` and empty history, no event.
    - Name whose spec differs from the previous spec: if the `rect` differs, remove its baseline and last metrics. Then `judge.step(new_spec, Observation::Unknown{mono_ns: now, reason: ConfigChanged})`; a transition → `record(name, fragment(name, &t, None, None))` pushed to the output.
    - Unchanged spec → nothing.
  - Finally `specs = cfg.specs.clone()`.
- **I2 `round(liveness, frame, now)`**
  - Never synced, or no specs → `vec![]` and no counter changes.
  - `NotLive`/`Stale`: for each spec, `step(spec, Unknown{mono_ns: now, reason: liveness.reason()})`; transitions → `record(fragment(name, &t, None, None))`. The frame is ignored and no counters change.
  - `Live`, `frame == None`, or `frame.id ≤ last_frame_id` → `vec![]`.
  - `Live` with a new frame `f`:
    - Counters: if `last_frame_id > 0`, `skipped += f.id − last_frame_id − 1`. Then `last_frame_id = f.id`; `analysed += 1`.
    - `f.luma` is `Err` → `decode_errors += 1`; each spec steps `Unknown{mono_ns: f.recv_mono_ns, reason: DecodeError}`; transitions → `fragment(name, &t, Some(&f), None)`.
    - `Ok(l)` → for each spec, `(obs, m) = observe(spec, l, baselines.get(name), f.recv_mono_ns)`. If `m` is `Some`, set `last[name] = (m, f.id)`. A transition → `fragment(name, &t, Some(&f), m.as_ref())`.
    - Every fragment goes through `record`. Output is in config order.
- **I3 `set_baseline(roi, frame_id, sha256, luma)`**
  - No specs → `Err("roi: no ROIs configured")`. `Some(name)` that is not configured → `Err(format!("roi.name: unknown ROI {name}"))`.
  - Targets are that one ROI or all of them in config order. For each: `roi_rect_px(&spec.rect, luma.width, luma.height)`.
    - `None` → item `{"roi", "baseline": null, "reason": "roi_too_small"}`.
    - Else insert `Baseline{frame_id, sha256, samples: sample_roi(luma, &px)}` (replacing any previous one) → item `{"roi", "baseline": b.to_json(), "reason": null}`.
  - No judge step, no history entry.
- **I4 `seq(roi)`** → `judges.get(roi).map(RoiJudge::seq)`.
- **I5 `json()`** → `{"config_rev": synced | null, "analysis": A, "rois": [per spec in config order: {"roi", "state", "reason", "transition_seq", "candidate": judge.to_json()["candidate"], "baseline": b.to_json() | null, "metrics": last m.to_json() | null, "metrics_frame_id": id | null, "history": [oldest → newest]}], "note": PIXEL_NOTE}`. `A = {"analysed", "skipped", "decode_errors", "last_frame_id": (0 → null)}`.
- **I6 `metric_json()`** → `{"config_rev", "analysis": A, "rois": [{"roi", "state", "reason", "transition_seq", "metrics", "metrics_frame_id"}], "note": PIXEL_NOTE}`.
- **I7 `fragment(roi, t, frame, metrics)`** → `{"roi", "config_rev": synced, "state": t.to.as_str(), "reason": t.reason as str | null, "transition_seq": t.seq, "transition": t.to_json(), "frame_id": f.id | null, "sha256": f.sha256 | null, "recv_utc_ns": U64 | null, "recv_mono_ns": U64 | null, "metrics": m.to_json() | null, "baseline": {"frame_id", "sha256"} of this ROI's baseline | null, "evidence": t.to == Changed && spec.evidence, "note": PIXEL_NOTE}`. **`record(roi, v)`** pushes a clone onto that ROI's history (pop the oldest beyond `ROI_HISTORY`) and returns `v`.

### B5a-E evidence plan
- **E1 `EvidencePlan::new(trigger, spec, fps, ring_first)`**
  - `pre_n = spec.evidence_pre`. `start = max(max(trigger.frame_id − pre_n, 1), ring_first.unwrap_or(trigger.frame_id))` (subtraction saturating). `pre = start..trigger.frame_id`.
  - `partial = ["pre_unavailable"]` when `pre.len() < pre_n`.
  - `post = []`, `post_want = spec.evidence_post`.
  - `deadline_mono_ns = trigger.mono_ns + (EVIDENCE_POST_GRACE_MS + post_want · 1000 / max(fps, 1)) ms` (saturating).
- **E2 `offer(latest)`**: `next = trigger.frame_id + 1 + post.len()`; `end = min(latest, trigger.frame_id + post_want)`. Append `next..=end` to `post` and return those ids (empty when `next > end`). `post` holds the post ids offered; a post frame that later turns out missing is omitted by `fit`.
- **E3 `drop_frame(id, reason)`**: remove `id` from `pre` if present; append `reason` to `partial` unless already there. Post and trigger ids stay listed (`fit` omits them when they have no size).
- **E4** `complete` → `post.len() == post_want`; `due(now)` → `complete() || now >= deadline_mono_ns`.
- **E5 `fit(sizes)`**
  - Not `complete()` → add `"post_deadline"` (once).
  - Candidates in priority order: trigger, then `pre` descending (nearest first), then `post` ascending.
  - For each candidate: no `sizes` entry → add `"frame_unavailable"` (once) and skip. Else `b64 = (size + 2) / 3 · 4`; if `used + b64 > EVIDENCE_BYTES_MAX` → add `"bytes_cap"` (once) and skip; else keep it and add `b64` to `used`.
  - Return the kept ids ascending with roles `"pre"` / `"trigger"` / `"post"`.
  - `pre`/`post` are not modified, so a repeated call gives the same result and adds no reason twice.

### B5a tests (Opus, `tests/inspect_contract.rs`, 10): what they prove
- Revisions: put/replace in place, `expect_rev` (checked first), 8-ROI bound, duplicates, absent remove is not an edit, error message/detail.
- List parsing names the item.
- Liveness reasons.
- Sync: silent add, threshold change → `config_changed` transition, removal, rect change drops the baseline.
- Full round payload byte-for-byte: no_baseline → normal → changed with `evidence:true`; skipped/analysed counters; frames never analysed twice; not_live/stale with no frame identity; decode_error counted; last metrics kept; `metric_json`.
- No ROIs → no work.
- Baseline targets/unknown/too small.
- History bounded and contiguous.
- Evidence: pre window vs ring, deadline, offers bounded, `drop_frame`, fit roles/missing/byte cap (≤ allowed)/deadline, reasons recorded once.

## Batch 5b — Rust wiring: ROI RPCs, analysis thread, events, wait/read, evidence bundle, CLI

### B5b lease
| File | Holder during B5b |
|---|---|
| `crates/loom-video/src/worker.rs` | **Sonnet**: W1–W17 |
| `crates/loom-video/src/gateway.rs` | **Sonnet**: G1–G8 |
| `crates/loom-video/src/cli.rs` | **Sonnet**: C0–C3 |
| `crates/loom-video/src/model.rs` | **Sonnet**: M1–M2 only |
| `lib.rs`, `analysis.rs`, `inspect.rs` (accepted), `media.rs`, every test | Opus / not leased (read-only) |

Pre-written by Opus, do not change:
- `lib.rs`: `KIND_ROI_CONFIG`, `KIND_ROI_STATE`, `KIND_ROI_METRIC`, `KIND_EVIDENCE`, `ROI_IDLE_CHANNEL`.
- `model.rs`: `ArtifactKind::Evidence`, with `unimplemented!` arms in `prefix`/`ext`/`content_type`.
- `worker.rs`:
  - Imports (analysis/inspect/kinds/`base64::Engine`).
  - `Shared` fields `roi_cfg`, `inspector`, `evidence`, `next_evidence` and their initializers.
  - `VideoWorker.roi`.
  - Stubs W1–W16.
- `gateway.rs`:
  - Imports, `ROI_STATES`, `SNAP_ROI_STATES`, `SNAP_EVIDENCE`.
  - `forward!` declarations `RoiPut`, `RoiRemove`, `RoiReplace`, `RoiList`, `RoiBaseline`.
  - The `roi_wait_match` stub.
  - Reducer fields `roi_config`, `roi_states`, `roi_metrics`, `evidence`.
- `cli.rs`: imports `RoiSpec`, `RoiConfig`; the `parse_rect` stub.

New fns/types/fields/imports are contract gaps: return the lease. Local closures/variables are fine.

**Acceptance**: `cargo test -p loom-video --no-fail-fast` (all targets, incl. `roi_contract` 6/6 and `roi_e2e` 3/3 with ffmpeg present; fixture tests may print SKIP only without ffmpeg/python) and `cargo clippy -p loom-video --tests` with 0 warnings. Expected red at handoff: `roi_contract` 0/6, `roi_e2e` 0/3.

**Global rules**
- Lock order: `roi_cfg` → `inspector` → `evidence` → `ring` → `pins`, never reversed. Never hold `inspector`/`evidence` while taking `roi_cfg`.
- JPEG decode happens outside every lock.
- Events never carry JPEG bytes, except the evidence file itself.
- Untrusted ROI names are validated by `RoiSpec`.
- Every new payload gets `"capture_id"` and `"capture_epoch": U64(epoch)` when it belongs to a capture.
- Memory-only edits (config, baseline) are **allowed while STOP is latched**. Judgments go UNKNOWN(`not_live`) instead.

### B5b-M model.rs
- **M1** `ArtifactKind::Evidence`: `prefix` `"evid"`, `ext` `"json"`, `content_type` `"application/json"`.
- **M2** `parse_artifact_id`: accept the `"evid"` prefix (same strict capture/number rules). `artifact_path` then yields `<capture_dir>/evid-<cap>-<n>.json`. `media.rs` is unchanged and serves it with the B2 disposition header.

### B5b-W worker.rs
- **W1 `analyse(sh)`** (thread `video-analysis`, spawned in `open()` right after the supervisor thread via the existing `thread()` helper; the handle is not kept):
  - Set `period = 1000 / ANALYSIS_HZ` ms, `last = 0`, `next_metric = now + METRIC_PERIOD_MS`.
  - Loop:
    - `ended = sh.finished` (Acquire).
    - `sh.analysis_round(&mut last)`.
    - If `ended`: `sh.step_evidence(true)`, `sh.emit_roi_metric()`, return.
    - If `now ≥ next_metric`: emit the metric and advance `next_metric` by the period.
    - Sleep `period`.
- **W2 `liveness()`**
  - STOP latch on, or `stop_reason().is_some()` → `NotLive`.
  - Otherwise by `state["state"]`:
    - `"live"` → `Stale` if `last_frame_at` is `None` or older than `STALL_MS`, else `Live`.
    - `"stalled"` → `Stale`.
    - Anything else → `NotLive`.
- **W3 `analysis_round(last_decoded)`**
  - `now = now_ns()`; `live = liveness()`.
  - Decode: when `live == Live` and `roi_cfg` has specs (brief lock), clone `ring.latest()`. If its id is `> *last_decoded`, set `*last_decoded = id` and `luma = decode_luma(&f.data)` (outside locks).
  - Then lock `roi_cfg` (held to the end), lock `inspector`:
    - `frags = ins.sync(&cfg, now)`.
    - Append `ins.round(live, frame.map(|(f, l)| FrameRef { id, sha256, recv_mono_ns, recv_utc_ns, luma: l.as_ref().map_err(String::as_str) }), now)`.
  - Drop `inspector`; `publish_roi(&cfg, frags)`; drop `roi_cfg`.
  - `step_evidence(false)`.
  - Because edits take `roi_cfg`, a config revision can never interleave with a round.
- **W4 `publish_roi(cfg, frags)`** — for each fragment, in order:
  - Insert `capture_id`, `capture_epoch`; `priority(KIND_ROI_STATE, p)`.
  - If `p["evidence"] == true` → `start_evidence(cfg, &p)`.
- **W5 `emit_roi_metric()`**
  - Skip when `roi_cfg` has no specs.
  - `p = inspector.metric_json()` + `capture_id`, `capture_epoch`, `"inspection_hz_max": ANALYSIS_HZ`.
  - Data lane `KIND_ROI_METRIC` (count `emit_errors` on failure like `emit_health`).
- **W6 `roi_config_event(cfg, change)`** → `priority(KIND_ROI_CONFIG, {"capture_id", "capture_epoch", "config_rev", "rois", "change", "note": PIXEL_NOTE})`. `config_rev`/`rois` come from `cfg.json()`.
  - `open()` calls it once, after the analysis thread starts, with `{"op":"open","roi":null,"expect_rev":null}` (lock `roi_cfg` for it).
- **W7 `start_evidence(cfg, frag)`**
  - `spec = cfg.get(frag.roi)`; `None` → return.
  - Lock `evidence`. If it already holds `MAX_PENDING_EVIDENCE` → priority `KIND_EVIDENCE {capture_id, capture_epoch, "state":"skipped", "reason":"evidence_busy", "roi", "transition_seq"}`; return.
  - `n = next_evidence + 1` (fetch_add). Trigger: `frame_id` from `frag["frame_id"]`, `mono_ns` = `frag["recv_mono_ns"]` parsed from its decimal string.
  - `plan = EvidencePlan::new(trigger, spec, self.cfg.fps, ring.range()["first_frame_id"].as_u64())`.
  - For each pre id plus the trigger id:
    - Ring frame → `pins.pin(&f, PinOwner::Evidence(n), now, EVIDENCE_PIN_TTL_MS)`; on `Err(r)` → `plan.drop_frame(id, r.reason())`.
    - Not in the ring → `drop_frame(id, "frame_unavailable")`.
  - `ctx = {"config_rev": frag.config_rev, "spec": spec.to_json(), "transition": frag.transition, "baseline": frag.baseline, "metrics": frag.metrics, "source": self.cfg.public()}`.
  - Push `(plan, ctx)`; priority `KIND_EVIDENCE {…, "state":"collecting", "evidence_id": n, "roi", "transition_seq", "trigger_frame_id", "pre": plan.pre, "post_want", "partial"}`.
- **W8 `step_evidence(ended)`** — lock `evidence`. `latest` = ring latest id (or 0). `now`. For each `(plan, ctx)`:
  - `plan.offer(latest)`. Pin each new post id as in W7, or `drop_frame`.
  - STOP latched → payload `{…, "state":"aborted", "reason":"stop_latch", "evidence_id", "roi", "transition_seq"}`; `pins.unpin_owner(Evidence(n))`; remove the plan; nothing is written.
  - Else if `ended || plan.due(now)`:
    - If `ended && !plan.complete()` → `plan.drop_frame(0, "capture_ended")` (id 0 never exists; this only records the reason).
    - `write_evidence(&mut plan, &ctx)`; remove the plan.
  - Publish the resulting payloads after releasing the `evidence` lock.
- **W9 `write_evidence(plan, ctx)`**
  - `sizes`/frames: for every pre/trigger/post id, `lookup(id)` (ring, then pins).
  - `list = plan.fit(&sizes)`. Frames: `[{role, frame_id, sha256, recv_utc_ns: U64, recv_mono_ns: U64, size, jpeg_b64: STANDARD.encode(bytes)}]` in list order.
  - `aid = artifact_id(Evidence, id, n)`.
  - Manifest = `ctx` merged with `{"schema": EVIDENCE_SCHEMA, "artifact_id", "capture_id", "capture_epoch", "evidence_id", "roi", "transition_seq", "partial", "frames", "note": PIXEL_NOTE}`. `bytes = serde_json::to_vec`.
  - Quota: `scan_usage(home, session).refuse(len)`; `Some(reason)` → `{"state":"skipped", "reason", "quota": usage.json()}`.
  - Write: `ensure_private_dir(capture_dir)`, `create_private_new(<aid>.json)`, `write_all` + `sync_all`. An IO error → `{"state":"failed", "error": msg}`.
  - Every outcome calls `pins.unpin_owner(Evidence(n))`.
  - Success payload: `{capture_id, capture_epoch, "state":"written", "evidence_id", "artifact_id", "roi", "transition_seq", "frames":[{role, frame_id, sha256, size}], "partial", "size": bytes.len(), "media_path": "/api/v1/video/artifact/<aid>", "note"}`. All payloads carry `evidence_id`/`roi`/`transition_seq`.
- **W10 `Shared::roi_baseline(roi, frame_id)`**
  - Frame: `Some(id)` → `lookup` (miss → `NotFound` with the snapshot message/detail); `None` → `ring.latest()`, or `NotFound "no frame yet"`.
  - `luma = decode_luma(&f.data)`; `Err(e)` → `Internal` with `format!("baseline frame {id} could not be decoded: {e}")`.
  - Lock `roi_cfg`, lock `inspector`:
    - `frags = sync(&cfg, now)`.
    - `items = set_baseline(roi.as_deref(), f.id, &f.sha256, &luma)`.
    - Drop `inspector`; `publish_roi(&cfg, frags)`.
    - `items` is `Err(m)` → `Usage(m)`.
  - `roi_config_event(&cfg, {"op":"baseline", "roi", "frame_id", "sha256", "baselines": items})`.
  - Return `{"capture_id", "frame_id", "sha256", "retained_by", "config_rev", "baselines": items}`.
- **W11 `expect_rev_arg`** — absent/null → `None`; `as_u64` → `Some`; else `Usage "expect_rev: expected a non-negative integer"`. **`config_err`**: `RevMismatch` → `Busy` with `e.message()`/`e.detail()`; `Invalid` → `Usage` with the same.
- **W12 `idle_draft(cx, payload)`** — like `Shared::draft` for `KIND_ROI_CONFIG`: `ts = cx.clock().ts(TsSource::HostReadComplete)`, `origin: Origin::Device`, `env: None`, every other optional field `None`, empty metrics/quality.
- **W13 `roi_edit(cx, call)`** — `expect = expect_rev_arg(call)?`; lock `self.roi`. By `call.method`:
  - `video.roi.put`: `spec = RoiSpec::from_json(&call.args["roi"])`, an error → `Usage(msg)`; `cfg.put(spec, expect).map_err(config_err)?`. Change `{"op":"put","roi":name,"expect_rev":expect}`. Return `{config_rev, rois, "roi": name}`.
  - `video.roi.remove`: `name = call.args["roi"].as_str()`, else `Usage "roi: expected a ROI name"`; `removed = cfg.remove(name, expect)?`. Emit only when removed. Return `{config_rev, rois, "roi": name, "removed"}`.
  - `video.roi.replace`: `specs = RoiConfig::parse_list(&call.args["rois"])` → `Usage`; `cfg.replace(specs, expect)?`. Change `{"op":"replace","roi":null,"expect_rev"}`. Return `{config_rev, rois}`.
  - Each applied edit → `emit_config(cx, &cfg, change)` while still holding the lock.
- **W14 `emit_config(cx, cfg, change)`**
  - With a current capture (`self.cur`, running or ended) → `sh.roi_config_event(cfg, change)`.
  - Otherwise `cx.emitter(ROI_IDLE_CHANNEL).priority(idle_draft(cx, {"capture_id": null, "config_rev", "rois", "change", "note"}))`; ignore an emit error.
- **W15 `roi_list()`** — lock `self.roi`, then the current capture's `inspector`. Returns `{config_rev, rois, "capture_id": id|null, "inspect": ins.json()|null, "limits": {"max_rois": MAX_ROIS, "max_samples": MAX_SAMPLES, "analysis_hz": ANALYSIS_HZ, "history": ROI_HISTORY, "pending_evidence": MAX_PENDING_EVIDENCE}, "note": PIXEL_NOTE}`.
- **W16 `VideoWorker::roi_baseline(call)`**
  - `target(call)`: no capture → `NotOpen`.
  - `frame_arg`; `roi` = absent/null → `None`, string → `Some`, else `Usage "roi: expected a ROI name"`.
  - `offload("video-roi-baseline", move || sh.roi_baseline(roi, id))`.
- **W17 wiring**
  - `on_call` routes:
    - `video.roi.put|remove|replace` → `ready(self.roi_edit(cx, &call))`.
    - `video.roi.list` → `ready(Ok(self.roi_list()))`.
    - `video.roi.baseline` → `self.roi_baseline(&call)`.
  - `status()` adds `"roi": {"config": cfg.json(), "inspect": current capture's ins.json() | null}` in every branch (lock order `roi` → `inspector`).
  - `open()` spawns `analyse(sh.clone())` after the supervisor and then emits the W6 open event.

### B5b-G gateway.rs
- **G1 `forward!` key arms**
  - `"roi"`: absent/null → skip; object or string → pass through; else `usage("roi: expected an ROI object or name")`.
  - `"rois"`: array → pass through; else `usage("rois: expected an array")`.
  - `"expect_rev"`: `opt_u64(o, k, u64::MAX)?` → pass through when present.
- **G2 `roi_wait_match(status, roi, roi_state, after_seq)`**
  - `roi_state` not in `ROI_STATES` → `Err(format!("roi_state: expected one of {}", ROI_STATES.join(", ")))`.
  - `roi` absent from `status.roi.config.rois[].name` → `Err(format!("roi {roi} is not configured"))`.
  - `row` = the `status.roi.inspect.rois[]` entry with that `roi`; inspect null or no row → `Ok(None)`.
  - `after_seq = Some(s)` → the first `history` entry (oldest first) with `transition_seq > s` and the state matching (if given) → `Ok(Some(entry))`, else `Ok(None)`.
  - `None` → the state matches (or is not given) → `Ok(Some(row without "history"))`, else `Ok(None)`.
- **G3 `kinds_for`**
  - `"roi"` → `[KIND_ROI_CONFIG, KIND_ROI_STATE]`; `"metric"` → `[KIND_ROI_METRIC]`; `"evidence"` → `[KIND_EVIDENCE]`.
  - `"all"` → the four old kinds + `KIND_ROI_CONFIG`, `KIND_ROI_STATE`, `KIND_EVIDENCE` (not the 1 s metric).
  - Error text lists every kind.
- **G4 `Wait`**
  - Allowed keys add `roi`, `roi_state`, `new`. `roi` is a string ≤ 32 chars. `roi_state`/`new` without `roi` → usage. None of frames/state/roi → `usage("give frames, state and/or roi")`.
  - Each poll: `m = roi_wait_match(&st, roi, roi_state, after)`; `Err(e)` → `usage(e)`.
  - With `new`, `after` is the ROI's `transition_seq` from the first poll where its row exists (0 if the row never existed). It is re-taken as 0 when `st.capture_id` differs from the first poll's.
  - `ok_roi = m.is_some()`. Success returns `{"matched": true, "status": st, "roi": m}` (`roi` null without a ROI condition).
  - The terminal-capture rule applies only after the match check.
- **G5 `VideoReducer`**
  - `kinds` add the four kinds.
  - `apply`:
    - `roi_config` → `roi_config = Some(p)`.
    - `roi_state` → upsert by `(capture_id, roi)`, bound `SNAP_ROI_STATES`.
    - `roi_metric` → upsert by `capture_id`, bound `SNAP_CAPTURES`.
    - `evidence` → upsert by `(capture_id, evidence_id)`, bound `SNAP_EVIDENCE`.
  - `snapshot` adds `"roi_config"` (unfiltered; null when none), and `"roi_states"`, `"roi_metrics"`, `"evidence"` (filtered like the others).
- **G6 `classify`**
  - `video.roi.put|remove|replace` → `(R0, "edits the shared ROI inspection config (memory only)")`.
  - `video.roi.baseline` → `(R0, "sets ROI baselines from an exact frame (memory only)")`.
- **G7 `register`**
  - Specs (write = `Request`, roles WRITE):
    - `video.roi.put` `{roi: object, expect_rev: i}`.
    - `video.roi.remove` `{roi: s, expect_rev: i}`.
    - `video.roi.replace` `{rois: array, expect_rev: i}`.
    - `video.roi.baseline` `{capture_id: s, frame_id: i, roi: s}`.
  - Read spec: `video.roi.list` `{}`.
  - `video.read` kind enum: `state, health, snapshot, record, roi, metric, evidence, all`.
  - `video.wait` adds `roi: s`, `roi_state: {"enum": ROI_STATES}`, `new: b`.
  - Event kinds (version 1, required keys):
    - `roi_config` `["config_rev", "rois"]`.
    - `roi_state` `["capture_id", "roi", "state", "transition_seq"]`, pointers `/payload/state` (enum `ROI_STATES`), `/payload/roi` (string).
    - `roi_metric` `["capture_id", "rois"]`.
    - `evidence` `["capture_id", "state", "evidence_id"]`, pointer `/payload/state` (string).
  - Risk rows R0 for the four writes ("memory-only ROI config" / "memory-only ROI baseline").
- **G8 `help()`**
  - Examples:
    - `loom video roi put --name lamp --rect 0.1,0.1,0.2,0.2 --enter 12` (configure).
    - `loom video roi baseline` (baseline = latest exact frame).
    - `loom video wait --roi lamp --roi-state changed --new --timeout 30s` (wait for a judged change).
    - `loom video read --kind evidence` (before/after bundles).
  - Notes:
    - The `PIXEL_NOTE` text.
    - "ROI judgments are computed once by the daemon from the same frames the UI shows; UNKNOWN means no trustworthy judgment (not live, stale, no/mismatched baseline, decode error, analysis gap, config change)."
    - "Inspection runs at most 10 Hz, separately from render fps; skipped frames are counted and transitions are never inferred."

### B5b-C cli.rs
- **C0 `parse_rect`**: split on `,` into exactly 4 `f64`s, each finite. Else `Err(format!("expected x,y,w,h (normalized), got {s:?}"))`.
- **C1 `roi` subcommand group** (`subcommand_required`)
  - `put`: `--name` (required), `--rect` (required, `parse_rect`), `--enter <f64>` (required), plus optional:
    - `--metric` (`luma_delta|mean_abs_diff|changed_px`), `--exit <f64>`.
    - `--dwell <dur>`, `--cooldown <dur>` (`parse_duration` → ms).
    - `--pixel-threshold <u64>`, `--no-evidence` (flag → `"evidence": false`), `--pre <u64>`, `--post <u64>`.
    - `--expect-rev <u64>`.
    - Build `roi` with **only the given keys**, in this key order: `name, x, y, w, h, enter, metric, exit, dwell_ms, cooldown_ms, pixel_threshold, evidence, evidence_pre, evidence_post`; floats as f64.
    - Validate locally with `RoiSpec::from_json` (its message is the usage error).
    - `video.roi.put {"roi": …, "expect_rev"?}`.
  - `remove --name [--expect-rev]` → `{"roi": name, "expect_rev"?}`.
  - `list` → `{}`.
  - `replace --rois <text> [--expect-rev]` (**A8**; was `--json`, see RF-1):
    - Text is a JSON array, or an object with `"rois"` (a saved `roi list`).
    - Parse error → usage; `RoiConfig::parse_list` → usage (its message).
    - → `{"rois": [the given objects, unchanged], "expect_rev"?}`.
  - `baseline [--capture] [--frame <u64> ≥ 1] [--name]` → `{"capture_id"?, "frame_id"?, "roi"?}`.
- **C2 `wait`**: add `--roi <name>`, `--roi-state <unknown|normal|changed>`, `--new`.
  - `--roi-state`/`--new` without `--roi` → usage.
  - None of frames/state/roi → `"give --frames, --state and/or --roi"`.
  - Params add `roi`, `roi_state`, `new: true` (only when given).
- **C3 `read --kind`** values add `roi`, `metric`, `evidence`.

### B5b contract amendments (Opus, after review)
- **A4**: within bodies, fully qualified `std::collections::BTreeMap` (W9) and `base64::engine::general_purpose::STANDARD` are accepted. They add no imports.
- **A5**: `Wait` checks the ROI name with the literal `32` (equal to `analysis::ROI_NAME_MAX`, which gateway.rs does not import).
- The gateway freeze hash in the assignment message was mistyped (`bb6e2118`); the real one is `bb6e21f1…`, as recorded in STATUS.

### Amendment A8 / RF-1 fix (ROOT-authorized after source freeze)
- **Defect**: the runtime found that `loom video roi replace --json <payload>` panics in the integrated `loom` binary (exit 101, "Mismatch between definition and access of `json`"). The subcommand arg id `json` collides with the loom-wide global `--json` (bool) output flag: `loom-cli` `context()` calls `m.get_flag("json")` on the propagated id. The standalone `VideoCli::command()` test could not see it.
- **Contract C1 (replace)**: the payload arg is `Arg::new("rois").long("rois").required(true)`, read with `get_one::<String>("rois")`. Nothing else in C1 changes: parsing, `RoiConfig::parse_list` validation and the `{"rois": [...], "expect_rev"?}` output stay as they are. The global `--json` output flag is unchanged.
- **Lease (Sonnet)**: `crates/loom-video/src/cli.rs`, the `replace` subcommand arg definition and its `to_call` read only.
- **Tests (Opus)**:
  - `roi_contract.rs` uses `--rois`, and asserts that `replace --json …` is rejected by the standalone command.
  - New shared test `crates/loom-cli/tests/video_roi_replace.rs` (ROOT-assigned, Opus spec only) parses through the integrated `loom_cli::command(&domains())`, with and without the global `--json`. It asserts:
    - no panic, and the root `get_flag("json")` mirrors the global flag;
    - the exact `video.roi.replace` params from the real domain mapping;
    - no video subcommand defines an arg id or long of `json` / `idempotency_key` (`--timeout` stays tolerated by design: `context()` reads it with `try_get_one`).

### B5b tests (Opus): what they prove
- `roi_contract.rs` (6):
  - Registration, roles and R0 risk; schemas; read kinds; wait params; event kinds; reducer kinds; help text.
  - Hooks reasons.
  - CLI put (sparse keys, full keys, local validation and refusals), remove, list, replace (array, `roi list` recipe, empty clears), baseline.
  - Wait/read CLI.
  - `roi_wait_match` table (not configured, not yet judged, no capture, current state, history after seq).
  - `evid-` ids, paths and `application/json` serving with an attachment name.
- `roi_e2e.rs` (3, real FFmpeg, generated clip):
  - Config without a capture: idle-channel event, `expect_rev` Busy, usage errors, recipe list → replace, absent remove not an edit, baseline needs a capture.
  - **Full V-01/V-02/V-04 path**:
    - Open announces the config on the capture channel.
    - Baseline = an exact black frame (sha equals `video.frame`).
    - The worker publishes unknown → … → normal → changed, each transition once with contiguous seqs.
    - The changed payload carries metrics, baseline identity, `evidence:true` and the pixel note.
    - Evidence `collecting` → `written` with pre/trigger/post = T−2…T+2 and partial `[]`.
    - The file is 0600 and its manifest carries spec/config_rev/baseline/transition/source.
    - Every base64 JPEG is byte-exact (sha256).
    - Evidence pins are released.
    - EOF → UNKNOWN(`not_live`).
    - Metrics are published with `inspection_hz_max`.
    - The reducer rebuilds config/state/metric/evidence.
    - Reopen keeps the config and drops the baseline (`no_baseline`).
  - **STOP** → UNKNOWN(`not_live`) at once, nothing inferred, memory-only config edit still allowed.

## Batch 6 — UI: V-01/V-02 ROI list/overlay/history · V-04 evidence viewer · ROI editor/baseline

### B6 lease
| File | Holder during B6 |
|---|---|
| `ui/src/panels/video/roiView.ts` | **Sonnet**: bodies M1–M10 (replace each `throw new Error('B6-M…')`). |
| `ui/src/panels/video/videoPanel.ts` | **Sonnet**: P1–P7 inside `createVideoWorkbench` (incl. extending `handle`, `applySnap`, `draw`, `leaveArtifactView`, `dispose`). The import list and `registerPanel` manifest stay as Opus wrote them. |
| `videoModel.ts` | not leased (read-only) |
| `video.css`, `videoRoi.test.ts` and every other test | Opus (do not edit) |

Pre-written by Opus:
- `roiView.ts`: every type and constant, and the M1–M10 signatures.
- Panel imports: `b64Bytes`, `evidenceText`, `historyText`, `inspectLine`, `METRIC_LABEL`, `METRICS`, `readEvidenceManifest`, `ROI_HISTORY_MAX`, `ROI_KINDS`, `RoiBook`, `roiFormSpec`, `type EvidenceManifest`, `type RoiForm`.
- Manifest actions `roi_put` / `roi_remove` / `roi_baseline` (operate), and topic kinds `[...VIDEO_KINDS, ...ROI_KINDS]`.
- CSS `.vd-roi*`, `.vd-evid*`.

Local state and closures inside the factory are yours. A new export, type, import or CSS rule is a gap: return the lease.

**Acceptance** (from `ui/`):
- `npx vitest run src/panels/video --maxWorkers=2` → 5 files, 54/54.
- Isolated `tsc` app/node (as B3) → exit 0.
- `npx eslint src/panels/video` → exit 0.

Expected red at handoff: 14 of 15 `videoRoi.test.ts` tests (the manifest test passes). `ui/src/shell/videoWiring.test.ts` stays a ROOT S7 item.

Common rules:
- Every judgment shown comes from a daemon payload. **No metric arithmetic or state derivation in the UI**, except `Math.abs` for `luma_delta` display (M8).
- Untrusted text goes through `textContent` / `title` / attributes only.
- Map keys are `${captureId}\u0000${x}`.
- Number fields are finite numbers; integers are non-negative safe integers; ns fields are decimal strings (`/^\d{1,20}$/`) else `null`; sha256 is `/^[0-9a-f]{64}$/` else `null`.

### B6-M roiView.ts
- **M1 `readRoiConfig(p, seq)`**
  - `config_rev` must be an integer and `rois` an array, else `null`.
  - Each ROI object is skipped unless:
    - `name` matches `/^[A-Za-z0-9_-]{1,32}$/`;
    - `x`, `y`, `w`, `h` and `enter` are finite numbers.
  - Optional fields and their defaults:
    - `metric`: string, default `'mean_abs_diff'`.
    - `exit`: finite, default `enter / 2`.
    - `dwellMs` / `cooldownMs` from `dwell_ms` / `cooldown_ms`: integers, defaults 300 / 1000.
    - `evidence`: boolean, default `true`.
  - `captureId` is the string `capture_id`, else `null`.
- **M2 `readRoiState(p)`**
  - Required, else `null`:
    - `capture_id` and `roi` are strings;
    - `state` is in `ROI_STATE_LABEL`;
    - `transition_seq` is an integer.
  - Other fields:
    - `reason`: string or `null`.
    - `frameId`: integer `frame_id` or `null`.
    - `sha256` and `recvUtcNs` (`recv_utc_ns`) per the common rules.
    - `metrics`: `{meanLuma, lumaDelta, meanAbsDiff, changedPx}` when the object has all four finite fields, else `null`.
    - `baselineFrameId`: integer `baseline.frame_id` or `null`.
    - `configRev`: integer or `null`.
    - `evidence`: `evidence === true`.
- **M3 `readRoiMetric(p)`**
  - `capture_id` must be a string, else `null`.
  - `configRev`: integer or `null`.
  - From `analysis`: `analysed`, `skipped`, `decodeErrors` (integers, default 0); `lastFrameId` (integer or `null`).
  - `hzMax`: integer `inspection_hz_max` or `null`.
  - `rois`: array rows that have a string `roi`, a valid `state` and an integer `transition_seq`; other rows are skipped. Each row carries `reason`, `metrics` (as M2) and `metricsFrameId`.
- **M4 `readEvidence(p)`**
  - Required, else `null`:
    - `capture_id` and `roi` are strings;
    - `evidence_id` is an integer ≥ 1;
    - `state` is in `EVIDENCE_STATE_LABEL`.
  - `seq` (`transition_seq`) and `triggerFrameId` (`trigger_frame_id`): integer or `null`.
  - `artifactId` only when it matches `/^evid-v[0-9a-f]{12}-\d{1,12}$/`.
  - `frames`: rows with a valid role, integer `frame_id`, sha256 and integer `size`; invalid rows are skipped.
  - `partial`: its string items. `reason`: string or `null`. `size`: integer or `null`.
- **M5 `readEvidenceManifest(j)`** (strict: any invalid item → `null`)
  - `schema === EVIDENCE_SCHEMA`.
  - `artifact_id`, `capture_id` and `roi` are strings.
  - `frames` is an array in which every frame has a valid role, integer `frame_id`, sha256 (hex64), integer `size` and a string `jpeg_b64`.
  - Output adds `seq` (`transition_seq`, integer or `null`), `partial` (strings) and `recvUtcNs` (per the common rules).
- **M6 `RoiBook`**
  - `apply(kind, p, seq)` → `true` when something changed:
    - `video.roi_config`: read it (`null` → `false`). If `config` exists with a larger `seq` → `false`. Otherwise replace `config`. When `change.op === 'baseline'` and `capture_id` is a string, then for each `change.baselines` item with a string `roi` and an integer `baseline.frame_id`, set `baselines[cap,roi]`.
    - `video.roi_judgment`: read it (`null` → `false`). If the stored state for `(cap, roi)` has `seq >= s.seq` → `false`. Otherwise store it, push it to `history` (shift the oldest beyond `ROI_HISTORY_MAX`), and set the baseline when `baselineFrameId !== null`.
    - `video.roi_metric`: read it → `metrics.set(cap, m)`.
    - `video.evidence`: read it. When the key is new and `evidence.size >= EVIDENCE_MAX`, delete the oldest inserted key. Then set it (an existing key keeps its position).
    - Any other kind → `false`.
  - `applySnapshotState(state, throughSeq)`:
    - `roi_config` object → apply as config with `throughSeq`.
    - Each array item of `roi_states` → store the state (same seq rule) and its baseline, **without** history.
    - `roi_metrics` / `evidence` arrays → apply as above.
    - Non-array or `null` values are ignored.
- **M7 rows / history / evidence**
  - `rows(cap)`: no config → `[]`. Otherwise one row per config ROI, in config order:
    - Inputs: `s` = the state for `(cap, roi)` (`undefined` when `cap` is `null`); `m` = the matching row of `metrics.get(cap)`.
    - `{name, state: s?.state ?? 'unknown', reason: s ? s.reason : 'no_state', chip: roiChip(s)}`.
    - `value: fmtMetrics(m?.metrics ?? s?.metrics ?? null, spec.metric)`.
    - `` threshold: `진입 ≥ ${enter} · 해제 ≤ ${exit}` ``.
    - `` baseline: b !== undefined ? `기준 frame ${b}` : '기준 없음' ``.
    - `seq: s?.seq ?? 0`.
  - `historyFor(cap)`: `null` → `[]`; otherwise that capture's history, newest first.
  - `evidenceFor(cap)`: `null` → `[]`; otherwise that capture's rows, sorted by `n` descending.
- **M8 text**
  - `roiChip`:
    - `undefined` → `'판정 불가 · 상태 없음'`; `normal` → `'정상'`; `changed` → `'변화'`.
    - `unknown` → `` `판정 불가 · ${reason === null ? '?' : ROI_REASON_LABEL[reason] ?? reason}` ``.
  - `fmtMetrics`:
    - `null` → `'—'`.
    - Otherwise `v` is `|lumaDelta|` for `luma_delta`, `changedPx` for `changed_px`, else `meanAbsDiff`.
    - Result: `` `${METRIC_LABEL[metric] ?? metric} ${v.toFixed(1)} · 평균 밝기 ${meanLuma.toFixed(1)}` ``.
  - `inspectLine(m, rev)`:
    - `undefined` → `` `검사 대기 · 설정 rev ${rev ?? '—'} · ${PIXEL_NOTE_KO}` ``.
    - Otherwise `` `검사 ≤${hzMax ?? '?'} Hz · 분석 ${analysed} · 건너뜀 ${skipped} · 디코드 실패 ${decodeErrors} · 설정 rev ${m.configRev ?? rev ?? '—'} · ${PIXEL_NOTE_KO}` ``.
  - `historyText` → `` `#${seq} ${roi} → ${roiChip(s)} · frame ${frameId ?? '—'} · ${fmtWallNs(recvUtcNs)}` ``.
  - `evidenceText`, with `L = EVIDENCE_STATE_LABEL[state]`:
    - `collecting` → `` `증거 #${n} ${roi} ${L} · trigger frame ${triggerFrameId ?? '—'}` ``.
    - `written` → `` `증거 #${n} ${roi} ${L} · ${frames.length} frames` `` plus `` ` · 부분: ${partial.join(', ')}` `` when `partial` is non-empty.
    - Otherwise → `` `증거 #${n} ${roi} ${L} · ${reason ?? '?'}` ``.
- **M9 `roiFormSpec(f)`** (inputs trimmed; first failure returned)
  - Errors:
    - name `/^[A-Za-z0-9_-]{1,32}$/` → `'ROI 이름: 영문·숫자·_·- 1~32자'`.
    - rect is 4 comma parts, each non-empty and a finite `Number` → `'ROI 영역: x,y,w,h (0~1 비율)'`.
    - metric in `METRICS` → `'측정 방식을 고르세요'`.
    - enter is finite and > 0 → `'진입 임계값: 0보다 큰 수'`.
    - exit is empty (omitted) or finite ≥ 0 → `'해제 임계값: 0 이상 수'`.
    - dwell is empty or `/^\d+$/` → `'대기 시간: ms 정수'`.
    - cooldown is empty or `/^\d+$/` → `'재전환 간격: ms 정수'`.
  - Success → `{roi: {name, x, y, w, h, metric, enter, exit?, dwell_ms?, cooldown_ms?}}`, in that key order.
- **M10 `b64Bytes`**: `atob` into a `Uint8Array`; a throw → `null`.

### B6-P videoPanel.ts (inside `createVideoWorkbench`)
- **P1 DOM** (existing `el`/`tid`; buttons `type='button'`; SVG built with `document.createElementNS('http://www.w3.org/2000/svg', …)`)
  - Overlay `video-roi-overlay` (class `vd-roi-overlay`) appended to `stage`; `viewBox="0 0 1 1"`, `preserveAspectRatio="none"`.
  - Section `video-roi` (`div.vd-roi`) placed after `snapsEl` in the root order. Children, in this order:
    - `video-roi-meta` (`div.vd-roi-meta`).
    - `video-roi-list` (`div.vd-roi-list`).
    - Editor `video-roi-editor` (`div.vd-roi-editor`):
      - Inputs named `roi-name`, `roi-rect` (placeholder `x,y,w,h`), select `roi-metric` (options = `METRICS`, labels from `METRIC_LABEL`, default `mean_abs_diff`), `roi-enter`, `roi-exit`, `roi-dwell`, `roi-cooldown`.
      - Buttons `video-roi-put` (`'ROI 저장'`) and `video-roi-baseline` (`'현재 frame을 기준으로'`).
      - Error line `video-roi-err` (`div.vd-roi-err`).
    - `video-roi-history` (`div.vd-roi-history`).
    - `video-evidence` (`div.vd-evidence`).
    - Evidence nav `video-evid-nav` (`div.vd-evid-nav`, hidden by default): `video-evid-prev` `◀`, `video-evid-label` (`span.vd-evid-label`), `video-evid-next` `▶`.
- **P2 ingest**
  - `handle(ev)`: for the 4 ROI kinds, skip when `pinned` and `capture_id` is a string other than `pinned` (an idle `roi_config` with `capture_id: null` always passes). Then `roiBook.apply(kind, payload, seq)` → invalidate. Other kinds are unchanged.
  - `applySnap`: for each item with `domain === 'video'`, also `roiBook.applySnapshotState(obj(item).state, snap.throughSeq)`.
- **P3 render** (in `draw`; `c = current()`)
  - Rows (`rows = roiBook.rows(c?.id ?? null)`), rebuilt only when their JSON key changes. Each row is `video-roi-row` (`div.vd-roi-row`, `dataset.roi`, `dataset.state`) with spans: name (`vd-mono`), `video-roi-chip` (`.vd-roi-chip`), `video-roi-value`, threshold, `video-roi-baseline-frame`, and a button `video-roi-remove` (`'✕ 삭제'`).
  - Overlay: one `rect.vd-roi-rect` per config ROI, with `x`/`y`/`width`/`height` = `String(spec.x)` etc., `data-roi`, and `data-state` from the matching row, plus a child `<title>` whose textContent is the name.
    - Position the SVG over the canvas box (`left/top/width/height` = `canvas.offsetLeft/Top/Width/Height` px).
    - Hidden while viewing an artifact.
  - `video-roi-meta` ← `inspectLine(c ? roiBook.metrics.get(c.id) : undefined, roiBook.config?.rev ?? null)`.
  - `video-roi-history` ← up to `ROI_HISTORY_MAX` rows `video-roi-hist-row` (`historyText`, `dataset.state`).
  - `video-evidence` ← rows `video-evidence-row` (`div.vd-evid-row`, `dataset.state`): a span with `evidenceText`, plus a button `video-evidence-view` (`'보기'`) only for `written` with an `artifactId`.
  - Buttons:
    - `putBtn.disabled = !canOperate()`.
    - `baseBtn.disabled = !canOperate() || !c || !f || f.captureId !== c.id || !(roiBook.config?.rois.length)`, where `f = frozen ?? drawn`.
    - `baseBtn.title` = `` f ? `기준 = 화면의 frame ${f.id}` : '프레임 없음' ``.
- **P4 editor**
  - `put` click: read the 7 fields into a `RoiForm`; `r = roiFormSpec(form)`.
    - string → `roiErr.textContent = r`; no call.
    - else clear the error; `act('roi_put', {...r, ...(cfg ? {expect_rev: cfg.rev} : {})}, 'ROI 저장')` with `cfg = roiBook.config`.
  - Row remove → `act('roi_remove', {roi: name, ...(expect_rev)}, 'ROI 삭제')`.
  - Baseline → `act('roi_baseline', {capture_id: c.id, frame_id: f.id}, 'ROI 기준')`: the exact frame on screen, frozen or drawn.
- **P5 evidence view `viewEvidence(aid)`**
  - `ctx.media(artifactUrl(aid), signal)` with the B1 timeout pattern.
    - `!r.ok` → toast `` `증거 보기 실패: ${status} ${error}` `` (`.trim()`).
    - JSON → `readEvidenceManifest`; `null` → toast `'증거 보기 실패: 형식 오류'`.
  - For every frame: `bytes = b64Bytes(b64)`, `sha = bytes ? hex(await crypto.subtle.digest('SHA-256', bytes)) : ''`, `ok = !!bytes && sha === frame.sha256 && bytes.length === frame.size`.
  - Keep `evid = {m, i, shas, oks}` with `i` = the trigger index (0 when there is none), then `showEvid(i)`.
  - Any throw → toast `` `증거 보기 실패: ${message}` ``.
- **P6 `showEvid(i)`**
  - `createImageBitmap(new Blob([bytes], {type: 'image/jpeg'}))` → `pendingView = {bmp, view: {artifactId: m.artifactId, frameId, expectedSha: frame.sha256, sha: shas[i], ok: oks[i]}}` (B1 viewer: frame/sha lines and overlay). Set `evid.i = i`; invalidate.
  - Label `video-evid-label` ← `` `증거 ${m.roi} · ${i + 1}/${m.frames.length} ${frame.role} · frame ${frameId} · sha256 ${ok ? '일치' : '불일치!'}` ``, with `dataset.ok = String(ok)`.
  - Nav: `video-evid-nav` is shown only while `evid` is set; prev is disabled at 0, next at the last frame.
- **P7 leave / dispose**: `leaveArtifactView` also sets `evid = null`, so `▶ 실시간` leaves the evidence view. `dispose` releases nothing new: no timers are added.

### Contract amendment A6 (Opus, during B6): ROI judgment kind renamed
- The ROI transition event kind is **`video.roi_judgment`**, not `video.roi_state`.
- Why: the shared hub (`loom-gateway/src/hub/mod.rs::is_channel_state`) and the UI (`ui/src/store/channels.ts::isStateKind`) treat any `<domain>.*_state` event on a channel as that channel's connectivity state. A ROI judgment (`changed`/`unknown`) on the capture channel would have overwritten the capture's channel entry for every client. Renaming is module-local; no shared edit is needed.
- Rust: only the value of `KIND_ROI_STATE` in `lib.rs` changes (Opus; every user goes through the constant).
- UI (B6 lease, Sonnet): in `roiView.ts`, the `ROI_KINDS` entry, the `apply` switch case and the header comment use `'video.roi_judgment'`. **This one constant edit is authorized by Opus.**
- Payload/param names are unchanged: `roi_state` (wait param), `--roi-state` (CLI), reducer snapshot key `roi_states`.
- Regression tests: `roi_contract` asserts that no new kind ends in `_state`; `videoRoi.test.ts` asserts `isStateKind(k) === false` for every ROI kind.

### B6 contract amendments (Opus, after review)
- **A7**: `showEvid(i)` sets `evid.i` synchronously, before the bitmap await, and drops a bitmap whose index is stale. Back-to-back ▶ clicks then land on the right frame.
- **A7**: the evidence nav is visible only while `viewing.artifactId === evid.m.artifactId`.
- **A7**: `as BlobPart` / `as BufferSource` typing casts are accepted.

### B6 tests (Opus, `videoRoi.test.ts`, 15): what they prove
- Readers: defaults, skips and strictness for config, state, metric, evidence and manifest.
- Book: seq ordering, dedupe by `transition_seq`, bounded history/evidence, baselines from both sources, snapshot without history.
- Rows/history/evidence ordering; every text table; form validation; base64.
- Manifest actions and topic kinds.
- Panel:
  - **The rendered judgment equals the daemon payload**: chips, values, baseline, overlay geometry/state, history, inspection line separate from render fps.
  - No RPC on render; an untrusted reason is rendered as text.
  - Idle-channel config is shown.
  - Editor validation; `expect_rev`; remove; baseline = the frame on screen.
  - Evidence: lifecycle rows; viewer opens the trigger frame; each frame's sha256 is verified; ◀▶ bounds; `▶ 실시간` leaves.
  - A sha mismatch is flagged; malformed and 404 replies are refused with toasts.

## Batch 2+ — contracts pending (written before each lease; architecture fixed in PLAN §2)
- **B3** — written above (§Batch 3).
- **B4** — written above (§Batch 4).
- **B5** — written above (§Batch 5a, §Batch 5b).
- **B6** — written above (§Batch 6).
- **B7** skill text.
