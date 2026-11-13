# UDS survey implementation / WORKING / 3 foreground Opus5.5 workers
User authorizes all11surveyitems inparallel, implementationOpus. ROOT orchestrates/reviews/nativeEgoonly, noUIcoding. Same localinteractiveSIMapp, preserveBenchLight densehumanworkbench + DID/Raw/config/contextsnapshots. Prior nativeRusttransportboundarydone, do NOTeditRust/rebuild/Cargo. No realCAN/EthDiagframes/hardwareclaims. Rootgitindexonly, noGit/pushrequested. Base main4fb848a plusaccepteduncommittedRustchanges+8unrelateddirtydocs; preserveall.

Ownership (sharedtree, disjointfiles; nointernalagents/backgroundjobs/modelsubstitution):
A uds-workflow-opus w1G:p1: app/workflow.js,workflow.css,selfcheck/workflow-check.cjs; team/WORKFLOW-HANDOFF.md. Items1savedcommands/recentrequestreuse,7watch+charts,8sequences.
B uds-trace-opus w1H:p1: app/trace-tools.js,trace-tools.css,layout.js,layout.css,selfcheck/trace-check.cjs,layout-check.cjs; team/TRACE-HANDOFF.md. Items2tracefilters/pause,3durablelogs/viewer,6layout.
C uds-core-opus w1J:p1: existingapp/engine.js,app.js,config.js,tools.js,index.html,styles.css,selfcheck/harness.cjs andexistingchecks; newhost.js,catalog.js/catalog.css,services.js/services.css,project.js/project.css,selfcheck/catalog-check.cjs,services-check.cjs,project-check.cjs,integration-check.cjs; team/CORE-HANDOFF.md. Items4DIDcatalogue,5ECUCRUD,9Session/TesterPresent,10projects,11DTC, plus sole sharedintegrationowner.
ROOT: TEAM/taskdocs/sharedREADME/RUN, qa/team/REVIEW.md/nativeartifacts; workersmaywriteownsourcechecklogs underqa/team/{workflow,trace,core}-*.log. Do NOTeditanotherownerfiles. NeedAPIchange? record/requestROOT, don'tguessconflictingchanges. Allworkerscanreadothersource/handoffs. No browser byworkers; ROOTsoleEgo. Existing4180server servesreports, no duplicate. Three workers modelargsclaude-opus-5-5, nativeforegrounddedicatedworkspaces. Useroverride ignoremissingHERDR_ENV persists.

Current11surveyitems are implemented in SAME app as functionalSIMfeatures, not newstaticmockvariants. Servicefeatures must use same engine requests/timing/route/context, not fake success UI. ExistingnativeRustSIM may be readforfixturesemantics but no newnativebackendintegrationinthisbatch. Allrunningautomations explicitstart; reload/importrestoresdefinitionsonly, noautoConnect/send/watch/TesterPresent/sequence/logresume. Preserveprofile/route/requestsnapshots andlate-responsegenerationguard. No universalconfirmation/dryrunforlocalmanualRead/Raw.

