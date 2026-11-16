# OPEN — archive UI silently evicts while declaring all pages loaded

Actual Ego18:p1 first3 build UI5be9486d / loomd24b7928d; private daemon55517 port62450.
Capture lc-1a1e0eef471-1: complete archive27499 annotations, raw3007362 samples, live9847 annotations and17652 dropped. Archive view loaded via repeated button clicks:
5652 ->11222 ->16702 ->20000 ->20000; more button then hidden, text `20,000행 · 아카이브에서 불러옴 (전체 27,499행)`.
Evidence browser/logic-archive-loaded.txt + .png (ROOT inspected screenshot).
Cause verified source: loadRows feeds CaptureData.addAnnotations (20000 cap, evicts prior rows); EOF sets archRowsDone true; archiveViewCoverage ignores held eviction and is used in copy metadata. Archive bytes remain present; this is bounded frontend paging/truthfulness defect, not missing durable acquisition data.
Assigned Logic Opus isolated worktree contract/tests -> existing Sonnet product bodies only. Required: bounded navigation/access across all archive windows, held-range/cap and search/copy scope explicit, no complete copy status on evicted subset. No root main source edits until correction returned. ROOT will integrate incremental correction, rebuild matching UI, and recheck actual27499-row scenario.
