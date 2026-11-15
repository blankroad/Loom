# bench-final-qa-opus (w2R) STATUS — concise (older: STATUS-HISTORY.md)

> ROOT follow-up: Windows/Ubuntu support changed desktop runtime/build sources after final2. Prior final2 artifacts/staging are preserved as historical proof; fresh matching Mac ordinary/e2e builds are required before accepting current-source native behavior. Read the ROOT update at the top of RESUME-FINAL2.md and `../../desktop-cross-platform/STATUS.md`. Native visible checks remain pending; no pass was promoted.

**State: RETURNED — FINAL2_BUILT, NATIVE PENDING (locked). Resume from RESUME-FINAL2.md** · updated 2026-11-15 ~12:45
- No QA process is running: no app, daemon, fixture or observer (pgrep checked). The ROOT CUA lease is returned (NATIVE_CHECKS_DONE for build #1).
- The ROOT-requested user unlock is pending. QA is not polling the screen and does not bypass the lock (no caffeinate or OS settings).
- **Ego: space 14 no longer exists** (the listing shows only user spaces 0/10/11/12). QA did not close it: no `finish()`, and the last op was a read-only claim/adopt.
  The canonical PNGs are kept in parity/. **ROOT decision:** ROOT creates ONE new TaskSpace (static REPORT preview while locked; that preview is not native PASS)
  and passes the ID → **space 16** (p1 = REPORT.html draft). Reuse it; never create another.

## Two builds — do not mix
| | Build #1 (pre-patch prefix) | **final2 (combined final)** |
|---|---|---|
| UI / loomd | 8215041d… / 8ed2ef33… | **643e5952… / 7efffad8…** |
| dist / e2e exe | 3643d396… / 159e30af… | **0a2a7a52… / 0265f8aa…** |
| Evidence | evidence/final, native-final-{attempt1-308,ordinary-cua,e2e-prefix} | evidence/final2 (build only) |
| Native | done (see REPORT.html §2) | **PENDING** |
final2 checks: full UI 686/686 + schema/tsc/lint exit 0, git diff --check clean; stage 6/6; verify-dist PASS; verify-bundle PASS; both apps embed the current index.

## Next (after unlock + ROOT notice), in this order
Fresh e2e run (identity, startup positive/negative) → macro wait/strip/hidden recipients/split/hidden REC → ETH 2 exports + failed-TX reason →
UDS collapsed capture with a selected row + li.empty geometry → Ego (new space after ROOT approval) digests and canonical captures →
ROOT CUA hold on a plain final2 app (keyboard broadcast, with a fresh `loom events --since now --follow --timeout 120` observer armed first) →
ui-mismatch + error/Retry → global STOP last → REPORT final, hygiene, cleanup → READY_FOR_REVIEW.

Report draft: **REPORT.html** (build #1 evidence + final2 build; every final2 native item is marked PENDING). Findings: FINDING-*.md.
