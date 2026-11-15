# DOCK-ROUTE-FIX — STATUS: READY (2026-11-15)

Worker: foreground Opus 5.5, direct implementation (no Agent/Task/subagent). No Git writes, build, browser or fixture actions.
Fixes FINDING-UART-SCOPE-LAYOUT.md item 2: an agent deep link collapsed the user's dut | dut2 split and remounted panels.
Includes the ROOT review follow-up: at MAX_PANELS inside the live module, the workspace stays and the addition is refused (no `h.load` fallback).

## Root cause
`DockLayout` route effect: `#/p/…` always ran `h.load(slotLayout)` (`api.clear()` + `fromJSON`), which destroyed every panel.
Returning to `#/s/uart` then ran `h.load(preset)`, which rebuilt one tab group. Each round trip therefore disposed and remounted
every panel instance (input, recording, macro and broadcast state) and lost the user's split.

## Change (ui/src/shell/dock/DockLayout.tsx only; dockHost.ts NOT modified)
- New `live` ref `{ key, service }`. It is set when a `#/s/<svc>` route loads and cleared by `#/l/…` and by every single-target replace.
- `#/p/<d>/…` while `live` is set, `serviceAllowsDomain(live.service, d)` holds, and the dock has panels (in-module live focus):
  - target already open → `h.open` activates that tab. No load and no remount.
  - target missing and panels < MAX_PANELS → `h.open` adds one tab next to same-domain panels. Groups and other instances are untouched.
  - target missing and panels == MAX_PANELS → no change, plus a toast (`key: 'deeplink-limit'`): "보기가 32개로 가득 차 … 열지 않았습니다". The workspace and its instances are kept.
- Otherwise (fresh/empty dock, cross-module link, or coming from `#/l/…` saved layout/preset) → unchanged behaviour: `h.load(slotLayout)`, which replaces the dock with a single-target view (UI-T10, history/diagnostic links).
- Returning to the same `#/s/<svc>` key while `live` is still set and the dock is non-empty → no load and no `trackDesired` reset. It calls `sync()` once, then re-subscribes. `reconcile` still prunes by `availableSlot` (channel-close/unknown-channel pruning, true target scope) and adds new arrivals according to the preserved `desired` set and the autoOpen setting.
- Unchanged: explicit `#/l/<name>` (saved layout/preset) always applies; a different service route reloads its projection; UART "레이아웃 초기화" acts on the dock api directly; MAX_PANELS bound; preset limit toast.
- The header comment's contract is narrowed to say this.

## Regression (ui/src/shell/dock/defaultLayout.test.tsx — new describe "live service workspace across agent deep links")
These tests use the real DockLayout + DockHost + Dockview in jsdom with real `uart.terminal`/`uart.trace` panels. Nothing is mocked out of `h.load`.
The user's split is made with the real pane action `splitGroup(api, group, 'right')`, and a draft `<input>` is placed inside dut2's instance DOM.
1. `#/p/uart/terminal?channel=dut` → `#/s/uart`. Checks: same group ids/sizes/tab order, same panel objects and content elements, still connected, draft value kept. Then 100 round trips with `&at=seq:i`: 2 panels, same geometry, no remount.
2. In-module missing target (`uart.trace?channel=dut`) is added as one tab and the split is kept. A history link `channel=ghost` is shown, then pruned on return. 100 distinct hops reach exactly MAX_PANELS and the rest are refused with the toast. On every hop it checks the original group ids and the dut/dut2 panel identities; at the cap it also checks the same content elements and the draft. A known session is still focused at the cap. On return the hop views are pruned → 3 panels, original groups and instances, connected, draft kept.
3. Contract kept: a cross-module ETH link replaces the dock and `#/s/uart` reprojects. An explicit `#/l/my-split` applies the saved split. A deep link from `#/l` stays a single-target view. The stored layout is untouched.
- Against the exact pre-fix DockLayout (rebuilt in scratchpad; sha256 matched the original 2d19c1c7…), tests 1 and 2 FAIL and test 3 passes. Against the fix, all pass.

## Verification (run in ui/)
- `npx vitest run src/shell/dock src/shell/workbenchShell.test.tsx src/shell/serviceSeparation.test.tsx src/shell/m1b.test.tsx` → 7 files, 116/116 passed.
- `npx tsc --noEmit -p .` → exit 0.
- `npx eslint src/shell/dock/DockLayout.tsx src/shell/dock/defaultLayout.test.tsx` → exit 0.
- Full UI suite not run, because no new failure justified it.

## Files / sha256
| file | before | after |
|---|---|---|
| ui/src/shell/dock/DockLayout.tsx | 2d19c1c74d3cd1a209238a0b97086c0cd6e13f7c54d6eeb746b655f6a8cbaac1 | 2fd58931487feef7997cc979aa613a5b426caef3efa6ddfd806203ea529b9baf |
| ui/src/shell/dock/defaultLayout.test.tsx | 7ec574e0409e17d764f3e45935cf29c5e4ca5d4f31609f05be06d6111db7cadf | e58d7c68ff57927d6bfe51df95637286d436397fced84cf2ce3036bb2b8f24cb |
| ui/src/shell/dock/dockHost.ts | a574f2b8…e24e19 | unchanged |

## For finalQA (w2R)
Repeat the same flow on the rebuilt app: split dut | dut2 → `#/p/uart/terminal?channel=dut` → `#/s/uart`. Expect 2 panes, and the 키 입력 대상 / broadcast / recording state kept, because the instances are not remounted.
Note: during a deep link, dockview detaches background tabs from the DOM as usual; their instances stay alive.
Not addressed (separate owner): item 1, the hidden-tab recipient exclusion (w2T macro/terminal scope).
