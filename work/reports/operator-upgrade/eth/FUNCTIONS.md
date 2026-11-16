# ETH operator upgrade — FUNCTIONS (Batch 1 detailed)

Owner of this contract: ETH Opus lead. Worktree `/Users/ctmctm/workspace/Loom/.worktrees/operator-eth` (baseline 610029e).
Batch 1 = **ETH-01 server side** (daemon gateway): honest scope/partial reasons for every journal read, capture-start anchor.
Batches 2–5 are listed in PLAN.md and get their own detailed section here before their lease starts.

## B1 lease

| Item | Value |
|---|---|
| Sonnet body files (full-file lease, Opus does not edit while leased) | `crates/loom-eth/src/gateway/common.rs`, `crates/loom-eth/src/gateway/handlers.rs`, `crates/loom-eth/src/gateway/evidence.rs` |
| Opus-owned test files (spec; Sonnet must not edit) | `crates/loom-eth/tests/range_scope.rs` (new), `crates/loom-eth/tests/support/mod.rs` (fake journal look-back), `#[cfg(test)] mod range_scope_tests` at the end of `common.rs` (Sonnet must not edit that module) |
| Already scaffolded by Opus (types/signatures only, bodies `todo!`) | `ScanSummary` new fields, `FromSource`, `ScanScope`, `REASON_*` consts, `range_scope`, `scope_json`, `scanned_json`, `partial_reasons`, `resume_cursor_json`, `complete_from`, `ChanInfo::start_at`, `ChannelBook::update_at` |
| Not touched in B1 | UI, worker, decode crate, `gateway.rs` (param schemas unchanged: only response fields are added), CLI, skills (docs B5) |
| Shared / ROOT needs | none (no new RPC, no schema param, no dependency, no Cargo.lock change) |

### Function-body allowlist (B1)
F1.1 `scan_packets` (modify body) · F1.2 `FromSource::as_str` · F1.3 `range_scope` · F1.4 `scope_json` · F1.5 `scanned_json` · F1.6 `partial_reasons` · F1.7 `resume_cursor_json` · F1.8 `ChannelBook::update_at` + `ChannelBook::update` (becomes a one-line delegate) · F1.9 `complete_from` · F1.9b `scan_complete_from` · F1.10 `Query::call` + `query_once` · F1.11 `Stats::call` · F1.12 `Follow::call` · F1.13 `Export::call` · F1.14 `Status::call` · F1.15 `EthReducer::apply` · F1.16 `EthReducer::snapshot`.
Nothing else. No new pub items, no new helpers; if one seems necessary, stop and return to Opus with the reason. Private closures inside the listed bodies are fine.

### Wire contract added by B1 (backward compatible: additive fields; one semantic change)
Read responses of `eth.query`, `eth.stats`, `eth.follow`, `eth.export` gain:

```jsonc
"scope": {"from": "-600s" | "<cursor as given, canonical Display>", "to": "<cursor>" | null,
          "from_source": "explicit" | "default_lookback", "lookback_secs": 600 | null},
"scanned": {"first_seq": "1380" | null, "last_seq": "2000" | null, "bytes": 4096},
"partial_reasons": ["default_lookback","scan_budget","scan_incomplete","decode_budget","loss","row_cap","output_cap"], // subset, this order
"resume_cursor": "<cursor>" | null,   // only when scan_budget or decode_budget is a reason
"complete": bool,                     // = complete_from(partial_reasons) = partial_reasons is EMPTY
"scan_complete": bool                 // = scan_complete_from(partial_reasons): applied range fully scanned+decoded
```

**Two flags, two meanings (ROOT review 2026-11-15, resolved before lease):**
- `complete` = *result completeness / evidence*: true only when `partial_reasons` is empty — explicit range, fully scanned and decoded, no loss entries, nothing cut by `row_cap`/`output_cap`. This is the **only** flag a UI may render as 완전/전체 and an agent may use for an absence claim.
- `scan_complete` = *scan coverage of the applied range* (which may be the default look-back): none of `scan_budget`, `scan_incomplete`, `decode_budget`. It is never evidence by itself; UI shows it only as coverage ("스캔 범위 전체 확인") next to the reasons.
- Consequence: `complete:true` can never coexist with `loss`, `row_cap`, `truncated:true`, or `default_lookback`.
`eth.export` additionally returns `"filter": "<expr>" | null` (the applied display filter).
`eth.capture.status` without `ch` (channel-book listing) adds per channel `"start_cursor": "e<E>:seq:<N>" | null` and `"start_seq": "<N>" | null`.
Reducer snapshot `eth.capture_state` items add `"start_cursor"` **only when known** (omitted otherwise).

**Semantic change (intended, ETH-01):** `complete` is now strict: `false` whenever any partial reason exists — no `range.from` (default 600 s look-back), loss entries, a row cap (`eth.query` `truncated` by `max`), an output cap, or a scan/decode stop. The old meaning ("the scanned window had no scan-level stop") moves to the new `scan_complete`. Previously `complete:true` meant "the look-back window was scanned", which agents/UI read as whole history (observed false negative: `complete:true, matched:0` for a packet 40 min old). No in-repo consumer reads `eth.query.complete` (checked: `crates/**`, `skills/**`, UI); UI will start reading it in B2. CLI JSON passes it through. Documented for agents in B5 (skill).

Unchanged: `rows`, `range` (seq range of returned rows), `truncated`, `matched`, `gaps`, `gaps_omitted`, `next_cursor`, `prev_cursor`, `eth.show`, `eth.wait_for`, evidence verifier, all error codes/messages.

---

## F1.1 `scan_packets` (common.rs) — modify body, signature unchanged
```rust
pub async fn scan_packets(ctx: &CallCtx, ch: &str, from: Cursor, until: Option<Cursor>, budget_bytes: u64,
    each: &mut (dyn FnMut(&PublishedRecord) -> bool + Send)) -> Result<ScanSummary, DomainError>
```
Keep every existing behaviour (gap collection, budget loop, `complete`, `budget_hit`, errors). Add:
- For each `eth.pkt` event `ev`: call `each(ev)`.
  - returns `true` → `first_pkt_seq.get_or_insert(ev.seq)`, `last_pkt_seq = Some(ev.seq)`.
  - returns `false` → `stopped_by_callback = true`, `resume_from = Some(Cursor::Seq{epoch: None, seq: ev.seq.get()})` (that packet was **not** counted in first/last; a continuation re-reads it), return `Ok(sum)` immediately (as today).
