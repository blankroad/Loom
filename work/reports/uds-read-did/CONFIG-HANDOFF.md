# UDS Read DID — connection configuration · Opus CONFIG-HANDOFF

Status: **ACCEPTED** — ROOT native acceptance (incl. C2 recheck) recorded in [`qa/config/NATIVE.md`](qa/config/NATIVE.md); SOURCE_FREEZE §5 verified 9/9 by ROOT (2026-11-14, Opus 5.5 w1D:p1, base main 4fb848a). Opus owns `app/` and this file only. Same mock page extended; no redesign.

## 0. Preservation
- The accepted Phase I sources are copied verbatim to `app/selfcheck/baseline-p1/`. `shasum -c selfcheck/source-sha256-freeze.txt` reports 7/7 OK there, and the accepted 70/70 result is copied alongside.
- The old `app/HANDOFF.md` and `selfcheck/source-sha256-freeze.txt` stay untouched. The new freeze goes in `app/selfcheck/source-sha256-freeze-config.txt`.

## 1. Audit — configuration currently hard-coded in the Read DID slice
| Item | Phase I state | Decision |
|---|---|---|
| DoIP channel / host / port / tester LA / ECU LA | constants (`ETH1`, `192.168.10.20:13400`, `0x0E00`, per-ECU LA) | **editable per ECU+route profile** |
| CAN-TP channel / bitrate / Tx-Rx IDs / 11-29-bit | constants (CAN1 500k, 11-bit) | **editable**; bitrate is labelled as metadata (the SIM uses no bit timing) |
| CAN-TP addressing mode | text "normal · physical" | **kept read-only**. Only normal physical addressing is modelled. No extended/mixed/functional mode, no segmentation options. |
| EthDiag channel / adapter | constants | **editable**: channel, adapter label and an endpoint note. The note is opaque metadata that is never parsed or used. Vendor mock, framing undefined, no DoIP fields. |
| Client P2 / P2* | global `TIMING` 1000/5000 | **per-profile**, integer ms, range-checked, defaults 1000/5000; the engine uses them for real |
| SIM latency / pending interval / late offset | SIM options | **unchanged and separate**. The late reply arrives at configured P2 + 1200 ms. |
| Profile provenance in request / detail / confirmed / export | n/a | **added**: id, name, source (preset/custom), base preset, rev, values, timing |
| Prefs schema version | unversioned v1 object | **schema 2 in the same key** `loom.udsReadDid.v1`. Top-level prefs are unchanged, so the baseline app still reads them; `config` is added. v1 migrates with presets active. |
| Config file import/export | n/a | **added after the core path**, separate from the trace export |
| Deferred, with rationale | | Functional/extended/mixed addressing and ISO-TP FC/STmin/BS/padding/CAN FD: not modelled, so offering them would be fake.<br>DoIP routing-activation type, TLS 3496, entity discovery/VIN/EID: no DoIP layer is simulated.<br>S3/TesterPresent, sessions, security, P6/P4 extended timings: out of scope per task.<br>Custom SIM latency values, history limit, DID catalogue/ODX editing: not a connection config; no user need stated. |

## 2. Chosen lifecycle (enforced in engine, mirrored in UI)
- **Scope.** Configuration belongs to the context, ECU × route. Each context has one read-only **preset** (`preset:BCM|doip` etc.; its values are never mutated) and zero or more **custom** profiles (`u<n>`) of the same transport. One profile is **active** per context; the default is the preset. Switching ECU or route shows only that context's own active profile and draft and never applies another context's values.
- **Draft.** `연결 설정` opens an inline panel under the context strip, scoped by the label `BCM · DoIP`.
  - There is one draft per context. It is kept in memory when you switch away and back, and it is not persisted.
  - Editing is allowed at any time, including while online. Validation is field-local and live.
  - The profile select changes only the *edit target*. It is locked while the draft has unsaved changes, so changes are never discarded silently.
- **Panel actions.**
  - `복제`: new unsaved custom draft.
  - `프리셋 값`: loads the preset values into the draft only.
  - `되돌리기`: draft back to the target's saved values.
  - `취소`: discard the draft and close.
  - `Esc`: close and keep the draft; the button shows `•`.
  - `삭제`: custom profiles only. Deleting the active profile requires offline, and the context then falls back to the preset, with a message.