## Shared host contract — C implements FIRST, A/B buildagainstthis
window.LoomWorkbench (H), scripts after engine/app/tools/config/host:
- H.engine (existingE), H.constants (existingU, dynamicECUS/DIDS); H.clock={now(),set(fn,ms),clear(handle)} (sameclockasengine; fakeclocktestable).
- H.context() => {ecu,transport,profile} currentcontext snapshot.
- H.compose({mode:'did'|'raw',didText?,rawText?}) fillseditor, never sends.
- H.send({ecu?,transport?,mode:'did'|'raw',didText?,rawText?,source?}) => {ok,txn?,error?,code?}; explicitcontext, sameE.read/sendRaw, noautoconnect, busy/offlinerefused. txn.source identifiesmanual/workflow/watch/tester-present etc; doesnotretargetoldrequests. Successrefreshesview/selectsifappropriate, requestprofilefrozenbyengine. Cancellation must compareownedtxn.id tocontext.active before E.cancel; nevercancelanotherowner'smanualrequest.
- H.selectTxn(id) existingselection; H.refresh() core render.
- H.subscribe(fn) =>unsubscribe; event {type,txn?,context?} forenginechange+contextchange, neverduplicateTX; don'trebuildfocusedformson200msticks. H.onRender(fn)=>unsubscribe aftercoreDOMrender; renderownsubtreesonly/noinfiniterenderloops.
- H.setTraceView({rows(baseRows),byId(id)}|null): CvisibleTxns callsrows afterbaseECUfilter, selecteddetail resolvesbyIdfallbackE.byId. Bownsfilters/frozenrows; liveengine+recordingcontinuewhileDISPLAYpaused. Filteringmustreconcileemptyselection, no hiddenoldrowdetail. Offline logviewer Bseparate/read-only, neverengineinject/replayautomatically.
- H.project.register(key,{export():json,validate(json):{ok,value?,error?},import(value):void}): registerowneddefinitions/preferences, validationpure. CprojectcoordinatorvalidatesALLsectionsbeforeapplying, rollbackoncommit/storagefailure. Neverimportactivejobs/links/history. Extensionkeys workflow/trace/layout. Projectrestore canusemissingextensiondefault; unknownversionsrejected.
- Mountslots inindex ownedC: #workflowMount, #watchMount, #traceToolsMount, #layoutToolsMount, #catalogMount, #servicesMount, #projectMount. A/Bmaycreateownedsubtreeswithinmatchingmount. Cprovidescompact collapsibletabs/drawers onWorkbench, notmassivelystackallpanels. Basemainhistory/DID/rawremainusable. Scriptlinks workflow.js,trace-tools.js,layout.js,catalog.js,services.js,project.js andCSSfiles addedC. Load Ccorehost beforefeatures. Extensions shouldbootstraponce ifHavailable; noautowatchersforfiles/noawaitpollforcore. UseDOMContentLoadedoruds:host-ready listener onlyifneeded.

C publishes firsthost checkpoint inCORE-HANDOFF rapidly, then corefeatures. A/B can use ownstubhost+readcurrentapp inchecks beforeintegration; sourcechecks!=browserproof. C then runs integrationchecks onceallmodulesready. Useexistingui/node_modules/jsdom, noinstalls. Harnessmustclosewindows/timers. Sourcechecks meaningfulforownership/snapshot/invalidatomicimports/automationstop+reload. Preserveoriginalregressionsemantics; ifnewservices intentionallychangeoldNRCfixtures updateexpectationshonestlyanddocument.

Progress: persist incrementalrunnablefiles +shortplan, narrowworkingpathfirst, not giantallatonce output. Return READY_FOR_REVIEW/hashes/selectors/sourcecheckevidence; C integrates existingsharedfiles only, fixesownerreturnedtoA/B. ROOTnativeQA viaEgo afterintegration, allfeaturesfunctional evidence, 1440/768/390. Preservepreviousfailureevidence, no sourcechurnafterfreeze. No broad repeatedsuccesssuites. Finishnormalexit/closeeachownedworkspaceonlyafterROOTaccepts; preserveMAINw8.

## Contract additions (ROOT accepted, beforeintegration)
- H.editor()=>{mode,didText,rawText} readonlyeditorstate (workflowR1; avoidsO.stdependency).
- H.activity.register(key,{list():[{ecu?,transport?,kind}],stop():void}) foractiveworkflowwatch/sequence, TP andrecording; H.activity.list() aggregates. ECUdeleteguardsrejectmatchingactiveautomationevenwhenitslinkoffline; projectimportguardsrequireallactivitiesstopped +affectedlinksdisconnectedBEFOREanymodulemutation. Aregisterworkflowruntime, Bregisterrecording(globalitemkindrecording), CregisterTP. Noautostop/restartoninvalidimport. StubhostscanimplementtinyregistryuntilCpublishes.

## Trace contract refinement (ROOTreview)
- H.traceScope()=>{filter:'all'|'ecu',ecu} readonlybasehistoryscope, forBfrozenrows; cannotinferscopefrombaseRowswhenlivehistoryempty/evicted.
- C rowtime honorsoptionaltraceview.timeText(tx); Celapsedtick resolvesdisplaytxn viaexistingtxById soBfrozen tEndholdswhileliveinflightcontinues.

## Ownership transfer (C → A, recorded by C)
- `app/project.js`, `app/project.css`, `app/selfcheck/project-check.cjs` are now owned by **A · uds-workflow-opus** (C had not started them). Contract: `team/PROJECT-CONTRACT.md` (`H.coreProject` export/validate/guard/apply/commit/disconnectAll is live in host.js). C keeps engine/catalog/services/host + integration.
