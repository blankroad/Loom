# FINDING (UI shell, A-owned; A is code-frozen → ROOT) — bootstrap fresh entry cannot reach `#/s/uart`. Native confirmation PENDING.

## Observation
The ordinary dist app was reopened (a new process, with `#boot=` and no `next`) while UART `dut` was the only open, live service. It landed on
**통합 / 기본 작업 공간 (`#/l/m1-default`)** with the generic interface list, not the UART workbench (evidence/native-current/plain-ordinary-reopen.png).
Entry criterion (a) in PARITY-PLAN requires the UART service workbench.

## Source analysis (ui/src/shell/ShellApp.tsx `useStartupModuleRoute` / `isAutoDefaultRoute`, bootstrap.ts)
- The bootstrap with no `next` → `next='/l/m1-default'` + `sessionStorage['loom.route.defaulted']='1'` (`markDefaultRoute`).
- `isAutoDefaultRoute()` **consumes** that flag (`removeItem`) whenever hash == `#/l/m1-default`.
- It is called (1) as the `useRef(isAutoDefaultRoute())` argument, which is **evaluated on every render**, and (2) **again** inside `check()`
  right before `location.replace('#/s/uart')`. Read (1) removes the flag, so read (2) returns false, and the redirect never fires on the
  bootstrap path. This is the path that both the desktop app and `loom ui` take.
- Test gap: `workbenchShell.test.tsx` "lands a default (non-explicit) start…" uses `location.hash = ''` (no flag needed), and
  "never overrides an explicit integrated route" uses `#/l/m1-default` with the flag cleared. No test covers `#/l/m1-default` + flag (the real bootstrap).

## Suggested minimal fix (owner/ROOT)
Read the flag exactly once per mount, e.g. `const pending = useRef<boolean | null>(null); if (pending.current === null) pending.current = isAutoDefaultRoute();`
In `check()`, do not call `isAutoDefaultRoute()` again: rely on `pending` plus the existing hashchange cancel, or verify `location.hash` is still the
initial default hash. Add a test: hash `#/l/m1-default` + `sessionStorage.setItem('loom.route.defaulted','1')` → a uart open event → `#/s/uart`.

## Native confirmation plan (QA, after ROOT CUA hold)
Use a fresh private home, open `dut` via the CLI **before** launching the e2e app, then run `wk.mjs geom` on the fresh entry (no nav) to record the route, rail and mtbar counts.
