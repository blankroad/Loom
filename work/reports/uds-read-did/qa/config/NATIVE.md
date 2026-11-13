# Configuration native acceptance / PASS

ROOT sole Ego Lite owner, newTaskSpace18 p1 for configuration follow-up. Prior TaskSpace17 finished and untouched. Preview4180 existing, source base mock accepted in ../NATIVE.md, no native backend.

Acceptance on final configuration assets:
1. Preset retains defaults; custom edit/duplicate/name/select/delete/reset lifecycle scoped to ECU+route, cancel/invalid save no mutation.
2. DoIP host/port/testerLA/ecuLA/channel valid/invalid; CAN11/29bit ID ranges + channel; EthDiag label/endpoint honesty; P2/P2* actual timeout behavior.
3. Online save guard and reconnect: no silent abort/connect/send; selected profile switch no orphan active request; per-ECU/route state independent.
4. Inflight/history/confirmed provenance snapshot survives edits/profile switch; trace JSON contains original config and timing. Older/late result guard preserved.
5. Persistence/reload migration and no autosend. Atomic config import malformed rejected without mutation, roundtrip actual export if implemented.
6. 1440/768/390 native reachable config form/buttons, keyboardcancel/focus, no horizontal overflow/overlap; selected BenchLight shell unchanged.
7. Source check result/freeze hashes, accepted artifact screenshots and sessioncleanup.

No result inferred from source-local jsdom checks. Failures/rechecks appended below.

## Native interactions before source freeze
- Existing unversioned v1 prefs were observed before implementation (no config, history0, offline). After normal configuration action storedschema2 and old prefs preserved.
- First reload had old cached engine/app plus new HTML/configscript (read-only globals proved it). Per-page Network.setCacheDisabled used for fresh reload; no profile/origin cache or storage cleared.
- DoIP duplicate: invalidhost999.2.3.4/port0/equal tester+ECULA errors, customcount0. Validu1 QA BCM DoIP ETH2/192.0.2.10:14500/0E80→1201 savedoffline withzeroTX. Built-in preset remained ETH1/192.168.10.20:13400/0E00→1001.
- Connect/Read#1 positive. Online drafthostecu-test.local/port15000: Save disabled, activeprofile/connection/#1 all retainr1 oldhost. Explicitdisconnect/save/reconnect/Read#2 positive usesr2newhost; #1 retainsr1/oldhost.
- CAN11bit Tx800 invalid and no newprofile. 29bit Tx1FFFFFFF/Rx1FFFFFFE, CAN2/250k, P2=100/P2*=600 savedu2; positive400ms request#3 timesoutat~101ms. No inventedresponse. Changeoffline P2=500/P2*=600 r2 and Pending×1:#4 pending~401ms then timeout~1003ms. Actual timing is used.
- GW CAN context shows own unchangedpreset IDs7E0/7E8. Switching back BCM restores unsavedinvaliddraftRxFFFFFFFF with29bit-rangeerror; savedRx1FFFFFFE unchanged. Revert works.
- EthDiag customu3 ETH2/adapterQA 벤더 어댑터/endpointnote bench-ethdiag/ECU-A, defaulttiming, nativeRead#5 Pending→positive. Route explicitly undefinedvendorframing, notDoIP; note isopaque.
- Actual configdownload profiles.json contains3customprofiles; actual trace.json contains5transactions, full original config/profileID/name/revision/base/P2/P2*/connectiongen. #1oldhost/r1 remainswhile#2r2newhost. Parsed on disk.
- Invalid import file: validfirstentry+second29bitID20000000 rejects wholefile; count3/activeu3/linkonline/history5 unchanged. Real exportedfile import adds3 renamed(2) profiles atomically; count6 but activeu3/linkonline/history5 unchanged (no autoapply/connect/send).
- Freshreload preserves6profiles and selectedu3, all3BCMroutecontexts offline/history0. 1440/768/390 panel reachable/documentwidth fits (390document390, panel375 dueverticalscrollbar).

Lifecycle/mobile:
- At390px Escape closes and returnsfocusbtnCfg, dirtydotshown; reopen keepsunsavedhost asdesigned. Invalidmobileport0 Save focusesfirstinvalid#cf-port, width390fits. Selectingpreset and Save resetsactive to builtin/default withoutdeletingcustoms.
- C2 FAIL: after Escape/reopen then [취소]/reopen, #cf-host DOM stillunsaved-qa.local ratherthan savedecu-test.local. Returned toOpus: cachedbuildsignature may reusev1 afterdraftdeletion. Require nativeDOM retest afterfix. Earlierdraft/presetchecks remainvalid.
- QA-script selector failure: toggled alreadyopenSIMclosed then tried#simPend hidden; stoppedwithElementResolutionError, inspectedstate(simHidden=true/scenariopending/offline/u1), recoveredviaexplicitopen. This is recordedasQAactionassumption, notproductdefect.


## Final acceptance — PASS
- Final native C2 retest: Esc retainsdiscard-new.local; Cancel/reopen displayedhost=drafthost=savedhost ecu-test.local, dirtyfalse. No staleDOM. Fix usesunique draftgeneration and invalidatesclosedpanelbuildsignature.
- Native in-flight edit: originalrequestwaiting/active1; draftinflight-draft.local allowed, Save disabled, saved/transactionhost remain ecu-test.local:15000. Request finishespositivewithoutabort/newTX, revert/disconnect explicit.
- OwnQA cleanup: initialcustomcount0 was observed; all6testprofiles were removed via normal UI double-confirm delete, activefallbackpreset verified perroute. No storage/cookie/cache clear, no preexistinguserprofile removal. Settings persistedschema2 with0customs and originalpresets. FinalUI allowseditingoffline and retainspositiveReadhistory for viewing.
- Final DoIP1440/768/390 screenshot/width checks pass (documentwidth≤viewport); 390footer reached bynativekeyboardTab. EthDiag1440/768/390 inspectedearlier. CAN390 finalpanel checked separately. Source U2 mobile block/flex flow preserved.
- Actual exports profiles.json/trace.json retained, invalid-profiles.json intentionallyinvalidfixture. Earlierdoip-1440.png is incrementalunstyledform; final-doip-* and final-editable-1440.png are acceptedcurrentstyle.
- Opus source-local jsdom/fakeclock regression70/70 and config72/72 pass, includingengineguard/cap/timing/migration/corruptprefs/import/C2DOM. Root reviewedsource/handoff and nativeevidence separately; no independentSolcodereviewclaimed. All9entries source-sha256-freeze-config.txt verifiedOK. PriorP1source/hash/result is preservedinbaseline-p1, notcurrentfreeze.
- Realnetwork/hardware/addressrouting unimplemented: stilllocalmock. IPv6/otheraddressingmodes remain explicitlyunsupported. Deferredsettings rationale inCONFIG-HANDOFF.md.

- Final navigation QA attempt initially hit offscreen after viewport transition; stopped, inspected geometry/snapshot, used nativewheel and waited for visiblebounds on resize. Then CAN390 passed(sw390/width390, Tx740/11bit); switchedDoIP and explicitdisconnect leftofflineediting withhistory1/customcount0. No codefault inferred from transientQAviewportaction. Per-page cacheDisabled restoredfalse before completion.