- **Save and apply.**
  - Saving applies the draft as the context's active profile.
  - **Online refuses.** The UI disables Save and shows the reason plus an explicit `연결 해제` action. The engine also returns `code:'online'` and nothing mutates.
  - **Invalid refuses.** Field errors appear, focus moves to the first invalid field, and nothing mutates.
  - The outcome depends on the edit target:
    - preset target, values unchanged → apply the preset (the **reset path**);
    - preset target, values changed → a new custom profile;
    - custom target → rev+1;
    - new target → a custom profile with rev 1.
- **Connection snapshot.** `connect()` freezes the active profile (identity, rev, values, route view) onto the context connection. `read()` uses only that snapshot. Each transaction deep-copies route, profile and timing, and confirmed values copy them as provenance. Deletes, saves and imports therefore never relabel old or in-flight transactions. The ECU+DID monotonic rule and the generation guard are unchanged.
- **SIM honesty.** The SIM ECU answers for the context's fixture ECU whatever address is configured; address routing is not modelled. The panel says this in one line.
- **Validation rules.**
  - Host: IPv4 dotted quad (0–255, no leading zeros) or an RFC 1123 hostname. IPv6 and `host:port` are rejected explicitly. No DNS lookup is made.
  - Port: integer 1–65535.
  - Logical address: 1–4 hex digits, 16-bit. Tester LA ≠ ECU LA.
  - CAN ID: 11-bit ≤ 0x7FF, 29-bit ≤ 0x1FFFFFFF. Tx ≠ Rx. A format change never truncates; the ID is flagged instead.
  - Timing: P2 is an integer 50–30000 ms. P2* is an integer 100–60000 ms and must be ≥ P2.
  - Name: 1–32 characters, unique per context.
  - Text metadata: no control characters, with length limits.
- **Persistence.** The key is `loom.udsReadDid.v1` with `schema:2`. Invalid stored profiles are dropped with a notice, and an active reference to a missing profile falls back to the preset. Reload never connects or sends.
- **Import.** Format `loom-uds-read-did-config/1`. The file is fully validated and then added atomically as new custom profiles; it never changes the active profiles or connections. Any invalid entry rejects the whole file.

## 3. Increments
1. Engine profile store, validation, and connection/txn snapshot, plus the DoIP edit/save/reconnect UI. **First runnable path.**
2. CAN-TP and EthDiag forms, timing behaviour, provenance in detail/values/export.
3. Persistence migration, config import/export, and responsive/keyboard polish.
4. Source checks (`selfcheck/config-check.cjs`), regression `check.cjs`, and SOURCE_FREEZE.

## 4. Progress log
- Audit and plan written; baseline snapshot verified.
- **First runnable path**: editable DoIP draft → save offline → reconnect → read uses the new snapshot. ROOT natively passed it, and the later CAN/timing/import/export/EthDiag/reload native passes are in `qa/config/NATIVE.md`.
- **ROOT source concern (cap)**: `saveProfile` creation is now capped at `MAX_PROFILES` (60), the same limit load and import use. The check runs before any id or store change. Updates and preset apply are still allowed. Covered by config-check E.
- **ROOT native C2 (fixed)**: after Cancel and reopen, the DOM showed the stale host.
  - Cause: a fresh draft restarted at v1, so the build signature repeated and the panel was not rebuilt.
  - Fix: one module-wide `draftGen` for every sealed or replaced draft, and the panel signature is dropped on close.
  - The new checks read the actual `#cf-host`/`#cf-port` DOM after Cancel and reopen, including two Cancels in a row.

