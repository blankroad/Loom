# Native Ego Lite acceptance — PASS

Owner ROOT, TaskSpace17 p1, agent-owned. Local server4180, engine/app implementation Opus. Native actions only; read-only runtime inspection for evidence. Viewport pinned each invocation. No fake-clock/browser mutation passes.

Initial build 2026-11-14:
- 1440×900 actual connect→Read #1 positive BCM/DoIP/F190: TX `22 F1 90`; RX `62 F1 90` + ASCII17 `LOOMSIM0000000001`; confirmed #1 same ECU/route/bytes.
- Invalid `22F190`: explicit four-digit DID error, history count remains1 (no TX). `0xF1 90` + native Enter normalizes exact same request; #2 positive. Native select RX timeline event shows matching SID0x62/DID0xF190/ASCII17/raw20bytes.
- Switch GW + current-ECU filter: zero rows, selected transaction null, detail empty, no BCM confirmed values leak.
- GW/CAN-TP #3/0101: TX `22 01 01`, RX `62 01 01 05 16`, 13.02V. Addresses CAN1/Tx0x7E0/Rx0x7E8; UDS simulation only, no ISO-TP frames claimed.
- GW/EthDiag #4 started, immediately switch BCM: #4 remains GW/EthDiag; current BCM retains own confirmed values. EthDiag disclosure explicitly vendor mock, ISO13400 DoIP not claimed, undefined framing.
- 1440/768/390 initial document scrollWidth≤innerWidth. Screens initial-{1440,768,390}.png; 390 vertically scrolls to detail, table internally scrolls.
- U1 FAIL: initial DID list empty, actual innerHTML empty. Returned to Opus; awaiting fix/retest.

Pending: simulator scenarios/races, real divider drag, exports/reload, final source/hash handoff, final screenshots.

Secondary controls and concurrency (native, before final CSS freeze):
- Reload initially: history empty, links offline. U1 fixed: DID list has all3 fixture entries.
- NRC #2 (`7F 22 31`) does not update confirmed; Pending #3 has exactly Tx,78,78,62 and finishes positive.
- Timeout #4 at~1001ms: finalResponse empty, no RX invented. Late positive at~2202ms logged ignored. Newer #5 confirmed retained.
- Cancel #6 after first78: status stays cancelled; subsequent78/final logged ignored. Positive #7 succeeds independently. SIM disconnect #8 ends link-lost; reconnect gen3 adds no transaction (count8).
- Start DoIP Pending×5 #9 then CAN-TP positive #10, same BCM+DID0101. #9 later positive but `applied=false`, note says newer#10 already confirmed. Confirmed #10 CAN-TP12.61V remains.
- Native divider drag changes history244px→198px /34%; history Home→ArrowDown selects#2 and matching detail. Pane collapse shows quick-DID selector and restore works. Select decoded DID highlights exact byte offsets1/2.
- Actual download files `native-export.json` and `.csv`: both10 transactions, CSV header+10rows; JSON keeps all events, ignored flags, absent timeout bytes, correct #10 raw `62 01 01 04 ED`. Parsed actual disk files, not in-memory export helper.
- Additional NRC #11 retains #10 12.61V and UI explicitly says latest#11 NRC, value below is older.
- Reload: history0/offline/Read disabled/DIDlist3. No automatic request. New positiveF190 exactly decoded in visible field panel.

U2 FAIL (native390): confirmed values overlap/hide response detail despite document width390. Original `pre-final-390.png` and first attempted fix `final-390-detail.png` retained as failures. After attempted 3-track fix, detail612px but center456px; values still begin inside detail. Returned to Opus again; no mobile acceptance yet. 1440/768 remain good. Need corrected source and fresh native390 inspection.


## Final acceptance — PASS

U2 second fix uses mobile block/flex flow. Native390 reload, connect/ReadF190, exact decodedVIN then real click raw byte/provenance:
- center y179 height1023.09 bottom1202.09; detail y590.09 height612 bottom1202.09; confirmed-values y1208.09. Detail fully contained, values begin6px after detail. All3 detail panes visible/reachable by normal scroll; final-390-detail.png visually inspected with no overlap.
- Final document scrollWidth=innerWidth at390,768,1440; screenshots final-{390,768,1440}.png. Final1440 image visually inspected. Failed-first-fix images retained under u2-first-fix-failed-*.
- 70/70 Opus source checks PASS (jsdom+fakeclock, distinct from native). All7 entries of app/selfcheck/source-sha256-freeze.txt verified OK after final fix. RuntimeJS unchanged for U2; native core scenarios above remain valid.
- Final runtime hashes: index42e0a4e5 / engine28244ac6 / app7b5c217f / toolsb86235c7; full finalCSS/check hashes in manifest. No source changes permitted after acceptance.
- Actual local mocked UDS interactions only; no hardware/native daemon integration claims. Clipboard permission not requested/tested. QA settings normalized through normal UI to BCM/DoIP/F190/positive with collapsedSIM/address details and reset divider; no profile/origin storage deletion needed. History is runtime-only and reload was verified not to replay.
