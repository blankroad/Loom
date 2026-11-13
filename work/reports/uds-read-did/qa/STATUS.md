# UDS Read DID native QA / PLANNED

Root sole Ego Lite owner. Guide2.2.0 fully read. New goal space17 `uds-read-did-review`, ownershipagent, p1. Previous spaces15(UART user-controlled) and16(finished Ethernet comparison) untouched. Browser updateavailable notice retained, no upgrade or settings change. Preview4180 HTTP200. Research is not browser/product validation.

Acceptance after research synthesis and Opus source freeze:
1. 1440×900/768×900/390×900 readable, no document horizontal overflow (scrollWidth<=innerWidth, scrollbar allowed); resize drag real pointer if provided; controls reachable. Pinviewport eachinvocation because driver resets1309.
2. Select ECU and CAN-TP / DoIP / EthDiag mock routes. Show underlying bus/Ethernet and addressing separately from ECU; route is clearly SIM, no invented EthDiag wire header. Keep selection and in-flight/history target/route identity consistent.
3. Valid DID request PDU preview exact22+DID; explicit Read produces raw TX, positive62+DID+payload, decoded value matches bytes. Pending7F2278 retained in response timeline followed by final response; no false negative atpending. Invalid/empty/odd/out-ofrange DID noTX.
4. Negative response shows raw7F22NRC and named status; previous confirmed value retained with provenance and does not masquerade as successful new result. Timeout and cancel/disconnect end request; no unprompted retries.
5. Switch target/route while in flight; response stays with original transaction/ECU, no stale value overwrite. Cancel then new request, wait beyond oldresponse, assert late data cannot replace new values. No active automation or sends on reload.
6. History selection drives raw request/all responses/decode consistently; filter/empty selection never keeps silent unrelated detail. Keyboard selection, boundedhistory, viewpause vs requeststate if provided.
7. Local template/config persistence, real trace download verified content/count, no fake file status. No state clearing beyond ownnewprefix and only after clearing-state guidance.
8. Resolve real defects via sourceowner, rerun affected checks only; finalphase asset hashes/screenshots/results. Root accepts then finish space once keeping only UDS result p1; normalexit and close ownedworkspaces, MAINw8 retained. Product/nativeNIC/CAN/daemon integration not claimed.


Final: PASS. See NATIVE.md for actual native actions, U1/U2 failures and corrective rechecks, exports, final geometry/screenshots and frozen source verification.