## 5. SOURCE_FREEZE (`app/selfcheck/source-sha256-freeze-config.txt`, 9/9 OK)
```
561f54d901b01d764898a008cdb2a924a6f3abcdd5725f83dc5187c4b6944115  index.html
a1cabcf8a5df9240d07af45435e458ebfaba906efcae12bd57814af845c0f4f4  styles.css
efc798bf7b6364fd864b4cec158757818b0274ecc94de7c3ae8accf0cecb571c  engine.js
2d2a2ece4a6926cb9db83dfeaa50be2f4dc26dd7988440ff5cdcebafac1aa18b  app.js
2f6b5e9b32df239d16ab50ac550f738946d4086789db8af5244a2071e4f3341a  tools.js
2b3592854ffe8cee8898c8733d46647f829b2bb6c2c365b8d64ef54adcd22231  config.js   (new)
dff61c7d9c82af545bf77af1091163ec42f39790253f05ff7ff4232cbb278c56  selfcheck/harness.cjs  (+config.js)
27f98e01ddc7f8db08316485ce32121da80986cfa545322523f474747f19c6d3  selfcheck/check.cjs    (U2 guard scoped to original 560 block)
c131415694560f13a45a4e5d10bef3846a3132916b110a8ba68f906049ff6fe5  selfcheck/config-check.cjs (new)
```
The baseline `selfcheck/baseline-p1/` still verifies 7/7 OK. All 6 assets return 200 on :4180.

## 6. Commands and results (source-local jsdom + fake clock, not native proof)
- `node work/reports/uds-read-did/app/selfcheck/check.cjs`: **70/70 PASS**, prior regression including U1/U2. Result in `selfcheck-result.json`; the accepted run is kept in `baseline-p1/`.
- `node work/reports/uds-read-did/app/selfcheck/config-check.cjs`: **72/72 PASS**. Result in `config-check-result.json`. What it covers:
  - A — validation: host, IPv6, port, LA, CAN 11/29-bit, timing, EthDiag, channel.
  - B, C — presets immutable; reset path; invalid saves don't mutate and leave the id counter alone.
  - D — engine online guard; txn, confirmed and export snapshot; reconnect; delete fallback; ECU/route isolation.
  - E — creation cap.
  - F — P2/P2* behaviour versus SIM latency and pending interval.
  - G — v1→schema 2 migration, reload, corrupt entries, unknown schema.
  - H — atomic import/export.
  - I — UI lifecycle: field errors, focus, Escape vs Cancel, C2 DOM, context drafts, online Save, explicit disconnect, delete confirm, duplicate, SIM label, labels.
- Both scripts close every jsdom window and clear the 200 ms tick, so the process exits.

## 7. Selectors (additions)
- **Context:** `#btnCfg` (aria-expanded, `#cfgDirty` dot) and `#cfgProv` (`프리셋` / `사용자 "name" rN`, class `custom`).
- **Panel:** `#cfgPanel` (form, region). Inside it:
  - `#cfgTitle` (scope), `#cfgProfile` (edit target; locked while dirty), `#cfgSrc`;
  - buttons `[data-cfg=dup|preset|delete|export|import|revert|cancel|disconnect]`, plus `#cfgDel`, `#cfgDisc`, `#cfgRevert`, `#cfgSave` (submit), `#cfgFile`;
  - `#cfgStatus` and `#cfgNotice`.
- **Fields:** `#cf-<k>`, with `#cf-<k>-err` and `#cf-<k>-hint`. The keys are:
  - `name`, `channel`, `p2`, `p2star` on every route;
  - DoIP: `host`, `port`, `testerLA`, `ecuLA`;
  - CAN-TP: `bitrate`, `idFormat`, `txId`, `rxId`;
  - EthDiag: `adapter`, `endpoint`.
- **Keys:** Enter = Save, Esc = close and keep the draft.
- **Shown elsewhere:** `#reqTarget` (profile + P2), `#routeInfo` (profile/gen snapshot + P2/P2*), `#fields` snapshot (`연결 프로필`, `클라이언트 타이밍`), `#valsBody .cv-p` (custom ptag), `#simTiming`.
- **Trace export fields:** `profileId/Name/Source/Base/Rev`, `p2Ms`, `p2starMs`, `connectionGen`, `config{}`.

## 8. Added and deferred
- **Added:** everything in table §1 marked editable/added.
- **Deferred**, as §1 says: no extra addressing modes, no ISO-TP or DoIP layer options, no session/S3/security/DTC/write/ODX/retry/poll, and no custom SIM latency or history limit.
- **Known limits:**
  - Addresses are tester-side metadata. The SIM ECU answers regardless, and the panel says so.
  - Drafts are memory-only, so a reload discards an unsaved draft.
  - A config import adds profiles and never applies them.
  - Corrupt stored entries are dropped on the next save, with a notice.
