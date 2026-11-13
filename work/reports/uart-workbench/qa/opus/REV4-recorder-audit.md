# Opus Recorder / timestamp gutter: READ-ONLY source audit (no browser)

- Auditor: QA (claude-opus-5-5). This is a source read only. No source edits, no browser, no local tests rerun. **Not browser verification.**
- File read: on-disk `opus/app.js` sha256 **ce2065c6** (index bfda50f8, styles 0f8f6329) at audit time.
  - REV4 manifest says app **f9c0b0c5**, but the owner had already moved on to REV5 (macro/broadcast), and REV4 is not kept as a separate file.
  - Line numbers below are for ce2065c6. Re-confirm every finding against the next frozen hash.

## Key acceptance points

| # | Point | Source evidence | Verdict (static) |
|---|---|---|---|
| 1 | Real file, periodic commit (close/reopen) | `commit()` app.js:404-411. Serialized on `this.q`: `writable.close()` → `committed = written` → `createWritable({keepExistingData:true})` → `seek(written)`. Interval from `recCommitMs()` is 1 s for files under 4 MB, then +1 s per 4 MB, at most 10 s. `stop()` does a final close (app.js:432). | Design OK. On-disk growth before Stop can only be proven natively (R2b: read the file from the shell ≥2 times while recording). |
| 2 | Buffer is explicitly not a file | Separate `sink:'buffer'`. Pill `● 버퍼`. Status text "파일에 저장되지 않음 — 내보내기 필요". Close toast "버퍼 종료 (내보내기 필요)". 5 MiB limit → `fail` with an export hint. | OK |
| 3 | Pause/clear/filter/cap taps raw | `feed()` app.js:271-276 calls `rec.tap('rx', raw, now)` before `xwrite` (pause) and before `logAssemble` (filter/3000-line cap). Clear touches the view only. TX is tapped in `transmit()` before echo (app.js:331). Each broadcast recipient goes through its own `transmit()`, so TX metadata is per recipient. | OK |
| 4 | Display timestamp, no VT injection | Gutter is a separate DOM `.tsgutter` driven by `registerMarker` per row (`markRows`, app.js:213-221). `xwrite` writes the raw bytes only. The file timestamp (`fileTs`) is independent of the display toggle. | OK, one display-only exception: the overflow notice `· 표시 누락 N B` is written into xterm as VT text (app.js:207). It is intended and labelled display-only. |
| 5 | `.log` line-based vs `.jsonl` raw chunk | `.log`: RX goes through `lineEmu` (handles \r, \b, ESC[C/D/K); line time = arrival of the line's first char. TX keystrokes are folded into logical lines with controls shown as `<xx>`. `.jsonl`: one `data {t,ms,dir,len,data}` per tap. | See findings F2 and F3 |
| 6 | Local Stop keeps TX already typed | `stop()` app.js:420-433 writes the partial RX line and the partial TX keystroke line with `⟨줄바꿈 없음⟩`, then the footer and a final flush, then closes after the pending `q`. | OK |
| 7 | Reload / disconnect | Disconnect or lost → `rec.stop(...)` (app.js:553). Tab or pane close → `stop` (app.js:197). `beforeunload` prompts while recording (app.js:534-539). Only `recPrefs` persist, so a recording never auto-resumes. | OK |

## Findings for the owner (non-blocking unless MAIN decides otherwise)
- **F1 (low): display data dropped silently on long pause.** `xwrite` pause buffer `xpend` is capped at 300 000 chars and shifts out the oldest chunks without adding to `dispGap` (app.js:203). After resume, no `표시 누락` notice appears for the dropped part. The recording is unaffected because the tap comes first. This conflicts with "pause-resume catches up, no lost display data" for very long pauses (≈70 min at mcu telemetry rate). Expected: count the shifted bytes into `dispGap` or show the notice.
- **F2 (medium for real-hardware fidelity, informational in this prototype): `.jsonl` is not raw bytes.** `data` is the decoded JS string and `len` is its UTF-8 length.
  - On a baud mismatch or with binary data, wire bytes ≠ `len`. For example, garbled `þ` counts 2 bytes, and a raw 0xFE byte cannot be represented losslessly.
  - A production adapter would need a byte encoding, such as `hex` or `b64` next to `data`.
- **F3 (informational): `.log` TX lines record keystrokes, not the command the device ran.**
  - `ver` + Tab is logged as `ver` with a literal TAB.
  - ↑ (history) then Enter logs an empty TX line because `stripAnsi` removes `ESC[A`.
  - The effective command only appears in the RX echo. That is accurate as TX, but it can mislead readers of `.log`.
- **F4 (doc nit):** the `beforeunload` comment says "~5s checkpoint" (app.js:538), but the code commits every 1 s for files under 4 MB.

## Native checks still required (after control returns and a frozen hash)
T1, T2, R1–R6 and R2b in `qa/opus/STATUS.md`. R1/R2 need MAIN to coordinate the native `showSaveFilePicker` dialog. Chrome writes `<name>.crswap` during recording, so the shell should read the chosen file name itself: its content = last committed checkpoint.

## MAIN disposition
- Static core Recorder/gutter structure accepted. Native file proof (R1/R2/R2b) still pending.
- F1 returned to owner as a narrow fix: count chunks dropped from the paused display into `dispGap` and show the notice. QA will re-check natively on the next freeze.
- F2/F3: prototype/documentation limits; hardware byte encoding is out of scope. F4: comment only.
