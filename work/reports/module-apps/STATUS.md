# Service applications / permanent top tabs

COMPLETE for module-app embedding and native service acceptance; commit/push in progress. User explicitly supersedes previous active-service-only navigation: all seven supported module tabs remain available, each module embeds its existing application with Connect/configuration before any interface is connected. Integrated keeps its current configurable observation workspace.

Baseline a8253c1 + substantial shared dirty Bench Light/cross-platform changes preserved (`preexisting-ui.diff`). No worker Git/index. ROOT orchestration/review/CUA only. Native foreground Claude Opus5.5 workers: A module-shell w31, B module-diag w32, C module-instruments w33. No subagents or background LLMs. CONTRACT.md describes unbound module panel semantics and ownership.

Progress: all production source frozen. B accepted (110 focused tests/lint; w32 exited/closed). C accepted (154 focused UI tests/type/lint + PSU native15/fmt; w33 exited/closed). A accepted `RETURN-A.md` (235 scoped tests/type/lint +274 adjacent tests), retained per-module docks with hidden rendering suspension, explicit unbound→bound handoff and async completion isolation; normal exit requested. QA `module-qa` w34 preflight returned, now received SOURCE_FREEZE: full UI gates once + matching release CLI + scratch strict app build, no GUI or fixture launch yet. QA additionally owns test-only `ui/e2e/workbench-ego/services.body.mjs` assertions for the deliberate permanent-tab rule; no production patch authority. ROOT reviews and owns actual CUA/Ego validation. No child agents/background LLMs.

Packaging plan: build new ordinary release into `/tmp/loom-module-current/Loom.app` scratch, preserving the currently running desktop/dist app until ROOT safely coordinates replacement. Native GUI and Ego16 control ROOT only. Broader prior final QA backlog remains distinct. No app or daemon has been stopped during implementation.

Integration gates: UI 66 files / 736 tests PASS, schema/type/lint PASS; release CLI built and scratch native dist strict bundle checks PASS. Source/UI digest now 8f57d3cd… (prior app remains old 643e5952…). A exited normally, w31 closed. QA w34 completes reports/harness and prepares own daemon/fixtures.

Native blocker discovered on actual CUA attempt: `cgWindowNotFound`; one read of screen_state confirms locked=true / displayAsleep=false. User async unlock question pending. No polling or bypass, no native-visible pass. ROOT continues independent owned-daemon / watch-viewer browserDOM validation using Ego16; QA may start fixture servers but must await ROOT viewer readiness before device RPC writes. Existing user app52634 / daemon52646 untouched; read-only status confirms latch=false, all existing worker writes0, others_since_last0. Native actual Connect flows and final user-app replacement await unlock.

Return sequence: focused module tests/self-review → RETURN-A/B/C.md READY_FOR_REVIEW → ROOT review/integration schema/typecheck/lint/full UI suite once source frozen → fresh shared daemon/app build → actual visible Mac UI verification (all seven tabs, module controls before connect, zero unintended channels, integrated semantics), protocol smoke with available simulators only as needed for changed connect paths. Do not claim physical hardware coverage. Existing macro/split/export final acceptance backlog is distinct and remains pending unless actually rerun on matching build.

Live user app52634/daemon52646 default ~/.loom port59563 is the preceding delivered build. Workers must not operate/stop it. ROOT coordinates replacing user-requested app after build; no other daemons/browser spaces/settings changed. Native/Ego ownership remains ROOT. Reuse Ego16 if browser parity is needed; no unrelated spaces.

Model usage telemetry not provided; no explicit token/time budget requested. No remote push authorized for this slice. ROOT sole index owner; mixed prior uncommitted work remains preserved.

## Actual browser / protocol checkpoint

Ego16 p2 WATCH viewer on owned daemon76131 port60491, UI8f57d3cd. All seven full module screens verified at1440/900px, no page overflow, STOP visible; initially all counts0 and worker writes0. ROOT inspected the actual screenshots. Native remains blocked by the pending user unlock request; user app52634/daemon52646 untouched.

UART actual PTY co-receive PASS: separate CLI observer armed before one fresh marker, exact fixture RX20 bytes, agent event123 TX/event126 RX, browser local terminal search shows matching ACK1/1 (qa/browser/uart-ack-confirmed.png). UDS externalDoIP protocol PASS: RA then exactly one R0F190, observer408/409/410, actual UI VIN OPUSW26DOIP000001. However bound panel loses native connection snapshot (link unreported/active overview empty). This is a real UI defect, not a protocol failure. New foreground Opus5.5 module-uds-state w35 is sole UDS source owner, TASK-UDS-STATE.md; all other production areas remain frozen. QA module-qa w34 continues ETH/CAN/PSU/logic/video owned fixtures while ROOT handles UI. ETH qa-eth synthetic capture live; ROOT signalled ETH_UI_READY for one loopback UDP marker. Old8f57 proofs and forthcoming fixed-build proofs must be separated.


