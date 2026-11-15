# ROOT integration acceptance

Seven permanent top-level tabs with zero channels: UART, CAN, Ethernet, UDS, PSU, logic, video, plus Integrated. This is the user's latest rule, replacing service-tab visibility based on active interfaces; actual channel/panel projections remain truthful.

| Module | Visible before connection | Connect behavior |
| --- | --- | --- |
| UART | saved sessions, macros, terminal workspace, split controls | select detected port or explicit manual/simulator; multi-session preserved |
| Ethernet | packet list, filter, tree/hex, source configuration | explicit alias/NIC/capture settings, no implicit lan1 |
| UDS | editable ECU presets, DoIP fields, DID/raw forms, history | explicit configured target, no invented ECU state/traffic |
| CAN | trace / RxTx / filtering / configured channel | supported engine/simulator selected explicitly, no implicit bench alias |
| PSU | gauges unknown until confirmed, settings/connect | configured alias only; no auto output-on or set on navigation |
| Logic | waveform/decode tool and capture config | explicit source/start; unavailable engine reported |
| Video | frame surface, source/capture/REC tools | explicit source/open; no auto camera access; platform limitation truthful |

Root checks after sources freeze: schema/type/lint/full UI suite once, fresh daemon + normal app packaging, exact UI identity; native UI all tabs at 1440/min-width and per-module Connect/configuration flows. Test switching modules preserves data/drafts/layout as implemented; no blank shell / generic launcher replacing developed panels. Integrated remains actual user-selected observations, not seven full module apps automatically stuffed into it. Navigation-only produces no actual channels or traffic.

Native browser parity uses Ego skill and existing task space16 (if still available); preserve unrelated user spaces. Native OS actions use CUA. Any simulator data test uses Loom skill-based observer on the same daemon, no direct device opens; external fixtures represent only the simulator side. Read/write rights, visibility and exact live records must be honest; no fabricated evidence or human login-code access. Scope is changed module Connect paths, not an automatic rerun of all prior acceptance backlog.

User-facing current app52634/daemon52646 initially left untouched during implementation. ROOT checks actual current channel state before any necessary replacement. Worker/test-owned resources only are cleaned; the final user-facing app remains open. No global settings, real-device connections or unrelated user sessions changed.
