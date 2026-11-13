# UDS Read DID — Raw UDS PDU sender · Opus RAW-HANDOFF

Status: **ACCEPTED** — ROOT final native acceptance recorded in [`qa/raw/NATIVE.md`](qa/raw/NATIVE.md); SOURCE_FREEZE §4 (10 hashes) verified by ROOT (2026-11-14, Opus 5.5 w1E:p1, base main 4fb848a). READY_RETURN. Opus owns `app/` and this file only.

## 0. Preservation
- The accepted CONFIG sources are copied verbatim to `app/selfcheck/baseline-config/`, along with the config freeze and both result files. `shasum -c source-sha256-freeze-config.txt` reports 9/9 OK there.
- `HANDOFF.md`, `CONFIG-HANDOFF.md`, `baseline-p1/` and both older freeze files stay untouched.
- The new freeze will be `app/selfcheck/source-sha256-freeze-raw.txt`.

## 1. Plan (matches the code as built)
### 1. Engine
`parseRawHex()` uses one grammar. The whole input is either accepted or rejected; nothing is parsed or sent in part.
- **Separators:** whitespace only (space, tab, newline).
- **Tokens:** each token is `0x`/`0X` followed by exactly 2 hex digits, or a compact even-length run of hex digits. `22 F1 90`, `22F190`, `0x22 0xF1 0x90` and `22 F190` are equivalent.
- **Rejected:** empty input, an odd-length run, non-hex characters (**including commas**), a bare `0x`, `0x2`, `0x220`, more than **4095 B** (`RAW_MAX_BYTES`), or more than 24576 characters (`RAW_TEXT_MAX`). These are prototype limits, not transport limits.
- **Error messages:** each error names the offending token, quoted and truncated to 12 characters.

`sendRaw()` sends the exact bytes. The txn records `kind:'raw'`, `sid` and `request`. It shares `begin()` with `read()`, so the route/profile/timing snapshot, online guard, generation, P2/P2*, pending, late and cancel handling are unchanged.

### 2. SIM
- **The [정상] scenario** is the mock ECU's base behaviour:
  - `22 hi lo` → the fixture path; an uncatalogued DID gets `7F 22 31`.
  - Any other `0x22` shape (1 B, 2 B, 4 B or more, multi-DID) → `7F 22 13`.
  - **Every non-`0x22` SID** → `7F <SID> 11`, labelled mock. This includes sub-function SPRMIB requests and response-range SIDs; there is no silent or timeout special case.
- **NRC and pending scenarios** send `7F <actual SID> nrc` and `7F <actual SID> 78`.
- **Confirmed values** change only when `sid==0x22`, the request is exactly 3 B and the echo is valid. A non-0x22 or generic positive response never applies.

### 3. UI
- A `DID | Raw` radiogroup (←→) sits in the request editor.
- The Raw editor has a textarea, a normalized preview with byte count, and a SID/byte-count summary.
- Each mode keeps its own input when you switch. Picking a DID from the list or quick select switches to DID mode, and the raw text is kept.
- Only `[전송]` or Ctrl/⌘+Enter sends. Enter adds a newline.
- Prefs: `reqMode`/`rawText` are additive top-level keys in schema 2. The config and its profiles are untouched. Raw text over `RAW_TEXT_MAX` is **not persisted** (never truncated); after a reload the field shows the last saved value or the default `22 F1 90`.

### 4. Checks
`selfcheck/raw-check.cjs`, plus the existing `check.cjs` and `config-check.cjs` → SOURCE_FREEZE.

## 2. Progress log
- Plan written; config baseline snapshot verified.
- **First runnable path (ready for ROOT native):** connect, click `[Raw]` (default text `22 F1 90`), then `[전송]`.
  - Txn #1 has `kind raw`, SID 0x22, request `22 F1 90`.
  - It goes positive and the VIN confirmed value is applied, tagged RAW.
  - The history cell shows `RAW F190 VIN`, and the detail `요청` row says `Raw UDS PDU · SID 0x22 … 입력 그대로`.
  - Regression: `check.cjs` 70/70 and `config-check.cjs` 72/72 PASS.
