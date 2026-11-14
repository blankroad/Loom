# Agent live UDS logs / WORKING

User asks to package UDS agent operation as a skill and verify real-time log visibility with an external server. Existing skill skills/loom-uds/SKILL.md will be extended (Opus implementation; ROOT source/native review).

## Before-fix native finding

External unmodified doip-server2026.6.14.post116, configured by qa/doip-interop YAML, localhost52570, ECU1001/tester0E80. Existing target/debug/loomd foreground with isolated /tmp/loom-uds-live-20261114/home; existing user daemons untouched. Observer and sender use distinct LOOM_CLIENT values.

ROOT starts `loom --json events --domain uds --channel live --since now --follow --timeout 4`, waits for its initial envelope, then sends explicit connect, VIN read and NRC request via loom CLI. Actual requests succeed at protocol level (NRC truthfully returned as error). Observer initialcount0, finalobserved_events0. Evidence before-fix.json.

Independent catch-up `loom --json events --domain uds --channel live --since seq:0 --max 100` returns7 events (state, tx/rx/result ×2) in catchup-before.json. This proves the data exists but live follow loses it.

Source cause under review: `follow()` retains moving cursor `now` on empty result, because only range.to updates cursor; gateway returns range:null for zero matches. Subsequent queries resolve now afresh. Gateway already provides stream_seq_hw and stream_epoch for a stable continuation boundary.

No browser/native backend bridge is added by this task. CLI observation and static mock visibility are different acceptance axes. Agent observations must come from native events, not browser DOM or local JS simulation.

## CLI/skill acceptance checkpoint

Opus A completed the narrow follow fix and skill update; 6 library tests pass in uds-only and default configurations. Mutation back to the old cursor logic fails the regression. Skill validator passes. Disposable `skills sync` copies the updated skill/reference byte-identically for Codex and Claude and repeats unchanged. ROOT default CLI clippy (-D warnings) passed. Only CLI was built (no gateway build); ROOT atomically updated repo target/debug/loom with the final default-feature binary, retaining all7domaincommands. PATH was not changed.

ROOT after-fix native (build1): first-empty observer receives all30tx/rx/result events from10actualexternalDoIPrequests,9VINpositive+1NRC, all10requestIDs complete, no duplicate(epoch,seq), max2perbatch, outputbeforeprocessend, firstevent205.49ms. Resume native: oneVIN whileobserveroff thennewNRCwhileobserving returns6events withmax1pages, bothcomplete, nooldreplay/noduplicates. Server-loss observer catchesactualuds.state/down/earlyeof andstalesession; status-after-loss.jsonagrees. Allbenchrequests useLoomCLI/nativeDoIP, nofakeDOMevents.

FinaldefaultCLI usesidenticalfollowruntimecode; onlycfg(test)/embeddedskillschangedfrombuild1. Skillssymlinkedportablyinproject .agents/skills and.claude/skills (loom +loom-udsonly), noexistingfilesoverwritten.

## Explicit dual-consumer gate — still WORKING

Usercorrectlyrequiresindependentskill-basedagentANDrealuserUIconcurrentreceive. NewnativeforegroundOpusB w1M:p1 uds-live-observer-opus session7850a39a-e250-48ae-bc56-34e7af55429c prepareobserveronly. Aworkerisw1K:p1 session340d2968-273c-483d-bf10-143d6a364144. ROOTsoleEgo21 uds-agent-ui-live p1 opensactualnativeUIatlocalhost52575 (differentfromstatic4180workbench).

ActualUIcurrentlyunauthenticated: ENV?/rights—/STALLED/ws_close1006, zerointerfaces. NoUIpassclaimed. Skillcommonhuman-onlybootstraprulepreventsROOTreadinglogincredential. Askeduseroneexplicitexceptionforwatch-onlyviewerontheownedisolateddaemon; pendingapproval, noauthbypass. IndependentobserverBEGINwillfollowUIready, andsameactualrequestIDs/PDUsmustappearinbothconsumersbeforeDONE. Staticmockstillhasnobackendbridge; verifyingoldnativeUIcannotcountascurrentmockintegration.

## UDS dual consumer native PASS

UserlatestinstructionexplicitlyrequiresdirectUIvalidationandmodulefixes; ROOTuseswatch-onlytestviewerontheownedisolateddaemon(nooperate/approve, noexistingusersession/policychange). Browserbootstrapneededreloadbecausehash-onlynavigationfromalready-mountedSPAdoesnotre-runstartup; reloadconsumedtheone-timefixturecode, strippedfragmentandconnectedwatch/LIVE1/1. Noauthcredentialispersistedinevidence.

Separatew1MOpusreadsloom-uds skillandstartsonefresh30sfollow, initialcount0/readyhw514/viewers1/1. ROOTsendsnewactualexternalDoIPVINw24 andNRCw25whilebothconsumersareactive. Agentgetsall6events, UIRowdata-request-idw24confirmed/w25failed, UIselecteddetails showexactresponsehex62F190... and7F2231. Automatedcrosscheckdual-ui-compare.json PASS withnativeeventepochseq531/532/533and536/537/538, sameIDs/exactbytes/actualviewer1/1atsend. NoDOMinjection/replay. DOMartifactsandnative screenshotsuds-ui-vin-detail.png/uds-ui-nrc.pngsaved. UDSroleaccepted; onlyremainingworkUART/ETHfixesandtheirUIcross-consumeracceptance.

## Final integration 2026-11-14

UDS native dual consumer acceptance: PASS. Actual external DoIP server VIN request w24 and NRC w25 matched six agent events and native UI request IDs/exact responses. Both saved UI images were visually inspected. CLI initial-empty follow regression, capped resume, server loss and reconnect evidence are preserved above.

Final source integration adds common isolated watch-fixture/UI acceptance docs, discoverable relative project skill links and ETH observer reference embedding. Final default CLI SHA256 is in final-skill-distribution.json (2b99f097…); intermediate d54e/22077 builds in earlier evidence are intentionally retained as historical identities. Final default-feature CLI lib tests: 6 passed. Common/UDS/ETH skill validation passed; both Codex/Claude distributions checked byte-for-byte against all six updated skill/reference files. Final gateway/UI fixes are tracked separately in work/reports/live-agent-ui/STATUS.md.
