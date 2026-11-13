/* Loom UDS workbench — project file coordinator (item 10; owner A, transferred from C). LOCAL SIM definitions only.
   One versioned file: core (catalog + connection profiles + services prefs) + registered extension sections (workflow/trace/layout).
   Import = validate EVERYTHING first → guard (no running activity, no online/busy context; never auto-stops) → snapshot →
   apply all → commit; any failure restores the snapshot. Restores definitions only: never connects/sends/starts anything,
   keeps history, confirmed values and live contexts. Export never includes history, captured logs, links or running jobs. */
(function (root) {
  'use strict';
  const FORMAT = 'loom-uds-project/1', VERSION = 1, MAX_BYTES = 1024 * 1024;
  const EXT_KEYS = ['workflow', 'trace', 'layout'];
  const isObj = x => !!x && typeof x === 'object' && !Array.isArray(x);
  const clone = x => (x === undefined ? undefined : JSON.parse(JSON.stringify(x)));
  function byteLen(s) {
    try { if (root.TextEncoder) return new root.TextEncoder().encode(s).length; } catch (e) { /* fallback */ }
    return unescape(encodeURIComponent(s)).length;
  }

  function create(H) {
    const CP = H.coreProject;
    const sections = () => (H.project && H.project.sections ? H.project.sections() : new Map());
    // Default for an extension section absent from the file (validated like a file value).
    function defaultFor(key, impl) {
      // Generic: every module supplies its own default via optional defaults(). Without it the section is kept (shown in preview).
      if (typeof impl.defaults === 'function') return { how: 'default', value: impl.defaults() };
      return { how: 'keep' };
    }

    /* ---------- pure: text → json (size before parse) ---------- */
    function parse(text) {
      if (typeof text !== 'string') return { ok: false, error: '텍스트 아님' };
      const n = byteLen(text);
      if (n > MAX_BYTES) return { ok: false, error: `파일 ${n} B — 최대 ${MAX_BYTES} B (해석하지 않음)` };
      let j; try { j = JSON.parse(text); } catch (e) { return { ok: false, error: 'JSON 구문 오류' }; }
      return { ok: true, json: j, bytes: n };
    }
    /* ---------- pure: json → validated plan (no mutation anywhere) ---------- */
    function plan(json) {
      if (!isObj(json)) return { ok: false, error: '프로젝트 객체 아님' };
      if (json.format !== FORMAT) return { ok: false, error: `형식 ${JSON.stringify(String(json.format)).slice(0, 48)} — ${FORMAT}만 지원` };
      if (json.version !== VERSION) return { ok: false, error: `버전 ${JSON.stringify(json.version)} — ${VERSION}만 지원` };
      const s = json.sections;
      if (!isObj(s)) return { ok: false, error: 'sections 객체 없음' };
      if (!isObj(s.core)) return { ok: false, section: 'core', error: 'core 섹션 없음' };
      const regs = sections();
      for (const k of Object.keys(s)) {
        if (k === 'core') continue;
        // allowed = exactly the extension sections registered on this page (export ↔ import symmetric); anything else is rejected
        if (!regs.has(k)) return { ok: false, section: k, error: EXT_KEYS.includes(k) ? `섹션 "${k}" 모듈이 이 페이지에 없음 — 가져오면 손실되므로 거부` : `알 수 없는 섹션 "${String(k).slice(0, 24)}"` };
      }
      const core = CP.validate(s.core);
      if (!core.ok) return { ok: false, section: 'core' + (core.section ? '.' + core.section : ''), error: core.error };
      const ext = {};
      for (const [k, impl] of regs) {
        let src, raw;
        if (Object.prototype.hasOwnProperty.call(s, k) && s[k] != null) { src = 'file'; raw = s[k]; }
        else { const dft = defaultFor(k, impl); src = dft.how; raw = dft.value; }
        if (src === 'keep') { ext[k] = { how: 'keep' }; continue; }
        let v; try { v = impl.validate(clone(raw)); } catch (e) { v = { ok: false, error: '검증 예외: ' + (e && e.message) }; }
        if (!v || !v.ok) return { ok: false, section: k, error: (v && v.error) || '검증 실패' };
        ext[k] = { how: src, value: v.value };
      }
      return { ok: true, plan: { core: core.value, ext, exportedAt: typeof json.exportedAt === 'string' ? json.exportedAt.slice(0, 40) : null } };
    }
    const guard = () => CP.guard();

    /* ---------- apply with snapshot rollback ---------- */
    function snapshot() {
      const coreRaw = CP.export(), cv = CP.validate(clone(coreRaw));
      if (!cv.ok) return { ok: false, error: '현재 core 정의를 백업 검증할 수 없음 — 가져오기 중단: ' + cv.error };
      const ext = {};
      for (const [k, impl] of sections()) {
        let v; try { v = impl.validate(clone(impl.export())); } catch (e) { v = { ok: false, error: e && e.message }; }
        if (!v || !v.ok) return { ok: false, error: `현재 "${k}" 정의를 백업 검증할 수 없음 — 가져오기 중단: ${v && v.error}` };
        ext[k] = v.value;
      }
      return { ok: true, core: cv.value, ext };
    }
    function restore(snap) {
      const errs = [];
      try { const r = CP.apply(snap.core); if (!r || !r.ok) errs.push('core: ' + (r && r.error)); } catch (e) { errs.push('core: ' + e.message); }
      for (const [k, impl] of sections()) if (k in snap.ext) { try { impl.import(clone(snap.ext[k])); } catch (e) { errs.push(k + ': ' + e.message); } }
      let committed = false; try { committed = CP.commit() === true; } catch (e) { /* reported below */ }
      if (!committed) errs.push('저장소 기록 실패 (메모리 상태는 복원됨)');
      return errs;
    }
    function apply(p) {
      if (!p || !p.core || !p.ext) return { ok: false, code: 'plan', error: '검증된 계획 아님' };
      const g = guard();
      if (g.length) return { ok: false, code: 'guard', guard: g, error: '실행 중 작업/연결이 있어 거부 — 먼저 명시적으로 중지/연결 해제' };
      const snap = snapshot();
      if (!snap.ok) return { ok: false, code: 'snapshot', error: snap.error };
      // Rollback restores every section's memory + persisted state from the snapshot (best effort).
      // A failed rollback step is reported DISTINCTLY (code 'rollback-incomplete'), never as a clean rollback.
      const fail = (step, msg) => {
        const rest = restore(snap);
        if (!rest.length) return { ok: false, code: 'rollback', step, error: `${step} 실패: ${msg} — 이전 정의로 복원됨 (메모리·저장소)`, restoreErrors: [] };
        return { ok: false, code: 'rollback-incomplete', step, error: `${step} 실패: ${msg}`, rollbackError: `복원 불완전: ${rest.join('; ')} — 해당 섹션의 저장된 값이 현재 화면과 다를 수 있음`, restoreErrors: rest };
      };
      try { const r = CP.apply(p.core); if (!r || !r.ok) return fail('core', (r && r.error) || '거부'); } catch (e) { return fail('core', e.message); }
      const regs = sections();
      for (const k of Object.keys(p.ext)) {
        const x = p.ext[k], impl = regs.get(k);
        if (x.how === 'keep') continue;
        if (!impl) return fail(k, '모듈 등록 해제됨');
        try { impl.import(clone(x.value)); } catch (e) { return fail(k, e.message); }
      }
      let committed = false; try { committed = CP.commit() === true; } catch (e) { committed = false; }
      if (!committed) return fail('저장', '저장소 용량 초과 또는 사용 불가');
      if (H.refresh) H.refresh();
      return { ok: true, applied: Object.keys(p.ext).filter(k => p.ext[k].how !== 'keep'), kept: Object.keys(p.ext).filter(k => p.ext[k].how === 'keep') };
    }

    /* ---------- export (definitions / prefs only) ---------- */
    function build() {
      const out = { format: FORMAT, version: VERSION, exportedAt: new Date(H.clock.now()).toISOString(), app: 'loom-uds-read-did (local SIM)', sections: { core: clone(CP.export()) } };
      for (const [k, impl] of sections()) out.sections[k] = clone(impl.export());
      return out;
    }
    function exportProject() {
      const json = build(), text = JSON.stringify(json, null, 2), n = byteLen(text);
      if (n > MAX_BYTES) return { ok: false, error: `프로젝트 ${n} B — 상한 ${MAX_BYTES} B 초과 (내보내지 않음)` };
      const name = `loom-uds-project-${H.ui && H.ui.stamp ? H.ui.stamp() : 'export'}.json`;
      H.ui.download(name, text, 'application/json');
      return { ok: true, name, bytes: n, json };
    }
    return { parse, plan, apply, guard, build, exportProject, snapshot, FORMAT, VERSION, MAX_BYTES };
  }

  /* =================== UI (#projectMount subtree only) =================== */
  function mountUI(P, H) {
    const d = root.document, mount = d.getElementById('projectMount');
    if (!mount) return null;
    const esc = H.ui.esc, trLabel = id => (H.constants.TRANSPORTS.find(t => t.id === id) || {}).label || id;
    let pending = null;    // { name, bytes, plan, exportedAt }
    let lastSig = '';
    mount.innerHTML = `<div class="pj" id="pj">` +
      `<section class="pj-sec" aria-labelledby="pjExpH"><div class="pj-h" id="pjExpH">내보내기</div>` +
      `<p class="pj-hint">ECU·DID 카탈로그(SIM fixture 포함), 연결 프로필, 서비스 설정, 저장 명령·감시·시퀀스 정의, 기록 보기·레이아웃 설정을 한 파일(<code>${FORMAT}</code>)로 저장합니다. 기록·로그 본문·연결·실행 중 작업은 포함하지 않습니다.</p>` +
      `<div class="pj-sum" id="pjSum"></div>` +
      `<div class="pj-row"><button type="button" class="btn primary" data-pj="export">프로젝트 내보내기</button></div></section>` +
      `<section class="pj-sec" aria-labelledby="pjImpH"><div class="pj-h" id="pjImpH">가져오기 (교체)</div>` +
      `<p class="pj-hint">파일 전체를 먼저 검증하고 미리보기를 보여줍니다. [적용]해야 현재 정의를 교체합니다. 실행 중 작업이나 연결이 있으면 거부합니다(자동 중지 없음). 적용해도 연결·전송·감시·시퀀스·TesterPresent·기록은 시작되지 않고, 트랜잭션 기록과 확정 값은 유지됩니다. 최대 ${(MAX_BYTES / 1024) | 0} KB.</p>` +
      `<div class="pj-row"><button type="button" class="btn" data-pj="choose">파일 선택…</button><input type="file" id="pjFile" accept=".json,application/json" hidden></div>` +
      `<div class="pj-prev" id="pjPrev" aria-live="polite"></div></section>` +
      `<div class="pj-msg" id="pjMsg" role="status" aria-live="polite"></div></div>`;
    const $ = id => d.getElementById(id);
    function msg(t, kind) { const m = $('pjMsg'); m.className = 'pj-msg' + (kind ? ' ' + kind : ''); m.textContent = t || ''; }
    function paint(el, html) {
      const a = d.activeElement, k = a && el.contains(a) ? a.getAttribute('data-k') : null;
      el.innerHTML = html;
      if (k) { const n = el.querySelector(`[data-k="${k}"]`); if (n) n.focus({ preventScroll: true }); }
    }
    function counts(core, ext) {
      const c = core || {}, cat = c.catalog || {}, pr = c.profiles || {};
      const fx = Object.values(cat.fixtures || {}).reduce((n, m) => n + Object.keys(m || {}).length, 0);
      const rows = [
        ['core', 'ECU·DID 카탈로그', `사용자 ECU ${(cat.ecus || []).length} · 사용자 DID ${(cat.dids || []).length} · SIM fixture ${fx}`],
        ['core', '연결 프로필', `사용자 프로필 ${(pr.profiles || []).length} · 적용 ${Object.keys(pr.active || {}).length}`],
        ['core', '서비스 설정', c.services ? 'TP·DTC 설정' : '기본값'],
      ];
      Object.keys(ext).forEach(k => {
        const x = ext[k], v = x.value;
        const label = { workflow: '명령·감시·시퀀스', trace: '기록 보기 설정', layout: '레이아웃' }[k] || k;
        const det = x.how === 'keep' ? '' : k === 'workflow' && v ? `명령 ${v.commands.length} · 감시 ${v.watches.length} · 시퀀스 ${v.sequences.length}` : k === 'layout' && v && v.named ? `이름 있는 레이아웃 ${v.named.length}` : '설정';
        rows.push([k, label, det, x.how]);
      });
      return rows;
    }
    const HOW = { file: '파일 값', default: '파일에 없음 → 기본값', keep: '파일에 없음 → 현재 유지 (기본값 API 없음)' };
    function renderSummary() {
      const ext = {}; for (const [k, impl] of H.project.sections()) { try { ext[k] = { how: 'file', value: impl.export() }; } catch (e) { ext[k] = { how: 'file', value: null }; } }
      let core = null; try { core = H.coreProject.export(); } catch (e) { /* shown as zero */ }
      const html = `<table class="pj-tbl"><tbody>${counts(core, ext).map(r => `<tr><th>${esc(r[1])}</th><td>${esc(r[2])}</td></tr>`).join('')}</tbody></table>`;
      if ($('pjSum').innerHTML !== html) paint($('pjSum'), html);
    }
    function guardHtml() {
      const g = P.guard();
      if (!g.length) return { sig: '', html: '', n: 0 };
      const keys = [...new Set(g.filter(x => x.kind === 'activity').map(x => x.key))];
      const online = g.some(x => x.kind === 'online'), busy = g.some(x => x.kind === 'busy');
      const html = `<div class="pj-guard" role="alert"><b>적용 불가 — 먼저 명시적으로 정리하세요 (자동 중지 없음)</b><ul>${g.map(x => `<li><span class="pj-k k-${x.kind}">${{ activity: '실행 중', online: '연결됨', busy: '요청 진행' }[x.kind] || x.kind}</span> ${esc(x.label || '')}${x.ecu ? ` <span class="pj-t">${esc(x.ecu)}${x.transport ? '·' + esc(trLabel(x.transport)) : ''}</span>` : ''}</li>`).join('')}</ul>` +
        `<div class="pj-row">${keys.map(k => `<button type="button" class="btn sm danger" data-pj="stop" data-key="${esc(k)}" data-k="stop-${esc(k)}">${esc(k)} 중지</button>`).join('')}` +
        (online ? `<button type="button" class="btn sm" data-pj="disconnect" data-k="disc">모두 연결 해제${busy ? ' (진행 중 요청은 링크 끊김 처리)' : ''}</button>` : '') + `</div></div>`;
      return { sig: JSON.stringify(g), html, n: g.length };
    }
    function renderPreview() {
      const el = $('pjPrev');
      if (!pending) { if (el.innerHTML) el.innerHTML = ''; lastSig = ''; return; }
      const gh = guardHtml();
      const rows = counts(pending.plan.core, pending.plan.ext);
      const html = `<div class="pj-file"><b>${esc(pending.name)}</b> <span class="pj-t">${pending.bytes} B${pending.plan.exportedAt ? ' · 내보낸 시각 ' + esc(pending.plan.exportedAt) : ''}</span> <span class="pj-ok">검증 통과</span></div>` +
        `<table class="pj-tbl"><thead><tr><th>섹션</th><th>가져올 내용</th><th>출처</th></tr></thead><tbody>${rows.map(r => `<tr class="${r[3] === 'keep' ? 'keep' : ''}"><th>${esc(r[1])}</th><td>${esc(r[2])}</td><td>${esc(r[3] ? HOW[r[3]] : '파일 값')}</td></tr>`).join('')}</tbody></table>` +
        gh.html +
        `<div class="pj-row"><button type="button" class="btn primary" data-pj="apply" data-k="apply" ${gh.n ? 'disabled' : ''}>적용 (현재 정의 교체)</button><button type="button" class="btn" data-pj="cancel" data-k="cancel">취소</button></div>`;
      const sig = html;
      if (sig !== lastSig) { lastSig = sig; paint(el, html); }
    }
    function loadText(text, name) {
      const pr = P.parse(text);
      if (!pr.ok) { pending = null; renderPreview(); msg(`가져오기 거부: ${pr.error} — 아무것도 바뀌지 않음`, 'bad'); return pr; }
      const pl = P.plan(pr.json);
      if (!pl.ok) { pending = null; renderPreview(); msg(`가져오기 거부${pl.section ? ' [' + pl.section + ']' : ''}: ${pl.error} — 아무것도 바뀌지 않음`, 'bad'); return pl; }
      pending = { name: String(name || 'project.json').slice(0, 80), bytes: pr.bytes, plan: pl.plan };
      msg(''); renderPreview();
      return { ok: true };
    }
    mount.addEventListener('click', e => {
      const b = e.target.closest('[data-pj]'); if (!b) return;
      const a = b.dataset.pj;
      if (a === 'export') { const r = P.exportProject(); msg(r.ok ? `내보냄: ${r.name} (${r.bytes} B) — 기록·로그·연결·실행 상태 제외` : r.error, r.ok ? 'ok' : 'bad'); }
      else if (a === 'choose') $('pjFile').click();
      else if (a === 'cancel') { pending = null; renderPreview(); msg('가져오기 취소 — 변경 없음'); }
      else if (a === 'stop') { H.activity.stop(b.dataset.key); renderPreview(); }
      else if (a === 'disconnect') { H.coreProject.disconnectAll(); renderPreview(); }
      else if (a === 'apply' && pending) {
        const r = P.apply(pending.plan);
        if (r.ok) { pending = null; renderPreview(); renderSummary(); msg(`적용됨 — ${['core'].concat(r.applied).join(', ')} 교체${r.kept.length ? ` · 현재 유지: ${r.kept.join(', ')}` : ''}. 연결·전송·실행 작업은 시작하지 않음.`, 'ok'); }
        else { renderPreview(); msg(r.error + (r.code === 'rollback-incomplete' ? ' ⚠ ' + r.rollbackError : ''), r.code === 'rollback-incomplete' ? 'bad severe' : 'bad'); }
      }
    });
    $('pjFile').addEventListener('change', e => {
      const f = e.target.files && e.target.files[0]; e.target.value = '';
      if (!f) return;
      if (f.size > MAX_BYTES) { pending = null; renderPreview(); msg(`가져오기 거부: 파일 ${f.size} B — 최대 ${MAX_BYTES} B (읽지 않음)`, 'bad'); return; }
      const rd = new root.FileReader();
      rd.onload = () => loadText(String(rd.result), f.name);
      rd.onerror = () => msg('파일 읽기 실패', 'bad');
      rd.readAsText(f);
    });
    const visible = () => !H.tools || !H.tools.current || H.tools.current() === 'project';
    const refresh = () => { if (!visible()) return; renderSummary(); renderPreview(); };
    H.subscribe(ev => { if (ev && (ev.type === 'link' || ev.type === 'txn' || ev.type === 'tool' || ev.type === 'catalog' || ev.type === 'config')) refresh(); });
    H.onRender(refresh);
    renderSummary();
    return { loadText, renderPreview, renderSummary, pending: () => pending };
  }

  const MAX = { FORMAT, VERSION, MAX_BYTES };
  let booted = null;
  function boot() {
    if (booted || !root.LoomWorkbench || !root.LoomWorkbench.coreProject) return booted;
    const H = root.LoomWorkbench, P = create(H);
    let ui = null; try { ui = mountUI(P, H); } catch (e) { if (root.console) root.console.error('project UI', e); }
    booted = { P, ui };
    root.LoomUdsProject = Object.assign(root.LoomUdsProject || {}, { create, mountUI, boot, instance: P, ui }, MAX);
    return booted;
  }
  root.LoomUdsProject = Object.assign(root.LoomUdsProject || {}, { create, mountUI, boot }, MAX);
  if (root.LoomWorkbench) boot();
  else if (root.document) root.document.addEventListener('uds:host-ready', boot, { once: true });
})(typeof window !== 'undefined' ? window : globalThis);