- **ROOT first native PASS** recorded by ROOT (`qa/raw/initial-1440.png`). ROOT's one label concern is fixed: the history header is now `DID · SID` (with a title explaining it), checked by raw-check G1b.
- Source checks were added and the freeze was taken (§4–§6).

## 3. Selectors (for ROOT native QA)
- **Mode:** `#segMode` (radiogroup), `#segMode [data-mode=did|raw]`, and the `#req` class `m-did` or `m-raw`.
- **Raw editor:** `#rawInput` (textarea; `aria-invalid`, class `invalid`), `#rawSum` (SID + name + byte count, live), `#rawPdu` (normalized preview, scrollable), `#rawHint`.
- **Shared controls:**
  - `#btnRead` with `#btnReadLabel` (`읽기` / `전송`);
  - `#btnCancel`;
  - `#didMsg`, the shared message line used by both modes (error text ends `— 전송하지 않음`).
- **History:** the header `.tx-head .c-did` reads `DID · SID`. Raw rows have `.tx-row.k-raw`, and `.c-did .rawtag` (`RAW`). The cell text is `F190 VIN` for the exact shape, `0x31 RoutineControl` for other services, and `0x22 ReadDataByIdentifier (형식 불일치)` for a malformed 0x22. `.c-req` shows the first 64 B, and its title holds the length.
- **Detail:**
  - `#seqHead` ends `· RAW 0xNN`;
  - `#fields` has the `요청` snapshot row and raw field names `SID`/`DID`/`형식`/`subFunction`/`parameters`/`SIDRQ-NR`/`NRC`;
  - `.applied` shows `Raw 요청 SID … — DID 확정 값과 무관` for raw requests that are not a single DID.
- **Values:** `#valsBody .cv-p .rawtag` marks a value confirmed from a raw request.
- **Export:** each JSON/CSV row adds `kind`, `sid`, `requestLen` (JSON also has `sidName`). `did`/`didName` are empty for raw requests that are not a single DID.

## 4. SOURCE_FREEZE (`app/selfcheck/source-sha256-freeze-raw.txt`, 10/10 OK)
```
d7c5d8c35252ee536dc3b1f5bbb5a8bf0b03a9932db59b9cd490e6bfd46b6faa  index.html
1cc606571d961bef067aafe3ba8486ff488713cd00cc744abe0023f06aa38c45  styles.css
2b0dd5cf3cafdfeb4ebc551f3d0880f7e75fc0dc523c72471a400faedfa5beb9  engine.js
11413b50abce503953560cc4005e8f384d94e403f0d0e0d1487bef28a97cfa4b  app.js
db1b9ce474f5bc5d0619233dd565280236f0112549ef5fbd774640abd259acf5  tools.js
2b3592854ffe8cee8898c8733d46647f829b2bb6c2c365b8d64ef54adcd22231  config.js              (unchanged)
dff61c7d9c82af545bf77af1091163ec42f39790253f05ff7ff4232cbb278c56  selfcheck/harness.cjs  (unchanged)
27f98e01ddc7f8db08316485ce32121da80986cfa545322523f474747f19c6d3  selfcheck/check.cjs    (unchanged)
c131415694560f13a45a4e5d10bef3846a3132916b110a8ba68f906049ff6fe5  selfcheck/config-check.cjs (unchanged)
9666ce20e27f64724a2872dd1a1935b81d2d005b77dba26a08b081e7395bb52f  selfcheck/raw-check.cjs (new)
```
- `baseline-config/` verifies 9/9 OK and `baseline-p1/` verifies 7/7 OK.
- All 6 assets return 200 on the existing :4180 preview; no new server was started.
- The 8 unrelated dirty docs and git are untouched.

## 5. Commands and results (source-local jsdom + fake clock, not native proof)
Every script closes its jsdom windows and clears the tick, so the process exits.