- Budget stop (the existing `Some(_) =>` arm): additionally `resume_from = Some(next.clone())` (the journal's own resume cursor, epoch kept).
- End of range (`None` arm): `resume_from` stays `None`.
- `last_seq` local used for gap `after_seq` keeps its current meaning (set before calling `each`).
Invariants: no extra journal calls, no extra allocation per event beyond today; never panics.
Examples: events pkt 5, pkt 6 (each→true), pkt 7 (each→false) → first 5, last 6, stopped true, resume `seq:7`, complete true. Budget hit after pkt 9 with journal resume `e1:seq:10` → last 9, resume `e1:seq:10`, budget_hit, complete false.
Tests: `range_scope.rs::eth01_default_query…` (`scanned.first_seq/last_seq`), existing `gateway.rs` (all 15 must stay green).

## F1.2 `FromSource::as_str(self) -> &'static str`
`Explicit → "explicit"`, `DefaultLookback → "default_lookback"`. Test: `explicit_from_keeps_epoch_and_has_no_lookback`.

## F1.3 `range_scope(params: &Value) -> Result<ScanScope, DomainError>`
Input: the whole request params. Reads `params.range.from` / `params.range.to` via existing `cursor_param` (so error code/message are identical to today: `E_USAGE "bad cursor …"`). `range` absent, null or non-object → both ends absent.
- `from` present → `ScanScope{from, to, source: Explicit}`.
- `from` absent → `ScanScope{from: lookback(), to, source: DefaultLookback}`.
- `to` is parsed (and errors) even when `from` is absent.
Examples: `{}` → `(-600s, None, DefaultLookback)`; `{"range":{"from":"e3:seq:1370","to":"seq:1390"}}` → `(Seq{3,1370}, Some(Seq{None,1390}), Explicit)`; `{"range":{"from":"-1h"}}` → Explicit (an explicit relative cursor is the caller's choice); `{"range":{"from":"1380"}}` → `E_USAGE` (bare number = clock grammar).
Tests: `missing_from_is_the_default_lookback_and_says_so`, `explicit_from_keeps_epoch_and_has_no_lookback`, `bad_cursor_is_usage_error`.

## F1.4 `scope_json(s: &ScanScope) -> Value`
`{"from": s.from.to_string(), "to": s.to.map(to_string) | null, "from_source": s.source.as_str(), "lookback_secs": DEFAULT_LOOKBACK_SECS if DefaultLookback else null}`. Cursor text = `loom_proto::cursor::Cursor` `Display` (`-600s`, `seq:9`, `e3:seq:1370`, `-1h`). Exactly these 4 keys.

## F1.5 `scanned_json(sum: &ScanSummary) -> Value`
`{"first_seq": first_pkt_seq as decimal string | null, "last_seq": … | null, "bytes": scanned_bytes as JSON number}`. (Seqs as strings = U64 wire rule; bytes ≤ 64 MiB budget, a number like other byte counts.)

## F1.6 `partial_reasons(scope, sum, decode_stopped, caps) -> Vec<&'static str>`
Push in this fixed order, each at most once:
1. `REASON_DEFAULT_LOOKBACK` if `scope.source == DefaultLookback`
2. `REASON_SCAN_BUDGET` if `sum.budget_hit`
3. `REASON_SCAN_INCOMPLETE` if `!sum.complete && !sum.budget_hit`
4. `REASON_DECODE_BUDGET` if `decode_stopped`
5. `REASON_LOSS` if `!sum.gaps.is_empty() || sum.gaps_omitted > 0`
6. each of `caps` in the given order, skipping ones already present.
Pure; no allocation beyond the Vec. Test: `reasons_are_ordered_unique_and_drive_complete`.

## F1.7 `resume_cursor_json(sum, reasons) -> Value`
`Value::String(sum.resume_from.to_string())` iff `reasons` contains `scan_budget` or `decode_budget` **and** `sum.resume_from.is_some()`; else `Value::Null`. Test: `resume_cursor_only_for_budget_stops`.

## F1.8 `ChannelBook::update_at(&self, ch, payload, generation, at: Option<(u64,u64)>)` and `update`
`update(ch, payload, generation)` body becomes exactly `self.update_at(ch, payload, generation, None)`.
`update_at` = today's `update` body (eviction, backend/iface/state, generation, started_mono_ns, touched — unchanged) plus `start_at`:
```
new_gen = generation.is_some() && generation != e.generation      // evaluated BEFORE e.generation is overwritten (same place started_mono_ns is reset)
if new_gen { e.start_at = None }
if payload.state == "starting" && at.is_some() { e.start_at = at }
```
(A brand-new entry has `generation == None`, so a first-seen generation counts as `new_gen`. A non-`starting` first record leaves it `None` = joined mid-generation, unknown.) Lock poisoning: return silently as today.
Test: `start_anchor_is_observed_never_guessed` (covers mid-generation join, same-generation later states, `update` without position, new generation reset, generation `None`).

## F1.9 `complete_from(reasons) -> bool`
`reasons.is_empty()`.

## F1.9b `scan_complete_from(reasons) -> bool`
`true` iff `reasons` contains none of `scan_budget, scan_incomplete, decode_budget`.
Tests (`reasons_are_ordered_unique_and_drive_complete`): every single reason clears `complete`; only scan_budget / scan_incomplete / decode_budget clear `scan_complete`; `[]` → both true.

## F1.10 `Query::call` / `query_once` (handlers.rs)
New private signature (Opus contract): `async fn query_once(ctx: &CallCtx, ch: &str, scope: &ScanScope, filter: Option<Filter>, max: usize, detail: bool) -> Result<Value, DomainError>`.
`Query::call`: replace the `range`/`from`/`to` parsing with `let scope = range_scope(&params)?;` (parse before the wait loop, as today) and pass `&scope` each loop iteration. Wait loop unchanged.
`query_once`: scan `scope.from.clone()`, `scope.to.clone()`; keep row/matched/prev/next logic. Then
```
let decode_stopped = stopped_early;
let caps = if matched > rows.len() { &[REASON_ROW_CAP][..] } else { &[] };
let reasons = partial_reasons(scope, &sum, decode_stopped, caps);
```
Output = today's object with `"complete": complete_from(&reasons)` (replaces `sum.complete && !stopped_early`), `"scan_complete": scan_complete_from(&reasons)`, plus `"scope": scope_json(scope)`, `"scanned": scanned_json(&sum)`, `"partial_reasons": reasons`, `"resume_cursor": resume_cursor_json(&sum, &reasons)`. `truncated` formula unchanged.
Examples (all in `range_scope.rs`): default query on stopped `qa` → `matched 0, complete false, partial_reasons ["default_lookback"], scope.from "-600s"`; `range.from "e1:seq:1"` → `matched 1, rows[0].ref "eth:qa@seq:1380", complete true, partial_reasons [], scanned {first "1380", last "2000"}, resume_cursor null`; `max 2` over 6 matches with a source.gap → `partial_reasons ["loss","row_cap"], complete false, scan_complete true, truncated true, prev_cursor "seq:4"`. Default query → `complete false, scan_complete true`.
Cancellation/STOP: read-only, no lease; deadline handled by the existing wait loop; no new waits.

## F1.11 `Stats::call`
`let scope = range_scope(&params)?;` replaces from/to parsing (scan with `scope.from/to`). `decode_stopped = partial` (the existing exhaustion flag). Reasons with no caps (top-N table truncation is a display limit, still reported by `truncated` as today). Output adds `scope`, `scanned`, `partial_reasons`, `resume_cursor`, `scan_complete`; `complete = complete_from(&reasons)`; `truncated` unchanged.

## F1.12 `Follow::call`
`scope = range_scope(&params)?`. Distinguish the two existing stop causes: `decoded > MAX_DECODE_PER_REQUEST` → `decode_stopped = true`; `take < data.len()` (max_bytes) → cap `REASON_OUTPUT_CAP`. Keep `truncated = truncated || sum.budget_hit`. Output adds `scope`, `scanned`, `partial_reasons`, `resume_cursor`, `scan_complete`; `complete = complete_from(&reasons)` (was `sum.complete`).

## F1.13 `Export::call`
`scope = range_scope(&params)?` (parsed before the file is created, so a bad cursor creates no file — same as today). Split the existing stop condition: `packets >= EXPORT_MAX_PACKETS || bytes >= EXPORT_MAX_BYTES` → cap `REASON_OUTPUT_CAP`; `m.exhausted()` → `decode_stopped`. Both still set `truncated = true`. Success output = today's keys plus `"filter": <applied filter string, or null when absent/blank>`, `"scope"`, `"scanned"`, `"partial_reasons"`, `"resume_cursor"`, `"complete": complete_from(&reasons)`, `"scan_complete": scan_complete_from(&reasons)`. Error paths (file cleanup on failure) unchanged.
Note: `scanned.first_seq/last_seq` are seqs of scanned packets whose callback returned true (including ones the filter rejected) — the scanned span, not the exported span; `packets` stays the exported count.

## F1.14 `Status::call` (book listing branch only; the `ch` → worker branch unchanged)
Each channel object adds `"start_cursor": i.start_at.map(|(e,s)| Cursor::Seq{epoch: Some(e), seq: s}.to_string())` → `"e1:seq:5"` | null, and `"start_seq": i.start_at.map(|(_,s)| s.to_string())` | null. Existing keys unchanged.

## F1.15 `EthReducer::apply` (evidence.rs)
`KIND_CAPTURE_STATE => self.book.update_at(channel_of(ev), &ev.payload, ev.generation, Some((ev.epoch.get(), ev.seq.get())))`. `KIND_PKT` arm unchanged.

## F1.16 `EthReducer::snapshot`
For each channel item, after `started_mono_ns`, add `v["start_cursor"] = "e<E>:seq:<N>"` **only if** `i.start_at` is `Some` (same omission rule as `started_mono_ns`). Nothing else changes.

---

## B1 tests (already written by Opus; Sonnet must not edit)
Unit (`common.rs` `range_scope_tests`): default look-back scope JSON; explicit epoch cursor; bad cursor `E_USAGE`; reason order/uniqueness, strict `complete_from` and `scan_complete_from`; resume cursor only for budget stops; scanned JSON incl. `u64::MAX`; start anchor observed-never-guessed.
Integration (`tests/range_scope.rs`, fake journal emulates the real 600 s look-back when `relative_now_wall` is set):
1. `eth01_default_query_is_partial_lookback_and_explicit_capture_range_finds_the_packet` — the ROOT repro (old: `complete:true, matched:0`).
2. `eth01_row_cap_and_loss_clear_complete_but_not_scan_coverage`.
3. `eth01_stats_follow_export_state_their_scope` (default → partial; explicit export → `filter`, `scope`, `scanned`).
4. `eth01_start_anchor_in_status_and_snapshot_is_null_when_not_observed`.
Baseline on scaffold (Opus, 2026-11-15, after ROOT review fix): unit 0/7 pass (todo), integration 0/4 pass, existing `gateway.rs` 15/15 pass. Scaffold contains 9 `todo!` (clippy fails only on those).

### Acceptance commands (B1)
```sh
cd /Users/ctmctm/workspace/Loom/.worktrees/operator-eth
export CARGO_TARGET_DIR=/Users/ctmctm/workspace/Loom/target CARGO_BUILD_JOBS=2
cargo test -p loom-eth --lib range_scope_tests          # 7 pass
cargo test -p loom-eth --test range_scope               # 4 pass
cargo test -p loom-eth --test gateway --test cli        # all pass (no regression)
cargo test -p loom-eth --lib                            # all pass
cargo clippy -p loom-eth --all-targets -- -D warnings   # clean
```
Sonnet returns: changed files, `git diff --stat`, `git diff | shasum -a 256`, exact test output lines. No git add/commit.

---
---

# Batch 2 — ETH-01 UI (explicit range, honest badge, paging, export scope, follow range) + ETH-05 (unbound start)

Depends on B1 (accepted, hash 4484b86f…). Wire fields consumed: `scope`, `scanned`, `partial_reasons`, `resume_cursor`, `complete` (strict), `scan_complete`, `prev_cursor`; `eth.capture.status` listing `start_cursor`; snapshot `start_cursor`; export `filter`.

## B2 lease
| Item | Value |
|---|---|
| Sonnet body files (full-file lease) | `ui/src/panels/eth/ethModel.ts`, `ui/src/panels/eth/ethPanel.ts`, `ui/src/panels/eth/dialogs.ts` |
| Opus-owned, Sonnet must not edit | all `*.test.ts` (`ethRange.test.ts` new, `ethComplete.test.ts` new, `eth.test.ts`, `ethUnbound.test.ts` updated), `eth.css` (B2 block added), `fixtures.ts`, every `import` line, exported types/interfaces/consts (`PAGE_ROWS`, `MAX_PAGE_STACK`, `RECENT_CHOICES`, `START_CURSOR_RE`, `PARTIAL_REASONS`, `RangeChoice`, `RangeAnchors`, `RangeRequest`, `ResolvedRange`, `ReadScope`, `ReadMeta`, `QueryPage`, `CompleteBadge`, `PageNav`, `ExportResult`, `QUERY_MAX`), function signatures |
| Scaffolded by Opus | ethModel F2.1–F2.12 signatures (bodies `return B2(...)` — replace each body; the private `B2` helper must be **deleted** when the last body is done — that one deletion is allowed), `ExportResult.filter/meta`, `openFollow(..., range: RangeRequest)` parameter, imports in ethPanel/dialogs |
| Not in B2 | SDK treeHex, filterBar, ifaceConfigs, socketDrawer, Rust, skills |
| Shared / ROOT | none. No new dependency. |
| Known lint state after bodies | `seqRange` import in ethPanel may become unused — **leave it**; Opus removes imports after review (imports are Opus-only). Report any other unused import instead of editing it. |

### Function-body allowlist (B2)
ethModel.ts: F2.1 `resolveRange` · F2.2 `parseRangeChoice` · F2.3 `rangeChoiceValue` · F2.4 `readReadMeta` · F2.5 `readQueryPage` · F2.6 `reasonText` · F2.7 `scopeText` · F2.8 `completeBadge` · F2.9 `navStart` · F2.10 `navOlder` · F2.11 `navNewer` · F2.12 `navResume`.
ethPanel.ts: F2.13 `readExportResult` (module fn) · inside `createEthPackets` only the following blocks/closures: F2.14 state declarations · F2.15 range-row construction · F2.16 unbound construction additions · F2.17 `refreshChannels` · F2.18 `renderOthers` · F2.19 `applyCapture` · F2.20 `draw` (listed parts only) · F2.21 `items` / `goComplete` / new `runQuery` / `goLive` · F2.22 new `pageOlder` / `pageNewer` / `pageResume` / `onRangeSelect` / `onRangeApply` + their listeners · F2.23 new `anchors` · F2.24 export button listener + new `openExportDialog` · F2.25 new `runExport` · F2.26 `openExportResult` · F2.27 `followBtn` listener · F2.28 `cfgConnect` click listener · F2.29 `dispose` (close the new dialog).
dialogs.ts: F2.30 `openFollow` body.
Nothing else. Existing behaviour not mentioned stays byte-for-byte (live list, filter pushdown, gap rows, drops, TX, socket drawer, keys).

## Pure model (ethModel.ts) — tests `ethRange.test.ts`
**F2.1 `resolveRange(choice, {startCursor, headSeq}) → {req, label, fallback}`** — `to = headSeq !== null && headSeq > 0n ? 'seq:'+headSeq : (omitted)` except seq choice with explicit `to`.
| choice | req.from | req.to | label | fallback |
|---|---|---|---|---|
| capture, startCursor set | startCursor (verbatim) | head | `캡처 시작 이후` | false |
| capture, startCursor null | `-600s` | head | `최근 10분 (캡처 시작 위치 모름)` | true |
| recent 600/3600/21600 | `-600s`/`-3600s`/`-21600s` | head | `최근 10분`/`최근 1시간`/`최근 6시간` | false |
| seq a..b | `seq:a` | `seq:b`, or head when b null | `seq a–b` / `seq a–최신` | false |
`req` has no `to` key at all when omitted (tests use `toEqual`).

**F2.2 `parseRangeChoice(value, fromText, toText)`** — `capture` → `{kind:'capture'}`; `recent:600|3600|21600` → recent; `seq` → trim both; from must match `/^\d{1,20}$/` and be ≤ 2^64−1 else `{error:'시작 seq를 숫자로 입력하세요'}`; to `''` → null, else same check else `{error:'끝 seq를 숫자로 입력하세요'}`; from > to → `{error:'시작 seq가 끝 seq보다 큽니다'}`; any other value → `{error:'알 수 없는 범위'}`.
**F2.3 `rangeChoiceValue`** — inverse: `capture`, `recent:<secs>`, `seq`.

**F2.4 `readReadMeta(v)`** (o = object or `{}`):
- `known = Array.isArray(o.partial_reasons)`; `reasons` = string entries in `PARTIAL_REASONS`, de-duplicated, daemon order; `unknownReasons` = other strings (non-strings ignored).
- `complete = known && o.complete === true && reasons.length === 0 && unknownReasons.length === 0`.
- `scanComplete` = boolean `o.scan_complete` else null.
- `scope` = object `o.scope` → `{from: string|null, to: string|null, fromSource: 'explicit'|'default_lookback'|null, lookbackSecs: number|null}` (strings only for from/to; lookback via the module's numeric reader `n`), else null.
- `scannedFirst/Last` = `u64(o.scanned.first_seq/last_seq)`; `resumeCursor` = non-empty string else null; `gaps = (Array.isArray(o.gaps) ? length : 0) + (n(o.gaps_omitted) ?? 0)`.
**F2.5 `readQueryPage(v, channel)`** — rows = array entries → `readPkt(obj(r), channel)` non-null; `rowsFrom = big(range.from) ?? rows[0]?.seq ?? null`, `rowsTo` likewise with last row; `matched = n(o.matched)`; `truncated = o.truncated === true`; `prevCursor/nextCursor` strings else null; `meta = readReadMeta(v)`.
**F2.6 `reasonText`** — `default_lookback` `기본 조회(최근 10분)만` · `scan_budget` `스캔 한도(64 MiB) 도달` · `scan_incomplete` `저널 스캔 불완전` · `decode_budget` `디코드 한도 도달` · `loss` `손실 구간 ${meta.gaps}건` · `row_cap` `최대 ${PAGE_ROWS ko-KR}행 초과` · `output_cap` `출력 한도 도달`.
**F2.7 `scopeText(meta)`** — `!known` → `요청/스캔 범위 정보 없음 (이전 daemon)`; else `요청 범위 ${scope?.from ?? '?'} → ${scope?.to ?? '최신'}` + (` (기본 최근 10분)` if fromSource default_lookback) + ` · 스캔 seq ${scannedFirst ?? '?'}–${scannedLast ?? '?'}` + (` · 스캔 범위 전체 확인` if scanComplete true, ` · 스캔 중단` if false, nothing if null).
**F2.8 `completeBadge(page, label)`** — n = rows.length ko-KR; seqs = ` · seq ${rowsFrom}–${rowsTo}` when both non-null; title = `scopeText(page.meta)`.
- `!known` → state `unknown`, `완전 여부 미확인 · ${label} · ${n}행${seqs}`
- `complete` → state `complete`, `완전 · ${label} · ${n}행${seqs}`
- else → state `partial`, `부분 · ${label} · ${n}행${m}${seqs} — ${list}` where m = ` / 일치 ${matched ko-KR}` iff matched > rows.length; list = reasonText of reasons + `기타 사유 ${r}` for unknown, joined ` · `; empty list → `완전 아님(사유 미보고)`.
`완전` alone may appear only in the complete branch (tests assert).
**F2.9–F2.12 paging** (pure, never mutate inputs): `navStart(req)` → `{base: req, current: req, newer: []}` (copies). `navOlder(nav, prev)` → newer = [...newer, current.to] (keep last `MAX_PAGE_STACK`), current = `{from: current.from, to: prev}`. `navNewer(nav)` → null if newer empty; else pop last `t`; current = `{from: current.from}` plus `to: t` only if `t !== undefined`. `navResume(nav, cursor)` → `{base, current: {from: cursor, ...(current.to !== undefined ? {to: current.to} : {})}, newer: []}`.

## Panel (ethPanel.ts) — tests `ethComplete.test.ts`, updated `eth.test.ts`, `ethUnbound.test.ts`
**F2.13 `readExportResult`** — existing fields unchanged plus `filter = typeof o.filter === 'string' && o.filter ? o.filter : null`, `meta = readReadMeta(o)`.
**F2.14 state** (replace the old `complete` object; keep `mode`, `completeState`, `completeErr`, `completeAc`):
`let rangeChoice: RangeChoice = { kind: 'capture' }; let startCursor: string | null = null; let nav: PageNav | null = null; let page: QueryPage | null = null; let pageLabel = ''; let exportScopeDlg: { close(): void } | null = null;`
**F2.23 `anchors(): RangeAnchors`** → `{ startCursor, headSeq: headSeq() > 0n ? headSeq() : null }` (define after `headSeq`).
**F2.15 range row** (built right after `scopeRow`; `fbar.append(scopeRow, rangeRow, filterHost)`), using the module's `el`/`btn`:
- `rangeRow = el('div','eth-frow eth-range-row')`, `data-testid="eth-range-row"`, `hidden = !bound`.
- `el('span','eth-flab','조회 범위')` title `완전(저널) 조회 범위 — 항상 명시 범위로 조회`.
- `rangeSel = el('select','wb-tsel')` `data-testid="eth-range"` aria-label `완전 조회 범위`; options (value/text): `capture`/`캡처 시작 이후`, `recent:600`/`최근 10분`, `recent:3600`/`최근 1시간`, `recent:21600`/`최근 6시간`, `seq`/`seq 범위…`; initial `capture`.
- `rangeFrom`, `rangeTo`: `input.eth-range-seq`, `data-testid` `eth-range-from`/`eth-range-to`, placeholders `시작 seq` / `끝 seq (비우면 최신)`, `inputMode='numeric'`, aria-labels same text, initially `hidden`.
- `rangeApply = btn('조회','wb-btn','seq 범위 조회')` `data-cmd="range-apply"`, initially hidden.
- `olderBtn = btn('◀ 이전 1,000행','wb-btn','더 오래된 1,000행 (prev_cursor)')` `data-cmd="page-older"`; `newerBtn = btn('최신 쪽 ▶','wb-btn','이전에 보던 더 최신 페이지')` `page-newer`; `resumeBtn = btn('계속 스캔 ▶','wb-btn','스캔 한도에서 멈춘 지점부터 이어서 조회 (resume_cursor)')` `page-resume`.
- `rangeScope = el('span','eth-range-scope')` `data-testid="eth-range-scope"`.
**F2.22 handlers** (bound only; unbound → no-op):
- `onRangeSelect` (rangeSel `change`): `seq` → show `rangeFrom/rangeTo/rangeApply`, no query; otherwise hide them, `rangeChoice = parseRangeChoice(value,'','')` (non-error), `goComplete('range')`.
- `onRangeApply` (rangeApply `click`): `c = parseRangeChoice('seq', rangeFrom.value, rangeTo.value)`; error → `ctx.ui.toast(c.error)`, nothing sent; else `rangeChoice = c; rangeSel.value = rangeChoiceValue(c); goComplete('range')`.
- `pageOlder`: if `mode==='complete' && nav && page?.prevCursor` → `nav = navOlder(nav, page.prevCursor); runQuery('page')`. `pageNewer`: `const n = nav && navNewer(nav)`; if n → `nav = n; runQuery('page')`. `pageResume`: if `nav && page?.meta.resumeCursor` → `nav = navResume(nav, …); runQuery('resume')`.
**F2.21** `items()` → complete: `page?.rows ?? []`. `goComplete(reason)`: `if (!bound) return; const r = resolveRange(rangeChoice, anchors()); pageLabel = r.label; nav = navStart(r.req); runQuery(reason);`. `runQuery(reason)` = old goComplete body with: request `{ ch, range: nav.current, ...(expr ? { filter: expr } : {}), max: QUERY_MAX, detail: false }` (key `range` always present); on success `page = readQueryPage(res, ch)`; `reason` kept for future use (may be unused → prefix `_reason`); errors as today. `goLive`: also `page = null; nav = null;`.
**F2.17 `refreshChannels` .then**: before rebuilding `others`, if `bound`: `own = channels.find(c => c.ch === ch)`; if own exists → `startCursor = typeof own.start_cursor === 'string' && START_CURSOR_RE.test(own.start_cursor) ? own.start_cursor : null` (own absent → unchanged).
**F2.19 `applyCapture`**: after the seq guard, if `generation !== null && captureGen !== null && generation !== captureGen` → `startCursor = null` (before `captureGen` is updated).
**F2.20 `draw`** (only these parts):
- snapshot loop: after `applyCapture(...)`, if `typeof st.start_cursor === 'string' && START_CURSOR_RE.test(st.start_cursor) && (typeof st.generation !== 'number' || st.generation === captureGen)` → `startCursor = st.start_cursor`.
- complete badge: loading → `조회 중… (저널)`; error → `조회 실패: ${completeErr}`; page → `b = completeBadge(page, pageLabel)`: `textContent = b.text`, `dataset.complete = b.state`, `title = b.title`. Live → `delete modeBadge.dataset.complete` (live text unchanged).
- range row (bound): `olderBtn.disabled = !(mode==='complete' && completeState==='idle' && !!page?.prevCursor)`; `newerBtn.disabled = !(mode==='complete' && completeState==='idle' && !!nav && nav.newer.length > 0)`; `resumeBtn.disabled = !(mode==='complete' && completeState==='idle' && !!page?.meta.resumeCursor)`; `rangeScope.textContent` = complete & page → `scopeText(page.meta)`; complete & loading → `조회 중… (저널)`; live → `다음 완전 조회: ${r.label} (${r.req.from} → ${r.req.to ?? '최신'})` with `r = resolveRange(rangeChoice, anchors())`.
- `sbCount` complete text: replace the leading `완전 범위 패킷` with `저널 조회 패킷` (rest unchanged) — a partial page must not say 완전.
- unbound `statusText`: `replaceChildren(el('span','ch-line', L1), ...(others.size ? [el('span','ch-line', L2)] : []))` with L1 `인터페이스 구성을 고르거나 새로 만든 뒤 [연결…] — 이 탭을 여는 것만으로는 캡처·송신하지 않습니다.` and L2 `이미 daemon에 있는 채널은 [열기]로 기록을 이어 봅니다 (캡처를 시작하지 않음).` (bound text unchanged).
- unbound `cfgConnect`: `c = selectedCfg(); existing = !!c && others.has(c.alias); want = existing ? 'open' : 'connect'`; only when `cfgConnect.dataset.mode !== want`: `replaceChildren(icon('play'), existing ? '기존 채널 열기' : '연결…')`, title `existing ? '이 이름의 daemon 채널이 이미 있음 — 그 채널 패널을 엽니다 (캡처 시작 아님). 다시 캡처는 그 패널의 [캡처 시작…]' : <today's title>`, `dataset.mode = want`. `cfgConnect.disabled = existing ? false : !op`.
**F2.16 unbound construction** (inside the existing `if (!bound)` block): `chbar.dataset.unbound = 'true'; addBtn.hidden = true; startBtn.hidden = true; stopBtn.hidden = true;`. VirtualTable `emptyText: bound ? '패킷 없음' : '채널 미연결 — 기존 채널 [열기] 또는 구성 [연결…]'`.
**F2.18 `renderOthers`**: if `!bound && list.length > 0`, the first child of `otherHost` is `el('span','eth-ch-head','기존 채널 이어 보기')` (before the cards). Cards unchanged.
**F2.28 `cfgConnect` click**: `c = selectedCfg()`; if `cfgConnect.dataset.mode === 'open' && c` → `ctx.ui.openPanel('eth.packets', { channel: c.alias }); ctx.ui.toast(`채널 ${c.alias} 패널을 열었습니다 — 캡처는 시작하지 않았습니다`); return;` else `connectCfg(c)`. (`iface-edit` unchanged; its dialog still refuses existing aliases.)
**F2.24 export button listener** → `if (exportBtn.disabled || !bound) return; openExportDialog();`.
`openExportDialog()` (closes a previous `exportScopeDlg`; same overlay/box/Esc pattern and classes as `openExportResult`):
- box `role=dialog`, aria-label `pcapng 내보내기 범위`, `data-testid="eth-export-dialog"`, header `pcapng 내보내기 — 범위 선택`.
- `div.eth-export-scope` with four `label`s, each `input[type=radio][name="eth-export-scope"][value=…]` + text: `view` `현재 조회 범위 — ${pageLabel}` (disabled unless `mode==='complete' && nav`); `capture` `캡처 시작 이후 전체 — ${resolveRange({kind:'capture'}, anchors()).label}`; `selected` `선택 행만 — seq ${selected.seq}` / `선택 행만 — 선택 없음` (disabled unless `selected?.kind==='pkt'`); `seq` `seq 범위` + `input[type=text][data-testid="eth-export-from"]` (placeholder `시작 seq`) and `…eth-export-to` (placeholder `끝 seq (비우면 최신)`).
- default checked: `view` if enabled else `capture`.
- if `filter.applied`: `label` > `input[type=checkbox][data-testid="eth-export-filter"]` (checked) + `표시 필터 적용: ${filter.applied}`.
- `div.eth-export-range[data-testid="eth-export-range"]`: `compute()` → view `{...nav.current}` · capture `resolveRange({kind:'capture'}, anchors()).req` · selected `{from:'seq:N', to:'seq:N'}` · seq `parseRangeChoice('seq', from, to)` → error or `resolveRange(choice, anchors()).req`. Text `범위 ${req.from} → ${req.to ?? '최신'}` or the error text (+ class `err`, run disabled). Recompute on `change` (bubbling from radios/checkbox) and `input` (seq fields).
- `p.muted` `파일은 daemon(벤치) 머신에 생성됩니다 — 브라우저 다운로드가 아닙니다.`; actions `취소 (Esc)` and `button[data-testid="eth-export-run"]` `내보내기`.
- run: `params = { ch, format: 'pcapng', range: req, ...(useFilter ? { filter: filter.applied } : {}) }` with `useFilter = checkbox present && checked && scope !== 'selected'`; close dialog; `runExport(params)`. Opening the dialog sends nothing.
**F2.25 `runExport(params)`** = today's listener body from `exportBtn.disabled = true` onward, with `params` instead of the inline object (busy/title/toast/result logic unchanged).
**F2.26 `openExportResult`**: after `바이트` add rows (`add(k, v, cls)`): `요청 범위` → `${scope.from ?? '?'} → ${scope.to ?? '최신'}` or `정보 없음 (이전 daemon)`; `표시 필터` → `r.filter ?? '(없음)'`; `완전성` → complete `완전 — 요청 범위 전체 기록` · !known `완전 여부 미확인 (이전 daemon)` · else `부분 — ${list || '사유 미보고'}` (list as in F2.8), cls `res-fail` when not complete. Existing rows unchanged.
**F2.27 `followBtn` listener**: `range = mode === 'complete' && nav ? { ...nav.current } : resolveRange(rangeChoice, anchors()).req`; pass as the new last argument of `openFollow`.
**F2.29 `dispose`**: also `exportScopeDlg?.close()`.
**F2.30 `openFollow` (dialogs.ts)**: query `eth.follow` with `{ ch, proto, stream, range }`; `meta = readReadMeta(r)`; `sum.textContent = `클라이언트 ${c} B · 서버 ${sv} B${r?.truncated ? ' · 잘림(최대 크기)' : ''} — daemon eth.follow · ${q}`` with q = complete `완전` · !known `완전 여부 미확인` · else `부분: ${list || '사유 미보고'}` (list as F2.8 via `reasonText`); `sum.title = scopeText(meta)`. Rest unchanged.

Cancellation/state: all reads are aborted on new requests as today (`completeAc`); no timers added; no writes added (export stays one explicit action; opening dialogs sends nothing). Text from the daemon is rendered with `textContent` only.

## B2 tests (Opus; baseline on scaffold 2026-11-15: 31 failed / 49 passed of 80 in `src/panels/eth` + `src/sdk/treeHex.test.ts`)
`ethRange.test.ts` (13: F2.1–F2.12) · `ethComplete.test.ts` (10: anchor query + 완전 badge; fallback; never-완전 with loss/row_cap/lookback/unknown/legacy; range select + invalid; paging older/newer/resume; generation/snapshot anchor; export dialog scopes + result; follow range; unbound heading/lines/hidden; no-channel line) · updated `eth.test.ts` (filter, Space, cursor grammar incl. export dialog, export result ×2, follow) · updated `ethUnbound.test.ts` (hidden toolbar duplicates; 기존 채널 열기).

### Acceptance commands (B2) — isolated type caches (COMMON)
```sh
cd /Users/ctmctm/workspace/Loom/.worktrees/operator-eth && mkdir -p work/typecheck && cd ui
npx vitest run src/panels/eth src/sdk/treeHex.test.ts                       # 80/80 pass
npx tsc -p tsconfig.app.json --noEmit --tsBuildInfoFile ../work/typecheck/app.tsbuildinfo    # 0 errors
npx tsc -p tsconfig.node.json --noEmit --tsBuildInfoFile ../work/typecheck/node.tsbuildinfo  # 0 errors
npx eslint src/panels/eth                                                    # only allowed: unused `seqRange` import (report it)
```
Never run plain `tsc -b` / `npm run typecheck` (shared node_modules cache). Return `work/reports/operator-upgrade/eth/B2-WORKER.md` (local) with READY_FOR_REVIEW, changed files, `git diff -- ui | shasum -a 256`, exact result lines.

---
---

# Batch 3a — ETH-03 `udp.stream` + `stream_proto`, ETH-02 daemon tree `filter`, ETH-01 CLI trailer

## B3a lease
| Item | Value |
|---|---|
| Sonnet body files (full-file lease) | `crates/loom-eth-decode/src/stream.rs`, `crates/loom-eth-decode/src/net.rs`, `crates/loom-eth/src/worker/pipeline.rs`, `crates/loom-eth/src/gateway/common.rs`, `crates/loom-eth/src/gateway/handlers.rs`, `crates/loom-eth/src/cli.rs` |
| Opus-owned (no Sonnet edit) | tests: `crates/loom-eth-decode/tests/udp_stream.rs` (new), `crates/loom-eth/tests/udp_follow_tree.rs` (new), `crates/loom-eth/tests/cli.rs` (+1 test), `crates/loom-eth/src/worker/tests.rs` (+1 test), `common.rs` `range_scope_tests` module; `fields.rs` (udp.stream registered), `lib.rs`, `model.rs` (`PktPayload.stream_proto`), all `use` lines, struct fields (`StreamTable.udp/next_udp_id/udp_evicted`, `UdpConv`), consts (`MAX_UDP_CONVERSATIONS`), signatures |
| Scaffolded by Opus | `StreamTable::udp_conversation` (todo), `StreamTable::udp_evicted` (done: getter), `net.rs udp(..., src, dst, streams)` new params + call-site args (glue only, body untouched), `handlers::filter_expr` (todo), `cli::completeness_note` (todo), `pipeline.rs` line `stream_proto: None, // B3a SCAFFOLD` (placeholder **you replace**) |
| Shared / ROOT | none (no schema/param change in B3a; `stream_proto` is an additive optional payload field) |

### Allowlist (B3a)
F3.1 `StreamTable::udp_conversation` · F3.2 `net::udp` (add numbering lines only) · F3.3 pipeline `PktPayload { stream, stream_proto }` expressions · F3.4 `common::decode_record` · F3.5 `common::row` · F3.6 `Follow::call` (one added check) · F3.7 `handlers::filter_expr` + `handlers::tree` (add `filter` key) · F3.8 `cli::completeness_note` + `render_rows` + `"eth.export"` render arm. Remove the `let _ = (...)` / `todo!` scaffold lines you replace. Nothing else.

**F3.1 `udp_conversation(vlan, src, dst, ts_ns, warnings) -> u64`** (stream.rs)
```
key = if src <= dst { Key{vlan, a: src, b: dst} } else { Key{vlan, a: dst, b: src} }   // same normalisation as feed()
if let Some(c) = self.udp.get_mut(&key) { c.last_ts = ts_ns; return c.id }
if self.udp.len() >= MAX_UDP_CONVERSATIONS {
    idle = keys with c.last_ts.saturating_add(STREAM_IDLE_NS) <= ts_ns; remove all; self.udp_evicted += idle.len()
    if idle is empty {
        (lk, lid) = entry with minimum (last_ts, id); remove; self.udp_evicted += 1
        warnings.push(("udp_conversation_evicted", format!("table full ({MAX_UDP_CONVERSATIONS}), evicted udp stream {lid}")))
    }
}
id = self.next_udp_id; self.next_udp_id += 1; insert UdpConv{id, last_ts: ts_ns}; id
```
No idle renumbering on lookup (an existing pair keeps its number). Independent of the TCP map/counters. Tests: `udp_stream.rs` (both directions, VLAN key, separate index space, idle-first eviction without warning, LRU with warning text, `udp_evicted`).

**F3.2 `net::udp`** — immediately after the `udp.checksum` field (before `c.proto = "UDP"` and before the length checks):
```
let mut w = Vec::new();
let sid = streams.udp_conversation(c.vlan, (src, sp), (dst, dp), d.meta.ts_ns, &mut w);
for (code, msg) in w { d.warn(code, "udp", msg); }
d.add(l, "udp.stream", FieldValue::Uint(sid), None);
```
Everything else in `udp` unchanged (DoIP/SOME-IP dispatch).

**F3.3 pipeline** — replace the `stream:` and placeholder `stream_proto:` expressions:
`tcp.stream` present → `(u32::try_from(v).ok(), Some("tcp"))`; else `udp.stream` present → `(…, Some("udp"))`; else `(None, None)`. `stream_proto` must be `None` whenever `stream` is `None` (also when the u32 conversion fails). Test: `worker::tests::replay_packets_carry_stream_and_stream_proto_by_transport`.

**F3.4 `decode_record`** — after `decode(...)`:
- `stream_proto` = payload string. If `payload.stream` is a u64 **and** stream_proto is `"udp"` → set every `udp.stream` field value to it. If `payload.stream` is a u64 and stream_proto is `"tcp"` or absent → set `tcp.stream` (today's behaviour). Other stream_proto strings → no override.
- If the record is **not** (`stream` u64 && stream_proto `"udp"`): remove every field named `udp.stream` from every layer (`retain`), so legacy / tcp-space records never expose a request-local conversation number to filters or trees.
Test: `eth03_udp_stream_filter_uses_the_worker_number_and_rows_carry_stream_proto`.

**F3.5 `row`** — add `"stream_proto": p.get("stream_proto").cloned().unwrap_or(Value::Null)` (next to `stream`).

**F3.6 `Follow::call`** — right after the existing `stream` equality check: skip the record (`return true`) unless `payload.stream_proto == proto`, treating an absent `stream_proto` as `"tcp"`. Test: `eth03_udp_follow_joins_both_directions_of_one_conversation_only`.

**F3.7 `filter_expr(name, value) -> Option<String>`** and `tree`:
- `def = field_def(name)?`.
- `value == None` → `Some(name)` iff `def.ty == FieldType::Protocol`, else `None`.
- `Uint(n)` → `"{name} == 0x{n:x}"` when `def.base == Base::Hex`, else `"{name} == {n}"`. `Bool(b)` → `"{name} == 1|0"`. `Ipv4/Ipv6(a)` → `"{name} == {a}"`. `Mac(m)` → `"{name} == {fmt_mac(m)}"`. `Bytes(b)`: empty or `len > 64` → `None`; else lowercase two-digit hex joined with `:` (`aa:0b:00`, single byte `aa`). `Str(s)`: `s.chars().count() > 128` → `None`; else `"{name} == \"{escaped}\""` escaping `\` → `\\`, `"` → `\"`, `\n` `\t` `\r` `\0` as such, other chars `< 0x20` or `0x7f` → `\xHH` (two lowercase hex digits); other chars verbatim.
- Finally `Filter::compile(&expr).is_ok()` → `Some(expr)` else `None` (guarantees every emitted filter is valid; covers type mismatch / range).
- `tree`: field nodes get `"filter": filter_expr(f.name, Some(&f.value))`; layer nodes `"filter": filter_expr(l.name, None)`; JSON `null` when `None`. Labels/offsets unchanged.
Tests: `eth02_show_tree_nodes_carry_compile_checked_filters`, `eth02_filter_expr_literals_escapes_caps_and_refusals`.

**F3.8 CLI** — `completeness_note(d)`: `d["partial_reasons"]` not an array → `None`; empty and `d["complete"] == true` → `Some("complete")`; empty otherwise → `Some("partial: unreported")`; non-empty → `Some(format!("partial: {}", strings.join(",")))`.
`render_rows`: after the existing `next:` part push the note (if any), then `resume: --since {c}` when `d["resume_cursor"]` is a string. `"eth.export"` arm: `"wrote {p} packets ({b} B) to {path}{truncated}{note}"` where note = `" · " + n` when `Some`. Older-daemon output must stay byte-identical (existing golden). Test: `trace_and_export_render_completeness_from_partial_reasons` (+ existing `trace_renders_tshark_columns_and_trailer`).

### Acceptance (B3a)
```sh
cd /Users/ctmctm/workspace/Loom/.worktrees/operator-eth
export CARGO_TARGET_DIR=/Users/ctmctm/workspace/Loom/target CARGO_BUILD_JOBS=2
cargo test --no-fail-fast -p loom-eth-decode -p loom-eth -p loom-eth-filter   # all pass (baseline: 9 new failing, all existing pass)
cargo clippy -p loom-eth-decode -p loom-eth --all-targets -- -D warnings      # clean
cargo fmt -p loom-eth-decode -p loom-eth --check                               # clean (run `cargo fmt` on YOUR leased files only if needed; report it)
```
Return local `eth/B3A-WORKER.md`: READY_FOR_REVIEW, changed files, `git diff -- crates | shasum -a 256`, exact result lines.

---
---

# Batch 4a — UI: ETH-02 field menu, ETH-03 follow index space, ETH-04 replay from the GUI, B2 follow-up

Depends on B3a (accepted): tree nodes carry `name`/`filter`; rows/pkts carry `stream_proto`. Replay uses the existing `eth.capture.start` `replay{path,speed,repeat}` params (no schema change). Native file picker = deferred (ROOT); the path is typed and is a DAEMON-machine path.

## B4a lease
| Item | Value |
|---|---|
| Sonnet body files (full-file lease) | `ui/src/sdk/treeHex.ts`, `ui/src/panels/eth/ethModel.ts`, `ui/src/panels/eth/ifaceConfigs.ts`, `ui/src/panels/eth/dialogs.ts`, `ui/src/panels/eth/ethPanel.ts` |
| Opus-owned | all tests (`treeHex.test.ts` +2, `ethB4.test.ts` new, `ethFields.test.ts` new, existing ones), `eth.css` (B4a block), `fixtures.ts`, all import lines, exported types/consts (`TreeNode.name/filter`, `TreeHexOptions.onNodeContext`, `PktRow.streamProto`, `REPLAY_SPEEDS`, `ReplaySpeed`, `MAX_REPLAY_PATH`, `ReplayOptions`, `IfaceConfig.backend/replay`, `REPLAY_TEMPLATE`), signatures |
| Scaffolded (bodies throw / placeholder) | `TreeHex.selectedNode`, `TreeHex.onTreeContext` (class-field arrow: replace its body only), `combineFilter`, `replayLabel`, `openReplayFile`; `readPkt` line `streamProto: null, // B4a SCAFFOLD` (replace) |
| Shared / ROOT | none. SDK TreeHex change is additive/optional (other consumers: none outside ETH today; default behaviour unchanged when no hook). |

### Allowlist (B4a)
treeHex.ts: F4.3 `onTreeContext` body + `contextmenu` add/remove in constructor/`dispose` · F4.4 `onTreeKey` extension · F4.5 `selectedNode` getter.
ethModel.ts: F4.1 `readPkt` `streamProto` expression · F4.2 `combineFilter`.
ifaceConfigs.ts: F4.6 `replayLabel` · F4.7 `isCfg`, `sanitizeConfigs`, `captureParams`.
dialogs.ts: F4.8 `captureForm` (replay backend) · F4.9 `openReplayFile`.
ethPanel.ts (inside `createEthPackets` unless noted): F4.10 `renderCfgs` option text · F4.11 unbound replay button · F4.12 REPLAY chips · F4.13 field menu (`openFieldMenu`, TreeHex `onNodeContext` option, detail-bar button, close on row change/dispose) · F4.14 status-bar field name · F4.15 follow proto/title · F4.16 live badge tooltip reset.

**F4.1 `readPkt`** — `streamProto = stream !== null && (row.stream_proto === 'tcp' || row.stream_proto === 'udp') ? row.stream_proto : null`.
**F4.2 `combineFilter(draft, expr, op)`** — `d = draft.trim()`; `d === '' ? expr : `(${d}) ${op} ${expr}``.

**F4.3 `TreeHex.onTreeContext(e: MouseEvent)`** — if `!this.options.onNodeContext` → return (no `preventDefault`). Node element = `(e.target as HTMLElement).closest('.th-node')`; no node or no flat entry → return. Else `e.preventDefault(); this.selectFlat(f); this.options.onNodeContext(f.node, f.el, { x: e.clientX, y: e.clientY })`. Register `this.treeEl.addEventListener('contextmenu', this.onTreeContext)` in the constructor, remove it in `dispose`.
**F4.4 `onTreeKey`** — before today's key filter: if (`e.key === 'ContextMenu'` or (`e.key === 'F10' && e.shiftKey`)) and a node + hook exist → `preventDefault`, `selectFlat(f)`, `r = f.el.getBoundingClientRect()`, hook `(f.node, f.el, { x: r.left, y: r.bottom })`, return. Without hook: ignore (not prevented). Existing keys unchanged.
**F4.5 `selectedNode`** — `this.selected?.node ?? null` (`set()` already resets `selected`).

**F4.6 `replayLabel(path)`** — `t = path.trim()`; base = text after the last `/` or `\`; empty → `'replay'`; else first 128 chars.
**F4.7 configs** — `isCfg` additionally: backend may be `'replay'`; when `'replay'`: `replay` must be an object with `path` string whose trimmed length is 1..`MAX_REPLAY_PATH`, `speed` ∈ `REPLAY_SPEEDS` (exact `'max'` or the numbers 1/2/10), `repeat` boolean — otherwise the config is invalid (dropped). Non-replay configs ignore any `replay`. `sanitizeConfigs` output object: today's fields, `bpf` kept only for live (else `''`), plus `replay: { path: trimmed, speed, repeat }` **only** for replay (key absent otherwise). `captureParams`: replay → `{ iface, ch, backend: 'replay', snaplen, promisc, buffer_mib, replay: { path, speed, repeat } }` (never `bpf`); live/sim unchanged.

**F4.8 `captureForm`** (dialogs.ts; used by capture start / add channel / connect / replay):
- backend `<select aria-label="백엔드">` options: `live`/`live — 실제 NIC`, `sim`/`sim — daemon 합성 탭 (SIM)`, `replay`/`replay — pcap/pcapng 파일 재생 (daemon 머신 경로)`.
- keep the NIC field element: `nicField = field('NIC', iface)`.
- replay box `div.eth-replay-fields[data-testid="eth-replay-fields"]` containing `field('파일 경로 (daemon 머신)', path)` with `input[data-testid="eth-replay-path"][aria-label="파일 경로 (daemon 머신)"]` placeholder `/path/on/daemon/host/capture.pcapng`; `field('재생 속도', speed)` with `select[data-testid="eth-replay-speed"]` options `max`/`최대 속도`, `1`/`원래 속도 ×1`, `2`/`×2`, `10`/`×10`; `field('반복', repeat)` with `input[type=checkbox][data-testid="eth-replay-repeat"]`. Inserted right after the backend field.
- `syncBackend()`: today's logic plus `replayBox.hidden = backend.value !== 'replay'`, `nicField.hidden = backend.value === 'replay'`, `bpf.disabled = backend.value !== 'live'`; `msg` (remove class `err`) text = replay ? `REPLAY: 파일을 재생합니다 — 실제 NIC 캡처가 아니며, 경로는 daemon이 실행 중인 머신 기준입니다.` : today's BPF note.
- prefill (`init`): `backend.value = init.backend`; replay → path/speed (`String(speed)`)/repeat from `init.replay`.
- `formConfig()`: `be = backend.value` ∈ live|sim|replay. Replay: `p = path.value.trim()`; empty → msg `파일 경로를 입력하세요 (daemon 머신 기준)` → null; `p.length > MAX_REPLAY_PATH` → `파일 경로가 너무 깁니다` → null; NIC check skipped; `iface = replayLabel(p)`; `speed = speed.value === 'max' ? 'max' : Number(speed.value)`; config includes `replay: { path: p, speed, repeat: repeat.checked }`, `bpf: ''`. Live/sim unchanged (NIC required).
- start `params = captureParams(cfg)` (replaces the inline object; identical for live/sim valid input).
**F4.9 `openReplayFile(host, ctx, on)`** — `captureForm(host, ctx, '', { add: { existing: <same eth.capture.status lookup as openConnectInterface>, onStarted: on.onStarted, onSave: on.onSave, initial: REPLAY_TEMPLATE, title: '파일 열기 — replay (daemon 머신의 pcap/pcapng)', testId: 'eth-replay-dialog' } })`.

**F4.10 `renderCfgs`** — option text suffix: sim ` (SIM)`, replay ` (REPLAY)`.
**F4.11 unbound replay** — in the unbound block: `cfgReplay = btn('파일 열기 (replay)…', 'wb-btn', 'pcap/pcapng 파일을 daemon에서 재생 — 경로는 daemon 머신 기준 (실제 NIC 아님)')`, `data-cmd="iface-replay"`, appended to `chActs` after `cfgDel`. Click → `openReplayFile(dialogHost, ctx, handlers)` where `handlers` are exactly the `onSave`/`onStarted` used by `connectCfg` (factor them into one `cfgHandlers(initialAlias)` closure inside the body; same toasts/storeCfg/openPanel behaviour; replay's `initialAlias` = null). `draw`: `cfgReplay.disabled = !op`.
**F4.12 REPLAY chips** — `chReplay = el('span', 'wb-chip replay', 'REPLAY 파일')`, title `daemon이 파일을 재생 중 — 실제 NIC 캡처 아님`, appended after `chSim`; `draw`: `chReplay.hidden = capture?.backend !== 'replay'`. `renderOthers`: `o.backend === 'replay'` → `nm.append(el('span', 'wb-chip replay', 'REPLAY'))`.
**F4.13 field menu** — state `let fieldMenu: { close(): void } | null = null`.
- TreeHex options gain `onNodeContext: (node, _anchor, at) => openFieldMenu(node, at)`.
- `fldBtn = btn('필드 동작…', 'wb-btn', '선택한 필드로 필터 적용/준비, 필드명 복사 (우클릭·Shift+F10도 가능)')`, `data-cmd="fld-menu"`, inserted in `detailBar` before `refBtn`. Click: `n = treeHex.selectedNode`; if n → `r = fldBtn.getBoundingClientRect(); openFieldMenu(n, { x: r.left, y: r.bottom })`. `draw`: `fldBtn.disabled = selected?.kind !== 'pkt' || !treeHex.selectedNode`.
- `openFieldMenu(node: TreeNode, at: {x, y})`: close previous; `div.eth-fld-menu[role=menu][data-testid="eth-fld-menu"]` aria-label `필드 동작`, `style.left = `${at.x}px``, `style.top = `${at.y}px``; head `div.eth-fld-head` = `${node.name ?? '(이름 없음)'}` + (` — ${node.filter}` when filter); six `button[role=menuitem]` in order with `data-cmd`/text: `fld-apply` 필터로 적용 · `fld-prepare` 필터로 준비 · `fld-and` …그리고(&&) 준비 · `fld-or` …또는(||) 준비 · `fld-copy-name` 필드명 복사 · `fld-copy-filter` 필터 식 복사. No `node.filter` → the 5 filter items disabled with title `daemon이 이 항목의 필터 식을 주지 않았습니다`; no `node.name` → copy-name disabled with title `필드명이 없습니다`. Append to `dialogHost`; focus the first enabled item.
- actions (each closes the menu first): apply `filter.setExpr(f); void filter.applyNow()` · prepare `filter.setExpr(f)` · and/or `filter.setExpr(combineFilter(filter.input.value, f, '&&' | '||'))` · copy name/filter: `navigator.clipboard?.writeText(t)` → then `ctx.ui.toast(`복사: ${t}`)`, on failure or no clipboard `ctx.ui.toast(t)`.
- keyboard on the menu: `Escape` → `stopPropagation`, close; `ArrowDown`/`ArrowUp` → `preventDefault`, move focus to the next/previous enabled item (wrap).
- outside: `document.addEventListener('pointerdown', h, true)` closing when the target is outside the menu; removed on close.
- `onSelect` (row change) and `dispose` call `fieldMenu?.close()`.
**F4.14 `sbField`** — packet + field label: `${fieldLabel}${name ? ` (${name})` : ''}${hl …}` with `name = treeHex.selectedNode?.name`.
**F4.15 follow** — listener: `proto = selected.streamProto ?? 'tcp'` (replaces the protocol-column regex). `draw` enabled title: `선택 패킷의 대화 따라가기 — ${proto.toUpperCase()} stream ${stream}` (same proto rule).
**F4.16** — `draw` live branch: `modeBadge.title = ''`.

## B4a tests (baseline on scaffold: 16 new failing, existing 80 pass of 96)
`treeHex.test.ts` (+2), `ethB4.test.ts` (F4.1/4.2/4.6/4.7), `ethFields.test.ts` (menu, prepare/apply, no-filter node + clipboard + outside close + detail button + row change, follow index space, replay dialog end-to-end + config, form switching/prefill, REPLAY chips, bound form offers replay, tooltip reset).

### Acceptance (B4a)
```sh
cd /Users/ctmctm/workspace/Loom/.worktrees/operator-eth/ui && mkdir -p ../work/typecheck
npx vitest run src/panels/eth src/sdk/treeHex.test.ts                                     # 96/96
npx tsc -p tsconfig.app.json  --noEmit --tsBuildInfoFile ../work/typecheck/app.tsbuildinfo  # 0
npx tsc -p tsconfig.node.json --noEmit --tsBuildInfoFile ../work/typecheck/node.tsbuildinfo # 0
npx eslint src/panels/eth src/sdk/treeHex.ts src/sdk/treeHex.test.ts                       # 0 (unused import → report, don't edit)
```
Return local `eth/B4A-WORKER.md` (READY_FOR_REVIEW, files, `git diff -- ui | shasum -a 256`, exact lines).

---
---

# Batch 3b — ETH-06 read-time Decode As (engine + gateway param + CLI)

Design (PLAN §6): rules are a per-request interpretation (Wireshark Decode As semantics). Records are never rewritten; the worker/live list is unchanged. Every packet decoded through a rule gets a `decode_as` warning; responses echo the rules used.

## B3b lease
| Item | Value |
|---|---|
| Sonnet body files (full-file lease) | `crates/loom-eth-decode/src/decode_as.rs`, `crates/loom-eth-decode/src/stream.rs`, `crates/loom-eth-decode/src/net.rs`, `crates/loom-eth/src/gateway/common.rs`, `crates/loom-eth/src/gateway/handlers.rs`, `crates/loom-eth/src/cli.rs` |
| Opus-owned | tests `crates/loom-eth-decode/tests/decode_as.rs`, `crates/loom-eth/tests/decode_as.rs`, `crates/loom-eth/tests/cli.rs` (+1), all earlier tests; `lib.rs` (mod + re-exports); `gateway.rs` (param schemas: `decode_as` array ≤ 8 on eth.query/show/stats/export); `cli.rs` `decode_as_arg()` + its `.arg(...)` attachments; all `use` lines, types, enums, fields (`StreamTable.decode_as`, `Matcher.redecode`), consts, signatures |
| Scaffolded | `Transport/AppProto::as_str`, `DecodeAs::new/lookup`, `DecodeAsError` Display, `StreamTable::with_decode_as`, `decode_as_param`, `decode_as_json`, `row_redecoded`, `Matcher::with_decode_as/check`, `parse_decode_as`, `cli::decode_as` (all `todo!`) |
| Shared / ROOT | additive optional param in domain-owned schemas (generated catalogs at ROOT integration). No dependency. |

### Allowlist (B3b)
F5.1–F5.4 decode_as.rs bodies · F5.5 `with_decode_as` · F5.6 `StreamTable::new_entry` app choice, `net::udp` dispatch, `net::tcp` warning · F5.7 `decode_as_param` · F5.8 `decode_as_json` · F5.9 `row_redecoded` · F5.10 `Matcher::with_decode_as`, `Matcher::check`, `Matcher::matches` (delegate) · F5.11 `Query::call` + `query_once` (new private signature below) · F5.12 `Show::call`, `Stats::call`, `Export::call` · F5.13 `parse_decode_as`, `cli::decode_as`, `to_call` arms trace/last/show/stats/export.

**F5.1** `Transport::as_str` → `tcp`/`udp`; `AppProto::as_str` → `doip`/`someip`.
**F5.2 `DecodeAs::new(rules)`** — iterate in order: `port == 0` → `Err(BadPort)`; exact duplicate of an accepted rule → skip; same transport+port as an accepted rule with another proto → `Err(Conflict{transport, port})`; else accept. After the loop: accepted `> MAX_DECODE_AS_RULES` → `Err(TooMany{max: MAX_DECODE_AS_RULES})`. Ok → rules in input order.
**F5.3 `lookup(t, src_port, dst_port)`** — first rule with `transport == t && port == dst_port`; else first with `port == src_port`; copied.
**F5.4 Display** — `BadPort` `decode_as port must be 1..65535` · `TooMany{max}` `decode_as allows at most {max} rules` · `Conflict{transport, port}` `decode_as maps {transport.as_str()} port {port} to two protocols`.
**F5.5 `with_decode_as(rules)`** — `StreamTable { decode_as: rules, ..Default::default() }`.
**F5.6 engine**
- `new_entry`: `app = self.decode_as.lookup(Transport::Tcp, k.a.1, k.b.1).map(|r| match r.proto { AppProto::Doip => App::Doip, AppProto::SomeIp => App::SomeIp }).or_else(|| app_for(k.a.1, k.b.1))` (a rule overrides the well-known ports).
- `net::udp`: replace the `let infos = if … else …` selection: `if let Some(r) = streams.decode_as().lookup(Transport::Udp, sp, dp)` → `d.warn("decode_as", "udp", format!("UDP port {} decoded as {} (Decode As)", r.port, r.proto.as_str()))` and decode with `doip::decode_datagram` / `someip::decode_datagram` by `r.proto`; else today's chain unchanged.
- `net::tcp`: after the feed warnings are added: `if let Some(r) = streams.decode_as().lookup(Transport::Tcp, sp, dp)` → `d.warn("decode_as", "tcp", format!("TCP port {} decoded as {} (Decode As)", r.port, r.proto.as_str()))`.
**F5.7 `decode_as_param(params)`** — `params.decode_as` absent/null → `Ok(DecodeAs::default())`. Not an array → `E_USAGE` `decode_as must be an array of {transport, port, proto}`. Item `i` (each error `E_USAGE` with detail `{"index": i}`): not an object → `decode_as[{i}]: expected an object {transport, port, proto}`; `transport` not `"tcp"|"udp"` → `decode_as[{i}]: transport must be tcp or udp`; `port` not a JSON integer 1..=65535 (strings refused) → `decode_as[{i}]: port must be an integer 1..65535`; `proto` not `"doip"|"someip"` → `decode_as[{i}]: proto must be doip or someip`. Then `DecodeAs::new(..)` error → `E_USAGE` with exactly its Display text.
**F5.8 `decode_as_json`** — `[{"transport": as_str, "port": n, "proto": as_str}, …]`.
**F5.9 `row_redecoded(rec, detail, d)`** — `r = row(rec, detail)`; `applied = d.warnings.iter().any(|w| w.code == "decode_as")`; if applied: `proto = d.summary.proto`, `src`/`dst` = summary value or `"?"` when empty (first 128 chars), `info` = summary info or `"undecoded frame"` when empty (first 512 chars); always set `r["decode_as_applied"] = applied`.
**F5.10 Matcher** — `with_decode_as(filter, rules)`: `redecode = !rules.is_empty()`, `streams = StreamTable::with_decode_as(rules)`, `decoded = 0`. `check(rec)`: no filter and `!redecode` → `(true, None)` (no decode, no count); else `decoded += 1`, `d = decode_record(rec, &mut self.streams)`, pass = filter absent or `d` matches; return `(pass, d)`. `matches(rec)` → `self.check(rec).0`. `new(filter)` unchanged.
**F5.11 Query** — `let decode_as = decode_as_param(&params)?;` before the wait loop. New private signature `async fn query_once(ctx: &CallCtx, ch: &str, scope: &ScanScope, filter: Option<Filter>, decode_as: &DecodeAs, max: usize, detail: bool)`. Use `Matcher::with_decode_as(filter, decode_as.clone())` and `check`; row = `row_redecoded(rec, detail, d)` when `m.redecode` and `d` is Some, else `row(rec, detail)`. Output adds `"decode_as": decode_as_json(..)` **only when non-empty** (otherwise key absent).
**F5.12** Show: parse `decode_as` first; `StreamTable::with_decode_as(decode_as.clone())` instead of `new()`; add `"decode_as"` when non-empty. Stats: parse; `Matcher::with_decode_as`; use `check`; for keys, when `d` is Some and has a `decode_as` warning use `d.summary.proto/src/dst` (empty → `"?"`) instead of the payload columns; add `"decode_as"` when non-empty. Export: parse right after the filter compile (before any file is created); `Matcher::with_decode_as`; add `"decode_as"` when non-empty. Follow / wait_for: unchanged.
**F5.13 CLI** — `parse_decode_as(s)`: `t:port=proto` with `t ∈ {tcp, udp}`, port = 1–5 ASCII digits parsing to 1..=65535, `proto ∈ {doip, someip}` → `Ok(json!({"transport": t, "port": port, "proto": proto}))`; anything else → `Err(format!("bad --decode-as {s:?}: expected tcp|udp:PORT=doip|someip (PORT 1..65535)"))`. `decode_as(m)`: the parsed values of `decode-as` (`try_get_many::<Value>`) as a JSON array in order; none → `Value::Null`. Add `"decode_as": decode_as(s)` to the request JSON of `trace`, `last`, `show`, `stats`, `export`.

## B3b tests (baseline on scaffold: 9 new failing — decode 5, gateway 3, cli 1; existing 109 pass; fmt clean)
### Acceptance (B3b)
```sh
cd /Users/ctmctm/workspace/Loom/.worktrees/operator-eth
export CARGO_TARGET_DIR=/Users/ctmctm/workspace/Loom/target CARGO_BUILD_JOBS=2
cargo test --no-fail-fast -p loom-eth-decode -p loom-eth -p loom-eth-filter    # all pass
cargo clippy -p loom-eth-decode -p loom-eth --all-targets -- -D warnings       # clean
cargo fmt -p loom-eth-decode -p loom-eth --check                                # clean (rustfmt only your leased files if needed)
```
Return local `eth/B3B-WORKER.md`.

---
---

# Batch 4b — ETH-06 UI: per-channel Decode As rules (read-time interpretation)

Wire (B3b, in progress in parallel; contract frozen above): optional `decode_as: [{transport, port, proto}]` on eth.query/show/export, `decode_as_applied` on query rows, `decode_as` warnings in eth.show. The UI only sends what the user chose and labels it; with no rules nothing changes (requests byte-identical to B4a).

## B4b lease
| Item | Value |
|---|---|
| Sonnet body files | `ui/src/panels/eth/decodeAs.ts` (new; scaffolded), `ui/src/panels/eth/ethModel.ts`, `ui/src/panels/eth/ethPanel.ts` |
| Opus-owned | `ethDecodeAs.test.ts` (new) and all tests, `eth.css` (B4b block), imports, exported consts/types (`DECODE_AS_KEY`, `MAX_DECODE_AS`, `MAX_DECODE_AS_CHANNELS`, `DecodeAsRule`, `PktRow.decodeAsApplied`), signatures |
| Scaffolded | decodeAs.ts F6.1–F6.7 (throwing via private `todo`; delete `todo` and the `void loadJson/saveJson` lines when done), `readPkt` line `decodeAsApplied: false, // B4b SCAFFOLD` |
| Storage | per-viewer `localStorage` (`loom.eth.decodeas.v1`, map channel → rules, ≤ 32 channels) — an interpretation preference, not shared state (COMMON browser-storage rule); storage failure → no rules |

### Allowlist (B4b)
decodeAs.ts F6.1–F6.7 · ethModel F6.8 · ethPanel (inside `createEthPackets`): F6.9 state + button, F6.10 `openDecodeAsDialog`, F6.11 request params (runQuery, onSelect/eth.show, export run), F6.12 badge suffixes, F6.13 proto column cell, F6.14 detail note, F6.15 dispose.

**F6.1 `sanitizeRules(v)`** — non-array → `[]`. Keep entries that are objects with `transport ∈ {tcp, udp}`, `port` a JS integer 1..65535 (strings refused), `proto ∈ {doip, someip}`; skip exact duplicates; skip a conflict (same transport+port, other proto; first wins); stop at `MAX_DECODE_AS`. Output fresh `{transport, port, proto}` objects in input order.
**F6.2 `parseRuleRow(transport, portText, proto)`** — check in this order: `transport.trim()` ∉ {tcp, udp} → `{error: '전송 계층은 tcp 또는 udp'}`; `portText.trim()` not `/^\d{1,5}$/` or outside 1..65535 → `{error: '포트는 1–65535'}`; `proto.trim()` ∉ {doip, someip} → `{error: '프로토콜은 doip 또는 someip'}`; else the rule.
**F6.3 `validateRules(rows)`** — `rows.length > MAX_DECODE_AS` → `{error: '규칙은 최대 8개입니다'}` (number from the const); per row i (1-based n): parse error → `{error: `${n}번째 규칙: ${e}`}`; exact duplicate of an earlier rule → skipped; same transport+port with another proto → `{error: `${n}번째 규칙: 같은 포트(${transport} ${port})가 다른 프로토콜로 이미 있습니다`}`; else rules.
**F6.4 `loadRules(ch)`** — `map = loadJson(DECODE_AS_KEY, isPlainObject, {})`; `sanitizeRules(map[ch])` (absent → `[]`).
**F6.5 `saveRules(ch, rules)`** — load map as above; `delete map[ch]`; if `sanitizeRules(rules)` non-empty → `map[ch] = it` (re-inserted last = newest); while more than `MAX_DECODE_AS_CHANNELS` keys → delete the first (oldest) key; `saveJson(DECODE_AS_KEY, map)`.
**F6.6 `rulesText`** — `${transport}:${port}=${proto}` joined `, `.
**F6.7 `decodeAsNotes(show)`** — `show.warnings` array entries with `code === 'decode_as'` and string `detail` → the details (order kept); anything else → `[]`.
**F6.8 `readPkt`** — `decodeAsApplied = row.decode_as_applied === true`.

**F6.9 state + button** — `let daRules: DecodeAsRule[] = bound ? loadRules(ch) : []`; `let daDlg: { close(): void } | null = null`; `let detailDaNotes: string[] = []`. `daBtn = btn('Decode As', 'wb-btn', '')`, `data-cmd="decode-as"`, inserted into `scopeRow` right after `capf`; `daBtn.hidden = !bound`; click → `openDecodeAsDialog()`. `draw` (bound): `n = daRules.length`; text `Decode As` / `Decode As (n)`; `dataset.active = String(n > 0)`; title n>0 → `Decode As: ${rulesText(daRules)} — 완전 조회·상세·내보내기 필터에 적용 (라이브 목록·기록은 그대로)`, else `비표준 포트를 DoIP/SOME/IP로 해석 (읽기 해석 — 기록은 그대로)`.
**F6.10 `openDecodeAsDialog()`** (closes a previous one; overlay/box/Escape pattern of the export dialog): box `role=dialog`, aria-label `Decode As`, `data-testid="eth-decodeas-dialog"`, header `Decode As — 읽기 해석 규칙 (${ch})`; `p.muted` `읽기 해석 규칙 — 저장된 기록은 바꾸지 않습니다. 완전(저널) 조회·상세·내보내기 필터에 적용되고, 라이브 목록은 원래 디코드 그대로입니다.`; `div.eth-da-rows` with one `div.eth-da-row` per current rule; a row = `select[aria-label="전송 계층"]` (options value/text `tcp`/`TCP`, `udp`/`UDP`), `input[aria-label="포트"]` (`inputMode=numeric`, value = port), `select[aria-label="해석"]` (`doip`/`DoIP`, `someip`/`SOME/IP`), `button[data-cmd="da-remove"]` `삭제` (removes the row). `button[data-cmd="da-add"]` `+ 규칙` appends a row (`udp`, empty port, `doip`) and is disabled while rows ≥ `MAX_DECODE_AS` (re-evaluate after add/remove). `div.eth-da-msg` (empty). Actions: `button[data-cmd="da-cancel"]` `취소 (Esc)` (close, nothing saved) and `button[data-testid="eth-decodeas-apply"]` `적용`: `r = validateRules(rows → {transport, port, proto} strings)`; error → `msg.textContent = r.error`, add class `err`, keep open; else `saveRules(ch, r)`, `daRules = r`, close, `if (mode === 'complete') goComplete('decode_as')`, `if (selected?.kind === 'pkt') onSelect(selected.key)`, `ctx.scheduler.invalidate()`. Opening sends nothing.
**F6.11 requests** — add `...(daRules.length ? { decode_as: daRules } : {})` to: `runQuery` eth.query params; `onSelect` eth.show params; the export dialog's run params. Never to eth.follow / filter.check / others.
**F6.12 badge** — complete with a page: `modeBadge.textContent = b.text + (daRules.length ? ` · Decode As ${rulesText(daRules)}` : '')`; live: today's text + (daRules.length ? ` · Decode As는 완전 조회·상세에만 적용` : '').
**F6.13 proto column** — `cell: (r) => r.kind === 'pkt' && r.decodeAsApplied ? { text: r.proto, cls: 'eth-da', title: 'Decode As 규칙으로 해석됨 (읽기 해석, 기록은 그대로)' } : r.proto`.
**F6.14 detail note** — `onSelect` resets `detailDaNotes = []`; on a show result `detailDaNotes = decodeAsNotes(res)`. `draw` detailInfo: today's text + (notes non-empty ? ` · Decode As: ${detailDaNotes.join(' · ')}` : '').
**F6.15 `dispose`** — `daDlg?.close()`.

## B4b tests (baseline on scaffold: 7 new failing, existing 96 pass of 103)
### Acceptance (B4b)
```sh
cd /Users/ctmctm/workspace/Loom/.worktrees/operator-eth/ui && mkdir -p ../work/typecheck
npx vitest run src/panels/eth src/sdk/treeHex.test.ts                                     # 103/103
npx tsc -p tsconfig.app.json  --noEmit --tsBuildInfoFile ../work/typecheck/app.tsbuildinfo  # 0
npx tsc -p tsconfig.node.json --noEmit --tsBuildInfoFile ../work/typecheck/node.tsbuildinfo # 0
npx eslint src/panels/eth src/sdk/treeHex.ts                                               # 0
```
Return local `eth/B4B-WORKER.md`.

---

# C1 — runtime correction (ETH-RT-02 show TCP context, ETH-RT-03 own-ETH request id) · Opus 2026-11-16

Authorized by ROOT after runtime QA (RUNTIME.md). Baseline = imported FIRST3 snapshot (worktree == main for every touched file; copies + sha in `c1/baseline/`, `c1/BASELINE.sha256`). The Opus scaffold is in place (types/fields/consts/signatures/tests/skill text). **Sonnet writes only the bodies listed below.** Remove the `todo!`/`throw` scaffold lines you replace, and nothing else. No new imports beyond what is already there unless a body cannot compile without one (say which in the worker return).

| Sonnet body files | `crates/loom-eth-decode/src/stream.rs`, `crates/loom-eth-decode/src/net.rs`, `crates/loom-eth/src/gateway/handlers.rs`, `ui/src/panels/eth/ethModel.ts` |
|---|---|
| Opus-only (do not edit) | `model.rs` (`TcpRelBase`, `Decoded::tcp_rel`), `lib.rs`, all consts, struct fields, signatures, tests (`crates/loom-eth-decode/tests/tcp_rel.rs`, `crates/loom-eth/tests/show_context.rs`, `ui/src/panels/eth/ethC1.test.ts`), `skills/**` |

**FC1.1 `StreamTable::feed` (stream.rs).** Maintain `DirState::isn_syn`: in the `if s.syn` branch set `st.isn_syn = true` (next to `st.isn = Some(s.seq)`); in the `else if st.isn.is_none()` branch set `st.isn_syn = false`. Then set `r.seq_from_syn = st.isn_syn`, and `r.ack_from_syn = r.rel_ack.is_some() && <the other direction's isn_syn>`. Read the other direction's flag *before* `let st = &mut e.dirs[d]`, next to `other_isn`. Do not change any other computation: `rel_seq`/`rel_ack` values, reassembly and analysis flags stay exactly as they are.

**FC1.2 `net.rs` TCP decode.** Immediately after `let r = streams.feed(&Segment {...});` set `d.tcp_rel = Some(TcpRelBase { seq_from_syn: r.seq_from_syn, ack_from_syn: r.ack_from_syn })`. The import of `TcpRelBase` (`crate::model::TcpRelBase` or `crate::TcpRelBase`) is allowed. Nothing else changes.

**FC1.3 `handlers::node_filter(layer, f)`.**
- `f.name == "tcp.seq"`: take the first field in `layer.fields` named `"tcp.seq_raw"` and return `filter_expr("tcp.seq_raw", Some(&raw.value))`; `None` if there is none.
- `f.name == "tcp.ack"`: the same with `"tcp.ack_raw"`.
- Otherwise: `filter_expr(f.name, Some(&f.value))`.

**FC1.4 `handlers::tree`.** In the field-node `json!`, replace `"filter": filter_expr(f.name, Some(&f.value))` with `"filter": node_filter(l, f)`. The layer-node `filter` is unchanged.

**FC1.5 `handlers::mark_tcp_rel_unknown(d)`.** If `d.tcp_rel` is `None`, return. Let `b = d.tcp_rel`, `changed = false`.
- If `!b.seq_from_syn`:
  - every field named `tcp.seq` in every layer named `tcp` gets `show = Some(TCP_REL_UNKNOWN.to_owned())`;
  - in each such layer's `title`, the first `"Seq: "` followed by ASCII digits gets the digits replaced by `?`;
  - in `d.summary.info`, the first `"Seq="` followed by ASCII digits gets the digits replaced by `?`;
  - `changed = true`.
- If `!b.ack_from_syn` **and** some `tcp` layer has a `tcp.ack` field:
  - each such field gets `show = Some(TCP_REL_UNKNOWN.to_owned())`;
  - in `d.summary.info`, the first `"Ack="` followed by digits gets the digits replaced by `?`;
  - `changed = true`.
- If `changed`, push `DecodeWarning { code: WARN_TCP_REL_UNKNOWN, layer: "tcp", detail: WARN_TCP_REL_UNKNOWN_DETAIL.to_owned() }`, but only when `d.warnings.len() < loom_eth_decode::MAX_WARNINGS`.
- Replace a pattern only when at least one digit follows it, and touch nothing else.

**FC1.6 `Show::call`.** Finding the record `rec` is unchanged. Then:
1. `let mut d = decode_record(&rec, &mut StreamTable::with_decode_as(decode_as.clone()))` with the same error as today.
2. If `d.tcp_rel.is_some()`:
   1. Let `from = Cursor::Seq { epoch, seq: seq.saturating_sub(SHOW_CONTEXT_SEQS) }`.
   2. Run `scan_packets(ctx, &ch, from.clone(), Some(at.clone()), SHOW_CONTEXT_BYTES, cb)`. One `StreamTable::with_decode_as(decode_as.clone())` decodes every record via `decode_record`. When `rec.seq.get() == seq`, keep that `Decoded` and return `false`.
   3. `reached` = the scan returned `Ok` and the target was decoded (`Some`). If reached, `d =` that decode. A scan `Err` or a miss means fall back silently to the single-record `d`, with `reached = false`.
   4. Compute the JSON before marking: `tcp_rel = {"window_from": from.to_string(), "reached": reached, "seq_known": b.seq_from_syn, "ack_known": <b.ack_from_syn if any tcp layer has a "tcp.ack" field, else null>}`, where `b = d.tcp_rel` of the chosen decode.
   5. Call `mark_tcp_rel_unknown(&mut d)`.
3. Build `out` exactly as today: summary/warnings/tree are taken from the possibly marked `d`. Add `out["tcp_rel"] = tcp_rel` only in the TCP case. The `decode_as` echo is unchanged.

**FC1.7 `ethModel.ts`.**
- `readEthLinkRequestId(v)`: `const l = obj(v); return l.domain === 'eth' ? s(l.request_id) : null;` (`s` already returns null for empty or non-string values).
- `readPkt`: `txRequestId: s(row.tx_request_id) ?? readEthLinkRequestId(row.link)`. `readLink`, `readLinkEvent` and ethPanel stay unchanged.

## C1 tests (Opus; baseline on scaffold 2026-11-16)
- `crates/loom-eth-decode/tests/tcp_rel.rs`: 4 failing (spec) + 1 control passing.
- `crates/loom-eth/tests/show_context.rs`: 4 failing + 1 control passing. It includes the self-match property: every ready filter of every shown packet compiles and selects that packet.
- `ui/src/panels/eth/ethC1.test.ts`: 3 failing.

Acceptance: these 13 pass, and so do all existing ETH Rust tests (decode, eth, filter, pcapng), clippy `-D warnings`, fmt, ETH UI tests, the whole UI suite, isolated tsc app/node, and eslint on the changed files. ROOT then rebuilds and does a focused runtime/GUI recheck. F1 stays FAIL until that recheck.

**C1 erratum (Opus, accepted at review):** FC1.7 assumed `s()` returns null for empty and non-string values. It does not: `s('')` is `''` and `s(7)` is `'7'`. The accepted body is `l.domain === 'eth' && typeof l.request_id === 'string' && l.request_id !== '' ? l.request_id : null`, as the spec test requires.

## Test resource amendment (ROOT, 2026-11-16) — applies to all later ETH batches
- Targeted Vitest only, always with `--maxWorkers=2`, e.g. from ui/: `npx vitest run --maxWorkers=2 src/panels/eth src/sdk/treeHex.test.ts` (plus named SDK consumers when a concrete concern exists).
- **No whole-UI-suite runs** by the ETH lead or worker. The full UI suite is ROOT-only, at the final integrated freeze.
- Unchanged: `CARGO_BUILD_JOBS=2`, isolated tsc, and no weakening of assertions or timeouts. This is resource scheduling, not a permission change. Checks that already passed are not re-run just to use the flag.

---

# C2 — runtime correction ETH-RT-04 (show context never reached; hard E_SCAN_LIMIT) · Opus 2026-11-16

Evidence: RUNTIME-C1.md (daemon 56402, epoch 1794778200). Ownership granted by ROOT: ETH gateway `common.rs` / `handlers.rs` (evidence.rs not needed). **No core/journal edit.** Baseline = current worktree = main for every touched file (`c2/baseline/`, `c2/BASELINE.sha256`). The Opus scaffold is in place: the `scan_limit_resume` stub, FakeJournal `below_ring` emulation, `tests/scan_resume.rs`, and `show_context.rs` expecting `window_from "seq:1"`.

| Sonnet body files | `crates/loom-eth/src/gateway/common.rs` (FC2.1, FC2.2), `crates/loom-eth/src/gateway/handlers.rs` (FC2.3) |
|---|---|
| Opus-only | signatures, consts, `tests/**` (incl. `tests/support/mod.rs`), skills, reports |

**FC2.1 `common::scan_limit_resume(err, from) -> Option<(Cursor, u64)>`.** Return `Some` only if **all** of the following hold; otherwise `None`.
1. `err.code == ErrorCode::ScanLimit`.
2. `err.detail["partial"]` is absent/null or an empty array. A non-empty array or any other type → `None`, so events are never dropped.
3. `err.detail["resume_from"]` is a JSON string that parses (`str::parse::<Cursor>()`) to `Cursor::Seq { epoch: re, seq: rs }`.
4. `err.detail["scanned_bytes"]` is a u64, given either as a JSON unsigned number or as a JSON string of decimal digits that parses to u64. Negative, non-numeric or missing → `None`.
5. Progress and epoch: if `from` is `Cursor::Seq { epoch: fe, seq: fs }`, then require `rs > fs`, and if both `fe` and `re` are `Some` they must be equal. A non-Seq `from` accepts any Seq resume.

Result: `Some((Cursor::Seq { epoch: re, seq: rs }, scanned_bytes))`.

**FC2.2 `common::scan_packets`.** Replace only `let r = ctx.journal().scan(&q, limit).await?;` by a `match` on the result:
- `Ok(r)` → continue exactly as today.
- `Err(e)` → `match scan_limit_resume(&e, &from)`:
  - `Some((next, bytes))` → `sum.scanned_bytes = sum.scanned_bytes.saturating_add(bytes);`, then:
    - if `sum.scanned_bytes < budget_bytes` → `from = next; continue;`
    - else → `sum.resume_from = Some(next); sum.budget_hit = true; sum.complete = false; return Ok(sum);`
    - This mirrors the existing `r.resume_from` arm at the end of the loop.
  - `None` → `return Err(e)`.

Nothing else in the loop changes: no new budget, no retry without progress, no change to gaps or callbacks.

**FC2.3 `handlers::Show::call`.** The context-window start becomes `seq: seq.saturating_sub(SHOW_CONTEXT_SEQS).max(1)`. Nothing else changes.

## C2 tests (Opus; baseline on scaffold 2026-11-16)
- `crates/loom-eth/tests/scan_resume.rs`: 6 failing + 2 controls passing. The controls are "malformed or non-progress stays an error" and "show budget exhausted stays truthfully unknown".
- `crates/loom-eth/tests/show_context.rs`: 3 failing (`window_from "seq:1"`) + 2 passing.
- Clippy fails only on the scaffold `todo!`.

Acceptance: all of the above pass; all ETH Rust crates pass (`cargo test -p loom-eth-decode -p loom-eth -p loom-eth-filter -p loom-eth-pcapng`); clippy `-D warnings` and fmt are clean. No UI change, so no Vitest is needed. ROOT then reconciles, rebuilds and re-runs the runtime steps 4/5 (expect `reached: true` for `eth:upqa-eth-c1@seq:10347`, relative seq 16).

---

# C3 — GUI finding: own status chip LIVE on a finished replay · Opus 2026-11-16

Evidence: `integration/browser/eth-c1mid-ended-state.json` (ROOT). The `upqa-eth-c1mid` panel, opened late, showed its own chip LIVE with [캡처 정지] enabled. `eth.capture.status`, the other cards and the daemon events said otherwise:
- seq 10366 `starting`, 10367 `live`, 10370 `replay_done` (generation 1);
- runtime log `c1logs/40-c1mid-capture-state-events.json`.

The panel's own state can be fed by a stale snapshot or backlog state that carries the high-water seq. `refreshChannels` ignored the own channel's authoritative status reply (it used only `start_cursor`). The scaffold reproduces the bug: "expected 'live' to be 'replay_done'".

Worker lifecycle (worker/mod.rs:145 `Live` at open, pipeline.rs:596–613 terminal at end): within one generation the order is starting → live → terminal (stopped | replay_done | error | permission_denied), never back. A restart is a new generation.

Ownership granted by ROOT: `ui/src/panels/eth/ethModel.ts`, `ui/src/panels/eth/ethPanel.ts`, only these functions. Baseline is in `c3/baseline/` (= main).

| Sonnet body | `ethModel.ts` FC3.1 `supersedesCapture` · `ethPanel.ts` FC3.2 inside `applyCapture`, FC3.3 inside `refreshChannels` |
|---|---|
| Opus-only | `TERMINAL_CAPTURE_STATES`, signatures, `ethC3.test.ts`, every other test |

**FC3.1 `supersedesCapture(prevState, prevGen, nextState, nextGen)`.**
- `n = normState(nextState)`; reuse the existing private `normState`.
- If `n === null` → `true` (the payload carries no state).
- If `prevState === null || prevState === '?'` → `true`.
- If `prevGen !== null && nextGen !== null && nextGen !== prevGen` → `nextGen > prevGen`.
- Otherwise it is the same generation (also when either generation is null):
  - `rank(x) = x === 'starting' ? 0 : TERMINAL_CAPTURE_STATES.has(x) ? 2 : 1`;
  - return `rank(n) >= rank(prevState)`.

**FC3.2 `applyCapture(p, generation, seq)` (ethPanel.ts).** As its **first** statement: `if (!supersedesCapture(capture?.state ?? null, captureGen, p.state, generation)) return;`. A rejected input changes nothing, not even `captureSeq`. The existing seq guard and the rest of the function are unchanged.

**FC3.3 `refreshChannels` (ethPanel.ts).** In the `if (bound)` block, when `own` is found:
1. **First**, if `typeof own.state === 'string'`, call `applyCapture(own, typeof own.generation === 'number' ? own.generation : null)`. **No seq argument**: the reply has no event seq and none is invented.
2. **Then** run the existing `startCursor` line, unchanged, so a new-generation reset from `applyCapture` cannot drop the reply's own cursor.

Nothing else changes: `others`, card rendering, the timers and the control handler stay as they are.

## C3 tests (Opus; baseline on scaffold)
- `ui/src/panels/eth/ethC3.test.ts`: 3 failing + 1 passing. The passing one guards against regression: a stale status reply (no seq) must never move back a newer control state, and an older generation is ignored.

Acceptance:
- **Tests:** from ui/, `npx vitest run --maxWorkers=2 src/panels/eth src/sdk/treeHex.test.ts` all pass, existing ETH UI tests included.
- **Typecheck and lint:** isolated tsc app/node 0; eslint on the 2 changed files and the new test 0.
- **Scope:** no Cargo, no whole UI suite. ROOT then rebuilds the UI and rechecks the `upqa-eth-c1mid` late-open: REPLAY_DONE, [캡처 정지] disabled.

---

# Standing rule (ROOT, 2026-11-16, after C3): imports are never worker edits
Sonnet writes only function, callback, reducer or RPC-wiring bodies. Imports, types, signatures, consts and manifest lines are Opus-only, even when a body cannot compile without them.
- Opus puts every import a leased body needs into the scaffold.
- If one is still missing, the worker stops and returns that file's lease, reporting which import. Opus makes the static edit, re-freezes, and sends RESUME with new hashes.
- Opus rejects at review any worker diff touching static lines.
