# Phase R synthesis → Phase I authorized

Root accepted `research/sol.md` and `research/opus.md` on 2026-11-14. **Opus alone may now implement app/**. One Read DID mockup, not full UDS service suite or backend integration. Sol remains research/read-only reviewer. Existing REQUEST scope is unchanged.

## Research decisions

Both reports converge on explicit ECU/logical-link context separate from diagnostic service, a parameter/PDU pairing with explicit Read, and per-request raw response plus decoded value. Root inspected the real Softing diagnostic screenshot: service tree + logical link + SID/DID/PDU + Transmit + retained Process Values blocks. Sol's current Vector2.0 official document text and Opus's explicitly historical Vector-authored2012 console/trace screenshots supply workflow precedents, not a claim of current Vector styling. No blind clone of legacy Windows chrome. Evidence distinctions/provenance remain in research/ and qa/SOURCES.md; icons/architecture diagram are excluded as application-layout proof.

Adopt: target/route strip, short DID list with raw prefix, exact request preview, dense transaction history, response event sequence and raw→decoded values with source transaction. Reject for this slice: full service tree, arbitrary-service raw sender, ODX import, cyclic send, session/security/DTC/write/routine/flashing, real bus/packet-engine binding, prominent agent controls. Read is sufficient user action; no generic confirm/dry-run approval flow.

## One composition

- Selected Ethernet Opus Bench Light shell. Actual existing UART/Ethernet links and UDS current nav. These are available mockup modules, not a claim that UART/ETH traffic is correlated to this UDS mock. Underlying selected CAN bus/Ethernet path always visible.
- Top compact context strip: ECU (two fixtures), transport route, connection state, minimal route-address summary. Details disclosure may show CAN bus+Tx/Rx IDs, DoIP Ethernet+host+tester/ECU logical addresses, or EthDiag mock vendor-adapter label. CAN-TP and EthDiag are UDS-level local simulation routes, not native support; EthDiag framing unspecified and not standard DoIP. Label raw data **UDS PDU**, never invented TP/DoIP/EthDiag wire frames.
- Small collapsible DID quick list/search (VIN F190 + up to two explicitly fixture-defined values) next to a request strip: constant SID0x22, editable DID, normalized exact PDU, Read, context-specific Cancel. Selection itself does not send. Optional0x/spaces may normalize; ultimately exactly four hex digits. Invalid/empty/odd/outofrange input gives noTX.
- Main transaction table: number/time/ECU/route/DID/request/final response/status/pending count/elapsed. Keep widths useful; full raw data in selected detail if a table cell truncates. Keyboard selection and real pane resizing/collapse where implemented.
- Selected detail: chronological TX/every RX-pending/final/error/cancel events, UDS bytes, decoded SID/DID/payload/value/unit/fixture encoding. These all come from the selected transaction. Byte↔field highlight welcome if small; do not rebuild an Ethernet dissector. Confirmed values may be a compact target-scoped table (collapsible at narrow widths), each with source transaction/time/route; never a detached KPI card or a green new-result indication after a failed read.
- Compact SIM-options disclosure: positive, NRC, pending→positive, timeout, disconnect; choose deterministic latency long enough to interact/cancel (up to a few seconds). Request timeout/pending semantics visible but no general fault dashboard. Initial state should make Read workflow obvious and not send automatically. Empty history honestly labelled; no fake successful hardware trace.
- Responsive1440 priority,768/390 controls reachable, no whole-document overflow; wide tables/bytes scroll inside bounded panes. Check hidden attributes against explicit display rules. Traffic/result updates must preserve focused controls and divider geometry.

## State/identity rules

1. A transaction freezes ECU, route/address snapshot, DID, request bytes, fixture response scenario and timings at Read. A later context change cannot relabel it.
2. Limit one active request per target-route context. Switching the composer to another context can inspect/run it independently; Cancel/Disconnect acts on the indicated context. No implicit cancel or re-send solely from selection. Any simpler concurrency choice must remain explicit and meet request-snapshot and cancel/new-request isolation.
3. Pending `7F 22 78` is a nonterminal response in the timeline; final positive `62 DID data` or NRC terminal state. Timeout/cancel has no fabricated response bytes. Final status is not replaced by a late completion.
4. Only a valid positive response updates confirmed values. Key by ECU+DID with route/time/transaction provenance (or finer route key). An older transaction completing later must not overwrite a newer confirmed value for the same key. Cancelled/timedout/disconnected generations never update confirmed values; late events may remain visibly ignored on their original transaction. NRC/timeout retain previous confirmed data with explicit old provenance, not as success for the failed request.
5. History selection drives all detail panes. When filtering hides the selection, choose a visible row or clear detail; no silent out-of-scope result. Bound history without deleting in-flight identities or creating false decoded values.
6. Local config/fixture presets only under own prefix; reload no automatic sends/poll/retry. Actual JSON/CSV trace download if implemented. No global storage wipe. No real network sockets or native CAN/NIC operations.

## Incremental delivery and verification

Persist runnable shell then positive Read DID path first (target ~5min), add the small error/pending/cancel paths next. Root starts native render while later source checks finish; freeze assets before final QA. Keep implementation small; prefer engine/model + DOM adapter, relative existing jsdom dependency if used. No code generation that waits for a whole giant suite before writing any runnable app.

Source checks: exact22/62/7F bytes and decoding, invalid noTX, pending sequence, NRC/timeout/cancel, frozen target-route, old/cancelled late response isolation and monotonic confirmed-value updates, selection consistency/persistence. Root native checks in qa/STATUS.md. Reviewer Sol may inspect implementation read-only for these rules; no duplicate implementation or browser activity. Return app/HANDOFF.md, asset hash manifest, source-check results; research reports remain separately attributable. Root accepts, runs Ego, returns concrete defects, then normal exits/closes owned workspaces. No commit/push in this task.