| Command (from repo root) | Result | Result file |
|---|---|---|
| `node work/reports/uds-read-did/app/selfcheck/check.cjs` | **70/70 PASS** (phase-1 regression incl. U1/U2) | `selfcheck-result.json` |
| `node work/reports/uds-read-did/app/selfcheck/config-check.cjs` | **72/72 PASS** (config regression incl. C2) | `config-check-result.json` |
| `node work/reports/uds-read-did/app/selfcheck/raw-check.cjs` | **55/55 PASS** | `raw-check-result.json` |

What `raw-check.cjs` covers:
- **A — parser:**
  - spaced, compact, `0x`, tab/newline and mixed input are equivalent;
  - empty, odd, non-hex, comma and bare/long `0x` input is rejected with no bytes, and the error names the token;
  - zeros, FF, one-byte and non-22 payloads are kept;
  - 4095 B is accepted and 4096 B rejected;
  - the text cap applies;
  - `rawDidOf` matches the exact shape only.
- **B — engine:**
  - offline or invalid input produces zero TX and no timers;
  - raw `22F190` gives a positive response and the value is applied with `kind:'raw'`;
  - 12 raw cases are sent byte-exact, each getting `7F SID 11`, `7F 22 13` or `7F 22 31`;
  - none of them touches confirmed values.
- **C — SID echo:**
  - NRC `7F 31 22` and pending `7F 2E 78`×2 echo the actual SID;
  - the DID path is unchanged;
  - after a raw timeout, the late reply is ignored;
  - generic positive decode never counts as a DID echo;
  - an SIDRQ mismatch is flagged;
  - one-byte and SPRMIB requests decode;
  - the legacy `decodePdu(bytes, did)` signature still works.
- **D — snapshot:**
  - a custom profile, route and timing are snapshotted;
  - one active request per context applies across both modes;
  - the online config guard holds;
  - another route stays independent;
  - after a raw cancel, late replies are ignored.
- **E — UI:**
  - labels; empty submit gives zero TX, focus and `aria-invalid`;
  - invalid input shows no partial preview;
  - Enter adds a newline, while Ctrl+Enter and ⌘+Enter send;
  - RAW row and detail labels, with no VIN label on raw rows that are not a DID read;
  - Tx field offsets;
  - editing or switching mode/route while pending leaves the row unchanged;
  - each mode keeps its own input;
  - busy state;
  - picking a DID switches to DID mode and keeps the raw text;
  - ←→ on the radiogroup;
  - one-byte `3E`;
  - 4095 B: preview, exact send, 256 hex rows, truncated row text with a length title;
  - JSON/CSV fields;
  - prefs persist and reload without connecting or sending;
  - oversize text is never truncated and not persisted.
- **F — migration:** config-era schema-2 prefs load in DID mode with the profiles and active selection identical. After a save the `config` block is byte-identical. Bad types fall back.
- **G — static:** the raw preview wraps and scrolls; the raw grid is a single column at ≤760; the history header reads `DID · SID`; the hex caption still says "UDS PDU only".

## 6. Known mock limits
- **Mock ECU services:** only 0x22 with a single DID is implemented.
  - Every other SID gets the mock `7F <SID> 11`, which is not a real ECU effect. There is no session, security, write, DTC, routine or flash logic.
  - A suppressed positive response (SPRMIB) is still answered with an NRC.
  - Multi-DID 0x22 is not modelled and gets `7F 22 13`.
- **SIM scenarios** (NRC, 0x78, timeout, late, disconnect) apply to any raw SID. [정상] means the mock ECU's base behaviour.
- **No framing:** raw bytes are UDS PDUs only. No CAN-TP segmentation, FC, DoIP header or EthDiag framing is generated. 4095 B is a prototype cap, not a transport limit.
- **Labels and decoding:** SID names are labels only. Raw decoding covers the SID, the exact 0x22 DID, and the sub-function/SPRMIB bit for sub-function services; the remaining bytes are shown raw, and table cells truncate after 48–64 B while the hex pane shows everything.
- **Prefs:** a reload restores the mode and raw text but never resends, reconnects or restores history.
- **Trace format:** still `loom-uds-read-did-trace/1`; the new fields are additive.
