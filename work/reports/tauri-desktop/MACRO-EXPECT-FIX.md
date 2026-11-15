# Focused UART real-engine macro fix — foreground Opus 5.5 worker

You are the IMPLEMENTATION WORKER yourself, not another orchestrator. Do not spawn any Agent/Task/subagent or background LLM. Work directly in this visible native foreground session. ROOT owns orchestration/review/index, finalQA w2R owns live UI/build, startup-fix-opus owns ShellApp+route tests. No file overlap.

Read `qa/FINDING-MACRO-EXPECT.md`. Own ONLY `ui/src/workbench/uart/workbench.ts`, its `workbench.test.ts`, and (only if needed for stale run strip) `ui/src/panels/uart/terminalPanel.ts` plus focused regression tests. Do not modify shell, other modules, original canonical prototypes, desktop, Git, production dist/build, existing live fixture or browser. No need long previous context.

Real failure: every macro wait sends pattern kind `text`, but actual Rust daemon and CLI use `literal|regex|prompt`. Fix to `literal` and assert actual request shape against verified Rust/CLI contract. Keep matched-only verdict, STOP/abort/generation guard, pre-register before send, cleanup and no retry. These are current required behavior, not a new design.

Secondary: finalQA screenshot has a stale '실행 중 1/2' strip above final failure while runningCount0. Inspect actual renderRun/cache/invalidation/replace behavior; reproduce with an async expect rejection, then remove stale running UI. Capture correct final failed/stopped state with focused meaningful renderer/state regression, not a copied implementation assertion. Consider whether pre-registration reject occurs before send→wait and no new run should leave old DOM. Keep the error visible and ensure no additional sends/retries.

Run focused UART workbench/panel tests + whole UI typecheck and scoped eslint. Do NOT run full UI suite or any build/browser. Return READY with exact files/tests/source fingerprint in `MACRO-EXPECT-FIX-STATUS.md` next to this file. FinalQA exercises actual current fixture and owns the one final rebuild combining this and startup fix. Coordinate findings only through ROOT, no extra hierarchy.