## Follow-up corrections (current)

UDS focused hydration accepted after ROOT review: RETURN-UDS-STATE.md, 61 focused tests/type/lint passed. Bound-only one-shot authoritative uds.status, native-shaped fixtures, stale live/down guard. Production UDS now frozen; worker exited normally (agent_not_found), own w35 closed. Actual rebuilt regression still required.

CAN protocol w8 exact BCM reply301 after5ms PASS but active UI summary0 is a real failure, not a PASS. PSU externalSCPI poll/meas protocol PASS with outputOFF; after full zero-channel tabtour UI0points/dashes, own browserreload restores301points/0.000V/A/W. New foregroundOpus module-can-live w36 diagnosed SubscriptionMux cap8 exhausted by retainedhidden topics, parked handles never re-placed. Interim RETURN-CAN-LIVE.md proposal reviewed; ROOT authorized bounded active leases/eviction/resume, API/client/core/protocol/FakeNet/SDKcontext plumbing, and shellservices/tests to fit all7basicvisiblemodules with shell in8slots. No backend cap increase. CAN parser fine; no CAN repeatedsend yet. User drafts/splits/history must stay mounted; hidden catch-up/resume verified in tests and actual rerun after rebuild.

Logic syntheticpins through real sigrokUART decoder actual co-receive PASS: QALOG-4AA4DF, capturelc-1a1de36d84d-1, rawtransitions and decodedhex seen whileactive, screenshot logic-waveform.png inspected. Hardware not exercised. Video externalMJPEG pending. QA module-qa w34 remains fixture/build owner and ROOT allGUI. New source freeze/build required after mux fix; old8f57 proofs kept distinct.


## a52 build / independent review checkpoint

ImplementationRETURN-CAN-LIVE accepted forintegration and sourcefrozen; 752fullUI tests/type/scopedlint plusQAfullESLint PASS, strictMacbuild a52f57b6 UI / app53012ccf / bundleddaemon e23a1c61 PASS. Ownw36 workerexitednormally+closed. lqa1 daemon76131 normallyclosed/stopped beforePTYreuse; fixturelogprefixsnapshots preserved. NewQA lqa2 daemon3497 port61881, extraPTY3505 ttyss008, alloldexternalfixturesreused. ROOTWATCH Ego16p2 attachesnormalwatchbootstrap; no operate elevation.

Newzerochannel7tour+Integrated PASS, countsall0/noautoloadwrites/nooverflow andactualsettledIntegrated0panels; qa/fix/browser. Real2UARTsidebyside, ownACK1/1 each/opposite0건 no cross-talk PASS. CANnow10fps andexactfresh301reply F1 44 9B 4C 2D 74 8C B0 withgap없음 PASSaftertour. PSU0.000V/A/W plus301points withoutreload PASS; explicitmeas/UDShandoffnext. NativeGUIstillMaclocked/userunlockpending, defaultuserapp52634/daemon52646 untouched (freshreadonlystatusallwrites0/hw14).

IndependentfreshOpus module-fix-review w37 returned REVIEW-FIX.md CHANGES_REQUESTED: F1medium mergedforeigngapcontaminatesotherchannels/REC; F2low premountCANagent-readmarks lost; F3low reconnectcanleaveinflight-suspendedhandleparked; F4low hiddenlazyinitialleasecanmergevisiblepanels unnecessarily. ROOTacceptedall4forfocusedfix, no userreleasePASS. NewfreshOpusmodule-review-corrections w38 solefollowupsourceownerTASK-REVIEW-CORRECTIONS.md, reviewerreadonlyDeltaafterreturn. a52runtimeevidenceimmutable/historical; newmatchingfinalbundle/daemon stillrequired. QAcontinuesboundedPSU/UDSchecks whilefixparallel; finalR6Integrated9demand+hidden-catchuptargetfinalbuild. No Git/indexchanges or remote push.


## Final source accepted / actual integrated run

AllF1-F4 fixed RETURN-REVIEW-CORRECTIONS.md. FullUI69files/761tests/type/scopedlint PASS, QAmissingfullESLint PASS. IndependentDeltaREVIEW-FIX209 PASS, 53focusedtests, allnegativecontrolsverified. Sourcefinalfreeze tree1303905e…; UI db441bfb…; strictMac app302e5c5b…; bundledloomd38219c57…; main31cf2b41…. Build/report hashes qa/final/sum-build-final.json; no codechangesafterfreeze. Source/reviewworkersrolesreleased; normalexit/ownworkspacecloseinprogress.

