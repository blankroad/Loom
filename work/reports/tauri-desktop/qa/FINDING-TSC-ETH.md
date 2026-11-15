# FINDING — production UI build blocked by an ETH typecheck error (final QA w2R, 2026-11-15 11:4x)

The single full-UI check ran with no concurrent load (`pgrep vitest|vite|cargo|rustc` empty). Log: `evidence/final/ui-checks.log`.
| check | result |
|---|---|
| `pnpm check:schema` | exit 0 |
| `pnpm typecheck` (`tsc -b`) | **exit 1** |
| `pnpm lint` | exit 0 |
| `pnpm test` | **59 files / 666 tests passed**. The earlier `shell/dock/defaultLayout` timeout (665/666 under load) does **not** reproduce. |

```
src/panels/eth/ethPanel.ts(293,33): error TS2345: Argument of type 'HTMLSpanElement & { title: string; }'
  is not assignable to parameter of type 'HTMLDivElement'.
```
- **Location:** `ui/src/panels/eth/ethPanel.ts:293`, in `refreshChannels`. `cards` is inferred as `HTMLDivElement[]` from the
  `.map(...)` that returns `card` (a div). The line then does `cards.push(Object.assign(el('span','eth-ch-note',…),{title}))`.
- **Impact:** `stage-loomd.sh --force-ui` runs `pnpm build` = `tsc -b && vite build`, so **no production UI build is possible**. Vitest does
  not typecheck, so the tests are green.
- **Note:** B's STATUS said tsc was clean for its files. The file mtime is 11:27:14, so this is possibly from a late edit.
- **Owner:** ETH (bench-diag-opus, w2P closed). QA did not edit it (cross-boundary).
- **Minimal type-only fix (for the owner/ROOT to apply):** widen the array type, e.g. `const cards: HTMLElement[] = others.map(...)`,
  or make the note a div. It has no runtime behaviour change; the DOM output is identical.
- **QA state:** halted before `stage-loomd`. No build, no app, no Ego operation, no fixtures running. QA resumes at
  stage-loomd after ROOT says the source is fixed, then reruns `pnpm typecheck` (and the full suite once more only if other files changed).

## Resolution (ROOT-approved limited ownership transfer of this one line to QA, 2026-11-15)
- Edit (only change): `ui/src/panels/eth/ethPanel.ts:267` `const cards = list.map(` → `const cards: HTMLElement[] = list.map(`.
  Type annotation only: the emitted JS and the DOM are identical. No other ETH change.
- sha256 before: 51e2ef1b3b9fb9f0464dff846821828a0e80ddccc77e4049aee83823f70ff2bd · after: 66268aad8d09ee4c91e708095baa0816e34943409fe78a37d2a6503d52690207
- `pnpm typecheck` (full `tsc -b`) → **exit 0**. Per ROOT, lint/schema/tests were not rerun: they passed in the same clean run (666/666), and
  a type-only change cannot alter them. ETH-focused tests were judged unnecessary for the same reason.

## Related limited transfer (ROOT, 2026-11-15): UDS empty-state CSS one line
`ui/src/panels/uds/uds.css` gained `.uds-wb .seq li.empty { display: block; }`, right after the `.seq li` grid rule. It is pure CSS, scoped to the
empty-state item; the event rows keep their grid. ROOT reviewed it visually: the 응답 순서 empty text was wrapping a word at a time inside the 58px first
column. No unit test (per ROOT). It is verified by the final-build screenshot geometry.

## Related limited transfer (ROOT, 2026-11-15): whitespace only
`ui/src/panels/uart/uart.css`: the trailing extra blank line at EOF was removed (2 newlines → 1). No other byte changed, and there is no design or behaviour change.
The whole-tree `git diff --check` is now clean.