QA normallyclosed/stopped lqa2 daemon3497 beforePTYreuse, preservedprefixlogs. Final lqa3 daemon18389 port62519, sameexternalfixturesand2PTYs, no thirdPTY. ROOTnormalWATCHviewerEgo16p2 attachedonIntegratedemptycount0. ROOTauthorized7fixturechannels(2UART,CAN,ETH,UDS,PSUOFF,video) open/captureonly, noUARTTX/CANTX/DID/logicstartuntilUIready. FinalR6actual8visibleviews+shell=9demands, plushiddenUARTcatchup willbedone ondb441bfb, notpromotedfroma52. Prior a52 actualUDSbeforeDIDlink/P2/route andVIN PASS qa/fix/browser; allprotocoltestsoverwrite/casecollisionnotes qa/CASE-COLLISION-NOTE.json honestsummarypreserved(no syntheticrawreconstruction). NativeMaclock/unlockblocker stillpending. Defaultappold52634/daemon52646 untouched sofar.


## Final return / publication

BROWSER_PROTOCOL_PASS_NATIVE_VISIBLE_PENDING. Finaldb441bfb actualR6 PASS:8visiblegroups,2UART+7domains+shell,UIhubsubs8;20s684eventscontiguous6440-7123/notruncated; freshUART/CAN/UDS dataUIequal. HiddenallUARTbehindUDS→freshUART2ACK→same8groups/sameUARTinstanceids/no reload, priorsearch/CANfilterpreserved. ScreenshotsinspectedROOT. IndependentOpusDeltaPASS,761UItests/type/fulleslint/strictbundle PASS. No physicalhardware or losslessUIREC/nativevisible performance claim.

QA closed allown channels/daemons/fixtures/observers, archivedlogs/manifests, removedlqa1/2/3homes/fx. Rolesreturned/exitedagent_not_found; allownedHerdrworkspacesclosed includingQA34, MAINw8untouched. ROOT atomicpublisheddesktop/dist/Loom.app afteroldapp/daemonno-devicecheck andnormalexit (52634/52646ended). Newapp23400/daemon23423port62968running normaldefault home, bundled38219c57/finalUI db441bfb, actualservedUI6/6equal. Oldbundlebackup /tmp/loom-module-prior-delivery/Loom.app; Linuxartifactspreserved. NativevisibleConnect checkstillneedsMacunlock, RESUME-NATIVE.md. No sourcechangesafterfreeze; mixedpriorworktree/indexpreserved, no commit/pushthisslice. Browserresultreportp1willbekeptforuser, QAviewer p2closedonfinish.

Finalbrowserfinish completed: Ego16user-owned, reportformerP1kept at http://127.0.0.1:64785/module-apps/REPORT.html, ownedQAviewerP2closed. Report26images allassets exist/nooverflow, actualrenderinspectedROOT. Reportserver64785keptforuserresult. HerdrinventoryonlyprotectedMAINw8. Currentnativeapp23400/daemon23423aretheleft-runningdeliverable, NOTQAprocessestocleanup.


## Native acceptance resumed / push authorized

User unlocked Mac and explicitly authorized commit+push after completion. Foreground Claude Opus5.5 module-native-qa w39 owns fixtures, CLI comparisons and native verdict; ROOT sole CUA executor/index owner. Installed app23400 / daemon23423 port62968 on final UI db441bfb remains live. No source changes since final freeze. Native UART1 Connect and UART2 save-only/no-autoconnect + right split Connect exercised; fresh CLI ACKs found 1/1 in each actual native terminal. Native CAN Connect via can_sim/default BCM exercised; live0x310 visible. Remaining native modules in progress, not promoted from browser evidence. Curated commit scope reviewed in PUSH-SCOPE.md; origin fetched, HEAD ahead1/behind0. No commit/push yet.


## Native acceptance complete (current)

NATIVE-RETURN.md accepted: foreground Opus5.5 lead / ROOT actual CUA, same installed db441bfb app23400 and daemon23423. All7 module explicit Connect/Start paths + UART2 save-only/rightsplit PASS; fresh CLI evidence matched actual native UI. UDS fixture-only19→17B correction, no production change. Native Integrated3visible UART/CAN/UDS fresh markers + independent2pane UART and3pane Integrated persistence PASS. Video frame1011/hash exact, documented manual reconnect after5s source stall. Prior broad macro/REC final2 native backlog remains separate. QA channels/fixtures/observers stopped, own savedUARTsessions/DoIPprofile andviews removed throughUI; all7tabs0active,8inactive histories. UDSdown target has no disconnectRPC; user app/daemon preserved. PUSH-SCOPE sourcegroupsL1–L5 hashes unchanged,761 tests not unnecessarily repeated. User explicitly authorized commit/push after acceptance.

Native QA w39 and scope reviewer w3A normalexit verified agent_not_found; own workspaces closed. HTML report rendered in Ego16 with10native screenshots, no overflow, all linked assets present. Source code and existing gates unchanged. ROOT publishes curated scope via C1–C6; final remote SHA verified directly through Git.

Publication-only test formatting: Opus w3B removed one extra EOF newline from portable/lib.test.mjs, focused9/9PASS; runtime source/bundle unchanged. Normalexit verified/w3Bclosed. Raw evidence retains byte-exact CR/whitespace; curated documentation passes separate whitespace check.
